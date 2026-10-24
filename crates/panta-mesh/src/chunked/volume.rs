//! 分块体网格的切面与质量统计（task 027）。

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::ChunkError;
use super::cache::{ChunkCache, parallel_map, read_merged};
use super::format::{
    ChunkFieldInfo, ChunkHeader, ChunkReadMode, ChunkSource, ChunkSurface, IoCounts,
    VOXELS_PER_AXIS, boundary_tuple_count, decode_f64s, decompress, open_chunk_source, read_index,
};
use crate::field_section::{FieldOutput, clip_with, section_polygon_with, tuple};
use crate::volume::{QualityPartial, clip_triangle, dot, fan, order_around, section_polygon, sub};
use crate::{CellType, SurfaceMesh, TetQuality};
use crate::{FieldAssociation, FieldComponent, FieldSection};

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

#[derive(Default)]
struct ResultCache {
    entries: HashMap<(usize, [u64; 6]), Arc<Vec<Triangle>>>,
    order: VecDeque<(usize, [u64; 6])>,
    bytes: usize,
}

/// 切面结果缓存上限（按 FIFO 淘汰）。
const RESULT_BUDGET: usize = 256 << 20;

/// 单个场按块解码的外表面值（只常驻一个场）。
type BoundaryValues = Arc<Vec<Vec<f64>>>;

/// 打开的分块体网格：块表与表面常驻，单元经 [`ChunkCache`] 按需读取。
pub struct ChunkedVolume {
    source: Arc<dyn ChunkSource>,
    headers: Arc<Vec<ChunkHeader>>,
    surfaces: Vec<ChunkSurface>,
    tet_count: u64,
    node_count: u64,
    cache: Arc<ChunkCache>,
    results: Mutex<ResultCache>,
    quality: Mutex<Vec<Option<QualityPartial>>>,
    fields: Vec<ChunkFieldInfo>,
    /// 最近选中场的各块外表面值（只留一个场，切换场时整段重读一次）。
    boundary_values: Mutex<Option<(usize, BoundaryValues)>>,
}

impl std::fmt::Debug for ChunkedVolume {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChunkedVolume")
            .field("chunks", &self.headers.len())
            .field("tets", &self.tet_count())
            .field("reader", &self.reader_kind())
            .finish_non_exhaustive()
    }
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
            fields: table.fields,
            boundary_values: Mutex::new(None),
        })
    }

    pub fn tet_count(&self) -> u64 {
        self.tet_count
    }

    pub fn node_count(&self) -> u64 {
        self.node_count
    }

    /// 文件级结果场表（task 029），与 `ChunkHeader::field_ranges` 同序。
    pub fn fields(&self) -> &[ChunkFieldInfo] {
        &self.fields
    }

    /// 按名称与关联查找场下标。
    pub fn field_index(&self, name: &str, association: FieldAssociation) -> Option<usize> {
        self.fields
            .iter()
            .position(|field| field.name == name && field.association == association)
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

    /// 与 [`crate::section_plane_origin`] 同义的平面：在节点沿 `normal` 投影区间的 `fraction` 处。
    /// 节点投影的极值必在外表面顶点上取得（单元都在外表面围成的闭区域内），
    /// 因此只扫描常驻的外表面即可，不读单元；与驻留 / 流式切面的位置逐位一致。
    pub fn plane_through_nodes(
        &self,
        normal: [f64; 3],
        fraction: f64,
    ) -> Result<SectionPlane, ChunkError> {
        let probe = SectionPlane::new([0.0; 3], normal)?;
        if !fraction.is_finite() {
            return Err(ChunkError::Invalid(
                "section fraction is not finite".to_owned(),
            ));
        }
        let unit = probe.normal;
        let (low, high) = self
            .surfaces
            .iter()
            .flat_map(|surface| surface.boundary.iter().flatten())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), point| {
                let offset = dot(*point, unit);
                (low.min(offset), high.max(offset))
            });
        if !low.is_finite() || !high.is_finite() {
            return self.plane_at(normal, fraction);
        }
        let offset = low + (high - low) * fraction.clamp(0.0, 1.0);
        SectionPlane::new(unit.map(|value| value * offset), unit)
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

    /// 带结果场值的精细切面（task 029）：几何与 [`Self::fine_section`] 逐位一致、三角形顺序相同。
    /// 选中场的外表面值整段一次读入并常驻（只留最近一个场）；命中块的块内值与单元一样合并读取，
    /// 不进块缓存（只在本次切面中使用）。取消返回 `Ok(None)`。
    pub fn fine_section_field(
        &self,
        plane: &SectionPlane,
        field: usize,
        component: FieldComponent,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<FieldSection>, ChunkError> {
        let info = self
            .fields
            .get(field)
            .ok_or_else(|| ChunkError::Invalid(format!("field {field} out of range")))?
            .clone();
        component
            .validate(info.components)
            .map_err(ChunkError::Invalid)?;
        let components = usize::from(info.components);
        let boundary = self.boundary_values(field)?;
        if cancelled() {
            return Ok(None);
        }
        let kept = self.kept_chunks(plane);
        let parts = parallel_map(&kept, |&id| {
            let mut out = FieldOutput::default();
            let surface = &self.surfaces[id];
            let values = &boundary[id];
            for (index, triangle) in surface.boundary.iter().enumerate() {
                let corner = |slot: usize| match info.association {
                    FieldAssociation::Node => tuple(values, components, index * 3 + slot),
                    FieldAssociation::Cell => tuple(values, components, index),
                };
                clip_with(
                    std::array::from_fn(|slot| {
                        let point = triangle[slot];
                        (point, plane.distance(point), corner(slot))
                    }),
                    &mut out,
                );
            }
            out
        });
        let mut output = FieldOutput::default();
        for part in parts {
            output.append(part);
        }
        let hits = self.hit_chunks(plane);
        for batch in self.budget_batches(&hits) {
            let cells = self.cache.get_many(batch)?;
            if cancelled() {
                return Ok(None);
            }
            let values = self.read_field_values(batch, field)?;
            if cancelled() {
                return Ok(None);
            }
            let jobs: Vec<usize> = (0..batch.len()).collect();
            let computed = parallel_map(&jobs, |&slot| {
                let (cells, values) = (&cells[slot], &values[slot]);
                let distance: Vec<f64> = cells.nodes.iter().map(|&p| plane.distance(p)).collect();
                let mut out = FieldOutput::default();
                for (index, tet) in cells.tets.iter().enumerate() {
                    match info.association {
                        FieldAssociation::Node => section_polygon_with(
                            &cells.nodes,
                            &distance,
                            tet.nodes,
                            plane.normal,
                            |node| tuple(values, components, node as usize),
                            &mut out,
                        ),
                        FieldAssociation::Cell => {
                            let value = tuple(values, components, index);
                            section_polygon_with(
                                &cells.nodes,
                                &distance,
                                tet.nodes,
                                plane.normal,
                                |_| value,
                                &mut out,
                            );
                        }
                    }
                }
                out
            });
            for part in computed {
                output.append(part);
            }
        }
        Ok(Some(output.finish(info.components, component)))
    }

    /// 选中场的各块外表面值：所有块的外表面值区段在文件中连续，一次读入后并行解压。
    fn boundary_values(&self, field: usize) -> Result<BoundaryValues, ChunkError> {
        let mut slot = self
            .boundary_values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((cached, values)) = slot.as_ref()
            && *cached == field
        {
            return Ok(values.clone());
        }
        let info = &self.fields[field];
        let ranges: Vec<(u64, u64)> = self
            .headers
            .iter()
            .map(|header| header.field_blocks[field].boundary_range())
            .collect();
        let values = read_merged(self.source.as_ref(), &ranges, |id, bytes| {
            let block = &self.headers[id].field_blocks[field];
            let values = decode_f64s(&decompress(bytes, block.boundary_raw_len as usize)?)?;
            let expected = boundary_tuple_count(self.surfaces[id].boundary.len(), info.association)
                * usize::from(info.components);
            if values.len() != expected {
                return Err(ChunkError::Format(
                    "field boundary count mismatch".to_owned(),
                ));
            }
            Ok(values)
        })?;
        let values = Arc::new(values);
        *slot = Some((field, values.clone()));
        Ok(values)
    }

    /// 读取若干块某个场的块内值（合并读）。
    fn read_field_values(&self, ids: &[usize], field: usize) -> Result<Vec<Vec<f64>>, ChunkError> {
        let ranges: Vec<(u64, u64)> = ids
            .iter()
            .map(|&id| self.headers[id].field_blocks[field].values_range())
            .collect();
        read_merged(self.source.as_ref(), &ranges, |slot, bytes| {
            let block = &self.headers[ids[slot]].field_blocks[field];
            decode_f64s(&decompress(bytes, block.values_raw_len as usize)?)
        })
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
            // .pchunk v2 只承载纯四面体网格（混合单元在 task 030 PR 3 前不建派生文件）。
            let computed = parallel_map(&cells, |cells| {
                let mut quality = QualityPartial::default();
                for tet in &cells.tets {
                    quality.add_cell(CellType::Tetra, &tet.nodes, &cells.nodes);
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
        let mut total = QualityPartial::default();
        for quality in done.iter().flatten() {
            total.merge(quality);
        }
        Ok(Some(total.finish()))
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
