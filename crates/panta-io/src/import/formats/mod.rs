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

/// 体网格导入的原始数据（task 030 起为原生混合单元）：接受线性 tet(10)、voxel(11，
/// 按 VTK 定义重排为 hex)、hex(12)、wedge(13)、pyramid(14)；polyhedron(42，task 031 PR 3 前)、
/// 二次单元与二维单元整体拒绝，不做部分导入。节点顺序保持来源 VTK 局部顺序。
///
/// `fields` 为来源中的节点 / 单元结果场（task 029），下标与 `points` / 原始单元一一对应；
/// 不支持的数组不阻止网格导入，原因记入 `skipped_fields`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawGrid {
    pub points: Vec<[f64; 3]>,
    pub types: Vec<panta_mesh::CellType>,
    pub connectivity: Vec<u32>,
    pub fields: Vec<panta_mesh::ResultField>,
    pub skipped_fields: Vec<String>,
}

/// VTK voxel 节点 2/3、6/7 与六面体编号互换。
const VOXEL_TO_HEXAHEDRON: [usize; 8] = [0, 1, 3, 2, 4, 5, 7, 6];

impl RawGrid {
    pub(crate) fn new(
        points: Vec<[f64; 3]>,
        cells: CellList,
        types: Vec<u64>,
    ) -> FormatResult<Self> {
        const VOXEL: u64 = 11;
        if types.len() != cells.len() {
            return Err(FormatError::new("cell types count mismatch"));
        }
        let mut kinds = Vec::with_capacity(cells.len());
        let mut connectivity = Vec::with_capacity(cells.connectivity.len());
        for (index, (&code, nodes)) in types.iter().zip(cells.iter()).enumerate() {
            let kind = if code == VOXEL {
                Some(panta_mesh::CellType::Hexahedron)
            } else {
                u8::try_from(code)
                    .ok()
                    .and_then(panta_mesh::CellType::from_vtk)
            };
            let Some(kind) = kind else {
                return Err(FormatError::new(format!(
                    "cell {index} has type {code}; only linear tet (10), voxel (11), hex (12), \
                     wedge (13) and pyramid (14) cells are supported"
                )));
            };
            // 多面体导入（VTU faces/faceoffsets 与 legacy 面流）随 task 031 PR 3 提供。
            let Some(node_count) = kind.fixed_node_count() else {
                return Err(FormatError::new(format!(
                    "cell {index} has type {code}; polyhedron import is not supported yet"
                )));
            };
            if nodes.len() != node_count {
                return Err(FormatError::new(format!(
                    "cell {index} ({}) must have {node_count} nodes",
                    kind.name()
                )));
            }
            let start = connectivity.len();
            for &node in nodes {
                if node as usize >= points.len() {
                    return Err(FormatError::new(format!(
                        "cell {index} references node outside the node range"
                    )));
                }
                connectivity.push(
                    u32::try_from(node).map_err(|_| FormatError::new("node index overflow"))?,
                );
            }
            if code == VOXEL {
                let voxel: [u32; 8] = std::array::from_fn(|corner| connectivity[start + corner]);
                for (slot, from) in VOXEL_TO_HEXAHEDRON.iter().enumerate() {
                    connectivity[start + slot] = voxel[*from];
                }
            }
            kinds.push(kind);
        }
        if kinds.is_empty() {
            return Err(FormatError::new("grid has no volume cells"));
        }
        for point in &points {
            checked_point(*point)?;
        }
        Ok(Self {
            points,
            types: kinds,
            connectivity,
            fields: Vec::new(),
            skipped_fields: Vec::new(),
        })
    }

    /// 原始单元数（单元场长度以此为准）。
    pub fn cell_count(&self) -> usize {
        self.types.len()
    }

    /// 取出几何构建领域体网格（结果场留在原处）：纯四面体走 `tet_mesh_from_cells`，
    /// 混合单元按原单元保存并拆分。
    pub fn take_mesh(&mut self) -> Result<panta_mesh::TetMesh, panta_mesh::VolumeError> {
        let types = std::mem::take(&mut self.types);
        let connectivity = std::mem::take(&mut self.connectivity);
        let offsets = panta_mesh::cell_offsets(&types, &connectivity)
            .map_err(|cell| panta_mesh::VolumeError(format!("cell {cell} node count mismatch")))?;
        panta_mesh::volume_mesh_from_cells(
            std::mem::take(&mut self.points),
            types,
            offsets,
            connectivity,
        )
    }

    /// 收下一个来源数组：首期只接受 1 或 3 分量、个数与节点 / 单元数一致的数组；
    /// 名称非法、重复或不受支持时跳过并记录原因，不影响网格本身。
    /// 非有限值（NaN / ±inf）原样保留，作为缺失值统计，不替换为 0。
    pub(crate) fn offer_field(
        &mut self,
        name: &str,
        association: panta_mesh::FieldAssociation,
        components: usize,
        values: FormatResult<Vec<f64>>,
    ) {
        let label = format!("{name} ({})", association.as_str());
        let tuples = match association {
            panta_mesh::FieldAssociation::Node => self.points.len(),
            panta_mesh::FieldAssociation::Cell => self.cell_count(),
        };
        let reason = if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
            Some("invalid array name".to_owned())
        } else if !matches!(components, 1 | 3) {
            Some(format!("{components} components are not supported"))
        } else if self
            .fields
            .iter()
            .any(|field| field.name == name && field.association == association)
        {
            Some("duplicate array name".to_owned())
        } else {
            None
        };
        if let Some(reason) = reason {
            self.skipped_fields.push(format!("{label}: {reason}"));
            return;
        }
        let values = match values {
            Ok(values) => values,
            Err(error) => {
                self.skipped_fields.push(format!("{label}: {error}"));
                return;
            }
        };
        if Some(values.len()) != tuples.checked_mul(components) {
            self.skipped_fields.push(format!(
                "{label}: {} values, expected {tuples} x {components}",
                values.len()
            ));
            return;
        }
        self.fields.push(panta_mesh::ResultField {
            name: name.to_owned(),
            association,
            components: components as u8,
            units: String::new(),
            values,
        });
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

    use crate::import::{ImportFormat, SourceSnapshot, detect_format, parse_mesh_snapshot};

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
    fn vtk_volume_fixtures_import_with_native_cells() -> Result<(), Box<dyn std::error::Error>> {
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
            let result = crate::import::parse_volume_snapshot(&snapshot);
            if name.starts_with("voxel") {
                // task 030：体素按六面体原生导入。
                let mut grid = result.map_err(|error| format!("{name}: {error}"))?;
                assert!(grid.cell_count() > 0, "{name}");
                assert!(
                    grid.types
                        .iter()
                        .all(|&kind| kind == panta_mesh::CellType::Hexahedron),
                    "{name}"
                );
                let mesh = grid.take_mesh()?;
                assert!(panta_mesh::validate_tet_mesh(&mesh).is_ok(), "{name}");
                continue;
            }
            let mut grid = result.map_err(|error| format!("{name}: {error}"))?;
            // 四种编码的夹具都带同一个节点法向数组，作为 3 分量节点结果场保留（task 029）。
            assert!(
                grid.skipped_fields.is_empty(),
                "{name}: {:?}",
                grid.skipped_fields
            );
            assert_eq!(grid.fields.len(), 1, "{name}");
            let normals = &grid.fields[0];
            assert_eq!(normals.name, "Normals", "{name}");
            assert_eq!(normals.association, panta_mesh::FieldAssociation::Node);
            assert_eq!(normals.components, 3);
            assert_eq!(normals.values.len(), grid.points.len() * 3);
            let summary = normals.summary();
            assert_eq!(summary.missing, 0, "{name}");
            let [low, high] = summary.range.ok_or("normals range missing")?;
            assert!(low > 0.99 && high < 1.01, "{name}: {low}..{high}");
            let mesh = grid.take_mesh()?;
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
