//! v3 资产的流式写入与按块读取（task 019 / 025）：内存只保留一个批次的块，
//! 使超出内存的表面资产与体网格资产可以分块生成、遍历、诊断与导出。

use super::{
    CHUNK_ALIGNMENT, ChunkEntry, HEADER_SIZE, Layout, MAGIC, MESH_ASSET_FORMAT_VERSION,
    MILLIMETERS, MeshAssetError, RESULT_KIND, SURFACE_KIND, Stream, TET_KIND, TOC_ENTRY_BYTES,
    TOC_PREFIX_BYTES, align_up, chunk_jobs, chunk_raw, container_encoded_len,
    decode_surface_records, encode_cell_record, encode_chunks, parallel_map, parse_layout,
    read_exact_io, toc_len, worker_count,
};
use crate::{CellBlock, CellType, Progress, SurfaceTriangle, Tetrahedron};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::Path;

/// 一个三角面：3 个顶点 × xyz，单位 mm。
pub type Triangle = [[f64; 3]; 3];

/// 流式写入 v3 表面资产：按块调用 `fill(range, out)` 生成三角面（可并行），
/// 每批压缩后立即写出，最后回填目录与头部。返回 `(文件字节数, 资产摘要)`；
/// `progress` 被取消时返回 `Ok(None)`（已写出的不完整文件由调用方删除）。
pub fn write_surface_asset_streaming<W, F>(
    writer: W,
    triangle_count: usize,
    fill: F,
    progress: Option<&Progress>,
) -> Result<Option<(u64, String)>, MeshAssetError>
where
    W: Write + Seek,
    F: Fn(Range<usize>, &mut Vec<Triangle>) + Sync,
{
    if triangle_count == 0 {
        return Err(MeshAssetError::InvalidMesh);
    }
    let jobs = chunk_jobs(Stream::SurfaceTriangles, triangle_count);
    write_streams(
        writer,
        SURFACE_KIND,
        &jobs,
        |(_, range), raw| {
            let mut triangles = Vec::with_capacity(range.len());
            fill(range.clone(), &mut triangles);
            if triangles.len() != range.len() {
                return;
            }
            for coordinate in triangles.iter().flatten().flatten() {
                if !coordinate.is_finite() {
                    return;
                }
                raw.extend_from_slice(&coordinate.to_le_bytes());
            }
        },
        progress,
    )
}

/// 体网格资产的记录数与声明计数（task 025）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeStreamCounts {
    pub nodes: usize,
    /// 拆分后的四面体数；纯四面体资产即单元数。
    pub tets: usize,
    /// 原始单元数（task 030）：单元场长度以此为准。
    pub cells: usize,
    pub boundary: usize,
    pub region_count: u32,
    pub boundary_group_count: u32,
    /// 多面体单元数（task 031），即追加在节点末尾的辅助中心节点数。
    pub polyhedra: usize,
}

impl VolumeStreamCounts {
    /// 源节点数：不含多面体辅助中心节点；节点场长度以此为准。
    pub fn source_nodes(&self) -> usize {
        self.nodes.saturating_sub(self.polyhedra)
    }
}

/// 按下标区间生成体网格记录的数据源；各方法须恰好追加 `range.len()` 条记录。
pub trait VolumeSource: Sync {
    fn nodes(&self, range: Range<usize>, out: &mut Vec<[f64; 3]>);
    /// 纯四面体网格的四面体；混合单元源（`counts.cells != counts.tets`）不会被调用。
    fn tets(&self, range: Range<usize>, out: &mut Vec<Tetrahedron>);
    fn boundary(&self, range: Range<usize>, out: &mut Vec<SurfaceTriangle>);
    /// 混合单元网格（task 030）的原始单元：向 `out` 追加 `range.len()` 个单元（类型、区域、
    /// 节点；多面体为规范面流，`apex` 为全局中心节点号，见 [`crate::CellLayout`]）。
    /// 只在 `counts.cells != counts.tets` 时调用；纯四面体源无需实现。同一区间可能被调用
    /// 多次（单元记录与多面体面流分属两个流），每次必须给出相同结果。
    fn cells(&self, range: Range<usize>, out: &mut CellBlock) {
        let _ = (range, out);
    }
}

/// 流式写入体网格资产（task 025；v5 头部，纯四面体的块布局同 v3）：节点、单元、边界三角形按块生成并立即写出。
/// `counts.cells != counts.tets` 时为混合单元资产（task 030）：写 `VolumeCells` 原始单元记录与
/// `VolumeCellSummary`（拆分四面体数 = `counts.tets`、多面体数 = `counts.polyhedra`），不写 `VolumeTets`；
/// 含多面体时另写与单元块一一配对的 `VolumePolyFaces`（task 031）。
/// 写入端只做逐记录检查（坐标有限、节点下标 / 区域 / 边界组在范围内、混合单元可拆分且拆分总数相符）；
/// 方向、重复单元等整体校验在完整读取（`MeshAssetReader`）时执行。
pub fn write_volume_asset_streaming<W: Write + Seek>(
    writer: W,
    counts: VolumeStreamCounts,
    source: &impl VolumeSource,
    progress: Option<&Progress>,
) -> Result<Option<(u64, String)>, MeshAssetError> {
    let mixed = counts.cells != counts.tets;
    if counts.nodes == 0
        || counts.tets == 0
        || counts.cells == 0
        || counts.cells > counts.tets
        || u32::try_from(counts.nodes).is_err()
        || u32::try_from(counts.tets).is_err()
        || counts.region_count == 0
        || counts.polyhedra > counts.cells
        || counts.polyhedra >= counts.nodes
        || (counts.polyhedra > 0 && !mixed)
    {
        return Err(MeshAssetError::InvalidMesh);
    }
    let mut jobs = vec![(Stream::VolumeCounts, 0..1)];
    jobs.extend(chunk_jobs(Stream::VolumeNodes, counts.nodes));
    if !mixed {
        jobs.extend(chunk_jobs(Stream::VolumeTets, counts.tets));
    }
    jobs.extend(chunk_jobs(Stream::VolumeBoundary, counts.boundary));
    if mixed {
        let cell_jobs = super::cell_chunk_jobs(counts.cells, counts.polyhedra > 0);
        jobs.extend(cell_jobs.iter().cloned());
        jobs.push((Stream::VolumeCellSummary, 0..2));
        if counts.polyhedra > 0 {
            jobs.extend(
                cell_jobs
                    .into_iter()
                    .map(|(_, range)| (Stream::VolumePolyFaces, range)),
            );
        }
    }
    let node_limit = counts.nodes as u32;
    let source_limit = counts.source_nodes() as u32;
    // 混合单元拆分出的四面体总数与多面体数，写完后与 `counts` 核对。
    let decomposed = std::sync::atomic::AtomicUsize::new(0);
    let polyhedra = std::sync::atomic::AtomicUsize::new(0);
    let written = write_streams(
        writer,
        TET_KIND,
        &jobs,
        |(stream, range), raw| match stream {
            Stream::VolumeCounts => {
                raw.extend_from_slice(&counts.region_count.to_le_bytes());
                raw.extend_from_slice(&counts.boundary_group_count.to_le_bytes());
            }
            Stream::VolumeNodes => {
                let mut nodes = Vec::with_capacity(range.len());
                source.nodes(range.clone(), &mut nodes);
                if nodes.len() != range.len() || nodes.iter().flatten().any(|v| !v.is_finite()) {
                    return;
                }
                for coordinate in nodes.iter().flatten() {
                    raw.extend_from_slice(&coordinate.to_le_bytes());
                }
            }
            Stream::VolumeTets => {
                let mut tets = Vec::with_capacity(range.len());
                source.tets(range.clone(), &mut tets);
                if tets.len() != range.len()
                    || tets.iter().any(|tet| {
                        tet.region >= counts.region_count
                            || tet.nodes.iter().any(|&node| node >= node_limit)
                    })
                {
                    return;
                }
                for tet in &tets {
                    for node in tet.nodes {
                        raw.extend_from_slice(&node.to_le_bytes());
                    }
                    raw.extend_from_slice(&tet.region.to_le_bytes());
                }
            }
            Stream::VolumeBoundary => {
                let mut faces = Vec::with_capacity(range.len());
                source.boundary(range.clone(), &mut faces);
                if faces.len() != range.len()
                    || faces.iter().any(|face| {
                        face.group >= counts.boundary_group_count
                            || face.nodes.iter().any(|&node| node >= node_limit)
                    })
                {
                    return;
                }
                for face in &faces {
                    for node in face.nodes {
                        raw.extend_from_slice(&node.to_le_bytes());
                    }
                    raw.extend_from_slice(&face.group.to_le_bytes());
                }
            }
            Stream::VolumeCells => {
                let mut cells = CellBlock::default();
                source.cells(range.clone(), &mut cells);
                if cells.len() != range.len() {
                    return;
                }
                let mut tets = Vec::new();
                for (kind, region, nodes) in cells.iter() {
                    tets.clear();
                    let valid = region < counts.region_count
                        && cell_nodes_in_range(kind, nodes, source_limit, node_limit)
                        && crate::cells::decompose_cell(kind, nodes, &mut tets).is_ok();
                    if !valid || encode_cell_record(kind, region, nodes, raw).is_none() {
                        raw.clear();
                        return;
                    }
                    if kind == CellType::Polyhedron {
                        polyhedra.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    decomposed.fetch_add(tets.len(), std::sync::atomic::Ordering::Relaxed);
                }
            }
            Stream::VolumePolyFaces => {
                let mut cells = CellBlock::default();
                source.cells(range.clone(), &mut cells);
                for (kind, _, nodes) in cells.iter() {
                    if kind == CellType::Polyhedron {
                        for word in &nodes[1..] {
                            raw.extend_from_slice(&word.to_le_bytes());
                        }
                    }
                }
            }
            Stream::VolumeCellSummary => {
                raw.extend_from_slice(&(counts.tets as u64).to_le_bytes());
                raw.extend_from_slice(&(counts.polyhedra as u64).to_le_bytes());
            }
            Stream::SurfaceTriangles | Stream::ResultTable | Stream::ResultValues => {}
        },
        progress,
    )?;
    if mixed
        && written.is_some()
        && (decomposed.load(std::sync::atomic::Ordering::Relaxed) != counts.tets
            || polyhedra.load(std::sync::atomic::Ordering::Relaxed) != counts.polyhedra)
    {
        return Err(MeshAssetError::InvalidMesh);
    }
    Ok(written)
}

/// 单元节点下标范围：多面体 `apex` 必须是中心节点（`source..nodes`），面上节点与其他类型的
/// 节点必须是源节点（`< source`）。
fn cell_nodes_in_range(kind: CellType, nodes: &[u32], source: u32, limit: u32) -> bool {
    if kind == CellType::Polyhedron {
        nodes
            .first()
            .is_some_and(|&apex| (source..limit).contains(&apex))
            && crate::faces(&nodes[1..])
                .flatten()
                .all(|&node| node < source)
    } else {
        nodes.iter().all(|&node| node < source)
    }
}

/// 通用流式写出：先占位头部与目录，按批并行编码并写出数据块，最后回填。
fn write_streams<W, F>(
    mut writer: W,
    kind: u8,
    jobs: &[(Stream, Range<usize>)],
    fill: F,
    progress: Option<&Progress>,
) -> Result<Option<(u64, String)>, MeshAssetError>
where
    W: Write + Seek,
    F: Fn((Stream, Range<usize>), &mut Vec<u8>) + Sync,
{
    let toc_bytes = jobs
        .len()
        .checked_mul(TOC_ENTRY_BYTES)
        .and_then(|len| len.checked_add(TOC_PREFIX_BYTES))
        .ok_or(MeshAssetError::SizeOverflow)?;
    let io = |error: std::io::Error| MeshAssetError::Io(error.kind());
    writer.seek(SeekFrom::Start(0)).map_err(io)?;
    writer
        .write_all(&vec![0; HEADER_SIZE + toc_bytes])
        .map_err(io)?;
    let mut cursor = (HEADER_SIZE + toc_bytes) as u64;
    let mut toc = Vec::with_capacity(toc_bytes);
    toc.extend_from_slice(&(jobs.len() as u64).to_le_bytes());
    let batch = worker_count(jobs.len()) * 2;
    let fill = &fill;
    for (batch_index, batch_jobs) in jobs.chunks(batch).enumerate() {
        if progress.is_some_and(Progress::is_cancelled) {
            return Ok(None);
        }
        let chunks = encode_chunks(batch_jobs, fill)?;
        for chunk in chunks {
            let offset = align_up(cursor).ok_or(MeshAssetError::SizeOverflow)?;
            let padding = (offset - cursor) as usize;
            writer
                .write_all(&[0; CHUNK_ALIGNMENT as usize][..padding])
                .map_err(io)?;
            writer.write_all(&chunk.stored).map_err(io)?;
            let mut entry = [0_u8; TOC_ENTRY_BYTES];
            entry[0] = chunk.stream as u8;
            entry[1] = chunk.codec;
            entry[8..16].copy_from_slice(&offset.to_le_bytes());
            entry[16..24].copy_from_slice(&(chunk.stored.len() as u64).to_le_bytes());
            entry[24..32].copy_from_slice(&(chunk.record_count as u64).to_le_bytes());
            entry[32..64].copy_from_slice(&chunk.digest);
            toc.extend_from_slice(&entry);
            cursor = offset + chunk.stored.len() as u64;
        }
        if let Some(progress) = progress {
            let done = ((batch_index + 1) * batch).min(jobs.len());
            progress.report(0.0, 0.99, done as u64, jobs.len() as u64);
        }
    }
    let digest = Sha256::digest(&toc);
    let mut header = [0_u8; HEADER_SIZE];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&super::container_version(kind).to_le_bytes());
    header[12] = kind;
    header[13] = MILLIMETERS;
    header[16..24].copy_from_slice(&(cursor - HEADER_SIZE as u64).to_le_bytes());
    header[24..56].copy_from_slice(&digest);
    writer.seek(SeekFrom::Start(0)).map_err(io)?;
    writer.write_all(&header).map_err(io)?;
    writer.write_all(&toc).map_err(io)?;
    writer.flush().map_err(io)?;
    if let Some(progress) = progress {
        progress.set_fraction(1.0);
    }
    Ok(Some((cursor, format!("sha256:{digest:x}"))))
}

/// 打开 v3 资产并校验头部与目录；`kind` 不符或 v2 资产返回 `UnsupportedLayout`。
pub(super) fn open_layout(path: &Path, kind: u8) -> Result<(File, Layout), MeshAssetError> {
    let io = |error: std::io::Error| MeshAssetError::Io(error.kind());
    let mut file = File::open(path).map_err(io)?;
    let file_len = file.metadata().map_err(io)?.len();
    let mut header = vec![0; HEADER_SIZE];
    read_exact_io(&mut file, &mut header)?;
    if container_encoded_len(&header, kind == RESULT_KIND)? as u64 != file_len {
        return Err(MeshAssetError::InvalidLength);
    }
    let version = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
    let chunked = if kind == RESULT_KIND {
        version == super::RESULT_CONTAINER_VERSION
    } else {
        version == MESH_ASSET_FORMAT_VERSION || version == super::V4 || version == super::V3
    };
    if !chunked || header[12] != kind {
        return Err(MeshAssetError::UnsupportedLayout);
    }
    let mut toc = vec![0; TOC_PREFIX_BYTES];
    read_exact_io(&mut file, &mut toc)?;
    let len = toc_len(&toc, file_len)?;
    toc.resize(len, 0);
    read_exact_io(&mut file, &mut toc[TOC_PREFIX_BYTES..])?;
    let layout = parse_layout(&header, &toc, file_len)?;
    Ok((file, layout))
}

/// 按目录顺序批量读取 `chunks`，并行解压校验并用 `decode` 解码，每批调用一次 `visit`；
/// `visit` 返回 `false` 或进度被取消时停止并返回 `Ok(false)`。
pub(super) fn visit_chunks<T: Send>(
    file: &mut File,
    chunks: &[ChunkEntry],
    progress: Option<(&Progress, f64, f64)>,
    decode: impl Fn(&ChunkEntry, &[u8]) -> Result<Vec<T>, MeshAssetError> + Sync,
    visit: impl FnMut(&[Vec<T>]) -> bool,
) -> Result<bool, MeshAssetError> {
    let groups: Vec<Vec<ChunkEntry>> = chunks.iter().map(|chunk| vec![chunk.clone()]).collect();
    visit_chunk_groups(
        file,
        &groups,
        progress,
        |_, group, raw| decode(&group[0], &raw[0]),
        visit,
    )
}

/// 按组读取（task 031）：每组若干块（如单元块与配对的面流块）一起读入、解压校验，
/// 并行调用 `decode(组下标, 组内块, 各块原始字节)`，每批调用一次 `visit`。
/// 每块仍是一次定位 + 一次整块读取。
pub(super) fn visit_chunk_groups<T: Send>(
    file: &mut File,
    groups: &[Vec<ChunkEntry>],
    progress: Option<(&Progress, f64, f64)>,
    decode: impl Fn(usize, &[ChunkEntry], &[Vec<u8>]) -> Result<T, MeshAssetError> + Sync,
    mut visit: impl FnMut(&[T]) -> bool,
) -> Result<bool, MeshAssetError> {
    let io = |error: std::io::Error| MeshAssetError::Io(error.kind());
    let batch = worker_count(groups.len()) * 2;
    for (batch_index, batch_groups) in groups.chunks(batch).enumerate() {
        if progress.is_some_and(|(progress, _, _)| progress.is_cancelled()) {
            return Ok(false);
        }
        let mut stored = Vec::with_capacity(batch_groups.len());
        for group in batch_groups {
            let mut members = Vec::with_capacity(group.len());
            for chunk in group {
                file.seek(SeekFrom::Start(chunk.offset)).map_err(io)?;
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(chunk.stored_len)
                    .map_err(|_| MeshAssetError::AllocationFailed)?;
                bytes.resize(chunk.stored_len, 0);
                file.read_exact(&mut bytes).map_err(io)?;
                members.push(bytes);
            }
            stored.push(members);
        }
        let first = batch_index * batch;
        let decoded = parallel_map(batch_groups.len(), |index| {
            let group = &batch_groups[index];
            let raw = group
                .iter()
                .zip(&stored[index])
                .map(|(chunk, bytes)| chunk_raw(chunk, bytes))
                .collect::<Result<Vec<_>, _>>()?;
            decode(first + index, group, &raw)
        })?;
        drop(stored);
        if !visit(&decoded) {
            return Ok(false);
        }
        if let Some((progress, start, end)) = progress {
            let done = ((batch_index + 1) * batch).min(groups.len());
            progress.report(start, end, done as u64, groups.len() as u64);
        }
    }
    Ok(true)
}

/// 已打开的 v3 表面资产：只读入头部与目录，数据块按批次按需读取与校验。
pub struct SurfaceAssetChunks {
    file: File,
    layout: Layout,
}

impl SurfaceAssetChunks {
    /// 打开并校验头部与目录；体网格或 v2 资产返回 `UnsupportedLayout`。
    pub fn open(path: &Path) -> Result<Self, MeshAssetError> {
        let (file, layout) = open_layout(path, SURFACE_KIND)?;
        Ok(Self { file, layout })
    }

    pub fn triangle_count(&self) -> u64 {
        self.layout
            .chunks
            .iter()
            .map(|chunk| chunk.record_count as u64)
            .sum()
    }

    pub fn chunk_count(&self) -> usize {
        self.layout.chunks.len()
    }

    pub fn file_len(&self) -> u64 {
        self.layout.total_len
    }

    pub fn digest(&self) -> &str {
        &self.layout.digest
    }

    /// 按目录顺序批量读取、并行解压校验，每批调用一次 `visit(第一面下标, 各块三角面)`。
    /// `progress` 按 `[start, end]` 报告；取消时返回 `Ok(false)`。
    pub fn visit_batches(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
        mut visit: impl FnMut(u64, &[Vec<Triangle>]),
    ) -> Result<bool, MeshAssetError> {
        self.visit_batches_until(progress, |first, batch| {
            visit(first, batch);
            true
        })
    }

    /// 同 [`Self::visit_batches`]，`visit` 返回 `false` 时提前停止并返回 `Ok(false)`。
    pub fn visit_batches_until(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
        mut visit: impl FnMut(u64, &[Vec<Triangle>]) -> bool,
    ) -> Result<bool, MeshAssetError> {
        let mut first = 0_u64;
        visit_chunks(
            &mut self.file,
            &self.layout.chunks,
            progress,
            |chunk, raw| {
                let mut triangles = vec![[[0.0; 3]; 3]; chunk.record_count];
                decode_surface_records(raw, &mut triangles)?;
                Ok(triangles)
            },
            |decoded| {
                let keep = visit(first, decoded);
                first += decoded.iter().map(|chunk| chunk.len() as u64).sum::<u64>();
                keep
            },
        )
    }
}

/// 带原始单元 ID 的拆分四面体批次访问器。
type CellTetVisitor<'a> = dyn FnMut(&[Vec<(u32, Tetrahedron)>]) -> bool + 'a;

/// 已打开的 v3 体网格资产（task 025）：头部、目录与计数记录已校验，
/// 节点 / 四面体 / 边界三角形按流分别按需读取，四面体可逐批遍历而不整体驻留。
pub struct VolumeAssetChunks {
    file: File,
    layout: Layout,
    counts: VolumeStreamCounts,
}

impl VolumeAssetChunks {
    pub fn open(path: &Path) -> Result<Self, MeshAssetError> {
        let (mut file, layout) = open_layout(path, TET_KIND)?;
        let records = |stream: Stream| {
            layout
                .chunks
                .iter()
                .filter(|chunk| chunk.stream == stream)
                .map(|chunk| chunk.record_count)
                .sum::<usize>()
        };
        let (nodes, mut tets, boundary, mixed_cells) = (
            records(Stream::VolumeNodes),
            records(Stream::VolumeTets),
            records(Stream::VolumeBoundary),
            records(Stream::VolumeCells),
        );
        let mut polyhedra = 0_usize;
        if mixed_cells > 0 {
            // 混合资产（v4 / v5）：拆分四面体数（与 v5 的多面体数）取自摘要记录（一次小块读取）。
            let summary: Vec<ChunkEntry> = layout
                .chunks
                .iter()
                .filter(|chunk| chunk.stream == Stream::VolumeCellSummary)
                .cloned()
                .collect();
            visit_chunks(
                &mut file,
                &summary,
                None,
                |_, raw| {
                    Ok(raw
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|v| u64::from_le_bytes(*v))
                        .collect())
                },
                |decoded| {
                    let mut values = decoded.iter().flatten();
                    if let Some(&value) = values.next() {
                        tets = usize::try_from(value).unwrap_or(0);
                    }
                    if let Some(&value) = values.next() {
                        polyhedra = usize::try_from(value).unwrap_or(usize::MAX);
                    }
                    true
                },
            )?;
            let has_faces = layout
                .chunks
                .iter()
                .any(|chunk| chunk.stream == Stream::VolumePolyFaces);
            if tets < mixed_cells
                || u32::try_from(mixed_cells).is_err()
                || polyhedra > mixed_cells
                || polyhedra >= nodes
                || has_faces != (polyhedra > 0)
            {
                return Err(MeshAssetError::InvalidMesh);
            }
        }
        let cells = if mixed_cells > 0 { mixed_cells } else { tets };
        if nodes == 0 || tets == 0 || u32::try_from(nodes).is_err() {
            return Err(MeshAssetError::InvalidMesh);
        }
        let count_chunks: Vec<ChunkEntry> = layout
            .chunks
            .iter()
            .filter(|chunk| chunk.stream == Stream::VolumeCounts)
            .cloned()
            .collect();
        let mut declared = [0_u32; 2];
        visit_chunks(
            &mut file,
            &count_chunks,
            None,
            |_, raw| {
                Ok(raw
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|v| u32::from_le_bytes(*v))
                    .collect())
            },
            |decoded| {
                for (slot, value) in declared.iter_mut().zip(decoded.iter().flatten()) {
                    *slot = *value;
                }
                true
            },
        )?;
        let [region_count, boundary_group_count] = declared;
        if region_count == 0 {
            return Err(MeshAssetError::InvalidMesh);
        }
        Ok(Self {
            file,
            layout,
            counts: VolumeStreamCounts {
                nodes,
                tets,
                cells,
                boundary,
                region_count,
                boundary_group_count,
                polyhedra,
            },
        })
    }

    pub fn counts(&self) -> VolumeStreamCounts {
        self.counts
    }

    /// 是否为原生混合单元资产（task 030）；此时四面体为加载时确定性重算的拆分。
    pub fn is_mixed(&self) -> bool {
        self.layout
            .chunks
            .iter()
            .any(|chunk| chunk.stream == Stream::VolumeCells)
    }

    pub fn digest(&self) -> &str {
        &self.layout.digest
    }

    pub fn file_len(&self) -> u64 {
        self.layout.total_len
    }

    fn stream_chunks(&self, stream: Stream) -> Vec<ChunkEntry> {
        self.layout
            .chunks
            .iter()
            .filter(|chunk| chunk.stream == stream)
            .cloned()
            .collect()
    }

    /// 逐批遍历节点（每批为若干块的节点数组，按下标顺序）。
    pub fn visit_nodes(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
        visit: impl FnMut(&[Vec<[f64; 3]>]) -> bool,
    ) -> Result<bool, MeshAssetError> {
        let chunks = self.stream_chunks(Stream::VolumeNodes);
        visit_chunks(
            &mut self.file,
            &chunks,
            progress,
            |_, raw| {
                raw.as_chunks::<24>()
                    .0
                    .iter()
                    .map(|record| {
                        let values = record.as_chunks::<8>().0;
                        let node = [0, 1, 2].map(|axis| f64::from_le_bytes(values[axis]));
                        if node.iter().all(|value| value.is_finite()) {
                            Ok(node)
                        } else {
                            Err(MeshAssetError::InvalidMesh)
                        }
                    })
                    .collect()
            },
            visit,
        )
    }

    /// 读入全部节点（24 B / 节点）；取消返回 `Ok(None)`。
    pub fn read_nodes(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
    ) -> Result<Option<Vec<[f64; 3]>>, MeshAssetError> {
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(self.counts.nodes)
            .map_err(|_| MeshAssetError::AllocationFailed)?;
        let finished = self.visit_nodes(progress, |batch| {
            for chunk in batch {
                nodes.extend_from_slice(chunk);
            }
            true
        })?;
        Ok(finished.then_some(nodes))
    }

    /// 逐批遍历四面体；节点下标与区域号在读取时校验范围。
    /// 混合资产逐块读取原生单元并就地拆分，批次中的四面体仍按全局拆分顺序排列。
    pub fn visit_tets(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
        visit: impl FnMut(&[Vec<Tetrahedron>]) -> bool,
    ) -> Result<bool, MeshAssetError> {
        let mut visit = visit;
        if self.is_mixed() {
            return self.visit_mixed_tets(progress, &mut |batch| {
                let tets: Vec<Vec<Tetrahedron>> = batch
                    .iter()
                    .map(|chunk| chunk.iter().map(|(_, tet)| *tet).collect())
                    .collect();
                visit(&tets)
            });
        }
        self.visit_plain_tets(progress, &mut visit)
    }

    fn visit_plain_tets(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
        visit: &mut dyn FnMut(&[Vec<Tetrahedron>]) -> bool,
    ) -> Result<bool, MeshAssetError> {
        let chunks = self.stream_chunks(Stream::VolumeTets);
        let counts = self.counts;
        visit_chunks(
            &mut self.file,
            &chunks,
            progress,
            |_, raw| {
                raw.as_chunks::<20>()
                    .0
                    .iter()
                    .map(|record| {
                        let values = record.as_chunks::<4>().0;
                        let tet = Tetrahedron {
                            nodes: [0, 1, 2, 3].map(|slot| u32::from_le_bytes(values[slot])),
                            region: u32::from_le_bytes(values[4]),
                        };
                        if tet.region < counts.region_count
                            && tet.nodes.iter().all(|&node| (node as usize) < counts.nodes)
                        {
                            Ok(tet)
                        } else {
                            Err(MeshAssetError::InvalidMesh)
                        }
                    })
                    .collect()
            },
            visit,
        )
    }

    /// 逐批遍历拆分四面体及其原始单元下标（task 030）。纯四面体资产的单元下标即四面体下标。
    pub fn visit_cell_tets(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
        mut visit: impl FnMut(&[Vec<(u32, Tetrahedron)>]) -> bool,
    ) -> Result<bool, MeshAssetError> {
        if self.is_mixed() {
            return self.visit_mixed_tets(progress, &mut visit);
        }
        let mut next = 0_u32;
        self.visit_plain_tets(progress, &mut |batch| {
            let tagged: Vec<Vec<(u32, Tetrahedron)>> = batch
                .iter()
                .map(|chunk| {
                    chunk
                        .iter()
                        .map(|tet| {
                            let cell = next;
                            next = next.wrapping_add(1);
                            (cell, *tet)
                        })
                        .collect()
                })
                .collect();
            visit(&tagged)
        })
    }

    /// 单元块分组：每个 `VolumeCells` 块与按下标配对的 `VolumePolyFaces` 块（若有）。
    fn cell_groups(&self) -> (Vec<Vec<ChunkEntry>>, Vec<usize>) {
        let cells = self.stream_chunks(Stream::VolumeCells);
        let faces = self.stream_chunks(Stream::VolumePolyFaces);
        let firsts = cells
            .iter()
            .scan(0_usize, |cursor, chunk| {
                let first = *cursor;
                *cursor += chunk.record_count;
                Some(first)
            })
            .collect();
        let groups = cells
            .into_iter()
            .enumerate()
            .map(|(index, chunk)| {
                let mut group = vec![chunk];
                group.extend(faces.get(index).cloned());
                group
            })
            .collect();
        (groups, firsts)
    }

    fn visit_mixed_tets(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
        visit: &mut CellTetVisitor<'_>,
    ) -> Result<bool, MeshAssetError> {
        let (groups, firsts) = self.cell_groups();
        let counts = self.counts;
        let mut emitted = 0_usize;
        let finished = visit_chunk_groups(
            &mut self.file,
            &groups,
            progress,
            |index, _, raw| {
                let block = decode_cell_group(counts, raw)?;
                let mut out = Vec::with_capacity(block.len() * 4);
                let mut scratch = Vec::with_capacity(6);
                for (local, (kind, region, nodes)) in block.iter().enumerate() {
                    scratch.clear();
                    crate::decompose_cell(kind, nodes, &mut scratch)
                        .map_err(|crate::InvalidCell| MeshAssetError::InvalidMesh)?;
                    let cell = (firsts[index] + local) as u32;
                    out.extend(
                        scratch
                            .iter()
                            .map(|&tet| (cell, Tetrahedron { nodes: tet, region })),
                    );
                }
                Ok(out)
            },
            |batch| {
                emitted += batch.iter().map(Vec::len).sum::<usize>();
                visit(batch)
            },
        )?;
        if finished && emitted != counts.tets {
            return Err(MeshAssetError::InvalidMesh);
        }
        Ok(finished)
    }

    /// 逐批遍历原始单元（task 030 / 031）：每块一个 [`CellBlock`]（多面体为规范面流，
    /// 单元块与配对的面流块一起读取）。纯四面体资产把每个四面体报告为 `Tetra`。
    /// 节点下标与区域号在读取时校验范围；供原生单元导出、质量统计与分块构建使用。
    pub fn visit_cells(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
        mut visit: impl FnMut(&[CellBlock]) -> bool,
    ) -> Result<bool, MeshAssetError> {
        if !self.is_mixed() {
            return self.visit_plain_tets(progress, &mut |batch| {
                let blocks: Vec<CellBlock> = batch
                    .iter()
                    .map(|chunk| {
                        let mut block = CellBlock::default();
                        for tet in chunk {
                            block.push(CellType::Tetra, tet.region, &tet.nodes);
                        }
                        block
                    })
                    .collect();
                visit(&blocks)
            });
        }
        let (groups, _) = self.cell_groups();
        let counts = self.counts;
        let mut polyhedra = 0_usize;
        let finished = visit_chunk_groups(
            &mut self.file,
            &groups,
            progress,
            |_, _, raw| decode_cell_group(counts, raw),
            |batch| {
                polyhedra += batch
                    .iter()
                    .flat_map(|block| &block.types)
                    .filter(|&&kind| kind == CellType::Polyhedron)
                    .count();
                visit(batch)
            },
        )?;
        if finished && polyhedra != counts.polyhedra {
            return Err(MeshAssetError::InvalidMesh);
        }
        Ok(finished)
    }

    /// 读入全部边界三角形（16 B / 面）；取消返回 `Ok(None)`。
    pub fn read_boundary(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
    ) -> Result<Option<Vec<SurfaceTriangle>>, MeshAssetError> {
        let chunks = self.stream_chunks(Stream::VolumeBoundary);
        let counts = self.counts;
        let mut faces = Vec::new();
        faces
            .try_reserve_exact(counts.boundary)
            .map_err(|_| MeshAssetError::AllocationFailed)?;
        let finished = visit_chunks(
            &mut self.file,
            &chunks,
            progress,
            |_, raw| {
                raw.as_chunks::<16>()
                    .0
                    .iter()
                    .map(|record| {
                        let values = record.as_chunks::<4>().0;
                        let face = SurfaceTriangle {
                            nodes: [0, 1, 2].map(|slot| u32::from_le_bytes(values[slot])),
                            group: u32::from_le_bytes(values[3]),
                        };
                        if face.group < counts.boundary_group_count
                            && face
                                .nodes
                                .iter()
                                .all(|&node| (node as usize) < counts.nodes)
                        {
                            Ok(face)
                        } else {
                            Err(MeshAssetError::InvalidMesh)
                        }
                    })
                    .collect()
            },
            |batch| {
                for chunk in batch {
                    faces.extend_from_slice(chunk);
                }
                true
            },
        )?;
        Ok(finished.then_some(faces))
    }
}

/// 解码一组单元块（`VolumeCells` 原始字节 + 可选的 `VolumePolyFaces` 原始字节）为规范单元块；
/// 校验区域号、节点范围（多面体中心节点在源节点之后、面上节点为源节点）与面流长度，
/// 面流必须被本块的多面体恰好用完。
fn decode_cell_group(
    counts: VolumeStreamCounts,
    raw: &[Vec<u8>],
) -> Result<CellBlock, MeshAssetError> {
    let invalid = MeshAssetError::InvalidMesh;
    let records = raw.first().ok_or(MeshAssetError::InvalidMesh)?;
    let words: Vec<u32> = raw
        .get(1)
        .map(|bytes| {
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| u32::from_le_bytes(*word))
                .collect()
        })
        .unwrap_or_default();
    let (source, limit) = (counts.source_nodes() as u32, counts.nodes as u32);
    let mut block = CellBlock::default();
    let mut cursor = 0_usize;
    let mut stream = Vec::new();
    for record in records.as_chunks::<40>().0 {
        let (kind, region, nodes) = super::decode_cell_record(record).ok_or(invalid)?;
        let nodes: &[u32] = if kind == CellType::Polyhedron {
            let len = nodes[1] as usize;
            let faces = cursor
                .checked_add(len)
                .and_then(|end| words.get(cursor..end))
                .ok_or(invalid)?;
            cursor += len;
            if crate::face_stream_len(faces) != Some(len) {
                return Err(invalid);
            }
            stream.clear();
            stream.push(nodes[0]);
            stream.extend_from_slice(faces);
            &stream
        } else {
            &nodes[..kind.fixed_node_count().unwrap_or(0)]
        };
        if region >= counts.region_count || !cell_nodes_in_range(kind, nodes, source, limit) {
            return Err(invalid);
        }
        block.push(kind, region, nodes);
    }
    if cursor != words.len() {
        return Err(invalid);
    }
    Ok(block)
}
