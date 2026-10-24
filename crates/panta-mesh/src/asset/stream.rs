//! v3 资产的流式写入与按块读取（task 019 / 025）：内存只保留一个批次的块，
//! 使超出内存的表面资产与体网格资产可以分块生成、遍历、诊断与导出。

use super::{
    CHUNK_ALIGNMENT, ChunkEntry, HEADER_SIZE, Layout, MAGIC, MESH_ASSET_FORMAT_VERSION,
    MILLIMETERS, MeshAssetError, RESULT_KIND, SURFACE_KIND, Stream, TET_KIND, TOC_ENTRY_BYTES,
    TOC_PREFIX_BYTES, align_up, chunk_jobs, chunk_raw, container_encoded_len,
    decode_surface_records, encode_chunks, parallel_map, parse_layout, read_exact_io, toc_len,
    worker_count,
};
use crate::{Progress, SurfaceTriangle, Tetrahedron};
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
    pub tets: usize,
    pub boundary: usize,
    pub region_count: u32,
    pub boundary_group_count: u32,
}

/// 按下标区间生成体网格记录的数据源；各方法须恰好追加 `range.len()` 条记录。
pub trait VolumeSource: Sync {
    fn nodes(&self, range: Range<usize>, out: &mut Vec<[f64; 3]>);
    fn tets(&self, range: Range<usize>, out: &mut Vec<Tetrahedron>);
    fn boundary(&self, range: Range<usize>, out: &mut Vec<SurfaceTriangle>);
}

/// 流式写入 v3 体网格资产（task 025）：节点、四面体、边界三角形按块生成并立即写出。
/// 写入端只做逐记录检查（坐标有限、节点下标 / 区域 / 边界组在范围内）；
/// 方向、重复单元等整体校验在完整读取（`MeshAssetReader`）时执行。
pub fn write_volume_asset_streaming<W: Write + Seek>(
    writer: W,
    counts: VolumeStreamCounts,
    source: &impl VolumeSource,
    progress: Option<&Progress>,
) -> Result<Option<(u64, String)>, MeshAssetError> {
    if counts.nodes == 0
        || counts.tets == 0
        || u32::try_from(counts.nodes).is_err()
        || counts.region_count == 0
    {
        return Err(MeshAssetError::InvalidMesh);
    }
    let mut jobs = vec![(Stream::VolumeCounts, 0..1)];
    jobs.extend(chunk_jobs(Stream::VolumeNodes, counts.nodes));
    jobs.extend(chunk_jobs(Stream::VolumeTets, counts.tets));
    jobs.extend(chunk_jobs(Stream::VolumeBoundary, counts.boundary));
    let node_limit = counts.nodes as u32;
    write_streams(
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
            Stream::SurfaceTriangles | Stream::ResultTable | Stream::ResultValues => {}
        },
        progress,
    )
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
    header[8..12].copy_from_slice(&MESH_ASSET_FORMAT_VERSION.to_le_bytes());
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
    if u32::from_le_bytes([header[8], header[9], header[10], header[11]])
        != MESH_ASSET_FORMAT_VERSION
        || header[12] != kind
    {
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
    mut visit: impl FnMut(&[Vec<T>]) -> bool,
) -> Result<bool, MeshAssetError> {
    let io = |error: std::io::Error| MeshAssetError::Io(error.kind());
    let batch = worker_count(chunks.len()) * 2;
    for (batch_index, batch_chunks) in chunks.chunks(batch).enumerate() {
        if progress.is_some_and(|(progress, _, _)| progress.is_cancelled()) {
            return Ok(false);
        }
        let mut stored = Vec::with_capacity(batch_chunks.len());
        for chunk in batch_chunks {
            file.seek(SeekFrom::Start(chunk.offset)).map_err(io)?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(chunk.stored_len)
                .map_err(|_| MeshAssetError::AllocationFailed)?;
            bytes.resize(chunk.stored_len, 0);
            file.read_exact(&mut bytes).map_err(io)?;
            stored.push(bytes);
        }
        let decoded = parallel_map(batch_chunks.len(), |index| {
            let raw = chunk_raw(&batch_chunks[index], &stored[index])?;
            decode(&batch_chunks[index], &raw)
        })?;
        drop(stored);
        if !visit(&decoded) {
            return Ok(false);
        }
        if let Some((progress, start, end)) = progress {
            let done = ((batch_index + 1) * batch).min(chunks.len());
            progress.report(start, end, done as u64, chunks.len() as u64);
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
        let (nodes, tets, boundary) = (
            records(Stream::VolumeNodes),
            records(Stream::VolumeTets),
            records(Stream::VolumeBoundary),
        );
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
                boundary,
                region_count,
                boundary_group_count,
            },
        })
    }

    pub fn counts(&self) -> VolumeStreamCounts {
        self.counts
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
    pub fn visit_tets(
        &mut self,
        progress: Option<(&Progress, f64, f64)>,
        visit: impl FnMut(&[Vec<Tetrahedron>]) -> bool,
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
