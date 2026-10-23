//! 表面网格类导入格式（OBJ、PLY、旧版 VTK、VTK XML VTU / VTP）的 Rust 解析后端。
//!
//! 各解析器把多边形扇形三角化、把体单元的外表面（只被一个单元使用的面）
//! 输出到 [`TriangleSink`]；`SurfaceMesh` 只是其中一个消费者，后续分块或流式
//! 写入可实现同一接口，不要求整体物化三角形。

mod cells;
mod obj;
mod ply;
mod text;
mod vtk_legacy;
mod vtk_xml;

use panta_mesh::SurfaceMesh;
use std::fmt::{Display, Formatter};

pub use obj::parse_obj;
pub use ply::parse_ply;
pub use vtk_legacy::{parse_vtk_legacy, parse_vtk_legacy_grid};
pub use vtk_xml::{parse_vtp, parse_vtu, parse_vtu_grid};

/// 体网格导入的原始数据：首期只接受线性四面体（VTK 单元类型 10），
/// 任何其他单元类型整体拒绝，不做部分导入。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawGrid {
    pub points: Vec<[f64; 3]>,
    pub tets: Vec<[u32; 4]>,
}

impl RawGrid {
    pub(crate) fn new(
        points: Vec<[f64; 3]>,
        cells: CellList,
        types: Vec<u64>,
    ) -> FormatResult<Self> {
        const LINEAR_TETRA: u64 = 10;
        if types.len() != cells.len() {
            return Err(FormatError::new("cell types count mismatch"));
        }
        let mut tets = Vec::with_capacity(cells.len());
        for (index, (kind, nodes)) in types.iter().zip(cells.iter()).enumerate() {
            if *kind != LINEAR_TETRA {
                return Err(FormatError::new(format!(
                    "cell {index} has type {kind}; only linear tetrahedra (10) are supported"
                )));
            }
            let nodes: [u64; 4] = nodes
                .try_into()
                .map_err(|_| FormatError::new(format!("tetrahedron {index} must have 4 nodes")))?;
            let mut tet = [0u32; 4];
            for (slot, node) in tet.iter_mut().zip(nodes) {
                if node as usize >= points.len() {
                    return Err(FormatError::new(format!(
                        "tetrahedron {index} references node outside the node range"
                    )));
                }
                *slot = u32::try_from(node).map_err(|_| FormatError::new("node index overflow"))?;
            }
            tets.push(tet);
        }
        if tets.is_empty() {
            return Err(FormatError::new("grid has no tetrahedra"));
        }
        for point in &points {
            checked_point(*point)?;
        }
        Ok(Self { points, tets })
    }
}

/// 解析失败的格式专属诊断；文本面向开发者日志，不直接作为界面文案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError(pub String);

impl FormatError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for FormatError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FormatError {}

pub(crate) type FormatResult<T> = Result<T, FormatError>;

/// 接收按顺序产出的三角形；返回错误时解析立即停止。
pub trait TriangleSink {
    fn triangle(&mut self, triangle: [[f64; 3]; 3]) -> FormatResult<()>;
}

impl TriangleSink for SurfaceMesh {
    fn triangle(&mut self, triangle: [[f64; 3]; 3]) -> FormatResult<()> {
        self.triangles
            .try_reserve(1)
            .map_err(|error| FormatError::new(format!("allocation failed: {error}")))?;
        self.triangles.push(triangle);
        Ok(())
    }
}

/// 以共享点表与多边形索引描述的表面；索引越界和非有限坐标在此统一拒绝。
pub(crate) struct IndexedPoints<'a> {
    pub points: &'a [[f64; 3]],
}

impl IndexedPoints<'_> {
    pub fn point(&self, index: u64) -> FormatResult<[f64; 3]> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.points.get(index))
            .copied()
            .ok_or_else(|| FormatError::new(format!("point index {index} out of range")))
    }

    /// 凸多边形扇形三角化；少于 3 个顶点的多边形忽略（点、线单元不构成表面）。
    pub fn polygon(&self, nodes: &[u64], sink: &mut impl TriangleSink) -> FormatResult<()> {
        if nodes.len() < 3 {
            return Ok(());
        }
        let first = self.point(nodes[0])?;
        for pair in nodes[1..].windows(2) {
            sink.triangle([first, self.point(pair[0])?, self.point(pair[1])?])?;
        }
        Ok(())
    }

    /// 三角带按奇偶交替翻转以保持一致朝向。
    pub fn strip(&self, nodes: &[u64], sink: &mut impl TriangleSink) -> FormatResult<()> {
        for (index, window) in nodes.windows(3).enumerate() {
            let (a, b, c) = if index % 2 == 0 {
                (window[0], window[1], window[2])
            } else {
                (window[1], window[0], window[2])
            };
            sink.triangle([self.point(a)?, self.point(b)?, self.point(c)?])?;
        }
        Ok(())
    }
}

/// 扁平单元表：所有单元节点连续存放，`ends[i]` 是第 i 个单元的结束位置。
/// 避免为每个单元分配独立 `Vec`，千万级单元时显著降低分配次数和常驻内存。
pub(crate) struct CellList {
    pub connectivity: Vec<u64>,
    pub ends: Vec<usize>,
}

impl CellList {
    /// 由 VTK 风格的结束偏移构造，并校验偏移单调且不越界。
    pub fn from_ends(connectivity: Vec<u64>, ends: &[u64]) -> FormatResult<Self> {
        let mut previous = 0usize;
        let ends = ends
            .iter()
            .map(|&end| {
                let end = usize::try_from(end)
                    .ok()
                    .filter(|&end| end >= previous && end <= connectivity.len())
                    .ok_or_else(|| FormatError::new("cell offsets out of range"))?;
                previous = end;
                Ok(end)
            })
            .collect::<FormatResult<_>>()?;
        Ok(Self { connectivity, ends })
    }

    pub fn len(&self) -> usize {
        self.ends.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &[u64]> {
        let mut start = 0usize;
        self.ends.iter().map(move |&end| {
            let cell = &self.connectivity[start..end];
            start = end;
            cell
        })
    }
}

pub(crate) fn checked_point(point: [f64; 3]) -> FormatResult<[f64; 3]> {
    if point.iter().all(|value| value.is_finite()) {
        Ok(point)
    } else {
        Err(FormatError::new("non-finite coordinate"))
    }
}

/// 收集解析结果并拒绝空表面；供各格式的 `parse_*` 入口共用。
pub(crate) fn finish_mesh(mesh: SurfaceMesh) -> FormatResult<SurfaceMesh> {
    if mesh.triangles.is_empty() {
        return Err(FormatError::new("no surface triangles"));
    }
    if mesh
        .summary()
        .dimensions
        .iter()
        .any(|size| !size.is_finite())
    {
        return Err(FormatError::new("coordinate range overflow"));
    }
    Ok(mesh)
}

#[cfg(test)]
mod fixture_tests {
    use std::path::PathBuf;

    use crate::{ImportFormat, SourceSnapshot, detect_format, parse_mesh_snapshot};

    /// 期望三角形数由 VTK 9.7 `vtkDataSetSurfaceFilter + vtkTriangleFilter` 交叉得到，
    /// 见 tests/fixtures/mesh/formats/README.md。
    const EXPECTED: &[(&str, usize)] = &[
        ("tet_ball_ascii.vtu", 64),
        ("tet_ball_appended_zlib.vtu", 64),
        ("voxel_block_binary.vtu", 20),
        ("voxel_block_binary_zlib.vtu", 20),
        ("voxel_block_appended_raw.vtu", 20),
        ("sphere_appended_raw.vtp", 100),
        ("sphere_binary_zlib.vtp", 100),
        ("tet_ball_legacy42_ascii.vtk", 64),
        ("tet_ball_legacy51_binary.vtk", 64),
        ("sphere_legacy51_ascii.vtk", 100),
        ("sphere_legacy42_binary.vtk", 100),
        ("sphere.obj", 100),
        ("sphere_ascii.ply", 100),
        ("sphere_binary_le.ply", 100),
    ];

    /// VTK 写出的四面体球：体网格导入后外表面与表面导入一致（64 个三角形），体素块整体拒绝。
    #[test]
    fn vtk_tet_fixtures_import_as_volume_and_other_cells_are_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/mesh/formats");
        for name in [
            "tet_ball_ascii.vtu",
            "tet_ball_appended_zlib.vtu",
            "tet_ball_legacy42_ascii.vtk",
            "tet_ball_legacy51_binary.vtk",
            "voxel_block_binary.vtu",
        ] {
            let path = root.join(name);
            let snapshot = SourceSnapshot {
                bytes: std::fs::read(&path)?,
                source_name: name.to_owned(),
                format: detect_format(&path)?,
                path,
                modified_unix_ms: None,
            };
            let result = crate::parse_volume_snapshot(&snapshot);
            if name.starts_with("voxel") {
                assert!(result.is_err(), "{name}");
                continue;
            }
            let grid = result.map_err(|error| format!("{name}: {error}"))?;
            let mesh = panta_mesh::tet_mesh_from_cells(grid.points, grid.tets)?;
            assert_eq!(mesh.boundary.len(), 64, "{name}");
        }
        Ok(())
    }

    #[test]
    fn vtk_written_fixtures_match_vtk_surface_triangle_counts()
    -> Result<(), Box<dyn std::error::Error>> {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/mesh/formats");
        for (name, expected) in EXPECTED {
            let path = root.join(name);
            let format = detect_format(&path)?;
            assert_ne!(format, ImportFormat::Stl);
            let snapshot = SourceSnapshot {
                bytes: std::fs::read(&path)?,
                source_name: (*name).to_owned(),
                format,
                path,
                modified_unix_ms: None,
            };
            let mesh =
                parse_mesh_snapshot(&snapshot).map_err(|error| format!("{name}: {error}"))?;
            assert_eq!(mesh.triangles.len(), *expected, "{name}");
            let size = mesh.summary().dimensions;
            assert!(
                size.iter().all(|value| *value > 0.5 && *value <= 2.0),
                "{name}: {size:?}"
            );
        }
        Ok(())
    }
}
