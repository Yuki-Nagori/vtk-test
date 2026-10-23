//! 大网格显示用 LOD（task 019）：基于均匀网格的顶点聚类简化。
//!
//! 算法：把包围盒切成边长相同的立方格，每个顶点归入所在格，格的代表点取格内顶点
//! 平均值；三个顶点落在三个不同格的三角面保留为“格三角面”，按旋转规范化去重
//! （保持绕序即法向）。内存只与输出规模（被占用的格与格三角面）相关，与输入
//! 规模无关，因此可以对按块读取的超大资产做两遍流式扫描（包围盒 → 聚类）。
//! 简化结果用于显示与拾取，不用于诊断或导出。

use crate::asset::{SurfaceAssetChunks, Triangle};
use crate::{MeshAssetError, Progress, SurfaceMesh};
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::path::Path;

/// 显示 LOD 默认面数预算：约 200 万面，主流 GPU 可流畅交互。
pub const DEFAULT_LOD_TRIANGLE_BUDGET: usize = 2_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceLod {
    pub mesh: SurfaceMesh,
    /// 源资产三角面数。
    pub source_triangle_count: u64,
    pub bounds_min: [f64; 3],
    pub bounds_max: [f64; 3],
    /// 最终聚类网格每轴格数；0 表示未简化（源面数不超过预算）。
    pub grid_resolution: u32,
}

impl SurfaceLod {
    pub fn is_simplified(&self) -> bool {
        self.grid_resolution > 0
    }
}

/// 轻量乘法哈希：格键已是均匀分布的整数，不需要抗碰撞攻击的 SipHash。
#[derive(Default)]
struct CellHasher(u64);

impl Hasher for CellHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 =
                (self.0.rotate_left(5) ^ u64::from(*byte)).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    }
    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<CellHasher>>;
type FastSet<K> = HashSet<K, BuildHasherDefault<CellHasher>>;

#[derive(Clone, Copy)]
struct Grid {
    origin: [f64; 3],
    inverse_cell: f64,
    resolution: u32,
}

impl Grid {
    fn new(bounds_min: [f64; 3], bounds_max: [f64; 3], resolution: u32) -> Self {
        let extent = (0..3)
            .map(|axis| bounds_max[axis] - bounds_min[axis])
            .fold(0.0_f64, f64::max)
            .max(f64::MIN_POSITIVE);
        Self {
            origin: bounds_min,
            inverse_cell: f64::from(resolution) / extent,
            resolution,
        }
    }

    fn key(&self, point: &[f64; 3]) -> u64 {
        let limit = self.resolution - 1;
        let mut key = 0_u64;
        for (axis, (value, origin)) in point.iter().zip(self.origin).enumerate() {
            let cell = ((value - origin) * self.inverse_cell).floor();
            let cell = if cell.is_nan() {
                0
            } else {
                (cell.max(0.0) as u32).min(limit)
            };
            key |= u64::from(cell) << (21 * axis);
        }
        key
    }
}

#[derive(Default)]
struct Clusters {
    cells: FastMap<u64, ([f64; 3], u64)>,
    triangles: FastSet<[u64; 3]>,
}

impl Clusters {
    fn add(&mut self, grid: &Grid, triangles: &[Triangle]) {
        for triangle in triangles {
            let keys = [0, 1, 2].map(|corner| grid.key(&triangle[corner]));
            for (corner, key) in keys.iter().enumerate() {
                let entry = self.cells.entry(*key).or_insert(([0.0; 3], 0));
                for (sum, value) in entry.0.iter_mut().zip(triangle[corner]) {
                    *sum += value;
                }
                entry.1 += 1;
            }
            if keys[0] != keys[1] && keys[1] != keys[2] && keys[0] != keys[2] {
                // 旋转到最小键在首位，保持绕序。
                let first = (0..3).min_by_key(|index| keys[*index]).unwrap_or(0);
                self.triangles
                    .insert([keys[first], keys[(first + 1) % 3], keys[(first + 2) % 3]]);
            }
        }
    }

    fn merge(&mut self, other: Self) {
        for (key, (sum, count)) in other.cells {
            let entry = self.cells.entry(key).or_insert(([0.0; 3], 0));
            for (total, value) in entry.0.iter_mut().zip(sum) {
                *total += value;
            }
            entry.1 += count;
        }
        self.triangles.extend(other.triangles);
    }

    fn into_mesh(self) -> SurfaceMesh {
        let representative = |key: &u64| {
            let (sum, count) = self.cells[key];
            let count = count.max(1) as f64;
            [sum[0] / count, sum[1] / count, sum[2] / count]
        };
        let mut keys: Vec<[u64; 3]> = self.triangles.iter().copied().collect();
        keys.sort_unstable();
        SurfaceMesh {
            triangles: keys
                .iter()
                .map(|triangle| triangle.map(|key| representative(&key)))
                .collect(),
        }
    }
}

fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
}

/// 并行聚类一批块：每块独立累积，随后串行合并。
fn cluster_batch(grid: &Grid, target: &mut Clusters, batch: &[Vec<Triangle>]) {
    let partials: Vec<Clusters> = std::thread::scope(|scope| {
        let handles: Vec<_> = batch
            .chunks(batch.len().div_ceil(worker_count()).max(1))
            .map(|group| {
                scope.spawn(move || {
                    let mut local = Clusters::default();
                    for chunk in group {
                        local.add(grid, chunk);
                    }
                    local
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect()
    });
    for partial in partials {
        target.merge(partial);
    }
}

fn initial_resolution(budget: usize) -> u32 {
    // 闭合曲面的格三角面约为 2–6 × 分辨率²，取保守系数 3。
    ((budget as f64 / 3.0).sqrt().floor() as u32).clamp(4, (1 << 21) - 1)
}

/// 对内存中的网格做聚类，直到面数不超过预算。
pub fn simplify_surface(mesh: &SurfaceMesh, budget: usize) -> (SurfaceMesh, u32) {
    simplify_from(mesh, budget, initial_resolution(budget))
}

fn simplify_from(mesh: &SurfaceMesh, budget: usize, start: u32) -> (SurfaceMesh, u32) {
    let (bounds_min, bounds_max) = bounds_of(&mesh.triangles);
    let mut resolution = start;
    loop {
        let grid = Grid::new(bounds_min, bounds_max, resolution);
        let mut clusters = Clusters::default();
        let groups: Vec<Vec<Triangle>> = mesh
            .triangles
            .chunks(1 << 16)
            .map(<[Triangle]>::to_vec)
            .collect();
        cluster_batch(&grid, &mut clusters, &groups);
        let count = clusters.triangles.len();
        if count <= budget || resolution <= 4 {
            return (clusters.into_mesh(), resolution);
        }
        resolution = shrink(resolution, count, budget);
    }
}

fn shrink(resolution: u32, count: usize, budget: usize) -> u32 {
    let factor = (budget as f64 / count as f64).sqrt() * 0.95;
    ((f64::from(resolution) * factor).floor() as u32).clamp(4, resolution - 1)
}

fn bounds_of(triangles: &[Triangle]) -> ([f64; 3], [f64; 3]) {
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for point in triangles.iter().flatten() {
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(point[axis]);
            maximum[axis] = maximum[axis].max(point[axis]);
        }
    }
    (minimum, maximum)
}

/// 从 v3 表面资产流式构建显示 LOD：源面数不超过 `budget` 时原样读取全部面；
/// 否则两遍扫描（包围盒 0–30%，聚类 30–95%），内存与输出规模成正比。
/// 取消时返回 `Ok(None)`。
pub fn build_surface_lod(
    path: &Path,
    budget: usize,
    progress: Option<&Progress>,
) -> Result<Option<SurfaceLod>, MeshAssetError> {
    let budget = budget.max(16);
    let mut chunks = SurfaceAssetChunks::open(path)?;
    let source_triangle_count = chunks.triangle_count();
    if source_triangle_count <= budget as u64 {
        let mut triangles = Vec::with_capacity(source_triangle_count as usize);
        let finished = chunks.visit_batches(progress.map(|p| (p, 0.0, 0.95)), |_, batch| {
            for chunk in batch {
                triangles.extend_from_slice(chunk);
            }
        })?;
        if !finished {
            return Ok(None);
        }
        let (bounds_min, bounds_max) = bounds_of(&triangles);
        return Ok(Some(SurfaceLod {
            mesh: SurfaceMesh { triangles },
            source_triangle_count,
            bounds_min,
            bounds_max,
            grid_resolution: 0,
        }));
    }
    let mut bounds_min = [f64::INFINITY; 3];
    let mut bounds_max = [f64::NEG_INFINITY; 3];
    let finished = chunks.visit_batches(progress.map(|p| (p, 0.0, 0.3)), |_, batch| {
        for chunk in batch {
            let (minimum, maximum) = bounds_of(chunk);
            for axis in 0..3 {
                bounds_min[axis] = bounds_min[axis].min(minimum[axis]);
                bounds_max[axis] = bounds_max[axis].max(maximum[axis]);
            }
        }
    })?;
    if !finished {
        return Ok(None);
    }
    let resolution = initial_resolution(budget);
    let grid = Grid::new(bounds_min, bounds_max, resolution);
    let mut clusters = Clusters::default();
    let finished = chunks.visit_batches(progress.map(|p| (p, 0.3, 0.95)), |_, batch| {
        cluster_batch(&grid, &mut clusters, batch);
    })?;
    if !finished {
        return Ok(None);
    }
    let count = clusters.triangles.len();
    let mut mesh = clusters.into_mesh();
    let mut grid_resolution = resolution;
    if count > budget {
        // 首遍超出预算时在已简化结果上再聚类（规模已小，内存中完成）。
        let (smaller, final_resolution) =
            simplify_from(&mesh, budget, shrink(resolution, count, budget));
        mesh = smaller;
        grid_resolution = final_resolution;
    }
    if let Some(progress) = progress {
        progress.set_fraction(1.0);
    }
    Ok(Some(SurfaceLod {
        mesh,
        source_triangle_count,
        bounds_min,
        bounds_max,
        grid_resolution,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MeshAsset, SyntheticTorus, decode_mesh_asset_with_digest, encode_mesh_asset,
        write_surface_asset_streaming,
    };

    fn write_torus(path: &Path, count: usize) -> Result<(u64, String), Box<dyn std::error::Error>> {
        let torus = SyntheticTorus::new(count);
        let file = std::fs::File::create(path)?;
        write_surface_asset_streaming(file, count, |range, out| torus.fill(range, out), None)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "cancelled".into())
    }

    #[test]
    fn streaming_writer_matches_in_memory_encoder() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("panta-lod-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("stream.pmesh");
        // 超过单块记录数，覆盖多块与多批次。
        let count = 150_001;
        let (len, digest) = write_torus(&path, count)?;
        let bytes = std::fs::read(&path)?;
        assert_eq!(bytes.len() as u64, len);
        let (decoded, decoded_digest) = decode_mesh_asset_with_digest(&bytes)?;
        assert_eq!(decoded_digest, digest);
        let torus = SyntheticTorus::new(count);
        let mut expected = Vec::new();
        torus.fill(0..count, &mut expected);
        let expected = MeshAsset::Surface(SurfaceMesh {
            triangles: expected,
        });
        assert_eq!(decoded, expected);
        assert_eq!(encode_mesh_asset(&expected)?.1, digest);

        let mut chunks = SurfaceAssetChunks::open(&path)?;
        assert_eq!(chunks.triangle_count(), count as u64);
        assert!(chunks.chunk_count() > 2);
        let mut seen = 0_u64;
        assert!(chunks.visit_batches(None, |first, batch| {
            assert_eq!(first, seen);
            seen += batch.iter().map(|chunk| chunk.len() as u64).sum::<u64>();
        })?);
        assert_eq!(seen, count as u64);
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn lod_respects_budget_and_bounds_and_passes_small_meshes_through()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("panta-lod-b-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("big.pmesh");
        write_torus(&path, 400_000)?;
        let progress = Progress::new();
        let lod = build_surface_lod(&path, 20_000, Some(&progress))?.ok_or("cancelled")?;
        assert!(lod.is_simplified());
        assert_eq!(lod.source_triangle_count, 400_000);
        assert!(lod.mesh.triangles.len() <= 20_000);
        assert!(
            lod.mesh.triangles.len() > 2_000,
            "{}",
            lod.mesh.triangles.len()
        );
        assert_eq!(progress.fraction(), 1.0);
        // 代表点是格内平均，必然落在源包围盒内。
        for point in lod.mesh.triangles.iter().flatten() {
            for ((value, minimum), maximum) in point.iter().zip(lod.bounds_min).zip(lod.bounds_max)
            {
                assert!(*value >= minimum - 1e-9 && *value <= maximum + 1e-9);
            }
        }
        assert!(lod.bounds_max[0] > 125.0 && lod.bounds_min[0] < -125.0);

        let full = build_surface_lod(&path, 1_000_000, None)?.ok_or("cancelled")?;
        assert!(!full.is_simplified());
        assert_eq!(full.mesh.triangles.len(), 400_000);

        let cancelled = Progress::new();
        cancelled.cancel();
        assert!(build_surface_lod(&path, 20_000, Some(&cancelled))?.is_none());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn in_memory_simplification_is_deterministic() -> Result<(), Box<dyn std::error::Error>> {
        let torus = SyntheticTorus::new(50_000);
        let mut triangles = Vec::new();
        torus.fill(0..50_000, &mut triangles);
        let mesh = SurfaceMesh { triangles };
        let (first, resolution) = simplify_surface(&mesh, 5_000);
        let (second, _) = simplify_surface(&mesh, 5_000);
        assert_eq!(first, second);
        assert!(first.triangles.len() <= 5_000);
        assert!(resolution >= 4);
        Ok(())
    }
}
