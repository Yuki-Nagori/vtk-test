//! 后端中立的网格数据、STL 解析和 Mesh IR 领域校验。
//!
//! `SurfaceMesh` 坐标保留解析来源单位，工程提交前由导入事务换算到毫米；
//! `TetMesh` 坐标固定为毫米，四面体局部节点顺序要求正有向体积。

use std::collections::HashSet;
use std::fmt::{Display, Formatter};

mod asset;
mod diagnostics;
mod lod;
mod progress;
mod synthetic;
mod volume;
pub use asset::{
    MESH_ASSET_FORMAT_VERSION, MeshAsset, MeshAssetError, MeshAssetReader, MeshAssetRef,
    SurfaceAssetChunks, Triangle, VolumeAssetChunks, VolumeSource, VolumeStreamCounts,
    decode_mesh_asset, decode_mesh_asset_with_digest, encode_mesh_asset, encode_mesh_asset_ref,
    is_readable_mesh_asset_version, mesh_asset_encoded_len, surface_mesh_expanded_bytes,
    write_surface_asset_streaming, write_volume_asset_streaming,
};
pub use diagnostics::{
    DEFAULT_RELATIVE_WELD_TOLERANCE, DiagnosticError, DiagnosticOptions, SurfaceDiagnostics,
    diagnose_surface, diagnose_surface_with_progress,
};
pub use lod::{DEFAULT_LOD_TRIANGLE_BUDGET, SurfaceLod, build_surface_lod, simplify_surface};
pub use progress::{PROGRESS_STRIDE, Progress};
pub use synthetic::{SyntheticTetBlock, SyntheticTorus};
pub use volume::{
    POOR_ASPECT_RATIO, POOR_MIN_DIHEDRAL_DEGREES, TetQuality, VolumeError, VolumeStreamError,
    boundary_faces, boundary_surface, boundary_surface_streaming, section_plane_origin,
    section_plane_streaming, section_surface, tet_mesh_from_cells, tet_quality,
    tet_quality_streaming,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceMesh {
    pub triangles: Vec<[[f64; 3]; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceSummary {
    pub triangle_count: u64,
    pub dimensions: [f64; 3],
}

impl SurfaceMesh {
    pub fn summary(&self) -> SurfaceSummary {
        let mut minimum = [f64::INFINITY; 3];
        let mut maximum = [f64::NEG_INFINITY; 3];
        for triangle in &self.triangles {
            for point in triangle {
                for axis in 0..3 {
                    minimum[axis] = minimum[axis].min(point[axis]);
                    maximum[axis] = maximum[axis].max(point[axis]);
                }
            }
        }
        SurfaceSummary {
            triangle_count: self.triangles.len() as u64,
            dimensions: std::array::from_fn(|axis| maximum[axis] - minimum[axis]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StlError {
    Empty,
    InvalidEncoding,
    InvalidData,
    NonFiniteCoordinate,
    SizeOverflow,
}

impl Display for StlError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for StlError {}

/// 优先识别长度精确匹配的 binary STL，其余输入按严格 UTF-8 ASCII STL
/// 读取。ASCII 的 vertex 行必须恰好有三个数值字段；至少包含一个完整三角形。
pub fn parse_stl(bytes: &[u8]) -> Result<SurfaceMesh, StlError> {
    parse_stl_with_progress(bytes, &|_| {})
}

/// 解析进度上报间隔（三角面或文本行）。
const PARSE_PROGRESS_STRIDE: usize = 1 << 16;

/// 与 [`parse_stl`] 相同，另按 [`PARSE_PROGRESS_STRIDE`] 间隔上报 0..=1 的解析进度。
pub fn parse_stl_with_progress(
    bytes: &[u8],
    on_progress: &dyn Fn(f64),
) -> Result<SurfaceMesh, StlError> {
    if bytes.is_empty() {
        return Err(StlError::Empty);
    }
    let mesh = if bytes.len() >= 84 {
        let count = u32::from_le_bytes(
            bytes[80..84]
                .try_into()
                .map_err(|_| StlError::InvalidData)?,
        ) as usize;
        let expected = binary_size(count)?;
        if count > 0 && expected == bytes.len() {
            parse_binary(bytes, count, on_progress)?
        } else {
            parse_ascii(bytes, on_progress)?
        }
    } else {
        parse_ascii(bytes, on_progress)?
    };
    if mesh
        .summary()
        .dimensions
        .iter()
        .any(|size| !size.is_finite())
    {
        return Err(StlError::SizeOverflow);
    }
    Ok(mesh)
}

fn binary_size(count: usize) -> Result<usize, StlError> {
    count
        .checked_mul(50)
        .and_then(|size| size.checked_add(84))
        .ok_or(StlError::SizeOverflow)
}

fn checked_point(point: [f64; 3]) -> Result<[f64; 3], StlError> {
    if point.iter().all(|value| value.is_finite()) {
        Ok(point)
    } else {
        Err(StlError::NonFiniteCoordinate)
    }
}

fn parse_binary(
    bytes: &[u8],
    count: usize,
    on_progress: &dyn Fn(f64),
) -> Result<SurfaceMesh, StlError> {
    let mut triangles = Vec::with_capacity(count);
    for triangle in 0..count {
        if triangle.is_multiple_of(PARSE_PROGRESS_STRIDE) {
            on_progress(triangle as f64 / count as f64);
        }
        let base = 84 + triangle * 50 + 12;
        let mut points = [[0.0; 3]; 3];
        for (vertex, point) in points.iter_mut().enumerate() {
            for (axis, coordinate) in point.iter_mut().enumerate() {
                let offset = base + vertex * 12 + axis * 4;
                *coordinate = f32::from_le_bytes(
                    bytes[offset..offset + 4]
                        .try_into()
                        .map_err(|_| StlError::InvalidData)?,
                ) as f64;
            }
            checked_point(*point)?;
        }
        triangles.push(points);
    }
    on_progress(1.0);
    Ok(SurfaceMesh { triangles })
}

fn parse_ascii(bytes: &[u8], on_progress: &dyn Fn(f64)) -> Result<SurfaceMesh, StlError> {
    let text = std::str::from_utf8(bytes).map_err(|_| StlError::InvalidEncoding)?;
    let mut triangles = Vec::new();
    let mut triangle = [[0.0; 3]; 3];
    let mut vertex = 0;
    let total = text.len().max(1) as f64;
    for (index, line) in text.lines().enumerate() {
        if index.is_multiple_of(PARSE_PROGRESS_STRIDE) {
            // 行切片指向原文本，偏移即已消费字节数。
            let consumed = line.as_ptr() as usize - text.as_ptr() as usize;
            on_progress(consumed as f64 / total);
        }
        let mut fields = line.split_whitespace();
        if !fields
            .next()
            .is_some_and(|field| field.eq_ignore_ascii_case("vertex"))
        {
            continue;
        }
        let mut point = [0.0; 3];
        for coordinate in &mut point {
            *coordinate = fields
                .next()
                .ok_or(StlError::InvalidData)?
                .parse::<f64>()
                .map_err(|_| StlError::InvalidData)?;
        }
        if fields.next().is_some() {
            return Err(StlError::InvalidData);
        }
        triangle[vertex] = checked_point(point)?;
        vertex += 1;
        if vertex == 3 {
            triangles.push(triangle);
            vertex = 0;
        }
    }
    if triangles.is_empty() || vertex != 0 {
        return Err(StlError::InvalidData);
    }
    on_progress(1.0);
    Ok(SurfaceMesh { triangles })
}

pub type MeshIndex = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tetrahedron {
    pub nodes: [MeshIndex; 4],
    pub region: MeshIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceTriangle {
    pub nodes: [MeshIndex; 3],
    pub group: MeshIndex,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TetMesh {
    /// Netgen 输出和工程契约中的毫米坐标。
    pub nodes: Vec<[f64; 3]>,
    pub tets: Vec<Tetrahedron>,
    pub boundary: Vec<SurfaceTriangle>,
    pub region_count: MeshIndex,
    pub boundary_group_count: MeshIndex,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeshValidationReport {
    pub issues: Vec<String>,
    /// 仅当校验成功时提供，单位为 mm³。
    pub volume_mm3: f64,
}

impl MeshValidationReport {
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }
}

/// 领域校验：输入源自后端转换后、自有 Mesh IR 提交前。
pub fn validate_tet_mesh(mesh: &TetMesh) -> MeshValidationReport {
    let mut issues = Vec::new();
    let mut unique_tets = HashSet::with_capacity(mesh.tets.len());
    let mut tet_faces = HashSet::with_capacity(mesh.tets.len().saturating_mul(4));
    let mut unique_boundary = HashSet::with_capacity(mesh.boundary.len());
    let mut volume_mm3 = 0.0;
    let mut volume_overflow = false;
    if mesh.nodes.is_empty() {
        issues.push("mesh has no nodes".to_owned());
    }
    if mesh.tets.is_empty() {
        issues.push("mesh has no volume elements".to_owned());
    }
    for (index, point) in mesh.nodes.iter().enumerate() {
        if point.iter().any(|value| !value.is_finite()) {
            issues.push(format!("node {index} has a non-finite coordinate"));
        }
    }
    if mesh.region_count as usize > mesh.tets.len() {
        issues.push("declared region count exceeds tetrahedron count".to_owned());
    }
    if mesh.boundary_group_count as usize > mesh.boundary.len() {
        issues.push("declared boundary group count exceeds boundary triangle count".to_owned());
    }
    let mut regions =
        vec![false; (mesh.region_count as usize).min(mesh.tets.len().saturating_add(1))];
    for (index, tet) in mesh.tets.iter().enumerate() {
        let in_range = tet
            .nodes
            .iter()
            .all(|&node| (node as usize) < mesh.nodes.len());
        if !in_range {
            issues.push(format!(
                "tetrahedron {index} references node outside the node range"
            ));
        }
        let unique = tet
            .nodes
            .iter()
            .enumerate()
            .all(|(i, node)| !tet.nodes[..i].contains(node));
        if !unique {
            issues.push(format!("tetrahedron {index} repeats a node"));
        }
        if unique {
            let mut key = tet.nodes;
            key.sort_unstable();
            if !unique_tets.insert(key) {
                issues.push(format!(
                    "tetrahedron {index} duplicates another tetrahedron"
                ));
            }
            if in_range {
                for omitted in 0..4 {
                    let mut face = [0; 3];
                    let mut next = 0;
                    for (corner, &node) in tet.nodes.iter().enumerate() {
                        if corner != omitted {
                            face[next] = node;
                            next += 1;
                        }
                    }
                    face.sort_unstable();
                    tet_faces.insert(face);
                }
            }
        }
        if in_range
            && unique
            && tet.nodes.iter().all(|&node| {
                mesh.nodes[node as usize]
                    .iter()
                    .all(|value| value.is_finite())
            })
        {
            let [a, b, c, d] = tet.nodes.map(|node| mesh.nodes[node as usize]);
            let u = sub(b, a);
            let v = sub(c, a);
            let w = sub(d, a);
            let volume6 = (u[1] * v[2] - u[2] * v[1]) * w[0]
                + (u[2] * v[0] - u[0] * v[2]) * w[1]
                + (u[0] * v[1] - u[1] * v[0]) * w[2];
            if !volume6.is_finite() {
                issues.push(format!("tetrahedron {index} has non-finite signed volume"));
            } else if volume6 <= 0.0 {
                issues.push(format!(
                    "tetrahedron {index} has non-positive signed volume"
                ));
            } else if !volume_overflow {
                let next = volume_mm3 + volume6 / 6.0;
                if next.is_finite() {
                    volume_mm3 = next;
                } else {
                    volume_overflow = true;
                }
            }
        }
        if let Some(used) = regions.get_mut(tet.region as usize) {
            *used = true;
        } else if tet.region >= mesh.region_count {
            issues.push(format!(
                "tetrahedron {index} references region outside the declared count"
            ));
        }
    }
    for (index, used) in regions.iter().enumerate() {
        if !used {
            issues.push(format!("region {index} is not referenced"));
        }
    }
    let mut groups = vec![
        false;
        (mesh.boundary_group_count as usize)
            .min(mesh.boundary.len().saturating_add(1))
    ];
    for (index, face) in mesh.boundary.iter().enumerate() {
        let in_range = !face
            .nodes
            .iter()
            .any(|&node| (node as usize) >= mesh.nodes.len());
        if !in_range {
            issues.push(format!(
                "boundary triangle {index} references node outside the node range"
            ));
        }
        let unique = !(face.nodes[0] == face.nodes[1]
            || face.nodes[0] == face.nodes[2]
            || face.nodes[1] == face.nodes[2]);
        if !unique {
            issues.push(format!("boundary triangle {index} repeats a node"));
        }
        if in_range && unique {
            let mut key = face.nodes;
            key.sort_unstable();
            if !unique_boundary.insert(key) {
                issues.push(format!(
                    "boundary triangle {index} duplicates another triangle"
                ));
            }
            if !tet_faces.contains(&key) {
                issues.push(format!(
                    "boundary triangle {index} is not a tetrahedron face"
                ));
            }
            let [a, b, c] = face.nodes.map(|node| mesh.nodes[node as usize]);
            if [a, b, c]
                .iter()
                .all(|point| point.iter().all(|coordinate| coordinate.is_finite()))
            {
                let u = sub(b, a);
                let v = sub(c, a);
                let cross = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let area4 = cross.iter().map(|value| value * value).sum::<f64>();
                if !area4.is_finite() {
                    issues.push(format!("boundary triangle {index} has non-finite area"));
                } else if area4 == 0.0 {
                    issues.push(format!("boundary triangle {index} has zero area"));
                }
            }
        }
        if let Some(used) = groups.get_mut(face.group as usize) {
            *used = true;
        } else if face.group >= mesh.boundary_group_count {
            issues.push(format!(
                "boundary triangle {index} references group outside the declared count"
            ));
        }
    }
    for (index, used) in groups.iter().enumerate() {
        if !used {
            issues.push(format!("boundary group {index} is not referenced"));
        }
    }
    if volume_overflow {
        issues.push("mesh has non-finite total volume".to_owned());
        volume_mm3 = 0.0;
    }
    if !issues.is_empty() {
        volume_mm3 = 0.0;
    }
    MeshValidationReport { issues, volume_mm3 }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|axis| a[axis] - b[axis])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_and_ascii_parse_report_monotonic_progress_to_completion() -> Result<(), StlError> {
        let count = 70_000usize;
        let mut bytes = vec![0u8; 84 + count * 50];
        bytes[80..84].copy_from_slice(&(count as u32).to_le_bytes());
        let seen = std::cell::RefCell::new(Vec::new());
        parse_stl_with_progress(&bytes, &|value| seen.borrow_mut().push(value))?;
        let ascii = "solid x\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 2 0\nendsolid\n";
        parse_stl_with_progress(ascii.as_bytes(), &|value| seen.borrow_mut().push(value))?;
        let seen = seen.into_inner();
        assert_eq!(&seen[..3], &[0.0, 65_536.0 / 70_000.0, 1.0]);
        assert_eq!(&seen[3..], &[0.0, 1.0]);
        Ok(())
    }

    #[test]
    fn ascii_accepts_exact_vertices_and_reports_bounds() -> Result<(), StlError> {
        let mesh = parse_stl(b"solid x\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 2 0\nendsolid\n")?;
        assert_eq!(mesh.summary().dimensions, [1.0, 2.0, 0.0]);
        assert_eq!(mesh.triangles.len(), 1);
        Ok(())
    }

    #[test]
    fn ascii_rejects_extra_fields_partial_triangles_and_nonfinite_points() {
        assert_eq!(
            parse_stl(b"vertex 0 0 0 extra\nvertex 1 0 0\nvertex 0 1 0"),
            Err(StlError::InvalidData)
        );
        assert_eq!(parse_stl(b"vertex 0 0 0\n"), Err(StlError::InvalidData));
        assert_eq!(
            parse_stl(b"vertex NaN 0 0\nvertex 1 0 0\nvertex 0 1 0"),
            Err(StlError::NonFiniteCoordinate)
        );
        assert_eq!(parse_stl(b"\xffvertex"), Err(StlError::InvalidEncoding));
        assert_eq!(parse_stl(b""), Err(StlError::Empty));
    }

    #[test]
    fn binary_requires_exact_length_and_finite_vertices() -> Result<(), StlError> {
        let mut bytes = vec![0u8; 134];
        bytes[80..84].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(parse_stl(&bytes)?.triangles.len(), 1);
        bytes[96..100].copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert_eq!(parse_stl(&bytes), Err(StlError::NonFiniteCoordinate));
        bytes.pop();
        assert!(parse_stl(&bytes).is_err());
        Ok(())
    }

    #[test]
    fn ascii_rejects_nonfinite_bounds_from_finite_coordinates() {
        let text = format!(
            "vertex {} 0 0\nvertex {} 0 0\nvertex 0 1 0\n",
            -f64::MAX,
            f64::MAX
        );
        assert_eq!(parse_stl(text.as_bytes()), Err(StlError::SizeOverflow));
    }

    #[test]
    fn binary_record_size_checks_integer_overflow() {
        assert_eq!(binary_size(1), Ok(134));
        assert_eq!(binary_size(usize::MAX), Err(StlError::SizeOverflow));
    }

    #[test]
    fn tet_validation_rejects_overflow_and_out_of_range_without_indexing() {
        let mesh = TetMesh {
            nodes: vec![
                [0.0; 3],
                [f64::MAX, 0.0, 0.0],
                [0.0, f64::MAX, 0.0],
                [0.0, 0.0, f64::MAX],
            ],
            tets: vec![Tetrahedron {
                nodes: [0, 1, 2, 3],
                region: 0,
            }],
            boundary: vec![SurfaceTriangle {
                nodes: [0, 1, 9],
                group: 0,
            }],
            region_count: 1,
            boundary_group_count: 1,
        };
        let report = validate_tet_mesh(&mesh);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("non-finite signed volume"))
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("outside the node range"))
        );
    }

    #[test]
    fn tet_validation_returns_total_volume() {
        let mesh = TetMesh {
            nodes: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            tets: vec![Tetrahedron {
                nodes: [0, 1, 2, 3],
                region: 0,
            }],
            boundary: vec![SurfaceTriangle {
                nodes: [0, 1, 2],
                group: 0,
            }],
            region_count: 1,
            boundary_group_count: 1,
        };
        let report = validate_tet_mesh(&mesh);
        assert!(report.is_ok());
        assert!((report.volume_mm3 - 1.0 / 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tet_validation_rejects_total_volume_overflow() {
        let mut nodes = Vec::new();
        let mut tets = Vec::new();
        let mut boundary = Vec::new();
        for index in 0..2_000_u32 {
            let origin = f64::from(index) * 1.0e103;
            let first_node = index * 4;
            nodes.extend_from_slice(&[
                [origin, 0.0, 0.0],
                [origin + 1.0e102, 0.0, 0.0],
                [origin, 1.0e102, 0.0],
                [origin, 0.0, 1.0e102],
            ]);
            tets.push(Tetrahedron {
                nodes: [first_node, first_node + 1, first_node + 2, first_node + 3],
                region: 0,
            });
            boundary.push(SurfaceTriangle {
                nodes: [first_node, first_node + 1, first_node + 2],
                group: 0,
            });
        }
        let mesh = TetMesh {
            nodes,
            tets,
            boundary,
            region_count: 1,
            boundary_group_count: 1,
        };
        let report = validate_tet_mesh(&mesh);
        assert!(!report.is_ok());
        assert_eq!(report.volume_mm3, 0.0);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue == "mesh has non-finite total volume")
        );
    }

    #[test]
    fn tet_validation_rejects_duplicate_cells_and_unmatched_boundary() {
        let mut mesh = TetMesh {
            nodes: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [2.0, 0.0, 0.0],
            ],
            tets: vec![Tetrahedron {
                nodes: [0, 1, 2, 3],
                region: 0,
            }],
            boundary: vec![SurfaceTriangle {
                nodes: [0, 1, 2],
                group: 0,
            }],
            region_count: 1,
            boundary_group_count: 1,
        };
        assert!(validate_tet_mesh(&mesh).is_ok());
        mesh.tets.push(mesh.tets[0]);
        mesh.boundary.push(mesh.boundary[0]);
        mesh.boundary.push(SurfaceTriangle {
            nodes: [0, 1, 4],
            group: 0,
        });
        let report = validate_tet_mesh(&mesh);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("duplicates another tetrahedron"))
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("duplicates another triangle"))
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("not a tetrahedron face"))
        );
    }
}
