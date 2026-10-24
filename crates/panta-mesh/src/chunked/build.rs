//! 分块构建（task 027）：k-d 空间划分 → Morton 排序块 → 并行编码 → 单文件大缓冲顺序写。
//! v3（task 030）：划分单位是原始单元，混合单元的拆分四面体整体落在同一块。

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::ops::Range;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use super::cache::parallel_map;
use super::format::{
    ChunkCellLayout, ChunkCells, ChunkFieldInfo, ChunkHeader, ChunkSurface, ChunkTable,
    FORMAT_VERSION, FieldBlock, HEAD_LEN, HEAD_MAGIC, VOXELS_PER_AXIS, Writer, compress,
    encode_cells, encode_surface, encode_table, encode_tail,
};
use super::{ChunkError, worker_count};
use crate::field_section::{range_key, tuple};
use crate::{
    CellLayout, FieldAssociation, ResultField, SurfaceMesh, SurfaceTriangle, TetMesh, Tetrahedron,
};

/// 分块构建的结果场来源（task 029）：逐个加载，构建峰值只多一个场。
pub trait FieldProvider {
    fn infos(&self) -> Vec<ChunkFieldInfo>;
    fn load(&mut self, index: usize) -> Result<ResultField, ChunkError>;
}

/// 无结果场。
pub struct NoFields;

impl FieldProvider for NoFields {
    fn infos(&self) -> Vec<ChunkFieldInfo> {
        Vec::new()
    }
    fn load(&mut self, _: usize) -> Result<ResultField, ChunkError> {
        Err(ChunkError::Invalid("no fields".to_owned()))
    }
}

/// 已驻留的结果场。
pub struct ResidentFields<'a>(pub &'a [ResultField]);

impl FieldProvider for ResidentFields<'_> {
    fn infos(&self) -> Vec<ChunkFieldInfo> {
        self.0.iter().map(field_info).collect()
    }
    fn load(&mut self, index: usize) -> Result<ResultField, ChunkError> {
        self.0
            .get(index)
            .cloned()
            .ok_or_else(|| ChunkError::Invalid(format!("field {index} missing")))
    }
}

impl FieldProvider for crate::ResultAssetReader {
    fn infos(&self) -> Vec<ChunkFieldInfo> {
        self.summaries()
            .iter()
            .map(|summary| ChunkFieldInfo {
                name: summary.name.clone(),
                association: summary.association,
                components: summary.components,
            })
            .collect()
    }
    fn load(&mut self, index: usize) -> Result<ResultField, ChunkError> {
        self.read_field(index)
            .map_err(|error| ChunkError::Format(format!("result asset: {error}")))
    }
}

fn field_info(field: &ResultField) -> ChunkFieldInfo {
    ChunkFieldInfo {
        name: field.name.clone(),
        association: field.association,
        components: field.components,
    }
}

/// 随机访问的四面体源：分块构建按下标取节点、单元与边界面，不要求整体驻留。
/// 混合单元网格（task 030）另给出原始单元布局，四面体为其拆分结果（按原始单元成段排列）。
pub trait TetSource: Sync {
    fn node_count(&self) -> usize;
    fn tet_count(&self) -> usize;
    fn node(&self, index: usize) -> [f64; 3];
    fn tet(&self, index: usize) -> Tetrahedron;
    fn boundary_count(&self) -> usize;
    fn boundary_face(&self, index: usize) -> SurfaceTriangle;
    /// 原始单元布局；纯四面体源为 `None`（原始单元即四面体）。
    fn cell_layout(&self) -> Option<&CellLayout> {
        None
    }
}

impl TetSource for TetMesh {
    fn node_count(&self) -> usize {
        self.nodes.len()
    }
    fn tet_count(&self) -> usize {
        self.tets.len()
    }
    fn node(&self, index: usize) -> [f64; 3] {
        self.nodes[index]
    }
    fn tet(&self, index: usize) -> Tetrahedron {
        self.tets[index]
    }
    fn boundary_count(&self) -> usize {
        self.boundary.len()
    }
    fn boundary_face(&self, index: usize) -> SurfaceTriangle {
        self.boundary[index]
    }
    fn cell_layout(&self) -> Option<&CellLayout> {
        self.cells.as_deref()
    }
}

/// 原始单元到拆分四面体的映射：纯四面体时恒等，混合时 `starts[c]..starts[c + 1]`。
struct CellMap<'a> {
    layout: Option<&'a CellLayout>,
    starts: Vec<u32>,
    count: usize,
}

impl<'a> CellMap<'a> {
    fn new(source: &'a dyn TetSource) -> Result<Self, ChunkError> {
        let Some(layout) = source.cell_layout() else {
            return Ok(Self {
                layout: None,
                starts: Vec::new(),
                count: source.tet_count(),
            });
        };
        let count = layout.cell_count();
        let invalid = || ChunkError::Invalid("cell layout does not match the tets".to_owned());
        if layout.tet_cells.len() != source.tet_count() || count == 0 {
            return Err(invalid());
        }
        let mut starts = Vec::with_capacity(count + 1);
        starts.push(0u32);
        for (tet, &owner) in layout.tet_cells.iter().enumerate() {
            // 要求单元依次出现、每个单元至少一个四面体：所属单元只能是当前单元或下一个。
            let (owner, current) = (owner as usize, starts.len() - 1);
            if owner == current + 1 {
                starts.push(tet as u32);
            } else if owner != current {
                return Err(invalid());
            }
        }
        if starts.len() != count {
            return Err(invalid());
        }
        starts.push(source.tet_count() as u32);
        Ok(Self {
            layout: Some(layout),
            starts,
            count,
        })
    }

    fn tets(&self, cell: usize) -> Range<usize> {
        if self.layout.is_some() {
            self.starts[cell] as usize..self.starts[cell + 1] as usize
        } else {
            cell..cell + 1
        }
    }
}

impl TetSource for crate::SyntheticTetBlock {
    fn node_count(&self) -> usize {
        crate::SyntheticTetBlock::node_count(self)
    }
    fn tet_count(&self) -> usize {
        crate::SyntheticTetBlock::tet_count(self)
    }
    fn node(&self, index: usize) -> [f64; 3] {
        crate::SyntheticTetBlock::node(self, index)
    }
    fn tet(&self, index: usize) -> Tetrahedron {
        crate::SyntheticTetBlock::tet(self, index)
    }
    fn boundary_count(&self) -> usize {
        crate::SyntheticTetBlock::boundary_count(self)
    }
    fn boundary_face(&self, index: usize) -> SurfaceTriangle {
        crate::SyntheticTetBlock::boundary_face(self, index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkBuildOptions {
    /// 每块原始单元数上限；k-d 中位数二分，实际块大小在上限的一半到上限之间。
    pub max_chunk_cells: usize,
    /// 简化外表面相对完整外表面的面数比例分母。
    pub simplify_ratio: usize,
}

impl Default for ChunkBuildOptions {
    fn default() -> Self {
        Self {
            max_chunk_cells: 1 << 18,
            simplify_ratio: 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkBuildSummary {
    pub chunk_count: usize,
    pub tet_count: u64,
    pub node_count: u64,
    /// 原始单元数（纯四面体等于 `tet_count`）。
    pub cell_count: u64,
    pub bytes_written: u64,
    /// 下层写调用次数（经 8 MiB 缓冲合并后）。
    pub write_calls: u64,
}

/// 写缓冲大小：MiB 级顺序写，避免 Windows 上的小写开销。
const WRITE_BUFFER: usize = 8 << 20;

#[derive(Clone, Copy)]
struct Item {
    centroid: [f32; 3],
    id: u32,
}

/// 计数写入器：统计下层写调用次数与字节数。
struct Counting<W> {
    inner: W,
    calls: u64,
    bytes: u64,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.calls += 1;
        self.bytes += written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

struct Encoded {
    header: ChunkHeader,
    cells: Vec<u8>,
    surface: Vec<u8>,
    index: LeafIndex,
}

/// 写场段时需要的块内全局下标（构建期间常驻，约 4 B / 块内节点 + 4 B / 单元 + 16 B / 外表面三角形）。
#[derive(Default)]
struct LeafIndex {
    nodes: Vec<u32>,
    cells: Vec<u32>,
    boundary_nodes: Vec<[u32; 3]>,
    boundary_owners: Vec<u32>,
}

/// 场段每批编码的块数：限制同时驻留的压缩场块。
const FIELD_BATCH: usize = 64;

/// 把 `source` 分块写入 `sink`。取消返回 `Ok(None)`；已写出的部分由调用方丢弃（写临时文件再改名）。
pub fn build_chunked_volume<W: Write>(
    source: &dyn TetSource,
    options: ChunkBuildOptions,
    sink: W,
    progress: &crate::Progress,
) -> Result<Option<ChunkBuildSummary>, ChunkError> {
    build_chunked_volume_with_fields(source, &mut NoFields, options, sink, progress)
}

/// 同 [`build_chunked_volume`]，并把 `fields` 的每个结果场按块写入场段（task 029）。
pub fn build_chunked_volume_with_fields<W: Write>(
    source: &dyn TetSource,
    fields: &mut dyn FieldProvider,
    options: ChunkBuildOptions,
    sink: W,
    progress: &crate::Progress,
) -> Result<Option<ChunkBuildSummary>, ChunkError> {
    let infos = fields.infos();
    for info in &infos {
        if info.components != 1 && info.components != 3 {
            return Err(ChunkError::Invalid(format!(
                "field {} has {} components",
                info.name, info.components
            )));
        }
    }
    let cells_end = if infos.is_empty() { 0.95 } else { 0.8 };
    let tet_count = source.tet_count();
    if tet_count == 0 || tet_count > u32::MAX as usize || source.node_count() > u32::MAX as usize {
        return Err(ChunkError::Invalid(format!(
            "tet count {tet_count} outside 1..=u32::MAX"
        )));
    }
    if options.max_chunk_cells == 0 || options.simplify_ratio == 0 {
        return Err(ChunkError::Invalid(
            "chunk options must be positive".to_owned(),
        ));
    }
    let cells = CellMap::new(source)?;
    let mut items = centroids(source, &cells);
    progress.set_fraction(0.1);
    let mut leaves = Vec::new();
    split(&mut items, 0, options.max_chunk_cells, 0, &mut leaves);
    leaves.sort_unstable_by_key(|range| range.0);
    let leaves = morton_order(&items, leaves);
    progress.set_fraction(0.2);
    if progress.is_cancelled() {
        return Ok(None);
    }
    let boundary = BoundaryIndex::new(source);

    let mut out = std::io::BufWriter::with_capacity(
        WRITE_BUFFER,
        Counting {
            inner: sink,
            calls: 0,
            bytes: 0,
        },
    );
    let mut head = Writer::default();
    head.0.extend_from_slice(HEAD_MAGIC);
    head.u32(FORMAT_VERSION);
    head.u32(0);
    out.write_all(&head.0)?;
    let mut offset = HEAD_LEN;

    let next = AtomicUsize::new(0);
    let cancelled = AtomicBool::new(false);
    let first_error = Mutex::new(None);
    let items = &items;
    let leaves = &leaves;
    let workers = worker_count();
    let mut headers = Vec::with_capacity(leaves.len());
    let mut surfaces = Vec::with_capacity(leaves.len());
    let mut indices = Vec::with_capacity(leaves.len());
    std::thread::scope(|scope| -> Result<(), ChunkError> {
        let (sender, receiver) = mpsc::sync_channel::<(usize, Encoded)>(workers * 2);
        for _ in 0..workers {
            let sender = sender.clone();
            let (next, cancelled, first_error, boundary, cells) =
                (&next, &cancelled, &first_error, &boundary, &cells);
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= leaves.len() || cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    let (start, end) = leaves[index];
                    match encode_leaf(source, cells, boundary, &items[start..end], options) {
                        Ok(encoded) => {
                            if sender.send((index, encoded)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            if let Ok(mut slot) = first_error.lock() {
                                slot.get_or_insert(error);
                            }
                            cancelled.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            });
        }
        drop(sender);
        // 按 Morton 顺序写出：乱序到达的块暂存，等前序块写完。
        let mut pending = BTreeMap::new();
        let mut written = 0;
        for (index, encoded) in receiver {
            pending.insert(index, encoded);
            while let Some(encoded) = pending.remove(&written) {
                let mut header = encoded.header;
                header.cells_offset = offset;
                header.cells_len = encoded.cells.len() as u32;
                if let Err(error) = out.write_all(&encoded.cells) {
                    cancelled.store(true, Ordering::Relaxed);
                    return Err(error.into());
                }
                offset += encoded.cells.len() as u64;
                headers.push(header);
                surfaces.push(encoded.surface);
                indices.push(encoded.index);
                written += 1;
                progress.report(0.2, cells_end, written as u64, leaves.len() as u64);
                if progress.is_cancelled() {
                    cancelled.store(true, Ordering::Relaxed);
                }
            }
        }
        Ok(())
    })?;
    if let Some(error) = first_error.into_inner().ok().flatten() {
        return Err(error);
    }
    if cancelled.load(Ordering::Relaxed) || headers.len() != leaves.len() {
        return Ok(None);
    }

    for (field_index, info) in infos.iter().enumerate() {
        let field = fields.load(field_index)?;
        check_field(&field, info, source, cells.count)?;
        for header in &mut headers {
            header.field_ranges.push([f64::NAN; 2]);
            header.field_blocks.push(FieldBlock::default());
        }
        for boundary in [false, true] {
            for start in (0..indices.len()).step_by(FIELD_BATCH) {
                let end = (start + FIELD_BATCH).min(indices.len());
                let encoded = parallel_map(&indices[start..end], |index| {
                    encode_field_block(&field, index, boundary)
                });
                for (chunk, item) in (start..end).zip(encoded) {
                    let (raw_len, bytes, range) = item?;
                    let block = &mut headers[chunk].field_blocks[field_index];
                    let len = u32::try_from(bytes.len())
                        .map_err(|_| ChunkError::Invalid("field block too large".to_owned()))?;
                    if boundary {
                        block.boundary_offset = offset;
                        block.boundary_len = len;
                        block.boundary_raw_len = raw_len;
                    } else {
                        block.values_offset = offset;
                        block.values_len = len;
                        block.values_raw_len = raw_len;
                        headers[chunk].field_ranges[field_index] = range;
                    }
                    out.write_all(&bytes)?;
                    offset += bytes.len() as u64;
                }
                if progress.is_cancelled() {
                    return Ok(None);
                }
            }
        }
        progress.report(0.8, 0.95, field_index as u64 + 1, infos.len() as u64);
    }

    let surface_offset = offset;
    let mut relative = 0u64;
    for (header, surface) in headers.iter_mut().zip(&surfaces) {
        header.surface_offset = relative;
        header.surface_len = surface.len() as u32;
        out.write_all(surface)?;
        relative += surface.len() as u64;
    }
    let table = ChunkTable {
        tet_count: tet_count as u64,
        node_count: source.node_count() as u64,
        cell_count: cells.count as u64,
        fields: infos,
        surface_offset,
        surface_len: relative,
        chunks: headers,
    };
    let raw_table = encode_table(&table);
    let table_bytes = compress(&raw_table)?;
    let table_offset = surface_offset + relative;
    out.write_all(&table_bytes)?;
    out.write_all(&encode_tail(
        table_offset,
        table_bytes.len() as u64,
        raw_table.len() as u64,
    ))?;
    out.flush()?;
    let counting = out
        .into_inner()
        .map_err(|error| ChunkError::Io(error.error().to_string()))?;
    progress.set_fraction(1.0);
    Ok(Some(ChunkBuildSummary {
        chunk_count: table.chunks.len(),
        tet_count: table.tet_count,
        node_count: table.node_count,
        cell_count: table.cell_count,
        bytes_written: counting.bytes,
        write_calls: counting.calls,
    }))
}

/// 从体网格资产构建分块文件（task 028 派生缓存）：节点、单元、边界各顺序读一遍，
/// 驻留紧凑数组（约 20 B / 单元 + 24 B / 节点；混合资产另加约 4 B / 拆分四面体 +
/// 37 B / 原始单元的布局）后分块。峰值约为单元数 × 40 B 加节点数组，
/// 远小于完整驻留（约 190 B / 单元）；完全不驻留的构建属于候选 task“超大体文件流式导入”。
/// 进度：读取占 0..0.3，分块占 0.3..1（使用调用方进度的默认窗口）。取消返回 `Ok(None)`。
pub fn build_chunked_from_asset<W: Write>(
    asset: &mut crate::VolumeAssetChunks,
    options: ChunkBuildOptions,
    sink: W,
    progress: &crate::Progress,
) -> Result<Option<ChunkBuildSummary>, ChunkError> {
    build_chunked_from_asset_with_fields(asset, &mut NoFields, options, sink, progress)
}

/// 同 [`build_chunked_from_asset`]，并写入结果场（通常是 `.presult` 读取器，逐个场读入）。
pub fn build_chunked_from_asset_with_fields<W: Write>(
    asset: &mut crate::VolumeAssetChunks,
    fields: &mut dyn FieldProvider,
    options: ChunkBuildOptions,
    sink: W,
    progress: &crate::Progress,
) -> Result<Option<ChunkBuildSummary>, ChunkError> {
    let asset_error = |error: crate::MeshAssetError| ChunkError::Format(error.to_string());
    let counts = asset.counts();
    let Some(nodes) = asset
        .read_nodes(Some((progress, 0.0, 0.05)))
        .map_err(asset_error)?
    else {
        return Ok(None);
    };
    let memory = || ChunkError::Invalid("not enough memory for the cell arrays".to_owned());
    let mut tets = Vec::new();
    let mut layout = None;
    if asset.is_mixed() {
        // 混合资产（task 030）：读原生单元流，按与驻留加载相同的规则确定性拆分，保留原始单元布局。
        let mut types = Vec::new();
        let mut regions = Vec::new();
        let mut connectivity = Vec::new();
        types
            .try_reserve_exact(counts.cells)
            .map_err(|_| memory())?;
        regions
            .try_reserve_exact(counts.cells)
            .map_err(|_| memory())?;
        let finished = asset
            .visit_cells(Some((progress, 0.05, 0.25)), |batch| {
                for (kind, region, nodes) in batch.iter().flatten() {
                    types.push(*kind);
                    regions.push(*region);
                    connectivity.extend_from_slice(&nodes[..kind.node_count()]);
                }
                !progress.is_cancelled()
            })
            .map_err(asset_error)?;
        if !finished {
            return Ok(None);
        }
        let invalid = |_| ChunkError::Format("mixed cell stream invalid".to_owned());
        let offsets = crate::cell_offsets(&types, connectivity.len()).map_err(invalid)?;
        if connectivity
            .iter()
            .any(|&node| node as usize >= nodes.len())
        {
            return Err(ChunkError::Format(
                "mixed cell node out of range".to_owned(),
            ));
        }
        let (decomposed, tet_cells) =
            crate::cells::decompose_cells(&types, &offsets, &connectivity, |cell| regions[cell])
                .map_err(invalid)?;
        if decomposed.len() != counts.tets {
            return Err(ChunkError::Format(
                "mixed cell tet count mismatch".to_owned(),
            ));
        }
        tets = decomposed;
        layout = Some(std::sync::Arc::new(CellLayout {
            types,
            offsets,
            connectivity,
            tet_cells,
        }));
    } else {
        tets.try_reserve_exact(counts.tets).map_err(|_| memory())?;
        let finished = asset
            .visit_tets(Some((progress, 0.05, 0.25)), |batch| {
                for chunk in batch {
                    tets.extend_from_slice(chunk);
                }
                !progress.is_cancelled()
            })
            .map_err(asset_error)?;
        if !finished {
            return Ok(None);
        }
    }
    let Some(boundary) = asset
        .read_boundary(Some((progress, 0.25, 0.3)))
        .map_err(asset_error)?
    else {
        return Ok(None);
    };
    let mesh = TetMesh {
        nodes,
        tets,
        boundary,
        region_count: counts.region_count,
        boundary_group_count: counts.boundary_group_count,
        cells: layout,
    };
    progress.set_window(0.3, 1.0);
    let result = build_chunked_volume_with_fields(&mesh, fields, options, sink, progress);
    progress.set_window(0.0, 1.0);
    result
}

/// 各原始单元的节点平均质心（纯四面体即四面体质心）。
fn centroids(source: &dyn TetSource, cells: &CellMap<'_>) -> Vec<Item> {
    let count = cells.count;
    let mut items = vec![
        Item {
            centroid: [0.0; 3],
            id: 0
        };
        count
    ];
    let per = count.div_ceil(worker_count()).max(1);
    std::thread::scope(|scope| {
        for (part, slice) in items.chunks_mut(per).enumerate() {
            scope.spawn(move || {
                for (offset, item) in slice.iter_mut().enumerate() {
                    let id = part * per + offset;
                    let tet;
                    let nodes = match cells.layout {
                        Some(layout) => layout.cell_nodes(id),
                        None => {
                            tet = source.tet(id).nodes;
                            &tet[..]
                        }
                    };
                    let mut sum = [0.0f64; 3];
                    for &node in nodes {
                        let point = source.node(node as usize);
                        (0..3).for_each(|axis| sum[axis] += point[axis]);
                    }
                    let corners = nodes.len() as f64;
                    *item = Item {
                        centroid: sum.map(|value| (value / corners) as f32),
                        id: id as u32,
                    };
                }
            });
        }
    });
    items
}

/// k-d 中位数二分：沿质心包围盒最长轴切分，直到不超过 `limit`。顶层几层并行。
fn split(
    items: &mut [Item],
    base: usize,
    limit: usize,
    depth: u32,
    leaves: &mut Vec<(usize, usize)>,
) {
    if items.len() <= limit {
        leaves.push((base, base + items.len()));
        return;
    }
    let (low, high) = items.iter().fold(
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
        |(mut low, mut high), item| {
            for axis in 0..3 {
                low[axis] = low[axis].min(item.centroid[axis]);
                high[axis] = high[axis].max(item.centroid[axis]);
            }
            (low, high)
        },
    );
    let axis = (0..3)
        .max_by(|&a, &b| (high[a] - low[a]).total_cmp(&(high[b] - low[b])))
        .unwrap_or(0);
    let middle = items.len() / 2;
    items.select_nth_unstable_by(middle, |a, b| a.centroid[axis].total_cmp(&b.centroid[axis]));
    let (left, right) = items.split_at_mut(middle);
    if depth < 3 {
        let mut right_leaves = Vec::new();
        std::thread::scope(|scope| {
            scope.spawn(|| split(right, base + middle, limit, depth + 1, &mut right_leaves));
            split(left, base, limit, depth + 1, leaves);
        });
        leaves.extend(right_leaves);
    } else {
        split(left, base, limit, depth + 1, leaves);
        split(right, base + middle, limit, depth + 1, leaves);
    }
}

/// 块按质心包围盒中心的 Morton 码排序，使一次切面命中的块在文件中尽量连续。
fn morton_order(items: &[Item], leaves: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let centers: Vec<[f32; 3]> = leaves
        .iter()
        .map(|&(start, end)| {
            let slice = &items[start..end];
            let sum = slice.iter().fold([0.0f64; 3], |mut sum, item| {
                (0..3).for_each(|axis| sum[axis] += f64::from(item.centroid[axis]));
                sum
            });
            sum.map(|value| (value / slice.len() as f64) as f32)
        })
        .collect();
    let (low, high) = centers.iter().fold(
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
        |(mut low, mut high), center| {
            for axis in 0..3 {
                low[axis] = low[axis].min(center[axis]);
                high[axis] = high[axis].max(center[axis]);
            }
            (low, high)
        },
    );
    let mut keyed: Vec<(u64, (usize, usize))> = leaves
        .into_iter()
        .zip(&centers)
        .map(|(leaf, center)| {
            let cell = |axis: usize| {
                let span = (high[axis] - low[axis]).max(f32::MIN_POSITIVE);
                (((center[axis] - low[axis]) / span) * 2_097_151.0) as u64
            };
            (morton3(cell(0), cell(1), cell(2)), leaf)
        })
        .collect();
    keyed.sort_unstable_by_key(|&(key, _)| key);
    keyed.into_iter().map(|(_, leaf)| leaf).collect()
}

fn morton3(x: u64, y: u64, z: u64) -> u64 {
    let spread = |mut v: u64| {
        v &= 0x1f_ffff;
        v = (v | v << 32) & 0x1f_0000_0000_ffff;
        v = (v | v << 16) & 0x1f_0000_ff00_00ff;
        v = (v | v << 8) & 0x100f_00f0_0f00_f00f;
        v = (v | v << 4) & 0x10c3_0c30_c30c_30c3;
        (v | v << 2) & 0x1249_2492_4924_9249
    };
    spread(x) | spread(y) << 1 | spread(z) << 2
}

/// 全局边界面索引：边界节点位图预筛，排序后的三节点为键。
struct BoundaryIndex {
    nodes: Vec<u64>,
    faces: HashMap<[u32; 3], SurfaceTriangle>,
}

impl BoundaryIndex {
    fn new(source: &dyn TetSource) -> Self {
        let mut nodes = vec![0u64; source.node_count().div_ceil(64)];
        let mut faces = HashMap::with_capacity(source.boundary_count());
        for index in 0..source.boundary_count() {
            let face = source.boundary_face(index);
            for node in face.nodes {
                nodes[node as usize / 64] |= 1 << (node % 64);
            }
            faces.insert(sorted(face.nodes), face);
        }
        Self { nodes, faces }
    }

    fn is_boundary_node(&self, node: u32) -> bool {
        self.nodes[node as usize / 64] & (1 << (node % 64)) != 0
    }

    fn face(&self, nodes: [u32; 3]) -> Option<SurfaceTriangle> {
        if nodes.iter().all(|&node| self.is_boundary_node(node)) {
            self.faces.get(&sorted(nodes)).copied()
        } else {
            None
        }
    }
}

fn sorted(mut nodes: [u32; 3]) -> [u32; 3] {
    nodes.sort_unstable();
    nodes
}

const TET_FACES: [[usize; 3]; 4] = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]];

fn encode_leaf(
    source: &dyn TetSource,
    cells: &CellMap<'_>,
    boundary: &BoundaryIndex,
    items: &[Item],
    options: ChunkBuildOptions,
) -> Result<Encoded, ChunkError> {
    // `ids` 是块内原始单元（升序）；`owners` 是每个拆分四面体所属的原始单元。
    let mut ids: Vec<u32> = items.iter().map(|item| item.id).collect();
    ids.sort_unstable();
    let mut tets = Vec::with_capacity(ids.len());
    let mut owners = Vec::with_capacity(ids.len());
    for &id in &ids {
        for tet in cells.tets(id as usize) {
            tets.push(source.tet(tet));
            owners.push(id);
        }
    }
    if u32::try_from(tets.len()).is_err() {
        return Err(ChunkError::Invalid("chunk too large".to_owned()));
    }
    let mut global_nodes: Vec<u32> = tets.iter().flat_map(|tet| tet.nodes).collect();
    global_nodes.sort_unstable();
    global_nodes.dedup();
    let nodes: Vec<[f64; 3]> = global_nodes
        .iter()
        .map(|&node| source.node(node as usize))
        .collect();
    let local = |node: u32| {
        global_nodes
            .binary_search(&node)
            .map_or(0, |index| index as u32)
    };
    let local_tets: Vec<Tetrahedron> = tets
        .iter()
        .map(|tet| Tetrahedron {
            nodes: tet.nodes.map(local),
            region: tet.region,
        })
        .collect();

    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for point in &nodes {
        for axis in 0..3 {
            bounds[0][axis] = bounds[0][axis].min(point[axis]);
            bounds[1][axis] = bounds[1][axis].max(point[axis]);
        }
    }
    let mut header = ChunkHeader::empty(bounds, tets.len() as u32, nodes.len() as u32);
    header.cell_count = ids.len() as u32;
    mark_voxels(&mut header, &nodes, &local_tets);

    let mut surface = ChunkSurface {
        boundary: Vec::new(),
        groups: Vec::new(),
        simplified: SurfaceMesh {
            triangles: Vec::new(),
        },
    };
    let mut index = LeafIndex::default();
    for (tet, &id) in tets.iter().zip(&owners) {
        for face in TET_FACES {
            if let Some(triangle) = boundary.face(face.map(|corner| tet.nodes[corner])) {
                surface
                    .boundary
                    .push(triangle.nodes.map(|node| source.node(node as usize)));
                surface.groups.push(triangle.group);
                index.boundary_nodes.push(triangle.nodes);
                index.boundary_owners.push(id);
            }
        }
    }
    if !surface.boundary.is_empty() {
        let full = SurfaceMesh {
            triangles: surface.boundary.clone(),
        };
        let budget = (surface.boundary.len() / options.simplify_ratio).max(16);
        surface.simplified = if surface.boundary.len() <= budget {
            full
        } else {
            crate::simplify_surface(&full, budget).0
        };
    }

    let layout = cells.layout.map(|layout| {
        let mut chunk = ChunkCellLayout {
            types: Vec::with_capacity(ids.len()),
            offsets: Vec::with_capacity(ids.len() + 1),
            connectivity: Vec::new(),
            tet_cells: Vec::with_capacity(tets.len()),
        };
        chunk.offsets.push(0);
        for (local_cell, &id) in ids.iter().enumerate() {
            chunk.types.push(layout.types[id as usize]);
            chunk.connectivity.extend(
                layout
                    .cell_nodes(id as usize)
                    .iter()
                    .map(|&node| local(node)),
            );
            chunk.offsets.push(chunk.connectivity.len() as u32);
            let tet_count = cells.tets(id as usize).len();
            chunk
                .tet_cells
                .extend(std::iter::repeat_n(local_cell as u32, tet_count));
        }
        chunk
    });
    let chunk_cells = ChunkCells {
        global_nodes: global_nodes.iter().map(|&node| u64::from(node)).collect(),
        nodes,
        tets: local_tets,
        global_cells: owners.iter().map(|&id| u64::from(id)).collect(),
        layout,
    };
    let raw_cells = encode_cells(&chunk_cells);
    index.nodes = global_nodes;
    index.cells = ids;
    let raw_surface = encode_surface(&surface);
    header.cells_raw_len = u32::try_from(raw_cells.len())
        .map_err(|_| ChunkError::Invalid("chunk too large".to_owned()))?;
    header.surface_raw_len = u32::try_from(raw_surface.len())
        .map_err(|_| ChunkError::Invalid("chunk surface too large".to_owned()))?;
    Ok(Encoded {
        header,
        cells: compress(&raw_cells)?,
        surface: compress(&raw_surface)?,
        index,
    })
}

fn check_field(
    field: &ResultField,
    info: &ChunkFieldInfo,
    source: &dyn TetSource,
    cell_count: usize,
) -> Result<(), ChunkError> {
    let tuples = match info.association {
        FieldAssociation::Node => source.node_count(),
        FieldAssociation::Cell => cell_count,
    };
    if field_info(field) != *info || field.values.len() != tuples * usize::from(info.components) {
        return Err(ChunkError::Invalid(format!(
            "field {} does not match the mesh",
            info.name
        )));
    }
    Ok(())
}

/// 一块一个场的块内值（`boundary == false`，同时返回值区间）或外表面值，返回（原始长度，压缩字节，区间）。
fn encode_field_block(
    field: &ResultField,
    index: &LeafIndex,
    boundary: bool,
) -> Result<(u32, Vec<u8>, [f64; 2]), ChunkError> {
    let components = usize::from(field.components);
    let mut out = Writer::default();
    let mut push = |id: u32| {
        let start = id as usize * components;
        field.values[start..start + components]
            .iter()
            .for_each(|&value| out.f64(value));
    };
    let mut range = [f64::NAN; 2];
    match (boundary, field.association) {
        (false, association) => {
            let ids = match association {
                FieldAssociation::Node => &index.nodes,
                FieldAssociation::Cell => &index.cells,
            };
            for &id in ids {
                push(id);
                let key = range_key(
                    tuple(&field.values, components, id as usize),
                    field.components,
                );
                if key.is_finite() {
                    range = if range[0].is_nan() {
                        [key, key]
                    } else {
                        [range[0].min(key), range[1].max(key)]
                    };
                }
            }
        }
        (true, FieldAssociation::Node) => {
            index
                .boundary_nodes
                .iter()
                .flatten()
                .for_each(|&id| push(id));
        }
        (true, FieldAssociation::Cell) => {
            index.boundary_owners.iter().for_each(|&id| push(id));
        }
    }
    let raw_len = u32::try_from(out.0.len())
        .map_err(|_| ChunkError::Invalid("field block too large".to_owned()))?;
    Ok((raw_len, compress(&out.0)?, range))
}

/// 标记每个单元包围盒覆盖的体素。
fn mark_voxels(header: &mut ChunkHeader, nodes: &[[f64; 3]], tets: &[Tetrahedron]) {
    let [low, high] = header.bounds;
    let size = VOXELS_PER_AXIS as f64;
    let cell = |value: f64, axis: usize| {
        let span = high[axis] - low[axis];
        if span <= 0.0 {
            0
        } else {
            (((value - low[axis]) / span * size) as usize).min(VOXELS_PER_AXIS - 1)
        }
    };
    for tet in tets {
        let mut range = [[usize::MAX, 0]; 3];
        for node in tet.nodes {
            let point = nodes[node as usize];
            for axis in 0..3 {
                let index = cell(point[axis], axis);
                range[axis][0] = range[axis][0].min(index);
                range[axis][1] = range[axis][1].max(index);
            }
        }
        for k in range[2][0]..=range[2][1] {
            for j in range[1][0]..=range[1][1] {
                for i in range[0][0]..=range[0][1] {
                    header.mark_voxel((k * VOXELS_PER_AXIS + j) * VOXELS_PER_AXIS + i);
                }
            }
        }
    }
}
