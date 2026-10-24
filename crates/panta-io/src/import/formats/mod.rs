//! 表面网格类导入格式（OBJ、PLY、旧版 VTK、VTK XML VTU / VTP）的 Rust 解析后端。
//!
//! 各解析器把多边形扇形三角化、把体单元的外表面（只被一个单元使用的面）
//! 输出到 [`TriangleSink`]；`SurfaceMesh` 只是其中一个消费者，后续分块或流式
//! 写入可实现同一接口，不要求整体物化三角形。

mod cells;
mod cgns;
mod obj;
mod openfoam;
mod ply;
pub mod pvd;
mod text;
mod vtk_legacy;
mod vtk_xml;

use panta_mesh::SurfaceMesh;
use std::fmt::{Display, Formatter};

pub use cgns::{parse_cgns_grid, parse_cgns_surface};
pub use obj::parse_obj;
pub use openfoam::bundle as openfoam_bundle;
pub use openfoam::{parse_openfoam_grid, parse_openfoam_surface};
pub use ply::parse_ply;
pub use pvd::{parse_pvd_grid, parse_pvd_surface};
pub use vtk_legacy::{parse_vtk_legacy, parse_vtk_legacy_grid};
pub use vtk_xml::{parse_vtp, parse_vtu, parse_vtu_grid};

/// 体网格导入的原始数据（task 030 起为原生混合单元）：接受线性 tet(10)、voxel(11，
/// 按 VTK 定义重排为 hex)、hex(12)、wedge(13)、pyramid(14) 与 polyhedron(42，task 031)；
/// 二次单元与二维单元整体拒绝，不做部分导入。节点顺序保持来源 VTK 局部顺序。
/// 多面体的面流来自 VTU `faces` / `faceoffsets`、VTK 9.4 的 `polyhedron_offsets` /
/// `polyhedron_to_faces` / `face_offsets` / `face_connectivity`，或旧版 VTK CELLS 中的序列化面流；
/// `connectivity` 中多面体占 `[0, nFaces, n0, ids…]`，首字为中心节点占位，由领域构建补齐。
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
    /// 每个原始单元的区域下标（空表示单区域）；`region_names` 与之对应（task 032）。
    pub cell_regions: Vec<u32>,
    pub region_names: Vec<String>,
    /// 来源声明的边界面及其分组（OpenFOAM patch）；`None` 时整个外表面为一组。
    /// [`RawGrid::take_mesh`] 之后 `boundary_group_names` 为实际使用的组名（按组下标）。
    pub boundary: Option<BoundaryFaces>,
    pub boundary_group_names: Vec<String>,
    /// 按来源面邻接（owner / neighbour）计算的面质量（checkMesh 口径，task 032 PR 2）；
    /// 只有带面邻接的来源（OpenFOAM）填写。
    pub face_quality: Option<panta_mesh::FaceQuality>,
    /// 多时间步结果场（task 033）：`fields` 始终是最新一步；这里记录最新步的时间与更早的各步。
    /// 只有来源声明了两个及以上时间步时为 `Some`。
    pub time_series: Option<TimeSeries>,
}

/// 时间序列结果场（task 033）：所有步共享同一网格，字段集合可以逐步不同。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimeSeries {
    /// 最新步之前的各步，时间升序。
    pub earlier: Vec<TimeStepFields>,
    pub latest_time: f64,
    pub latest_label: String,
}

/// 一个时间步：物理时间、来源标签（OpenFOAM 时间目录名 / CGNS 解名）与该步的结果场。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimeStepFields {
    pub time: f64,
    pub label: String,
    pub fields: Vec<panta_mesh::ResultField>,
}

/// 更早时间步的场值总量上限（f64 个数，2^28 = 2 GiB）；超出时只保留最新步（task 033 保守决策）。
pub const MAX_TIME_STEP_VALUES: usize = 1 << 28;

/// 来源边界面：扁平节点表 + 结束偏移，每面一个组下标。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoundaryFaces {
    pub nodes: Vec<u32>,
    pub ends: Vec<usize>,
    pub groups: Vec<u32>,
    pub names: Vec<String>,
}

/// VTK voxel 节点 2/3、6/7 与六面体编号互换。
const VOXEL_TO_HEXAHEDRON: [usize; 8] = [0, 1, 3, 2, 4, 5, 7, 6];

impl RawGrid {
    /// `faces` 为各单元的多面体面流（`None` 时多面体单元的节点表本身就是面流，即旧版 VTK）。
    pub(crate) fn new(
        points: Vec<[f64; 3]>,
        cells: CellList,
        types: Vec<u64>,
        faces: Option<&PolyFaces>,
    ) -> FormatResult<Self> {
        const VOXEL: u64 = 11;
        if types.len() != cells.len() || faces.is_some_and(|faces| faces.len() != cells.len()) {
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
                     wedge (13), pyramid (14) and polyhedron (42) cells are supported"
                )));
            };
            let Some(node_count) = kind.fixed_node_count() else {
                let stream = match faces {
                    Some(faces) => faces.cell(index),
                    None => nodes,
                };
                push_polyhedron(index, stream, points.len(), &mut connectivity)?;
                kinds.push(kind);
                continue;
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
            ..Self::default()
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
        let mut mesh = panta_mesh::volume_mesh_from_cells(
            std::mem::take(&mut self.points),
            types,
            offsets,
            connectivity,
        )?;
        self.apply_regions(&mut mesh)?;
        self.apply_boundary_groups(&mut mesh);
        Ok(mesh)
    }

    fn apply_regions(
        &mut self,
        mesh: &mut panta_mesh::TetMesh,
    ) -> Result<(), panta_mesh::VolumeError> {
        if self.cell_regions.is_empty() {
            return Ok(());
        }
        let count = u32::try_from(self.region_names.len())
            .map_err(|_| panta_mesh::VolumeError("too many regions".to_owned()))?;
        if self.cell_regions.len() != mesh.cell_count()
            || self.cell_regions.iter().any(|&region| region >= count)
        {
            return Err(panta_mesh::VolumeError(
                "cell region table mismatch".to_owned(),
            ));
        }
        for tet in 0..mesh.tets.len() {
            let cell = mesh.tet_cell(tet);
            mesh.tets[tet].region = self.cell_regions[cell];
        }
        mesh.region_count = count;
        Ok(())
    }

    /// 把领域构建得到的外表面三角形映射回来源边界面所在组；未被任何来源面覆盖的三角形
    /// （来源边界不完整）归入 `unassigned` 组。未被引用的组（如零面 patch）删除并重新编号。
    fn apply_boundary_groups(&mut self, mesh: &mut panta_mesh::TetMesh) {
        let Some(boundary) = self.boundary.take() else {
            return;
        };
        let node_count = mesh.nodes.len();
        let mut starts = vec![0usize; node_count + 1];
        let face_range = |face: usize| {
            let start = if face == 0 {
                0
            } else {
                boundary.ends[face - 1]
            };
            start..boundary.ends[face]
        };
        for face in 0..boundary.ends.len() {
            for &node in &boundary.nodes[face_range(face)] {
                if let Some(slot) = starts.get_mut(node as usize + 1) {
                    *slot += 1;
                }
            }
        }
        for node in 0..node_count {
            starts[node + 1] += starts[node];
        }
        let mut cursor = starts.clone();
        let mut incident = vec![0u32; starts[node_count]];
        for face in 0..boundary.ends.len() {
            for &node in &boundary.nodes[face_range(face)] {
                if let Some(slot) = cursor.get_mut(node as usize) {
                    incident[*slot] = face as u32;
                    *slot += 1;
                }
            }
        }
        let unassigned = boundary.names.len() as u32;
        let mut used = vec![false; boundary.names.len() + 1];
        for triangle in &mut mesh.boundary {
            let [a, b, c] = triangle.nodes;
            let group = incident[starts[a as usize]..starts[a as usize + 1]]
                .iter()
                .find(|&&face| {
                    let nodes = &boundary.nodes[face_range(face as usize)];
                    nodes.contains(&b) && nodes.contains(&c)
                })
                .map_or(unassigned, |&face| boundary.groups[face as usize]);
            used[group as usize] = true;
            triangle.group = group;
        }
        let mut names = boundary.names;
        names.push("unassigned".to_owned());
        let mut remap = vec![0u32; names.len()];
        self.boundary_group_names.clear();
        for (group, name) in names.into_iter().enumerate() {
            if used[group] {
                remap[group] = self.boundary_group_names.len() as u32;
                self.boundary_group_names.push(name);
            }
        }
        for triangle in &mut mesh.boundary {
            triangle.group = remap[triangle.group as usize];
        }
        mesh.boundary_group_count = self.boundary_group_names.len() as u32;
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

    /// 逐步收集结果场（task 033）：`read_step` 向 `self.fields` 提交一步的字段（经
    /// [`RawGrid::offer_field`] 校验）。`steps` 为时间升序的 (时间, 标签)；更早步的跳过说明加
    /// `[标签]` 前缀。结束后 `fields` 为最新步，两步及以上时填写 `time_series`。
    pub(crate) fn collect_time_steps(
        &mut self,
        steps: &[(f64, String)],
        mut read_step: impl FnMut(&mut Self, usize),
    ) {
        let mut earlier = Vec::new();
        for (index, (time, label)) in steps.iter().enumerate() {
            self.fields.clear();
            let skipped = self.skipped_fields.len();
            read_step(self, index);
            if index + 1 == steps.len() {
                break;
            }
            for note in &mut self.skipped_fields[skipped..] {
                *note = format!("[{label}] {note}");
            }
            earlier.push(TimeStepFields {
                time: *time,
                label: label.clone(),
                fields: std::mem::take(&mut self.fields),
            });
        }
        let Some((latest_time, latest_label)) = steps.last().cloned() else {
            return;
        };
        if earlier.is_empty() {
            return;
        }
        let values: usize = earlier
            .iter()
            .flat_map(|step| &step.fields)
            .map(|field| field.values.len())
            .sum();
        if values > MAX_TIME_STEP_VALUES {
            self.skipped_fields.push(format!(
                "time steps: {} earlier steps hold {values} values (limit {MAX_TIME_STEP_VALUES}); \
                 only the latest step {latest_label} was imported",
                earlier.len()
            ));
            return;
        }
        self.time_series = Some(TimeSeries {
            earlier,
            latest_time,
            latest_label,
        });
    }
}

/// 把一个多面体面流 `[nFaces, n0, ids…]` 校验后以 `[0, 面流…]` 追加到 `connectivity`。
/// 闭合性与朝向由领域构建（`volume_mesh_from_cells`）检查，这里只查结构与节点范围。
fn push_polyhedron(
    index: usize,
    stream: &[u64],
    point_count: usize,
    connectivity: &mut Vec<u32>,
) -> FormatResult<()> {
    let invalid = |what: &str| FormatError::new(format!("cell {index} (polyhedron) {what}"));
    let start = connectivity.len();
    connectivity.push(0);
    for &word in stream {
        connectivity.push(u32::try_from(word).map_err(|_| invalid("face stream value overflow"))?);
    }
    let words = &connectivity[start + 1..];
    if words.is_empty() {
        return Err(invalid("has no face stream"));
    }
    if panta_mesh::face_stream_len(words) != Some(words.len()) {
        return Err(invalid("face stream is malformed"));
    }
    if words[0] < 4 {
        return Err(invalid("has fewer than 4 faces"));
    }
    for face in panta_mesh::faces(words) {
        if face.len() < 3 {
            return Err(invalid("has a face with fewer than 3 nodes"));
        }
        if face.iter().any(|&node| node as usize >= point_count) {
            return Err(invalid("references node outside the node range"));
        }
    }
    Ok(())
}

/// 每个单元的多面体面流 `[nFaces, n0, ids…]`（非多面体单元为空）。
#[derive(Debug, Default)]
pub(crate) struct PolyFaces {
    words: Vec<u64>,
    ends: Vec<usize>,
}

impl PolyFaces {
    /// VTU 旧式数组：`faces` 依次存各多面体面流，`faceoffsets[i]` 为第 i 个单元面流的结束位置，
    /// 非多面体为 -1。
    pub fn from_vtu_faces(faces: Vec<u64>, faceoffsets: &[f64]) -> FormatResult<Self> {
        let mut previous = 0usize;
        let mut ends = Vec::with_capacity(faceoffsets.len());
        for &end in faceoffsets {
            if end >= 0.0 {
                if end.fract() != 0.0 || end > faces.len() as f64 || (end as usize) < previous {
                    return Err(FormatError::new("faceoffsets out of range"));
                }
                previous = end as usize;
            } else if end != -1.0 {
                return Err(FormatError::new("faceoffsets out of range"));
            }
            ends.push(previous);
        }
        if previous != faces.len() {
            return Err(FormatError::new("faces and faceoffsets do not match"));
        }
        Ok(Self { words: faces, ends })
    }

    /// VTK 9.4 数组：`polyhedron_offsets`（每单元面数前缀和，省略首个 0）索引
    /// `polyhedron_to_faces`，其元素是 `face_offsets` / `face_connectivity` 描述的面号。
    pub fn from_vtu_polyhedra(
        polyhedron_offsets: &[u64],
        polyhedron_to_faces: &[u64],
        face_offsets: &[u64],
        face_connectivity: &[u64],
    ) -> FormatResult<Self> {
        let faces = CellList::from_ends(face_connectivity.to_vec(), face_offsets)?;
        let faces: Vec<&[u64]> = faces.iter().collect();
        let polyhedra = CellList::from_ends(polyhedron_to_faces.to_vec(), polyhedron_offsets)?;
        let mut words = Vec::new();
        let mut ends = Vec::with_capacity(polyhedra.len());
        for cell_faces in polyhedra.iter() {
            if !cell_faces.is_empty() {
                words.push(cell_faces.len() as u64);
                for &face in cell_faces {
                    let nodes = usize::try_from(face)
                        .ok()
                        .and_then(|face| faces.get(face))
                        .ok_or_else(|| FormatError::new("polyhedron_to_faces out of range"))?;
                    words.push(nodes.len() as u64);
                    words.extend_from_slice(nodes);
                }
            }
            ends.push(words.len());
        }
        Ok(Self { words, ends })
    }

    pub fn len(&self) -> usize {
        self.ends.len()
    }

    pub fn cell(&self, index: usize) -> &[u64] {
        let start = index
            .checked_sub(1)
            .map_or(0, |previous| self.ends[previous]);
        self.ends
            .get(index)
            .map_or(&[], |&end| &self.words[start..end])
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
