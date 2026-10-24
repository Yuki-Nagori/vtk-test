//! 分块体网格的切面与质量统计（task 027）。

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::ChunkError;
use super::cache::{ChunkCache, parallel_map};
use super::format::{
    ChunkHeader, ChunkReadMode, ChunkSource, ChunkSurface, IoCounts, VOXELS_PER_AXIS,
    open_chunk_source, read_index,
};
use crate::volume::{clip_triangle, dot, fan, order_around, section_polygon, single_quality, sub};
use crate::{POOR_ASPECT_RATIO, POOR_MIN_DIHEDRAL_DEGREES, SurfaceMesh, TetQuality};

type Triangle = [[f64; 3]; 3];

/// 切面平面：`dot(p - origin, normal) <= 0` 一侧保留；`normal` 已单位化。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionPlane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
}

impl SectionPlane {
    /// 与 [`crate::section_surface`] 相同的单位化，保证结果逐位一致。
    pub fn new(origin: [f64; 3], normal: [f64; 3]) -> Result<Self, ChunkError> {
        let length = dot(normal, normal).sqrt();
        if !length.is_finite() || length == 0.0 || origin.iter().any(|value| !value.is_finite()) {
            return Err(ChunkError::Invalid(
                "section plane is degenerate".to_owned(),
            ));
        }
        Ok(Self {
            origin,
            normal: normal.map(|value| value / length),
        })
    }

    fn distance(&self, point: [f64; 3]) -> f64 {
        dot(sub(point, self.origin), self.normal)
    }

    /// 包围盒 8 个角点的距离区间，按包围盒尺度放宽，避免舍入导致漏块。
    fn span(&self, bounds: &[[f64; 3]; 2]) -> (f64, f64) {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for corner in 0..8 {
            let point = std::array::from_fn(|axis| bounds[(corner >> axis) & 1][axis]);
            let distance = self.distance(point);
            low = low.min(distance);
            high = high.max(distance);
        }
        let margin = 1e-9 * (high - low).abs().max(1.0);
        (low - margin, high + margin)
    }

    fn key(&self) -> [u64; 6] {
        let [a, b, c] = self.origin.map(f64::to_bits);
        let [d, e, f] = self.normal.map(f64::to_bits);
        [a, b, c, d, e, f]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ChunkQuality {
    count: usize,
    min_dihedral: f64,
    max_aspect: f64,
    aspect_sum: f64,
    poor: usize,
}

#[derive(Default)]
struct ResultCache {
    entries: HashMap<(usize, [u64; 6]), Arc<Vec<Triangle>>>,
    order: VecDeque<(usize, [u64; 6])>,
    bytes: usize,
}

/// 切面结果缓存上限（按 FIFO 淘汰）。
const RESULT_BUDGET: usize = 256 << 20;

/// 打开的分块体网格：块表与表面常驻，单元经 [`ChunkCache`] 按需读取。
pub struct ChunkedVolume {
    source: Arc<dyn ChunkSource>,
    headers: Arc<Vec<ChunkHeader>>,
    surfaces: Vec<ChunkSurface>,
    tet_count: u64,
    node_count: u64,
    cache: Arc<ChunkCache>,
    results: Mutex<ResultCache>,
    quality: Mutex<Vec<Option<ChunkQuality>>>,
}

impl ChunkedVolume {
    /// 按默认实现打开：优先只读映射，映射不可用时回退合并定位读。
    pub fn open_file(path: &Path, cache_budget: usize) -> Result<Self, ChunkError> {
        Self::open_file_with(path, cache_budget, ChunkReadMode::Auto)
    }

    pub fn open_file_with(
        path: &Path,
        cache_budget: usize,
        mode: ChunkReadMode,
    ) -> Result<Self, ChunkError> {
        Self::open(open_chunk_source(path, mode)?, cache_budget)
    }

    pub fn open(source: Arc<dyn ChunkSource>, cache_budget: usize) -> Result<Self, ChunkError> {
        let (table, surfaces) = read_index(source.as_ref())?;
        let headers = Arc::new(table.chunks);
        let cache = Arc::new(ChunkCache::new(
            source.clone(),
            headers.clone(),
            cache_budget,
        ));
        let chunk_count = headers.len();
        Ok(Self {
            source,
            headers,
            surfaces,
            tet_count: table.tet_count,
            node_count: table.node_count,
            cache,
            results: Mutex::new(ResultCache::default()),
            quality: Mutex::new(vec![None; chunk_count]),
        })
    }

    pub fn tet_count(&self) -> u64 {
        self.tet_count
    }

    pub fn node_count(&self) -> u64 {
        self.node_count
    }

    pub fn headers(&self) -> &[ChunkHeader] {
        &self.headers
    }

    pub fn cache(&self) -> &Arc<ChunkCache> {
        &self.cache
    }

    pub fn io_counts(&self) -> IoCounts {
        self.source.io_counts()
    }

    /// 实际使用的块读取实现（`mmap` / `pread`）。
    pub fn reader_kind(&self) -> &'static str {
        self.source.kind()
    }

    /// 常驻层（块表 + 表面）字节数估算。
    pub fn resident_index_bytes(&self) -> usize {
        let headers = self.headers.len() * (std::mem::size_of::<ChunkHeader>() + 512);
        let surfaces: usize = self
            .surfaces
            .iter()
            .map(|surface| surface.boundary.len() * 76 + surface.simplified.triangles.len() * 72)
            .sum();
        headers + surfaces
    }

    /// 全局包围盒。
    pub fn bounds(&self) -> [[f64; 3]; 2] {
        self.headers.iter().fold(
            [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
            |[low, high], header| {
                [
                    std::array::from_fn(|axis| low[axis].min(header.bounds[0][axis])),
                    std::array::from_fn(|axis| high[axis].max(header.bounds[1][axis])),
                ]
            },
        )
    }

    /// 沿 `normal` 在全局包围盒投影区间的 `fraction` 处取平面。
    pub fn plane_at(&self, normal: [f64; 3], fraction: f64) -> Result<SectionPlane, ChunkError> {
        let probe = SectionPlane::new([0.0; 3], normal)?;
        let (low, high) = probe.span(&self.bounds());
        let offset = low + (high - low) * fraction.clamp(0.0, 1.0);
        SectionPlane::new(probe.normal.map(|value| value * offset), probe.normal)
    }

    /// 与平面相交的块（包围盒跨越平面）。
    pub fn hit_chunks(&self, plane: &SectionPlane) -> Vec<usize> {
        (0..self.headers.len())
            .filter(|&id| {
                let (low, high) = plane.span(&self.headers[id].bounds);
                low <= 0.0 && high > 0.0
            })
            .collect()
    }

    fn kept_chunks(&self, plane: &SectionPlane) -> Vec<usize> {
        (0..self.headers.len())
            .filter(|&id| plane.span(&self.headers[id].bounds).0 <= 0.0)
            .collect()
    }

    /// 粗级别：简化外表面裁剪 + 命中块占据体素与平面的交，只用常驻数据，不读单元。
    pub fn coarse_section(&self, plane: &SectionPlane) -> SurfaceMesh {
        let kept = self.kept_chunks(plane);
        let parts = parallel_map(&kept, |&id| {
            let mut out = Vec::new();
            for triangle in &self.surfaces[id].simplified.triangles {
                clip(plane, triangle, &mut out);
            }
            let header = &self.headers[id];
            let (low, high) = plane.span(&header.bounds);
            if low <= 0.0 && high > 0.0 {
                voxel_section(plane, header, &mut out);
            }
            out
        });
        SurfaceMesh {
            triangles: parts.into_iter().flatten().collect(),
        }
    }

    /// 精细级别：完整外表面裁剪 + 命中块全量单元截面。命中块中未缓存的一次合并读取。
    /// `cancelled` 在读取前后查询；取消返回 `Ok(None)`。
    pub fn fine_section(
        &self,
        plane: &SectionPlane,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<SurfaceMesh>, ChunkError> {
        let hits = self.hit_chunks(plane);
        let key = plane.key();
        let mut ready: HashMap<usize, Arc<Vec<Triangle>>> = HashMap::new();
        if let Ok(results) = self.results.lock() {
            for &id in &hits {
                if let Some(result) = results.entries.get(&(id, key)) {
                    ready.insert(id, result.clone());
                }
            }
        }
        let missing: Vec<usize> = hits
            .iter()
            .copied()
            .filter(|id| !ready.contains_key(id))
            .collect();
        if cancelled() {
            return Ok(None);
        }
        // 按缓存预算一半分批取块，避免一次命中块过多时瞬时解压量越过预算。
        for batch in self.budget_batches(&missing) {
            let cells = self.cache.get_many(batch)?;
            if cancelled() {
                return Ok(None);
            }
            let pairs: Vec<(usize, &Arc<crate::chunked::ChunkCells>)> =
                batch.iter().copied().zip(cells.iter()).collect();
            let computed = parallel_map(&pairs, |(_, cells)| {
                let distance: Vec<f64> = cells.nodes.iter().map(|&p| plane.distance(p)).collect();
                let mut out = Vec::new();
                for tet in &cells.tets {
                    section_polygon(&cells.nodes, &distance, tet, plane.normal, &mut out);
                }
                Arc::new(out)
            });
            let mut results = self
                .results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (&id, result) in batch.iter().zip(computed) {
                let bytes = result.len() * std::mem::size_of::<Triangle>();
                results.bytes += bytes;
                results.entries.insert((id, key), result.clone());
                results.order.push_back((id, key));
                ready.insert(id, result);
            }
            while results.bytes > RESULT_BUDGET {
                let Some(old) = results.order.pop_front() else {
                    break;
                };
                if let Some(entry) = results.entries.remove(&old) {
                    results.bytes -= entry.len() * std::mem::size_of::<Triangle>();
                }
            }
        }
        let kept = self.kept_chunks(plane);
        let mut parts = parallel_map(&kept, |&id| {
            let mut out = Vec::new();
            for triangle in &self.surfaces[id].boundary {
                clip(plane, triangle, &mut out);
            }
            out
        });
        for id in hits {
            if let Some(result) = ready.get(&id) {
                parts.push(result.as_ref().clone());
            }
        }
        Ok(Some(SurfaceMesh {
            triangles: parts.into_iter().flatten().collect(),
        }))
    }

    /// 把块号切成若干批，每批解压后大小（按 `cells_raw_len` 估计）不超过缓存预算的一半；单块超限时独占一批。
    fn budget_batches<'a>(&self, ids: &'a [usize]) -> Vec<&'a [usize]> {
        let batch_bytes = (self.cache.budget() / 2).max(1);
        let mut batches = Vec::new();
        let mut start = 0;
        while start < ids.len() {
            let mut end = start;
            let mut bytes = 0usize;
            while end < ids.len() && (end == start || bytes < batch_bytes) {
                bytes += self.headers[ids[end]].cells_raw_len as usize;
                end += 1;
            }
            batches.push(&ids[start..end]);
            start = end;
        }
        batches
    }

    /// 质量统计：按块并行，每块结果缓存（与平面无关，只算一次）。分批读取，每批不超过缓存预算的一半。
    pub fn quality(
        &self,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<TetQuality>, ChunkError> {
        let todo: Vec<usize> = {
            let done = self
                .quality
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (0..self.headers.len())
                .filter(|&id| done[id].is_none())
                .collect()
        };
        for ids in self.budget_batches(&todo) {
            if cancelled() {
                return Ok(None);
            }
            let cells = self.cache.get_many(ids)?;
            let computed = parallel_map(&cells, |cells| {
                let mut quality = ChunkQuality {
                    count: cells.tets.len(),
                    min_dihedral: f64::INFINITY,
                    max_aspect: 0.0,
                    aspect_sum: 0.0,
                    poor: 0,
                };
                for tet in &cells.tets {
                    let (dihedral, aspect) =
                        single_quality(tet.nodes.map(|node| cells.nodes[node as usize]));
                    quality.min_dihedral = quality.min_dihedral.min(dihedral);
                    quality.max_aspect = quality.max_aspect.max(aspect);
                    quality.aspect_sum += aspect;
                    if dihedral < POOR_MIN_DIHEDRAL_DEGREES || aspect > POOR_ASPECT_RATIO {
                        quality.poor += 1;
                    }
                }
                quality
            });
            let mut done = self
                .quality
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (&id, quality) in ids.iter().zip(computed) {
                done[id] = Some(quality);
            }
        }
        let done = self
            .quality
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut total = TetQuality {
            tet_count: 0,
            min_dihedral_degrees: f64::INFINITY,
            max_aspect_ratio: 0.0,
            mean_aspect_ratio: 0.0,
            poor_count: 0,
        };
        let mut sum = 0.0;
        for quality in done.iter().flatten() {
            total.tet_count += quality.count;
            total.min_dihedral_degrees = total.min_dihedral_degrees.min(quality.min_dihedral);
            total.max_aspect_ratio = total.max_aspect_ratio.max(quality.max_aspect);
            total.poor_count += quality.poor;
            sum += quality.aspect_sum;
        }
        if total.tet_count == 0 {
            total.min_dihedral_degrees = 0.0;
        } else {
            total.mean_aspect_ratio = sum / total.tet_count as f64;
        }
        Ok(Some(total))
    }
}

fn clip(plane: &SectionPlane, triangle: &Triangle, out: &mut Vec<Triangle>) {
    let polygon = clip_triangle(triangle.map(|point| (point, plane.distance(point))));
    fan(&polygon, out);
}

/// 命中块内与平面相交的占据体素：每个体素盒与平面的交多边形。
fn voxel_section(plane: &SectionPlane, header: &ChunkHeader, out: &mut Vec<Triangle>) {
    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7),
        (0, 2),
        (1, 3),
        (4, 6),
        (5, 7),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    let [low, high] = header.bounds;
    let size = VOXELS_PER_AXIS as f64;
    let step: [f64; 3] = std::array::from_fn(|axis| (high[axis] - low[axis]) / size);
    for k in 0..VOXELS_PER_AXIS {
        for j in 0..VOXELS_PER_AXIS {
            for i in 0..VOXELS_PER_AXIS {
                if !header.voxel_occupied((k * VOXELS_PER_AXIS + j) * VOXELS_PER_AXIS + i) {
                    continue;
                }
                let index = [i, j, k];
                let corners: [[f64; 3]; 8] = std::array::from_fn(|corner| {
                    std::array::from_fn(|axis| {
                        low[axis] + step[axis] * (index[axis] + ((corner >> axis) & 1)) as f64
                    })
                });
                let distance = corners.map(|point| plane.distance(point));
                let kept = distance.iter().filter(|&&value| value <= 0.0).count();
                if kept == 0 || kept == 8 {
                    continue;
                }
                let mut points = Vec::with_capacity(6);
                for (a, b) in EDGES {
                    if (distance[a] <= 0.0) != (distance[b] <= 0.0) {
                        let t = distance[a] / (distance[a] - distance[b]);
                        points.push(std::array::from_fn(|axis| {
                            corners[a][axis] + (corners[b][axis] - corners[a][axis]) * t
                        }));
                    }
                }
                order_around(&mut points, plane.normal);
                fan(&points, out);
            }
        }
    }
}
