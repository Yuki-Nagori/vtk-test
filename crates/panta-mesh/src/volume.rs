//! 线性四面体体网格：由单元构建、边界面提取、单平面切面与质量统计（task 021）。
//!
//! 首期只支持线性四面体；切面只用于显示，不生成新的网格资产。

use std::collections::HashMap;
use std::fmt::{Display, Formatter};

use crate::{MeshIndex, SurfaceMesh, SurfaceTriangle, TetMesh, Tetrahedron, validate_tet_mesh};

/// 正有向四面体 `[a, b, c, d]` 的四个外法向面（局部节点序）。
const OUTWARD_FACES: [[usize; 3]; 4] = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]];
/// 质量报告阈值：仅统计“差单元”数量，不做修复。
pub const POOR_MIN_DIHEDRAL_DEGREES: f64 = 5.0;
pub const POOR_ASPECT_RATIO: f64 = 10.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeError(pub String);

impl Display for VolumeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for VolumeError {}

/// 由节点与线性四面体单元构建体网格：负有向单元交换节点 2/3 修正朝向，
/// 外表面取只被一个单元引用的面，单区域、单边界组，最后执行领域校验。
pub fn tet_mesh_from_cells(
    nodes: Vec<[f64; 3]>,
    cells: Vec<[MeshIndex; 4]>,
) -> Result<TetMesh, VolumeError> {
    let mut tets = Vec::with_capacity(cells.len());
    for (index, mut cell) in cells.into_iter().enumerate() {
        if cell.iter().any(|&node| node as usize >= nodes.len()) {
            return Err(VolumeError(format!(
                "tetrahedron {index} references node outside the node range"
            )));
        }
        if signed_volume6(&nodes, cell) < 0.0 {
            cell.swap(2, 3);
        }
        tets.push(Tetrahedron {
            nodes: cell,
            region: 0,
        });
    }
    let boundary = boundary_faces(&tets);
    let mesh = TetMesh {
        nodes,
        tets,
        boundary,
        region_count: 1,
        boundary_group_count: 1,
    };
    let report = validate_tet_mesh(&mesh);
    if !report.is_ok() {
        let shown: Vec<&str> = report.issues.iter().take(3).map(String::as_str).collect();
        return Err(VolumeError(format!(
            "invalid tetrahedral mesh ({} issues): {}",
            report.issues.len(),
            shown.join("; ")
        )));
    }
    Ok(mesh)
}

/// 只被一个四面体引用的面即外表面，按首次出现顺序输出、外法向。
pub fn boundary_faces(tets: &[Tetrahedron]) -> Vec<SurfaceTriangle> {
    let mut faces: HashMap<[MeshIndex; 3], (usize, [MeshIndex; 3])> = HashMap::new();
    let mut sequence = 0usize;
    for tet in tets {
        for local in OUTWARD_FACES {
            let ordered = local.map(|corner| tet.nodes[corner]);
            let mut key = ordered;
            key.sort_unstable();
            if faces.remove(&key).is_none() {
                faces.insert(key, (sequence, ordered));
            }
            sequence += 1;
        }
    }
    let mut open: Vec<(usize, [MeshIndex; 3])> = faces.into_values().collect();
    open.sort_unstable_by_key(|(sequence, _)| *sequence);
    open.into_iter()
        .map(|(_, nodes)| SurfaceTriangle { nodes, group: 0 })
        .collect()
}

/// 体网格外表面，供显示、拾取与 STL 另存。
pub fn boundary_surface(mesh: &TetMesh) -> SurfaceMesh {
    SurfaceMesh {
        triangles: mesh
            .boundary
            .iter()
            .map(|face| face.nodes.map(|node| mesh.nodes[node as usize]))
            .collect(),
    }
}

/// 单平面切面显示：保留 `dot(p - origin, normal) <= 0` 一侧的外表面（逐三角形裁剪），
/// 并加入与平面相交单元的截面多边形（法向沿 `normal`）。`normal` 不必单位化。
pub fn section_surface(
    mesh: &TetMesh,
    origin: [f64; 3],
    normal: [f64; 3],
) -> Result<SurfaceMesh, VolumeError> {
    let length = dot(normal, normal).sqrt();
    if !length.is_finite() || length == 0.0 || origin.iter().any(|value| !value.is_finite()) {
        return Err(VolumeError("section plane is degenerate".to_owned()));
    }
    let normal = normal.map(|value| value / length);
    let distance: Vec<f64> = mesh
        .nodes
        .iter()
        .map(|&point| dot(sub(point, origin), normal))
        .collect();
    let mut triangles = Vec::new();
    for face in &mesh.boundary {
        let polygon = clip_triangle(
            face.nodes
                .map(|node| (mesh.nodes[node as usize], distance[node as usize])),
        );
        fan(&polygon, &mut triangles);
    }
    for tet in &mesh.tets {
        let kept = tet
            .nodes
            .iter()
            .filter(|&&node| distance[node as usize] <= 0.0)
            .count();
        if kept == 0 || kept == 4 {
            continue;
        }
        let mut points = Vec::with_capacity(4);
        for i in 0..4 {
            for j in i + 1..4 {
                let (a, b) = (tet.nodes[i] as usize, tet.nodes[j] as usize);
                if (distance[a] <= 0.0) != (distance[b] <= 0.0) {
                    points.push(lerp_on_plane(
                        mesh.nodes[a],
                        distance[a],
                        mesh.nodes[b],
                        distance[b],
                    ));
                }
            }
        }
        order_around(&mut points, normal);
        fan(&points, &mut triangles);
    }
    Ok(SurfaceMesh { triangles })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TetQuality {
    pub tet_count: usize,
    /// 全网格最小二面角（度）。
    pub min_dihedral_degrees: f64,
    /// 纵横比 = 外接球半径 / (3 × 内切球半径)，正四面体为 1。
    pub max_aspect_ratio: f64,
    pub mean_aspect_ratio: f64,
    /// 最小二面角低于 [`POOR_MIN_DIHEDRAL_DEGREES`] 或纵横比高于 [`POOR_ASPECT_RATIO`] 的单元数。
    pub poor_count: usize,
}

/// 体网格质量统计；`on_progress` 收到 0..=1。
pub fn tet_quality(mesh: &TetMesh, on_progress: &dyn Fn(f64)) -> TetQuality {
    let mut quality = TetQuality {
        tet_count: mesh.tets.len(),
        min_dihedral_degrees: f64::INFINITY,
        max_aspect_ratio: 0.0,
        mean_aspect_ratio: 0.0,
        poor_count: 0,
    };
    let mut sum = 0.0;
    on_progress(0.0);
    for (index, tet) in mesh.tets.iter().enumerate() {
        let (dihedral, aspect) = single_quality(tet.nodes.map(|node| mesh.nodes[node as usize]));
        quality.min_dihedral_degrees = quality.min_dihedral_degrees.min(dihedral);
        quality.max_aspect_ratio = quality.max_aspect_ratio.max(aspect);
        sum += aspect;
        if dihedral < POOR_MIN_DIHEDRAL_DEGREES || aspect > POOR_ASPECT_RATIO {
            quality.poor_count += 1;
        }
        if (index + 1).is_multiple_of(crate::PROGRESS_STRIDE) {
            on_progress((index + 1) as f64 / mesh.tets.len() as f64);
        }
    }
    if mesh.tets.is_empty() {
        quality.min_dihedral_degrees = 0.0;
    } else {
        quality.mean_aspect_ratio = sum / mesh.tets.len() as f64;
    }
    on_progress(1.0);
    quality
}

/// 返回（最小二面角°，纵横比）；退化单元为（0，∞）。
fn single_quality(p: [[f64; 3]; 4]) -> (f64, f64) {
    let normals = OUTWARD_FACES.map(|[a, b, c]| cross(sub(p[b], p[a]), sub(p[c], p[a])));
    let areas = normals.map(|n| dot(n, n).sqrt() / 2.0);
    let (a, b, c) = (sub(p[1], p[0]), sub(p[2], p[0]), sub(p[3], p[0]));
    let volume6 = dot(a, cross(b, c));
    let area_sum: f64 = areas.iter().sum();
    if volume6.is_nan() || volume6 <= 0.0 || areas.contains(&0.0) {
        return (0.0, f64::INFINITY);
    }
    let mut min_dihedral = f64::INFINITY;
    for i in 0..4 {
        for j in i + 1..4 {
            let cosine = dot(normals[i], normals[j]) / (4.0 * areas[i] * areas[j]);
            let angle = std::f64::consts::PI - cosine.clamp(-1.0, 1.0).acos();
            min_dihedral = min_dihedral.min(angle.to_degrees());
        }
    }
    let center = scale(
        add(
            add(scale(cross(b, c), dot(a, a)), scale(cross(c, a), dot(b, b))),
            scale(cross(a, b), dot(c, c)),
        ),
        1.0 / (2.0 * volume6),
    );
    let circumradius = dot(center, center).sqrt();
    let inradius = (volume6 / 2.0) / area_sum;
    (min_dihedral, circumradius / (3.0 * inradius))
}

fn signed_volume6(nodes: &[[f64; 3]], cell: [MeshIndex; 4]) -> f64 {
    let [a, b, c, d] = cell.map(|node| nodes[node as usize]);
    dot(sub(b, a), cross(sub(c, a), sub(d, a)))
}

fn clip_triangle(corners: [([f64; 3], f64); 3]) -> Vec<[f64; 3]> {
    let mut output = Vec::with_capacity(4);
    for index in 0..3 {
        let (point, distance) = corners[index];
        let (next, next_distance) = corners[(index + 1) % 3];
        if distance <= 0.0 {
            output.push(point);
        }
        if (distance <= 0.0) != (next_distance <= 0.0) {
            output.push(lerp_on_plane(point, distance, next, next_distance));
        }
    }
    output
}

fn lerp_on_plane(a: [f64; 3], da: f64, b: [f64; 3], db: f64) -> [f64; 3] {
    let t = da / (da - db);
    std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t)
}

/// 截面点按绕 `normal` 的逆时针排列，使扇形三角形法向沿 `normal`。
fn order_around(points: &mut [[f64; 3]], normal: [f64; 3]) {
    if points.len() < 3 {
        return;
    }
    let count = points.len() as f64;
    let center = points
        .iter()
        .fold([0.0; 3], |sum, &point| add(sum, point))
        .map(|value| value / count);
    let helper = if normal[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = cross(normal, helper);
    let v = cross(normal, u);
    points.sort_by(|&left, &right| {
        let angle = |point: [f64; 3]| {
            let offset = sub(point, center);
            dot(offset, v).atan2(dot(offset, u))
        };
        angle(left).total_cmp(&angle(right))
    });
    let first = cross(sub(points[1], points[0]), sub(points[2], points[0]));
    if dot(first, normal) < 0.0 {
        points.reverse();
    }
}

fn fan(polygon: &[[f64; 3]], output: &mut Vec<[[f64; 3]; 3]>) {
    for index in 1..polygon.len().saturating_sub(1) {
        let triangle = [polygon[0], polygon[index], polygon[index + 1]];
        let area = cross(sub(triangle[1], triangle[0]), sub(triangle[2], triangle[0]));
        if dot(area, area) > 0.0 {
            output.push(triangle);
        }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|axis| a[axis] - b[axis])
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|axis| a[axis] + b[axis])
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    a.map(|value| value * factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// 单位立方体剖分为 5 个四面体（含一个负有向单元以覆盖朝向修正）。
    fn cube() -> Result<TetMesh, VolumeError> {
        let nodes = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let cells = vec![
            [0, 1, 3, 4],
            [1, 2, 3, 6],
            [1, 6, 5, 4],
            [3, 4, 6, 7],
            [1, 3, 4, 6],
        ];
        tet_mesh_from_cells(nodes, cells)
    }

    fn signed_volume(surface: &SurfaceMesh) -> f64 {
        surface
            .triangles
            .iter()
            .map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0)
            .sum()
    }

    #[test]
    fn cube_boundary_is_closed_outward_and_reoriented() -> TestResult {
        let mesh = cube()?;
        assert_eq!(mesh.boundary.len(), 12);
        assert!((validate_tet_mesh(&mesh).volume_mm3 - 1.0).abs() < 1e-12);
        assert!((signed_volume(&boundary_surface(&mesh)) - 1.0).abs() < 1e-12);
        Ok(())
    }

    #[test]
    fn section_closes_the_kept_half() -> TestResult {
        let mesh = cube()?;
        let section = section_surface(&mesh, [0.5, 0.5, 0.3], [0.0, 0.0, 1.0])?;
        assert!((signed_volume(&section) - 0.3).abs() < 1e-12);
        let tilted = section_surface(&mesh, [0.5, 0.5, 0.5], [1.0, 1.0, 1.0])?;
        assert!((signed_volume(&tilted) - 0.5).abs() < 1e-12);
        assert!(section_surface(&mesh, [0.0; 3], [0.0; 3]).is_err());
        Ok(())
    }

    #[test]
    fn regular_tetrahedron_quality_is_ideal_and_slivers_are_poor() -> TestResult {
        let s = 1.0 / 2f64.sqrt();
        let regular = tet_mesh_from_cells(
            vec![
                [1.0, 0.0, -s],
                [-1.0, 0.0, -s],
                [0.0, 1.0, s],
                [0.0, -1.0, s],
            ],
            vec![[0, 1, 2, 3]],
        )?;
        let quality = tet_quality(&regular, &|_| {});
        assert!((quality.max_aspect_ratio - 1.0).abs() < 1e-9);
        assert!((quality.min_dihedral_degrees - 70.528_779_365_509_31).abs() < 1e-9);
        assert_eq!(quality.poor_count, 0);
        let sliver = tet_mesh_from_cells(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.01],
            ],
            vec![[0, 1, 2, 3]],
        )?;
        assert_eq!(tet_quality(&sliver, &|_| {}).poor_count, 1);
        Ok(())
    }

    #[test]
    fn invalid_cells_are_rejected() {
        assert!(tet_mesh_from_cells(vec![[0.0; 3]; 3], vec![[0, 1, 2, 3]]).is_err());
        assert!(
            tet_mesh_from_cells(
                vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
                vec![[0, 1, 2, 3]]
            )
            .is_err()
        );
    }
}
