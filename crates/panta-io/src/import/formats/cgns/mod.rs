//! CGNS（HDF5 后端）非结构网格与 FlowSolution 导入（task 032 PR 3）。
//!
//! 读取第一个三维 `CGNSBase_t` 中的全部非结构 `Zone_t`（按名称排序，每个 Zone 成为一个
//! 区域）。`Elements_t` 支持线性 TETRA_4 / PYRA_5 / PENTA_6 / HEXA_8、MIXED（CGNS 3.4 起的
//! `ElementStartOffset` 布局与更早的无偏移布局）以及 NGON_n + NFACE_n 多面体；
//! 线性 TRI_3 / QUAD_4 面单元作为边界面。高阶体单元整体拒绝，不做部分导入。
//! `ZoneBC` 中的 BC 成为边界组：FaceCenter（及旧版 `ElementRange` / `ElementList`）按面单元
//! 编号匹配，Vertex 按节点集合匹配面单元；未被任何 BC 覆盖的面单元截面以截面名成组。
//! `FlowSolution_t` 的 Vertex / CellCenter 数组成为节点 / 单元结果场，`XxxX/Y/Z` 合并为向量。
//! 节点与单元编号沿用 CGNS 局部顺序：CGNS 的四类线性体单元与本项目（VTK）的节点顺序一致。
//! ADF 后端文件直接拒绝并提示用 `cgnsconvert` 转为 HDF5。

use super::{BoundaryFaces, FormatError, FormatResult, RawGrid, checked_point};
use hdf5_pure::{File, Group};
use panta_mesh::{CellType, FieldAssociation, SurfaceMesh};
use std::collections::BTreeMap;

fn invalid(message: impl Into<String>) -> FormatError {
    FormatError::new(message)
}

fn h5<T>(context: &str, result: Result<T, hdf5_pure::Error>) -> FormatResult<T> {
    result.map_err(|error| invalid(format!("{context}: {error}")))
}

const MIXED: i64 = 20;
const NGON_N: i64 = 22;
const NFACE_N: i64 = 23;

/// CGNS `ElementType_t` 的每单元节点数（下标为类型码；MIXED / NGON_n / NFACE_n 为 0）。
const NODES_PER_ELEMENT: [usize; 57] = [
    0, 0, 1, 2, 3, 3, 6, 4, 8, 9, 4, 10, 5, 14, 6, 15, 18, 8, 20, 27, 0, 13, 0, 0, 4, 9, 10, 12,
    16, 16, 20, 21, 29, 30, 24, 38, 40, 32, 56, 64, 5, 12, 15, 16, 25, 22, 34, 35, 29, 50, 55, 33,
    66, 75, 44, 98, 125,
];

fn nodes_per_element(code: i64) -> Option<usize> {
    usize::try_from(code)
        .ok()
        .and_then(|code| NODES_PER_ELEMENT.get(code).copied())
        .filter(|&count| count > 0)
}

fn element_dimension(code: i64) -> u8 {
    match code {
        2 => 0,
        3 | 4 | 24 | 40 => 1,
        5..=9 | 25..=28 | 41..=44 => 2,
        _ => 3,
    }
}

fn linear_cell(code: i64) -> Option<CellType> {
    match code {
        10 => Some(CellType::Tetra),
        12 => Some(CellType::Pyramid),
        14 => Some(CellType::Wedge),
        17 => Some(CellType::Hexahedron),
        _ => None,
    }
}

fn is_linear_face(code: i64) -> bool {
    matches!(code, 5 | 7)
}

/// 一个 CGNS 节点（HDF5 组）：名称、`label` 属性与可选的 `" data"` 数据集。
struct Node {
    name: String,
    group: Group,
}

impl Node {
    fn attr(&self, key: &str) -> String {
        self.group
            .attrs()
            .ok()
            .and_then(|attrs| {
                attrs
                    .get(key)
                    .and_then(|value| value.as_str().map(str::to_owned))
            })
            .unwrap_or_default()
    }

    fn label(&self) -> String {
        self.attr("label")
    }

    /// 子节点按名称排序，使结果与 HDF5 链接顺序无关。
    fn children(&self) -> FormatResult<Vec<Node>> {
        let mut names = h5(&self.name, self.group.groups())?;
        names.sort_unstable();
        names
            .into_iter()
            .map(|name| {
                let group = h5(&name, self.group.group(&name))?;
                Ok(Node { name, group })
            })
            .collect()
    }

    fn children_labelled(&self, label: &str) -> FormatResult<Vec<Node>> {
        Ok(self
            .children()?
            .into_iter()
            .filter(|child| child.label() == label)
            .collect())
    }

    fn child(&self, name: &str) -> Option<Node> {
        self.group.group(name).ok().map(|group| Node {
            name: name.to_owned(),
            group,
        })
    }

    fn data_type(&self) -> String {
        self.attr("type")
    }

    fn ints(&self) -> FormatResult<Vec<i64>> {
        match self.data_type().as_str() {
            "I4" | "I8" => {
                let dataset = h5(&self.name, self.group.dataset(" data"))?;
                h5(&self.name, dataset.read_i64())
            }
            other => Err(invalid(format!(
                "{}: expected integer data, found '{other}'",
                self.name
            ))),
        }
    }

    fn reals(&self) -> FormatResult<Vec<f64>> {
        match self.data_type().as_str() {
            "R4" | "R8" | "I4" | "I8" => {
                let dataset = h5(&self.name, self.group.dataset(" data"))?;
                h5(&self.name, dataset.read_f64())
            }
            other => Err(invalid(format!(
                "{}: expected numeric data, found '{other}'",
                self.name
            ))),
        }
    }

    fn text(&self) -> FormatResult<String> {
        if self.data_type() != "C1" {
            return Err(invalid(format!("{}: expected character data", self.name)));
        }
        let dataset = h5(&self.name, self.group.dataset(" data"))?;
        let bytes = h5(&self.name, dataset.read_u8())?;
        Ok(String::from_utf8_lossy(&bytes)
            .trim_end_matches(['\0', ' '])
            .to_owned())
    }

    /// 定长 32 字符名称数组（`FlowSolutionPointers` 等，C1 `[N][32]`），去掉尾部空白 / NUL。
    fn names(&self) -> FormatResult<Vec<String>> {
        if self.data_type() != "C1" {
            return Err(invalid(format!("{}: expected character data", self.name)));
        }
        let dataset = h5(&self.name, self.group.dataset(" data"))?;
        let bytes = h5(&self.name, dataset.read_u8())?;
        if bytes.len() % 32 != 0 {
            return Err(invalid(format!(
                "{}: name array is not a multiple of 32 characters",
                self.name
            )));
        }
        Ok(bytes
            .chunks(32)
            .map(|name| {
                String::from_utf8_lossy(name)
                    .trim_end_matches(['\0', ' '])
                    .to_owned()
            })
            .collect())
    }

    /// `GridLocation` 子节点（缺省为 Vertex）。
    fn location(&self) -> FormatResult<String> {
        match self.child("GridLocation") {
            Some(node) => node.text(),
            None => Ok("Vertex".to_owned()),
        }
    }
}

/// 打开快照：拒绝 ADF 后端与非 HDF5 文件。
fn open(snapshot: &[u8]) -> FormatResult<Node> {
    if snapshot.get(4..24) == Some(b"ADF Database Version".as_slice()) {
        return Err(invalid(
            "CGNS file uses the ADF backend, which is not supported; convert it to HDF5 \
             first (for example `cgnsconvert -h in.cgns out.cgns`)",
        ));
    }
    if !hdf5_pure::is_hdf5_bytes(snapshot) {
        return Err(invalid("not an HDF5-based CGNS file"));
    }
    let file = h5("CGNS", File::from_bytes(snapshot.to_vec()))?;
    Ok(Node {
        name: "/".to_owned(),
        group: file.root(),
    })
}

/// 第一个（按名称）三维 `CGNSBase_t`；其余基被忽略并记录。
fn base(root: &Node, notes: &mut Vec<String>) -> FormatResult<Node> {
    let mut chosen = None;
    for base in root.children_labelled("CGNSBase_t")? {
        let dims = base.ints()?;
        if chosen.is_none() && dims.first() == Some(&3) {
            chosen = Some(base);
        } else {
            notes.push(format!("CGNSBase '{}' ignored", base.name));
        }
    }
    chosen.ok_or_else(|| invalid("CGNS file has no 3D CGNSBase_t"))
}

/// 面单元表：按单元编号区间保存的面节点（区域局部 0 起编号）。
/// MIXED 截面中的体单元占空面，以保持区间稠密。
struct FaceSection {
    name: String,
    first: i64,
    nodes: Vec<u32>,
    ends: Vec<usize>,
    /// 来自 NGON_n（内部面也在其中，不作为默认边界组）。
    ngon: bool,
}

impl FaceSection {
    fn face(&self, element: i64) -> Option<&[u32]> {
        let index = usize::try_from(element.checked_sub(self.first)?).ok()?;
        let end = *self.ends.get(index)?;
        let start = if index == 0 { 0 } else { self.ends[index - 1] };
        let face = &self.nodes[start..end];
        (!face.is_empty()).then_some(face)
    }

    fn len(&self) -> usize {
        self.ends.len()
    }
}

/// 单个 Zone 的解析结果（节点为区域局部编号）。
struct ZoneMesh {
    name: String,
    points: Vec<[f64; 3]>,
    types: Vec<CellType>,
    /// 固定类型单元的节点；多面体为 `[nFaces, n0, ids…]` 面流。
    cells: Vec<Vec<u32>>,
    faces: Vec<FaceSection>,
    declared_cells: usize,
}

struct Section {
    name: String,
    code: i64,
    first: i64,
    count: usize,
    connectivity: Vec<i64>,
    offsets: Option<Vec<i64>>,
}

fn read_section(node: &Node) -> FormatResult<Section> {
    let header = node.ints()?;
    let code = *header
        .first()
        .ok_or_else(|| invalid(format!("section '{}' has no element type", node.name)))?;
    let range = node
        .child("ElementRange")
        .ok_or_else(|| invalid(format!("section '{}' has no ElementRange", node.name)))?
        .ints()?;
    let (first, last) = match range.as_slice() {
        [first, last] if first <= last && *first >= 1 => (*first, *last),
        _ => {
            return Err(invalid(format!(
                "section '{}' has an invalid range",
                node.name
            )));
        }
    };
    let count = usize::try_from(last - first + 1)
        .map_err(|_| invalid(format!("section '{}' is too large", node.name)))?;
    let connectivity = node
        .child("ElementConnectivity")
        .ok_or_else(|| invalid(format!("section '{}' has no connectivity", node.name)))?
        .ints()?;
    let offsets = match node.child("ElementStartOffset") {
        Some(child) => Some(child.ints()?),
        None => None,
    };
    Ok(Section {
        name: node.name.clone(),
        code,
        first,
        count,
        connectivity,
        offsets,
    })
}

/// 逐单元切分截面连接表：返回 (类型码, 节点表)。MIXED 每单元以类型码开头；
/// NGON_n / NFACE_n 与带偏移的 MIXED 按 `ElementStartOffset`，无偏移布局按类型节点数推进
/// （NGON_n / NFACE_n 旧布局每单元以个数开头）。
fn split_elements(section: &Section) -> FormatResult<Vec<(i64, &[i64])>> {
    let name = &section.name;
    let bad = || invalid(format!("section '{name}' connectivity is malformed"));
    let data = section.connectivity.as_slice();
    let mut elements = Vec::with_capacity(section.count);
    match (section.code, &section.offsets) {
        (MIXED | NGON_N | NFACE_N, Some(offsets)) => {
            if offsets.len() != section.count + 1 || offsets.first() != Some(&0) {
                return Err(bad());
            }
            for pair in offsets.windows(2) {
                let start = usize::try_from(pair[0]).map_err(|_| bad())?;
                let end = usize::try_from(pair[1]).map_err(|_| bad())?;
                let element = data
                    .get(start..end)
                    .filter(|_| start <= end)
                    .ok_or_else(bad)?;
                if section.code == MIXED {
                    let (&code, nodes) = element.split_first().ok_or_else(bad)?;
                    elements.push((code, nodes));
                } else {
                    elements.push((section.code, element));
                }
            }
            if usize::try_from(offsets[section.count]).ok() != Some(data.len()) {
                return Err(bad());
            }
        }
        (MIXED | NGON_N | NFACE_N, None) => {
            let mut cursor = 0usize;
            for _ in 0..section.count {
                let &head = data.get(cursor).ok_or_else(bad)?;
                let (code, count) = if section.code == MIXED {
                    let count = nodes_per_element(head).ok_or_else(|| {
                        invalid(format!(
                            "section '{name}' has unsupported element type {head}"
                        ))
                    })?;
                    (head, count)
                } else {
                    (section.code, usize::try_from(head).map_err(|_| bad())?)
                };
                let nodes = data.get(cursor + 1..cursor + 1 + count).ok_or_else(bad)?;
                elements.push((code, nodes));
                cursor += 1 + count;
            }
            if cursor != data.len() {
                return Err(bad());
            }
        }
        (code, _) => {
            let count = nodes_per_element(code).ok_or_else(|| {
                invalid(format!(
                    "section '{name}' has unsupported element type {code}"
                ))
            })?;
            if Some(data.len()) != section.count.checked_mul(count) {
                return Err(bad());
            }
            elements.extend(data.chunks_exact(count).map(|nodes| (code, nodes)));
        }
    }
    Ok(elements)
}

fn local_node(node: i64, point_count: usize) -> FormatResult<u32> {
    usize::try_from(node - 1)
        .ok()
        .filter(|&index| index < point_count)
        .and_then(|index| u32::try_from(index).ok())
        .ok_or_else(|| invalid(format!("element references node {node} outside the zone")))
}

fn read_zone(zone: &Node) -> FormatResult<ZoneMesh> {
    let size = zone.ints()?;
    let (vertices, declared_cells) = match size.as_slice() {
        [vertices, cells, ..] => (
            usize::try_from(*vertices).map_err(|_| invalid("invalid zone size"))?,
            usize::try_from(*cells).map_err(|_| invalid("invalid zone size"))?,
        ),
        _ => return Err(invalid(format!("zone '{}' has an invalid size", zone.name))),
    };
    let coordinates = zone
        .child("GridCoordinates")
        .ok_or_else(|| invalid(format!("zone '{}' has no GridCoordinates", zone.name)))?;
    let mut axes = Vec::with_capacity(3);
    for axis in ["CoordinateX", "CoordinateY", "CoordinateZ"] {
        let values = coordinates
            .child(axis)
            .ok_or_else(|| {
                invalid(format!(
                    "zone '{}' has no {axis} (only Cartesian coordinates are supported)",
                    zone.name
                ))
            })?
            .reals()?;
        if values.len() != vertices {
            return Err(invalid(format!(
                "zone '{}' {axis} has {} values, expected {vertices}",
                zone.name,
                values.len()
            )));
        }
        axes.push(values);
    }
    let mut points = Vec::with_capacity(vertices);
    for ((x, y), z) in axes[0].iter().zip(&axes[1]).zip(&axes[2]) {
        points.push(checked_point([*x, *y, *z])?);
    }
    let mut sections = zone
        .children_labelled("Elements_t")?
        .iter()
        .map(read_section)
        .collect::<FormatResult<Vec<_>>>()?;
    sections.sort_by_key(|section| section.first);
    let mut mesh = ZoneMesh {
        name: zone.name.clone(),
        points,
        types: Vec::new(),
        cells: Vec::new(),
        faces: Vec::new(),
        declared_cells,
    };
    let mut nface = Vec::new();
    for section in &sections {
        if section.code == NFACE_N {
            nface.push(section);
            continue;
        }
        let elements = split_elements(section)?;
        let mut faces = FaceSection {
            name: section.name.clone(),
            first: section.first,
            nodes: Vec::new(),
            ends: Vec::with_capacity(elements.len()),
            ngon: section.code == NGON_N,
        };
        for (code, nodes) in elements {
            let dimension = element_dimension(code);
            if section.code == NGON_N || is_linear_face(code) {
                if nodes.len() < 3 {
                    return Err(invalid(format!(
                        "section '{}' has a degenerate face",
                        section.name
                    )));
                }
                for &node in nodes {
                    faces.nodes.push(local_node(node, mesh.points.len())?);
                }
            } else if dimension == 3 {
                let Some(kind) = linear_cell(code) else {
                    return Err(invalid(format!(
                        "section '{}' has element type {code}; only linear TETRA_4, PYRA_5, \
                         PENTA_6, HEXA_8 and NGON_n/NFACE_n cells are supported",
                        section.name
                    )));
                };
                let cell = nodes
                    .iter()
                    .map(|&node| local_node(node, mesh.points.len()))
                    .collect::<FormatResult<Vec<_>>>()?;
                if Some(cell.len()) != kind.fixed_node_count() {
                    return Err(invalid(format!(
                        "section '{}' has a short cell",
                        section.name
                    )));
                }
                mesh.types.push(kind);
                mesh.cells.push(cell);
            }
            faces.ends.push(faces.nodes.len());
        }
        if !faces.nodes.is_empty() {
            mesh.faces.push(faces);
        }
    }
    for section in nface {
        for (_, cell_faces) in split_elements(section)? {
            let mut stream = vec![cell_faces.len() as u32];
            for &signed in cell_faces {
                let face = mesh
                    .faces
                    .iter()
                    .filter(|faces| faces.ngon)
                    .find_map(|faces| faces.face(signed.abs()))
                    .ok_or_else(|| {
                        invalid(format!(
                            "section '{}' references missing NGON face {signed}",
                            section.name
                        ))
                    })?;
                stream.push(face.len() as u32);
                if signed < 0 {
                    stream.extend(face.iter().rev());
                } else {
                    stream.extend(face);
                }
            }
            mesh.types.push(CellType::Polyhedron);
            mesh.cells.push(stream);
        }
    }
    if mesh.types.len() != declared_cells {
        return Err(invalid(format!(
            "zone '{}' declares {declared_cells} cells but its sections contain {}",
            zone.name,
            mesh.types.len()
        )));
    }
    Ok(mesh)
}

/// 区域内边界组：(组名, 面节点表)。
fn zone_boundary(
    zone: &Node,
    mesh: &ZoneMesh,
    notes: &mut Vec<String>,
) -> FormatResult<BoundaryGroups> {
    let mut used: Vec<Vec<bool>> = mesh
        .faces
        .iter()
        .map(|faces| vec![false; faces.len()])
        .collect();
    let mut groups = Vec::new();
    let bcs = match zone.child("ZoneBC") {
        Some(node) => node.children_labelled("BC_t")?,
        None => Vec::new(),
    };
    for bc in &bcs {
        let ids = ["PointRange", "ElementRange", "PointList", "ElementList"]
            .iter()
            .find_map(|name| bc.child(name).map(|node| (*name, node)));
        let Some((kind, node)) = ids else {
            notes.push(format!("BC '{}/{}': no point set", mesh.name, bc.name));
            continue;
        };
        let values = node.ints()?;
        let ids: Vec<i64> = if kind.ends_with("Range") {
            match values.as_slice() {
                [first, last] if first <= last => (*first..=*last).collect(),
                _ => return Err(invalid(format!("BC '{}' has an invalid range", bc.name))),
            }
        } else {
            values
        };
        let location = if kind.starts_with("Element") {
            "FaceCenter".to_owned()
        } else {
            bc.location()?
        };
        let mut faces = Vec::new();
        match location.as_str() {
            "FaceCenter" => {
                for &id in &ids {
                    let found = mesh.faces.iter().enumerate().find_map(|(section, faces)| {
                        faces
                            .face(id)
                            .map(|face| (section, (id - faces.first) as usize, face))
                    });
                    match found {
                        Some((section, index, face)) => {
                            used[section][index] = true;
                            faces.push(face.to_vec());
                        }
                        None => {
                            return Err(invalid(format!(
                                "BC '{}' references missing face element {id}",
                                bc.name
                            )));
                        }
                    }
                }
            }
            "Vertex" => {
                let mut member = vec![false; mesh.points.len()];
                for &id in &ids {
                    member[local_node(id, mesh.points.len())? as usize] = true;
                }
                for (table, used) in mesh.faces.iter().zip(used.iter_mut()) {
                    if table.ngon {
                        continue;
                    }
                    for (index, used) in used.iter_mut().enumerate() {
                        if *used {
                            continue;
                        }
                        let Some(face) = table.face(table.first + index as i64) else {
                            continue;
                        };
                        if face.iter().all(|&node| member[node as usize]) {
                            *used = true;
                            faces.push(face.to_vec());
                        }
                    }
                }
            }
            other => {
                notes.push(format!(
                    "BC '{}/{}': location {other} not supported",
                    mesh.name, bc.name
                ));
                continue;
            }
        }
        if faces.is_empty() {
            notes.push(format!(
                "BC '{}/{}': no matching boundary faces",
                mesh.name, bc.name
            ));
        } else {
            groups.push((bc.name.clone(), faces));
        }
    }
    for (section, table) in mesh.faces.iter().enumerate() {
        if table.ngon {
            continue;
        }
        let faces: Vec<Vec<u32>> = (0..table.len())
            .filter(|&index| !used[section][index])
            .filter_map(|index| table.face(table.first + index as i64).map(<[u32]>::to_vec))
            .collect();
        if !faces.is_empty() {
            groups.push((table.name.clone(), faces));
        }
    }
    Ok(groups)
}

/// 一个 Zone 的结果场：键为 (关联, 名称)，值为 (分量数, 数据)。
type ZoneFields = BTreeMap<(FieldOrder, String), (usize, Vec<f64>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FieldOrder {
    Node,
    Cell,
}

impl FieldOrder {
    fn association(self) -> FieldAssociation {
        match self {
            Self::Node => FieldAssociation::Node,
            Self::Cell => FieldAssociation::Cell,
        }
    }
}

/// `XxxX` / `XxxY` / `XxxZ` 三者齐全且长度一致时合并为三分量向量 `Xxx`。
fn combine_vectors(arrays: Vec<(String, Vec<f64>)>) -> Vec<(String, usize, Vec<f64>)> {
    let mut by_name: BTreeMap<String, Vec<f64>> = arrays.into_iter().collect();
    let stems: Vec<String> = by_name
        .keys()
        .filter_map(|name| name.strip_suffix('X'))
        .filter(|stem| !stem.is_empty())
        .map(str::to_owned)
        .collect();
    let mut result = Vec::new();
    for stem in stems {
        let names = [format!("{stem}X"), format!("{stem}Y"), format!("{stem}Z")];
        let lengths: Vec<Option<usize>> = names
            .iter()
            .map(|name| by_name.get(name).map(Vec::len))
            .collect();
        if lengths.iter().any(Option::is_none) || lengths.windows(2).any(|pair| pair[0] != pair[1])
        {
            continue;
        }
        let parts: Vec<Vec<f64>> = names
            .iter()
            .filter_map(|name| by_name.remove(name))
            .collect();
        let tuples = parts[0].len();
        let mut values = Vec::with_capacity(tuples * 3);
        for index in 0..tuples {
            values.extend(parts.iter().map(|part| part[index]));
        }
        result.push((stem, 3, values));
    }
    result.extend(by_name.into_iter().map(|(name, values)| (name, 1, values)));
    result
}

/// `only` 为时间步引用的解名（task 033）；`None` 时读全部解。
fn zone_fields(
    zone: &Node,
    mesh: &ZoneMesh,
    prefix: bool,
    only: Option<&[String]>,
    notes: &mut Vec<String>,
) -> FormatResult<ZoneFields> {
    let mut fields = ZoneFields::new();
    for solution in zone.children_labelled("FlowSolution_t")? {
        if only.is_some_and(|names| !names.contains(&solution.name)) {
            continue;
        }
        let label = |array: &str| format!("{}/{}/{array}", mesh.name, solution.name);
        let location = solution.location()?;
        let (order, tuples) = match location.as_str() {
            "Vertex" => (FieldOrder::Node, mesh.points.len()),
            "CellCenter" => (FieldOrder::Cell, mesh.declared_cells),
            other => {
                notes.push(format!("{}: location {other} not supported", label("*")));
                continue;
            }
        };
        if solution.child("PointRange").is_some() || solution.child("PointList").is_some() {
            notes.push(format!(
                "{}: partial solutions are not supported",
                label("*")
            ));
            continue;
        }
        // Rind（幽灵层）会让数组长度多于单元 / 节点数，首期整组跳过而不猜测裁剪方式。
        if let Some(rind) = solution.child("Rind")
            && rind.ints()?.iter().any(|&layers| layers != 0)
        {
            notes.push(format!("{}: rind layers are not supported", label("*")));
            continue;
        }
        let mut arrays = Vec::new();
        for array in solution.children_labelled("DataArray_t")? {
            match array.reals() {
                Ok(values) if values.len() == tuples => arrays.push((array.name.clone(), values)),
                Ok(values) => notes.push(format!(
                    "{}: {} values, expected {tuples}",
                    label(&array.name),
                    values.len()
                )),
                Err(error) => notes.push(format!("{}: {error}", label(&array.name))),
            }
        }
        for (name, components, values) in combine_vectors(arrays) {
            let name = if prefix {
                format!("{}/{name}", solution.name)
            } else {
                name
            };
            fields.insert((order, name), (components, values));
        }
    }
    Ok(fields)
}

/// 多个同位置 FlowSolution（常见于时间步）时以 `解名/数组名` 区分，避免同名场互相覆盖。
fn needs_prefix(zones: &[Node]) -> FormatResult<bool> {
    for zone in zones {
        let mut seen = Vec::new();
        for solution in zone.children_labelled("FlowSolution_t")? {
            let location = solution.location()?;
            if seen.contains(&location) {
                return Ok(true);
            }
            seen.push(location);
        }
    }
    Ok(false)
}

/// 时间步（task 033）：`BaseIterativeData_t/TimeValues` 给出各步时间，各 Zone
/// `ZoneIterativeData_t` 下的 `FlowSolution*Pointers` 给出每步使用的解名。
struct StepPlan {
    times: Vec<f64>,
    /// 下标依次为 Zone（与 Zone 列表同序）、时间步；值为该步引用的解名。
    solutions: Vec<Vec<Vec<String>>>,
}

/// 读取时间步声明；声明不完整（缺 TimeValues、某 Zone 无指针或长度不符）时记录说明并退回
/// 032 的「全部解按前缀区分」行为，不猜测对应关系。
fn iterative_steps(
    base: &Node,
    zones: &[Node],
    notes: &mut Vec<String>,
) -> FormatResult<Option<StepPlan>> {
    let Some(data) = base
        .children_labelled("BaseIterativeData_t")?
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    let mut fallback = |reason: String| {
        notes.push(format!(
            "time steps: {reason}; solutions are imported as separate fields"
        ));
        Ok(None)
    };
    let times = match data.child("TimeValues") {
        Some(node) => node.reals()?,
        None => return fallback(format!("{} has no TimeValues", data.name)),
    };
    if times.is_empty() || times.iter().any(|time| !time.is_finite()) {
        return fallback("TimeValues is empty or not finite".to_owned());
    }
    let mut solutions = Vec::with_capacity(zones.len());
    for zone in zones {
        let mut per_step = vec![Vec::new(); times.len()];
        let mut found = false;
        for iterative in zone.children_labelled("ZoneIterativeData_t")? {
            for pointers in iterative.children_labelled("DataArray_t")? {
                if !(pointers.name.starts_with("FlowSolution")
                    && pointers.name.ends_with("Pointers"))
                {
                    continue;
                }
                let names = pointers.names()?;
                if names.len() != times.len() {
                    return fallback(format!(
                        "zone '{}' {} has {} entries for {} steps",
                        zone.name,
                        pointers.name,
                        names.len(),
                        times.len()
                    ));
                }
                found = true;
                for (step, name) in per_step.iter_mut().zip(names) {
                    if !name.is_empty() && name != "Null" && !step.contains(&name) {
                        step.push(name);
                    }
                }
            }
        }
        if !found {
            return fallback(format!("zone '{}' has no FlowSolutionPointers", zone.name));
        }
        solutions.push(per_step);
    }
    Ok(Some(StepPlan { times, solutions }))
}

/// 边界组：组名与各面的零基节点表。
type BoundaryGroups = Vec<(String, Vec<Vec<u32>>)>;
/// Zone 网格、边界组与逐时间步的结果场（无时间步声明时只有一项）。
type ZoneRecord = (ZoneMesh, BoundaryGroups, Vec<ZoneFields>);

struct Document {
    zones: Vec<ZoneRecord>,
    /// 时间步的 (时间, 标签)；无时间步声明时为单个占位步。
    steps: Vec<(f64, String)>,
    notes: Vec<String>,
}

fn read_document(snapshot: &[u8], with_fields: bool) -> FormatResult<Document> {
    let root = open(snapshot)?;
    let mut notes = Vec::new();
    let base = base(&root, &mut notes)?;
    let mut zones = Vec::new();
    for zone in base.children_labelled("Zone_t")? {
        let kind = match zone.child("ZoneType") {
            Some(node) => node.text()?,
            None => String::new(),
        };
        if kind == "Unstructured" {
            zones.push(zone);
        } else {
            notes.push(format!(
                "zone '{}' ignored: {kind} zones are not supported",
                zone.name
            ));
        }
    }
    if zones.is_empty() {
        return Err(invalid("CGNS base has no unstructured zones"));
    }
    let plan = if with_fields {
        iterative_steps(&base, &zones, &mut notes)?
    } else {
        None
    };
    let prefix = with_fields && plan.is_none() && needs_prefix(&zones)?;
    let mut parsed = Vec::with_capacity(zones.len());
    for (index, zone) in zones.iter().enumerate() {
        let mesh = read_zone(zone)?;
        let boundary = zone_boundary(zone, &mesh, &mut notes)?;
        let fields = match (&plan, with_fields) {
            (_, false) => vec![ZoneFields::new()],
            (None, true) => vec![zone_fields(zone, &mesh, prefix, None, &mut notes)?],
            (Some(plan), true) => {
                let steps = &plan.solutions[index];
                for solution in zone.children_labelled("FlowSolution_t")? {
                    if !steps.iter().any(|names| names.contains(&solution.name)) {
                        notes.push(format!(
                            "{}/{}: not referenced by FlowSolutionPointers, ignored",
                            zone.name, solution.name
                        ));
                    }
                }
                steps
                    .iter()
                    .map(|names| zone_fields(zone, &mesh, false, Some(names), &mut notes))
                    .collect::<FormatResult<_>>()?
            }
        };
        parsed.push((mesh, boundary, fields));
    }
    let steps = match plan {
        Some(plan) => plan
            .times
            .into_iter()
            .map(|time| (time, format!("{time}")))
            .collect(),
        None => vec![(0.0, String::new())],
    };
    // 同一解被多步引用时，各步读取产生的说明相同，只保留一条。
    let mut seen = std::collections::HashSet::new();
    notes.retain(|note| seen.insert(note.clone()));
    Ok(Document {
        zones: parsed,
        steps,
        notes,
    })
}

/// 体网格：各 Zone 依次追加为区域；边界组名在多 Zone 时为 `zone/组名`。
pub fn parse_cgns_grid(snapshot: &[u8]) -> FormatResult<RawGrid> {
    let document = read_document(snapshot, true)?;
    let multi = document.zones.len() > 1;
    let mut grid = RawGrid::default();
    let mut boundary = BoundaryFaces::default();
    let mut node_offsets = Vec::with_capacity(document.zones.len());
    let mut cell_offsets = Vec::with_capacity(document.zones.len());
    let total_points: usize = document
        .zones
        .iter()
        .map(|(mesh, ..)| mesh.points.len())
        .sum();
    let mut stream = Vec::new();
    for (region, (mesh, groups, _)) in document.zones.iter().enumerate() {
        let offset = u32::try_from(grid.points.len()).map_err(|_| invalid("too many points"))?;
        node_offsets.push(grid.points.len());
        cell_offsets.push(grid.types.len());
        grid.points.extend_from_slice(&mesh.points);
        grid.region_names.push(mesh.name.clone());
        for (kind, cell) in mesh.types.iter().zip(&mesh.cells) {
            let index = grid.types.len();
            if *kind == CellType::Polyhedron {
                stream.clear();
                let mut cursor = 1usize;
                stream.push(u64::from(cell[0]));
                while cursor < cell.len() {
                    let count = cell[cursor] as usize;
                    stream.push(count as u64);
                    stream.extend(
                        cell[cursor + 1..cursor + 1 + count]
                            .iter()
                            .map(|&node| u64::from(node + offset)),
                    );
                    cursor += 1 + count;
                }
                super::push_polyhedron(index, &stream, total_points, &mut grid.connectivity)?;
            } else {
                grid.connectivity
                    .extend(cell.iter().map(|&node| node + offset));
            }
            grid.types.push(*kind);
            grid.cell_regions.push(region as u32);
        }
        for (name, faces) in groups {
            let group = boundary.names.len() as u32;
            boundary.names.push(if multi {
                format!("{}/{name}", mesh.name)
            } else {
                name.clone()
            });
            for face in faces {
                boundary
                    .nodes
                    .extend(face.iter().map(|&node| node + offset));
                boundary.ends.push(boundary.nodes.len());
                boundary.groups.push(group);
            }
        }
    }
    if grid.types.is_empty() {
        return Err(invalid("CGNS zones contain no volume cells"));
    }
    if document.zones.len() == 1 {
        grid.cell_regions.clear();
        grid.region_names.clear();
    }
    grid.boundary = Some(boundary);
    grid.skipped_fields = document.notes;
    grid.collect_time_steps(&document.steps, |grid, step| {
        merge_fields(grid, &document.zones, step, &node_offsets, &cell_offsets);
    });
    Ok(grid)
}

/// 同名场跨 Zone 合并；缺该场的 Zone 填 NaN（作为缺失值统计），分量数冲突时跳过。
fn merge_fields(
    grid: &mut RawGrid,
    zones: &[ZoneRecord],
    step: usize,
    node_offsets: &[usize],
    cell_offsets: &[usize],
) {
    let mut keys: Vec<&(FieldOrder, String)> = zones
        .iter()
        .flat_map(|(.., fields)| fields[step].keys())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    for key in keys {
        let (order, name) = key;
        let total = match order {
            FieldOrder::Node => grid.points.len(),
            FieldOrder::Cell => grid.cell_count(),
        };
        let mut merged: Option<(usize, Vec<f64>)> = None;
        let mut conflict = false;
        for (index, (.., fields)) in zones.iter().enumerate() {
            let Some((components, values)) = fields[step].get(key) else {
                continue;
            };
            let target =
                merged.get_or_insert_with(|| (*components, vec![f64::NAN; total * components]));
            if target.0 != *components {
                conflict = true;
                break;
            }
            let start = match order {
                FieldOrder::Node => node_offsets[index],
                FieldOrder::Cell => cell_offsets[index],
            } * components;
            target.1[start..start + values.len()].copy_from_slice(values);
        }
        if conflict {
            grid.skipped_fields.push(format!(
                "{name} ({}): component count differs between zones",
                order.association().as_str()
            ));
            continue;
        }
        if let Some((components, values)) = merged {
            grid.offer_field(name, order.association(), components, Ok(values));
        }
    }
}

/// 表面模式：体单元外表面（与 VTU 相同的面配对），不读结果场。
pub fn parse_cgns_surface(snapshot: &[u8]) -> FormatResult<SurfaceMesh> {
    let document = read_document(snapshot, false)?;
    let mut mesh = SurfaceMesh {
        triangles: Vec::new(),
    };
    for (zone, ..) in &document.zones {
        let mut surface = super::cells::CellSurface::new(&zone.points);
        let mut nodes: Vec<u64> = Vec::new();
        for (kind, cell) in zone.types.iter().zip(&zone.cells) {
            nodes.clear();
            nodes.extend(cell.iter().map(|&node| u64::from(node)));
            if *kind == CellType::Polyhedron {
                surface.add_polyhedron(&nodes)?;
            } else {
                surface.add(*kind as u8, &nodes, &mut mesh)?;
            }
        }
        surface.finish(&mut mesh)?;
    }
    super::finish_mesh(mesh)
}

#[cfg(test)]
mod tests;
