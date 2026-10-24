//! 分块构建（task 027）：k-d 空间划分 → Morton 排序块 → 并行编码 → 单文件大缓冲顺序写。

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use super::format::{
    ChunkCells, ChunkHeader, ChunkSurface, ChunkTable, FORMAT_VERSION, HEAD_LEN, HEAD_MAGIC,
    VOXELS_PER_AXIS, Writer, compress, encode_cells, encode_surface, encode_table, encode_tail,
};
use super::{ChunkError, worker_count};
use crate::{SurfaceMesh, SurfaceTriangle, TetMesh, Tetrahedron};

/// 随机访问的四面体源：分块构建按下标取节点、单元与边界面，不要求整体驻留。
pub trait TetSource: Sync {
    fn node_count(&self) -> usize;
    fn tet_count(&self) -> usize;
    fn node(&self, index: usize) -> [f64; 3];
    fn tet(&self, index: usize) -> Tetrahedron;
    fn boundary_count(&self) -> usize;
    fn boundary_face(&self, index: usize) -> SurfaceTriangle;
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
    /// 每块单元数上限；k-d 中位数二分，实际块大小在上限的一半到上限之间。
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
}

/// 把 `source` 分块写入 `sink`。取消返回 `Ok(None)`；已写出的部分由调用方丢弃（写临时文件再改名）。
pub fn build_chunked_volume<W: Write>(
    source: &dyn TetSource,
    options: ChunkBuildOptions,
    sink: W,
    progress: &crate::Progress,
) -> Result<Option<ChunkBuildSummary>, ChunkError> {
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
    let mut items = centroids(source);
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
    std::thread::scope(|scope| -> Result<(), ChunkError> {
        let (sender, receiver) = mpsc::sync_channel::<(usize, Encoded)>(workers * 2);
        for _ in 0..workers {
            let sender = sender.clone();
            let (next, cancelled, first_error, boundary) =
                (&next, &cancelled, &first_error, &boundary);
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= leaves.len() || cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    let (start, end) = leaves[index];
                    match encode_leaf(source, boundary, &items[start..end], options) {
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
                written += 1;
                progress.report(0.2, 0.95, written as u64, leaves.len() as u64);
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
        field_count: 0,
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
        bytes_written: counting.bytes,
        write_calls: counting.calls,
    }))
}

/// 从 v3 体网格资产构建分块文件（task 028 派生缓存）：节点、单元、边界各顺序读一遍，
/// 驻留紧凑数组（约 20 B / 单元 + 24 B / 节点）后分块。峰值约为单元数 × 40 B 加节点数组，
/// 远小于完整驻留（约 190 B / 单元）；完全不驻留的构建属于候选 task“超大体文件流式导入”。
/// 进度：读取占 0..0.3，分块占 0.3..1（使用调用方进度的默认窗口）。取消返回 `Ok(None)`。
pub fn build_chunked_from_asset<W: Write>(
    asset: &mut crate::VolumeAssetChunks,
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
    let mut tets = Vec::new();
    tets.try_reserve_exact(counts.tets)
        .map_err(|_| ChunkError::Invalid("not enough memory for the tet array".to_owned()))?;
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
    };
    progress.set_window(0.3, 1.0);
    let result = build_chunked_volume(&mesh, options, sink, progress);
    progress.set_window(0.0, 1.0);
    result
}

fn centroids(source: &dyn TetSource) -> Vec<Item> {
    let count = source.tet_count();
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
                    let tet = source.tet(id);
                    let mut sum = [0.0f64; 3];
                    for node in tet.nodes {
                        let point = source.node(node as usize);
                        (0..3).for_each(|axis| sum[axis] += point[axis]);
                    }
                    *item = Item {
                        centroid: sum.map(|value| (value / 4.0) as f32),
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
    boundary: &BoundaryIndex,
    items: &[Item],
    options: ChunkBuildOptions,
) -> Result<Encoded, ChunkError> {
    let mut ids: Vec<u32> = items.iter().map(|item| item.id).collect();
    ids.sort_unstable();
    let tets: Vec<Tetrahedron> = ids.iter().map(|&id| source.tet(id as usize)).collect();
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
    mark_voxels(&mut header, &nodes, &local_tets);

    let mut surface = ChunkSurface {
        boundary: Vec::new(),
        groups: Vec::new(),
        simplified: SurfaceMesh {
            triangles: Vec::new(),
        },
    };
    for tet in &tets {
        for face in TET_FACES {
            if let Some(triangle) = boundary.face(face.map(|corner| tet.nodes[corner])) {
                surface
                    .boundary
                    .push(triangle.nodes.map(|node| source.node(node as usize)));
                surface.groups.push(triangle.group);
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

    let cells = ChunkCells {
        global_nodes: global_nodes.iter().map(|&node| u64::from(node)).collect(),
        nodes,
        tets: local_tets,
        global_cells: ids.iter().map(|&id| u64::from(id)).collect(),
    };
    let raw_cells = encode_cells(&cells);
    let raw_surface = encode_surface(&surface);
    header.cells_raw_len = u32::try_from(raw_cells.len())
        .map_err(|_| ChunkError::Invalid("chunk too large".to_owned()))?;
    header.surface_raw_len = u32::try_from(raw_surface.len())
        .map_err(|_| ChunkError::Invalid("chunk surface too large".to_owned()))?;
    Ok(Encoded {
        header,
        cells: compress(&raw_cells)?,
        surface: compress(&raw_surface)?,
    })
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
