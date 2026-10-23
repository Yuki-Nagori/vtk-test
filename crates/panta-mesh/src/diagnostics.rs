//! 表面网格只读质量诊断：顶点焊接、退化面、开放边界、非流形边与法向一致性。
//!
//! 诊断先按容差焊接三角形汤得到共享顶点，再基于无向边邻接统计拓扑问题；
//! 不修改输入网格，修复由后续 STL 修复管线负责。

use std::collections::HashMap;

use crate::{PROGRESS_STRIDE, Progress, SurfaceMesh};

/// 默认焊接容差：模型包围盒对角线长度乘以该相对比例。
pub const DEFAULT_RELATIVE_WELD_TOLERANCE: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiagnosticOptions {
    /// 相对包围盒对角线的焊接容差比例，必须是有限非负数。
    pub relative_weld_tolerance: f64,
}

impl Default for DiagnosticOptions {
    fn default() -> Self {
        Self {
            relative_weld_tolerance: DEFAULT_RELATIVE_WELD_TOLERANCE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticError {
    InvalidTolerance,
    /// 宿主经 [`Progress::cancel`] 请求取消。
    Cancelled,
}

impl std::fmt::Display for DiagnosticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DiagnosticError {}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceDiagnostics {
    /// 实际使用的绝对焊接容差，单位与来源坐标一致。
    pub weld_tolerance: f64,
    pub triangle_count: u64,
    /// 三角形汤中的顶点引用数（三角形数 × 3）。
    pub raw_vertex_count: u64,
    /// 按容差焊接后的唯一顶点数。
    pub welded_vertex_count: u64,
    /// 坐标不完全相同、但在容差内被焊接到已有顶点的顶点引用数。
    pub near_duplicate_vertex_count: u64,
    /// 焊接后顶点重合或面积在容差内为零的三角形数。
    pub degenerate_triangle_count: u64,
    /// 仅被一个有效三角形使用的边。
    pub boundary_edge_count: u64,
    /// 由边界边组成的连通开放边界（孔）数量。
    pub hole_count: u64,
    /// 被三个及以上有效三角形使用的边。
    pub non_manifold_edge_count: u64,
    /// 两个相邻三角形以相同方向使用的流形边，表示法向不一致。
    pub inconsistent_orientation_edge_count: u64,
}

impl SurfaceDiagnostics {
    pub fn is_clean(&self) -> bool {
        self.near_duplicate_vertex_count == 0
            && self.degenerate_triangle_count == 0
            && self.boundary_edge_count == 0
            && self.non_manifold_edge_count == 0
            && self.inconsistent_orientation_edge_count == 0
    }
}

/// 诊断三角形汤。额外内存与焊接后顶点数和边数成线性关系，不复制坐标三角形。
pub fn diagnose_surface(
    mesh: &SurfaceMesh,
    options: DiagnosticOptions,
) -> Result<SurfaceDiagnostics, DiagnosticError> {
    diagnose_surface_with_progress(mesh, options, &Progress::new())
}

/// 同 [`diagnose_surface`]，并在焊接（0–60%）与边统计（60–95%）阶段报告进度、
/// 每 [`PROGRESS_STRIDE`] 个三角形查询一次取消。
pub fn diagnose_surface_with_progress(
    mesh: &SurfaceMesh,
    options: DiagnosticOptions,
    progress: &Progress,
) -> Result<SurfaceDiagnostics, DiagnosticError> {
    let total = mesh.triangles.len() as u64;
    let checkpoint = |index: usize, start: f64, end: f64| {
        if index % PROGRESS_STRIDE == 0 {
            if progress.is_cancelled() {
                return Err(DiagnosticError::Cancelled);
            }
            progress.report(start, end, index as u64, total);
        }
        Ok(())
    };
    let relative = options.relative_weld_tolerance;
    if !relative.is_finite() || relative < 0.0 {
        return Err(DiagnosticError::InvalidTolerance);
    }
    let dimensions = mesh.summary().dimensions;
    let diagonal = dimensions
        .iter()
        .map(|size| size * size)
        .sum::<f64>()
        .sqrt();
    let tolerance = if mesh.triangles.is_empty() {
        0.0
    } else {
        diagonal * relative
    };

    let mut welder = Welder::new(tolerance);
    let mut indexed = Vec::with_capacity(mesh.triangles.len());
    for (index, triangle) in mesh.triangles.iter().enumerate() {
        checkpoint(index, 0.0, 0.6)?;
        indexed.push(triangle.map(|point| welder.insert(point)));
    }

    let mut degenerate = 0_u64;
    // 键为有序无向边，值为 (使用次数, 正向次数)；正向指 a<b 时按 a→b 使用。
    let mut edges: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
    for (index, (triangle, nodes)) in mesh.triangles.iter().zip(&indexed).enumerate() {
        checkpoint(index, 0.6, 0.95)?;
        if nodes[0] == nodes[1]
            || nodes[1] == nodes[2]
            || nodes[0] == nodes[2]
            || twice_area(triangle) <= tolerance * tolerance
        {
            degenerate += 1;
            continue;
        }
        for corner in 0..3 {
            let (from, to) = (nodes[corner], nodes[(corner + 1) % 3]);
            let key = (from.min(to), from.max(to));
            let entry = edges.entry(key).or_insert((0, 0));
            entry.0 += 1;
            if from < to {
                entry.1 += 1;
            }
        }
    }

    let mut boundary = 0_u64;
    let mut non_manifold = 0_u64;
    let mut inconsistent = 0_u64;
    let mut loops = DisjointSet::new(welder.points.len());
    let mut boundary_vertices = Vec::new();
    for (&(a, b), &(uses, forward)) in &edges {
        match uses {
            1 => {
                boundary += 1;
                loops.union(a, b);
                boundary_vertices.push(a);
            }
            2 if forward != 1 => inconsistent += 1,
            2 => {}
            _ => non_manifold += 1,
        }
    }
    let mut roots: Vec<u32> = boundary_vertices
        .into_iter()
        .map(|vertex| loops.find(vertex))
        .collect();
    roots.sort_unstable();
    roots.dedup();

    progress.set_fraction(1.0);
    Ok(SurfaceDiagnostics {
        weld_tolerance: tolerance,
        triangle_count: mesh.triangles.len() as u64,
        raw_vertex_count: mesh.triangles.len() as u64 * 3,
        welded_vertex_count: welder.points.len() as u64,
        near_duplicate_vertex_count: welder.near_duplicates,
        degenerate_triangle_count: degenerate,
        boundary_edge_count: boundary,
        hole_count: roots.len() as u64,
        non_manifold_edge_count: non_manifold,
        inconsistent_orientation_edge_count: inconsistent,
    })
}

fn twice_area(triangle: &[[f64; 3]; 3]) -> f64 {
    let u: [f64; 3] = std::array::from_fn(|axis| triangle[1][axis] - triangle[0][axis]);
    let v: [f64; 3] = std::array::from_fn(|axis| triangle[2][axis] - triangle[0][axis]);
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    cross.iter().map(|value| value * value).sum::<f64>().sqrt()
}

/// 均匀网格哈希焊接；单元边长等于容差，查询相邻 27 个单元，保证容差内的点必被找到。
struct Welder {
    tolerance: f64,
    cells: HashMap<[i64; 3], Vec<u32>>,
    points: Vec<[f64; 3]>,
    near_duplicates: u64,
}

impl Welder {
    fn new(tolerance: f64) -> Self {
        Self {
            tolerance,
            cells: HashMap::new(),
            points: Vec::new(),
            near_duplicates: 0,
        }
    }

    fn cell(&self, point: [f64; 3]) -> [i64; 3] {
        if self.tolerance > 0.0 {
            point.map(|value| (value / self.tolerance).floor() as i64)
        } else {
            // 零容差只合并完全相同的坐标；+0.0 与 -0.0 视为同一位置。
            point.map(|value| (value + 0.0).to_bits() as i64)
        }
    }

    fn insert(&mut self, point: [f64; 3]) -> u32 {
        let cell = self.cell(point);
        let radius = if self.tolerance > 0.0 { 1 } else { 0 };
        let limit = self.tolerance * self.tolerance;
        for dx in -radius..=radius {
            for dy in -radius..=radius {
                for dz in -radius..=radius {
                    let key = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                    let Some(candidates) = self.cells.get(&key) else {
                        continue;
                    };
                    for &index in candidates {
                        let existing = self.points[index as usize];
                        let distance: f64 = (0..3)
                            .map(|axis| (existing[axis] - point[axis]).powi(2))
                            .sum();
                        if distance <= limit {
                            if existing != point {
                                self.near_duplicates += 1;
                            }
                            return index;
                        }
                    }
                }
            }
        }
        let index = self.points.len() as u32;
        self.points.push(point);
        self.cells.entry(cell).or_default().push(index);
        index
    }
}

struct DisjointSet {
    parent: Vec<u32>,
}

impl DisjointSet {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size as u32).collect(),
        }
    }

    fn find(&mut self, mut node: u32) -> u32 {
        while self.parent[node as usize] != node {
            let grandparent = self.parent[self.parent[node as usize] as usize];
            self.parent[node as usize] = grandparent;
            node = grandparent;
        }
        node
    }

    fn union(&mut self, a: u32, b: u32) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent[a as usize] = b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticError, DiagnosticOptions, SurfaceDiagnostics, diagnose_surface};
    use crate::{Progress, SurfaceMesh, diagnose_surface_with_progress, parse_stl};

    #[test]
    fn progress_reaches_completion_and_cancel_stops_diagnosis() {
        let mesh = SurfaceMesh {
            triangles: vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        };
        let progress = Progress::new();
        assert!(diagnose_surface_with_progress(&mesh, DiagnosticOptions::default(), &progress).is_ok());
        assert_eq!(progress.fraction(), 1.0);
        let cancelled = Progress::new();
        cancelled.cancel();
        assert_eq!(
            diagnose_surface_with_progress(&mesh, DiagnosticOptions::default(), &cancelled),
            Err(DiagnosticError::Cancelled)
        );
    }

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/mesh");

    fn fixture(name: &str) -> TestResult<SurfaceMesh> {
        let bytes = std::fs::read(format!("{FIXTURES}/{name}"))?;
        Ok(parse_stl(&bytes)?)
    }

    fn diagnose(name: &str) -> TestResult<SurfaceDiagnostics> {
        Ok(diagnose_surface(
            &fixture(name)?,
            DiagnosticOptions::default(),
        )?)
    }

    #[test]
    fn closed_tetrahedron_reports_no_issues() -> TestResult {
        let report = diagnose("clean_tetrahedron.stl")?;
        assert!(report.is_clean(), "{report:?}");
        assert_eq!(report.welded_vertex_count, 4);
        assert_eq!(report.hole_count, 0);
        Ok(())
    }

    #[test]
    fn missing_face_is_reported_as_one_hole() -> TestResult {
        let report = diagnose("open_tetrahedron.stl")?;
        assert_eq!(report.boundary_edge_count, 3);
        assert_eq!(report.hole_count, 1);
        assert_eq!(report.non_manifold_edge_count, 0);
        Ok(())
    }

    #[test]
    fn flipped_face_is_reported_as_inconsistent_orientation() -> TestResult {
        let report = diagnose("flipped_tetrahedron.stl")?;
        assert_eq!(report.inconsistent_orientation_edge_count, 3);
        assert_eq!(report.boundary_edge_count, 0);
        Ok(())
    }

    #[test]
    fn three_faces_on_one_edge_are_non_manifold() -> TestResult {
        let report = diagnose("non_manifold_fins.stl")?;
        assert_eq!(report.non_manifold_edge_count, 1);
        Ok(())
    }

    #[test]
    fn degenerate_faces_are_excluded_from_edge_topology() -> TestResult {
        let report = diagnose("degenerate_faces.stl")?;
        assert_eq!(report.degenerate_triangle_count, 2);
        assert!(!report.is_clean());
        assert_eq!(report.non_manifold_edge_count, 0);
        assert_eq!(report.boundary_edge_count, 0);
        Ok(())
    }

    #[test]
    fn near_duplicate_vertices_weld_within_relative_tolerance_only() -> TestResult {
        let mesh = fixture("unwelded_tetrahedron.stl")?;
        let welded = diagnose_surface(&mesh, DiagnosticOptions::default())?;
        assert_eq!(welded.near_duplicate_vertex_count, 1);
        assert_eq!(welded.boundary_edge_count, 0);

        let strict = diagnose_surface(
            &mesh,
            DiagnosticOptions {
                relative_weld_tolerance: 0.0,
            },
        )?;
        assert_eq!(strict.near_duplicate_vertex_count, 0);
        assert_eq!(strict.welded_vertex_count, 5);
        assert!(strict.boundary_edge_count > 0);
        Ok(())
    }

    #[test]
    fn invalid_tolerances_are_rejected() -> TestResult {
        let mesh = fixture("clean_tetrahedron.stl")?;
        for relative in [-1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                diagnose_surface(
                    &mesh,
                    DiagnosticOptions {
                        relative_weld_tolerance: relative
                    }
                ),
                Err(DiagnosticError::InvalidTolerance)
            );
        }
        Ok(())
    }
}
