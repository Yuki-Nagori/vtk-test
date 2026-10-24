//! OpenFOAM polyMesh 与体结果场导入（task 032）。
//!
//! 输入是 [`bundle`] 编码的算例快照：各区域的 `points` / `faces` / `owner` /
//! `neighbour` / `boundary`（ASCII、binary 或 `.gz`），以及最新时间目录中的
//! `volScalarField` / `volVectorField`。单元由 owner / neighbour 面集合重建：
//! 能识别为 tet / pyramid / wedge / hex 的按标准单元保存，其余按多面体面流保存；
//! 面朝向统一为外法向（owner 原样，neighbour 反转）。边界 patch 成为边界组，
//! 多区域（`constant/<region>/polyMesh`）成为区域，组名为 `区域/patch`。

pub mod bundle;
mod lexer;

use super::{BoundaryFaces, FormatError, FormatResult, IndexedPoints, RawGrid, checked_point};
use lexer::{Lexer, Token};
use panta_mesh::{CellType, FieldAssociation, SurfaceMesh};
use std::borrow::Cow;
use std::collections::BTreeMap;

fn invalid(message: impl Into<String>) -> FormatError {
    FormatError::new(message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Patch {
    name: String,
    start: usize,
    count: usize,
}

struct RegionMesh {
    name: String,
    points: Vec<[f64; 3]>,
    face_nodes: Vec<u32>,
    face_ends: Vec<usize>,
    owner: Vec<u32>,
    neighbour: Vec<u32>,
    cell_count: usize,
    patches: Vec<Patch>,
}

impl RegionMesh {
    fn face(&self, face: usize) -> &[u32] {
        let start = if face == 0 {
            0
        } else {
            self.face_ends[face - 1]
        };
        &self.face_nodes[start..self.face_ends[face]]
    }
}

/// 体场文件：保持快照中的原始（可能压缩）字节，合并时才逐个解压，避免多时间步同时展开。
struct FieldFile<'a> {
    time: String,
    region: String,
    name: String,
    relative: &'a str,
    bytes: &'a [u8],
}

struct Case<'a> {
    regions: Vec<RegionMesh>,
    fields: Vec<FieldFile<'a>>,
    skipped: Vec<String>,
}

fn decompress<'a>(relative: &str, bytes: &'a [u8]) -> FormatResult<Cow<'a, [u8]>> {
    if !relative.ends_with(".gz") {
        return Ok(Cow::Borrowed(bytes));
    }
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|error| invalid(format!("{relative}: gzip error: {error}")))?;
    Ok(Cow::Owned(out))
}

fn context<T>(relative: &str, result: FormatResult<T>) -> FormatResult<T> {
    result.map_err(|error| invalid(format!("{relative}: {}", error.0)))
}

/// 一个区域的 polyMesh 文件：文件名（去 `.gz`）→ (包内相对路径, 解压后字节)。
type MeshFiles<'a> = BTreeMap<&'a str, (String, Cow<'a, [u8]>)>;

/// 单元面 CSR：单元起点偏移与 (面下标, 是否为 neighbour 侧)。
type CellFaceTable = (Vec<usize>, Vec<(u32, bool)>);

fn read_case(snapshot: &[u8]) -> FormatResult<Case<'_>> {
    let entries = bundle::decode(snapshot)?;
    let mut meshes: BTreeMap<String, MeshFiles<'_>> = BTreeMap::new();
    let mut fields = Vec::new();
    let mut skipped = Vec::new();
    for entry in &entries {
        let parts: Vec<&str> = entry.relative.split('/').collect();
        match parts.as_slice() {
            ["constant", "polyMesh", file] => {
                let name = file.strip_suffix(".gz").unwrap_or(file);
                meshes.entry(String::new()).or_default().insert(
                    name,
                    (
                        entry.relative.to_owned(),
                        decompress(entry.relative, entry.bytes)?,
                    ),
                );
            }
            ["constant", region, "polyMesh", file] => {
                let name = file.strip_suffix(".gz").unwrap_or(file);
                meshes.entry((*region).to_owned()).or_default().insert(
                    name,
                    (
                        entry.relative.to_owned(),
                        decompress(entry.relative, entry.bytes)?,
                    ),
                );
            }
            ["skipped", "@time-steps"] => {
                skipped.push(String::from_utf8_lossy(entry.bytes).into_owned());
            }
            ["skipped", rest @ ..] => {
                let class = String::from_utf8_lossy(entry.bytes);
                skipped.push(format!(
                    "{} (cell): OpenFOAM {class} is not supported",
                    rest.join("/")
                ));
            }
            [_, file] | [_, _, file] => {
                let region = if parts.len() == 3 { parts[1] } else { "" };
                fields.push(FieldFile {
                    time: parts[0].to_owned(),
                    region: region.to_owned(),
                    name: file.strip_suffix(".gz").unwrap_or(file).to_owned(),
                    relative: entry.relative,
                    bytes: entry.bytes,
                });
            }
            _ => {
                return Err(invalid(format!(
                    "unexpected snapshot entry {}",
                    entry.relative
                )));
            }
        }
    }
    if meshes.is_empty() {
        return Err(invalid("case has no polyMesh"));
    }
    let mut regions = Vec::with_capacity(meshes.len());
    for (name, files) in meshes {
        let file = |key: &str| {
            files
                .get(key)
                .ok_or_else(|| invalid(format!("polyMesh of region '{name}' is missing {key}")))
        };
        let (path, bytes) = file("points")?;
        let points = context(path, parse_points(bytes))?;
        let (path, bytes) = file("faces")?;
        let (face_nodes, face_ends) = context(path, parse_faces(bytes, points.len()))?;
        let (path, bytes) = file("owner")?;
        let owner = context(path, parse_labels(bytes))?;
        let neighbour = match files.get("neighbour") {
            Some((path, bytes)) => context(path, parse_labels(bytes))?,
            None => Vec::new(),
        };
        let (path, bytes) = file("boundary")?;
        let patches = context(path, parse_boundary(bytes))?;
        regions.push(build_region(
            name, points, face_nodes, face_ends, owner, neighbour, patches,
        )?);
    }
    Ok(Case {
        regions,
        fields,
        skipped,
    })
}

fn parse_points(bytes: &[u8]) -> FormatResult<Vec<[f64; 3]>> {
    let mut lexer = Lexer::new(bytes)?;
    let values = lexer.scalars(3)?;
    values
        .as_chunks::<3>()
        .0
        .iter()
        .map(|point| checked_point(*point))
        .collect()
}

fn to_u32(values: Vec<u64>) -> FormatResult<Vec<u32>> {
    values
        .into_iter()
        .map(|value| u32::try_from(value).map_err(|_| invalid("label exceeds 32-bit index range")))
        .collect()
}

fn parse_labels(bytes: &[u8]) -> FormatResult<Vec<u32>> {
    to_u32(Lexer::new(bytes)?.labels()?)
}

fn parse_faces(bytes: &[u8], point_count: usize) -> FormatResult<(Vec<u32>, Vec<usize>)> {
    let (nodes, ends) = Lexer::new(bytes)?.faces()?;
    let nodes = to_u32(nodes)?;
    if nodes.iter().any(|&node| node as usize >= point_count) {
        return Err(invalid("face references a point outside the point list"));
    }
    let mut start = 0;
    for &end in &ends {
        if end < start + 3 {
            return Err(invalid("face has fewer than 3 points"));
        }
        start = end;
    }
    Ok((nodes, ends))
}

fn parse_boundary(bytes: &[u8]) -> FormatResult<Vec<Patch>> {
    let mut lexer = Lexer::new(bytes)?;
    if !matches!(lexer.peek_punct(), Some(b'(')) {
        lexer.label()?;
    }
    lexer.expect(b'(')?;
    let mut patches = Vec::new();
    loop {
        let name = match lexer.next()? {
            Some(Token::Punct(b')')) => return Ok(patches),
            Some(Token::Word(name)) => name.to_owned(),
            other => return Err(invalid(format!("unexpected {other:?} in boundary list"))),
        };
        lexer.expect(b'{')?;
        let (mut start, mut count) = (None, None);
        loop {
            match lexer.next()? {
                Some(Token::Punct(b'}')) => break,
                Some(Token::Word("startFace")) => {
                    start = Some(lexer.label()?);
                    lexer.expect(b';')?;
                }
                Some(Token::Word("nFaces")) => {
                    count = Some(lexer.label()?);
                    lexer.expect(b';')?;
                }
                Some(Token::Word(_)) => lexer.skip_entry()?,
                other => return Err(invalid(format!("unexpected {other:?} in patch {name}"))),
            }
        }
        let (Some(start), Some(count)) = (start, count) else {
            return Err(invalid(format!("patch {name} lacks startFace / nFaces")));
        };
        patches.push(Patch { name, start, count });
    }
}

fn build_region(
    name: String,
    points: Vec<[f64; 3]>,
    face_nodes: Vec<u32>,
    face_ends: Vec<usize>,
    owner: Vec<u32>,
    neighbour: Vec<u32>,
    patches: Vec<Patch>,
) -> FormatResult<RegionMesh> {
    let label = if name.is_empty() {
        "polyMesh".to_owned()
    } else {
        name.clone()
    };
    let face_count = face_ends.len();
    if owner.len() != face_count {
        return Err(invalid(format!(
            "{label}: owner has {} entries for {face_count} faces",
            owner.len()
        )));
    }
    if neighbour.len() > face_count {
        return Err(invalid(format!("{label}: neighbour is longer than faces")));
    }
    let cell_count = owner
        .iter()
        .chain(&neighbour)
        .max()
        .map_or(0, |&cell| cell as usize + 1);
    if cell_count == 0 {
        return Err(invalid(format!("{label}: mesh has no cells")));
    }
    let internal = neighbour.len();
    for patch in &patches {
        let end = patch.start.checked_add(patch.count);
        if patch.start < internal || end.is_none_or(|end| end > face_count) {
            return Err(invalid(format!(
                "{label}: patch {} faces {}..+{} are outside the boundary face range",
                patch.name, patch.start, patch.count
            )));
        }
    }
    Ok(RegionMesh {
        name,
        points,
        face_nodes,
        face_ends,
        owner,
        neighbour,
        cell_count,
        patches,
    })
}

/// 单元的外法向面（扁平存放）。
#[derive(Default)]
struct CellFaces {
    nodes: Vec<u32>,
    ends: Vec<usize>,
}

impl CellFaces {
    fn clear(&mut self) {
        self.nodes.clear();
        self.ends.clear();
    }

    fn push(&mut self, face: &[u32], reverse: bool) {
        if reverse {
            // 反转但保持首节点：n0 nk … n1。
            self.nodes.push(face[0]);
            self.nodes.extend(face[1..].iter().rev());
        } else {
            self.nodes.extend_from_slice(face);
        }
        self.ends.push(self.nodes.len());
    }

    fn iter(&self) -> impl Iterator<Item = &[u32]> {
        let mut start = 0;
        self.ends.iter().map(move |&end| {
            let face = &self.nodes[start..end];
            start = end;
            face
        })
    }

    fn sizes(&self) -> impl Iterator<Item = usize> + '_ {
        self.iter().map(<[u32]>::len)
    }

    /// 与 `base` 中节点 `node` 相连、但不在 `base` 中的唯一节点。
    fn partner(&self, node: u32, base: &[u32]) -> Option<u32> {
        let mut found = None;
        for face in self.iter() {
            for (index, &candidate) in face.iter().enumerate() {
                if candidate != node {
                    continue;
                }
                let previous = face[(index + face.len() - 1) % face.len()];
                let next = face[(index + 1) % face.len()];
                for other in [previous, next] {
                    if base.contains(&other) {
                        continue;
                    }
                    match found {
                        None => found = Some(other),
                        Some(existing) if existing == other => {}
                        Some(_) => return None,
                    }
                }
            }
        }
        found
    }

    fn sorted_sets(&self) -> Vec<Vec<u32>> {
        let mut sets: Vec<Vec<u32>> = self
            .iter()
            .map(|face| {
                let mut face = face.to_vec();
                face.sort_unstable();
                face
            })
            .collect();
        sets.sort();
        sets
    }
}

/// 按 VTK 局部编号识别标准单元；面集合不完全一致时返回 `None`（按多面体保存）。
fn classify(faces: &CellFaces) -> Option<(CellType, Vec<u32>)> {
    let (mut triangles, mut quads) = (0, 0);
    for size in faces.sizes() {
        match size {
            3 => triangles += 1,
            4 => quads += 1,
            _ => return None,
        }
    }
    let first = |size: usize| faces.iter().find(|face| face.len() == size);
    let reversed = |face: &[u32]| face.iter().rev().copied().collect::<Vec<_>>();
    let (kind, nodes) = match (faces.ends.len(), triangles, quads) {
        (4, 4, 0) => {
            let base = reversed(first(3)?);
            let apex = faces
                .nodes
                .iter()
                .copied()
                .find(|node| !base.contains(node))?;
            (CellType::Tetra, [base, vec![apex]].concat())
        }
        (5, 4, 1) => {
            let base = reversed(first(4)?);
            let apex = faces
                .nodes
                .iter()
                .copied()
                .find(|node| !base.contains(node))?;
            (CellType::Pyramid, [base, vec![apex]].concat())
        }
        (5, 2, 3) => {
            let base = first(3)?.to_vec();
            let top = base
                .iter()
                .map(|&node| faces.partner(node, &base))
                .collect::<Option<Vec<_>>>()?;
            (CellType::Wedge, [base, top].concat())
        }
        (6, 0, 6) => {
            let base = reversed(first(4)?);
            let top = base
                .iter()
                .map(|&node| faces.partner(node, &base))
                .collect::<Option<Vec<_>>>()?;
            (CellType::Hexahedron, [base, top].concat())
        }
        _ => return None,
    };
    let mut unique = nodes.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != nodes.len() {
        return None;
    }
    (expected_faces(kind, &nodes) == faces.sorted_sets()).then_some((kind, nodes))
}

fn expected_faces(kind: CellType, nodes: &[u32]) -> Vec<Vec<u32>> {
    let pick = |indices: &[usize]| {
        let mut face: Vec<u32> = indices.iter().map(|&index| nodes[index]).collect();
        face.sort_unstable();
        face
    };
    let mut sets: Vec<Vec<u32>> = match kind {
        CellType::Tetra => vec![
            pick(&[0, 1, 2]),
            pick(&[0, 1, 3]),
            pick(&[1, 2, 3]),
            pick(&[0, 2, 3]),
        ],
        CellType::Pyramid => vec![
            pick(&[0, 1, 2, 3]),
            pick(&[0, 1, 4]),
            pick(&[1, 2, 4]),
            pick(&[2, 3, 4]),
            pick(&[3, 0, 4]),
        ],
        CellType::Wedge => vec![
            pick(&[0, 1, 2]),
            pick(&[3, 4, 5]),
            pick(&[0, 1, 4, 3]),
            pick(&[1, 2, 5, 4]),
            pick(&[2, 0, 3, 5]),
        ],
        _ => {
            let mut sets = vec![pick(&[0, 1, 2, 3]), pick(&[4, 5, 6, 7])];
            for side in 0..4 {
                let next = (side + 1) % 4;
                sets.push(pick(&[side, next, next + 4, side + 4]));
            }
            sets
        }
    };
    sets.sort();
    sets
}

/// 区域内每个单元的面（CSR：单元 → (面, 是否为 neighbour 侧)）。
fn cell_faces(region: &RegionMesh) -> FormatResult<CellFaceTable> {
    let mut counts = vec![0usize; region.cell_count + 1];
    for &cell in region.owner.iter().chain(&region.neighbour) {
        counts[cell as usize + 1] += 1;
    }
    for cell in 0..region.cell_count {
        if counts[cell + 1] < 4 {
            return Err(invalid(format!(
                "cell {cell} has {} faces; a closed cell needs at least 4",
                counts[cell + 1]
            )));
        }
        counts[cell + 1] += counts[cell];
    }
    let mut cursor = counts.clone();
    let mut faces = vec![(0u32, false); counts[region.cell_count]];
    for (face, &cell) in region.owner.iter().enumerate() {
        faces[cursor[cell as usize]] = (face as u32, false);
        cursor[cell as usize] += 1;
    }
    for (face, &cell) in region.neighbour.iter().enumerate() {
        faces[cursor[cell as usize]] = (face as u32, true);
        cursor[cell as usize] += 1;
    }
    Ok((counts, faces))
}

/// 领域多面体拆分（task 031）从每面最小节点号起扇形三角化，只依赖拓扑。OpenFOAM 面常含
/// 共线节点（对偶网格、加密后的边中点），最小节点恰在共线边上时会产生零面积三角形，
/// 进而产生零体积四面体。对这类面，按节点号升序找第一个扇形无退化的起点，把面改写为
/// 显式三角形子面；起点只由节点号与坐标决定，owner / neighbour 两侧得到同一组三角形，
/// 保持共形。找不到合格起点时保留原面（由领域校验报告）。
fn degenerate_face_fans(region: &RegionMesh) -> std::collections::HashMap<u32, Vec<[u32; 3]>> {
    let mut fans = std::collections::HashMap::new();
    for face in 0..region.face_ends.len() {
        let nodes = region.face(face);
        if nodes.len() < 4 {
            continue;
        }
        let fan_ok = |origin: usize| {
            let point =
                |index: usize| region.points[nodes[(origin + index) % nodes.len()] as usize];
            let first = point(0);
            (1..nodes.len() - 1).all(|step| {
                let (a, b) = (point(step), point(step + 1));
                let u = [a[0] - first[0], a[1] - first[1], a[2] - first[2]];
                let v = [b[0] - first[0], b[1] - first[1], b[2] - first[2]];
                let cross = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let area2 = cross.iter().map(|c| c * c).sum::<f64>();
                let scale = u.iter().chain(&v).map(|c| c * c).sum::<f64>();
                area2 > 1.0e-16 * scale * scale
            })
        };
        let mut order: Vec<usize> = (0..nodes.len()).collect();
        order.sort_by_key(|&index| nodes[index]);
        if fan_ok(order[0]) {
            continue;
        }
        if let Some(&origin) = order[1..].iter().find(|&&index| fan_ok(index)) {
            let at = |index: usize| nodes[(origin + index) % nodes.len()];
            let triangles = (1..nodes.len() - 1)
                .map(|step| [at(0), at(step), at(step + 1)])
                .collect();
            fans.insert(face as u32, triangles);
        }
    }
    fans
}

/// 由 `dimensions [kg m s K mol A cd]` 生成 SI 单位串，例如 `m s^-1`、`K`；无量纲为空串。
fn si_units(exponents: &[f64]) -> String {
    const BASE: [&str; 7] = ["kg", "m", "s", "K", "mol", "A", "cd"];
    let mut parts = Vec::new();
    for (symbol, &exponent) in BASE.iter().zip(exponents) {
        if exponent == 0.0 {
            continue;
        }
        if exponent == 1.0 {
            parts.push((*symbol).to_owned());
        } else if exponent.fract() == 0.0 {
            parts.push(format!("{symbol}^{}", exponent as i64));
        } else {
            parts.push(format!("{symbol}^{exponent}"));
        }
    }
    parts.join(" ")
}

struct ParsedField {
    components: usize,
    units: String,
    values: Vec<f64>,
}

/// 读取体场的 `dimensions` 与 `internalField`（`uniform` 按单元数展开）；不读 boundaryField。
fn parse_field(bytes: &[u8], cell_count: usize) -> FormatResult<ParsedField> {
    let mut lexer = Lexer::new(bytes)?;
    let components = match lexer.header.class.as_str() {
        "volScalarField" => 1,
        "volVectorField" => 3,
        other => return Err(invalid(format!("unsupported field class {other}"))),
    };
    let mut units = String::new();
    loop {
        match lexer.next()? {
            None => return Err(invalid("internalField not found")),
            Some(Token::Word("dimensions")) => {
                lexer.expect(b'[')?;
                let mut exponents = Vec::new();
                while lexer.peek_punct() != Some(b']') {
                    exponents.push(lexer.number()?);
                }
                lexer.expect(b']')?;
                lexer.expect(b';')?;
                units = si_units(&exponents);
            }
            Some(Token::Word("internalField")) => {
                let values = match lexer.word()? {
                    "uniform" => {
                        let mut tuple = Vec::with_capacity(components);
                        lexer.ascii_tuple(components, &mut tuple)?;
                        tuple.repeat(cell_count)
                    }
                    "nonuniform" => {
                        let kind = lexer.word()?;
                        let expected = if components == 1 {
                            "List<scalar>"
                        } else {
                            "List<vector>"
                        };
                        if kind != expected {
                            return Err(invalid(format!("unexpected internalField type {kind}")));
                        }
                        lexer.scalars(components)?
                    }
                    other => return Err(invalid(format!("unexpected internalField form {other}"))),
                };
                return Ok(ParsedField {
                    components,
                    units,
                    values,
                });
            }
            Some(Token::Word(_)) => lexer.skip_entry()?,
            Some(other) => return Err(invalid(format!("unexpected {other:?}"))),
        }
    }
}

/// 体网格：重建单元、区域、边界组与单元结果场。
pub fn parse_openfoam_grid(snapshot: &[u8]) -> FormatResult<RawGrid> {
    let case = read_case(snapshot)?;
    let multi = case.regions.len() > 1 || case.regions.iter().any(|region| !region.name.is_empty());
    let mut grid = RawGrid::default();
    let mut boundary = BoundaryFaces::default();
    let mut region_offsets = Vec::with_capacity(case.regions.len() + 1);
    let mut faces = CellFaces::default();
    let mut stream: Vec<u64> = Vec::new();
    let mut face_quality = panta_mesh::FaceQuality::default();
    let total_points: usize = case.regions.iter().map(|region| region.points.len()).sum();
    for (region_index, region) in case.regions.iter().enumerate() {
        let node_offset =
            u32::try_from(grid.points.len()).map_err(|_| invalid("too many points"))?;
        region_offsets.push(grid.types.len());
        grid.points.extend_from_slice(&region.points);
        grid.region_names.push(if region.name.is_empty() {
            "internalMesh".to_owned()
        } else {
            region.name.clone()
        });
        face_quality.merge(&panta_mesh::poly_mesh_face_quality(
            &region.points,
            &region.face_ends,
            &region.face_nodes,
            &region.owner,
            &region.neighbour,
            region.cell_count,
        ));
        let (starts, cell_faces) = cell_faces(region)?;
        let fans = degenerate_face_fans(region);
        for cell in 0..region.cell_count {
            faces.clear();
            let mut has_fan = false;
            for &(face, reverse) in &cell_faces[starts[cell]..starts[cell + 1]] {
                match fans.get(&face) {
                    Some(triangles) => {
                        has_fan = true;
                        for triangle in triangles {
                            faces.push(triangle, reverse);
                        }
                    }
                    None => faces.push(region.face(face as usize), reverse),
                }
            }
            let global_index = grid.types.len();
            let standard = if has_fan { None } else { classify(&faces) };
            if let Some((kind, nodes)) = standard {
                grid.types.push(kind);
                grid.connectivity
                    .extend(nodes.iter().map(|&node| node + node_offset));
            } else {
                stream.clear();
                stream.push(faces.ends.len() as u64);
                for face in faces.iter() {
                    stream.push(face.len() as u64);
                    stream.extend(face.iter().map(|&node| u64::from(node + node_offset)));
                }
                super::push_polyhedron(
                    global_index,
                    &stream,
                    total_points,
                    &mut grid.connectivity,
                )?;
                grid.types.push(CellType::Polyhedron);
            }
            grid.cell_regions.push(region_index as u32);
        }
        for patch in &region.patches {
            if patch.count == 0 {
                continue;
            }
            let group = boundary.names.len() as u32;
            boundary.names.push(if multi {
                format!("{}/{}", region.name, patch.name)
            } else {
                patch.name.clone()
            });
            for face in patch.start..patch.start + patch.count {
                boundary
                    .nodes
                    .extend(region.face(face).iter().map(|&node| node + node_offset));
                boundary.ends.push(boundary.nodes.len());
                boundary.groups.push(group);
            }
        }
    }
    region_offsets.push(grid.types.len());
    grid.boundary = Some(boundary);
    grid.face_quality = Some(face_quality);
    grid.skipped_fields = case.skipped;
    // 时间目录名按数值升序成步（task 033）；单个时间目录时与 032 行为一致。
    let mut steps: Vec<(f64, String)> = Vec::new();
    for file in &case.fields {
        if !steps.iter().any(|(_, label)| *label == file.time) {
            let time = file.time.parse::<f64>().unwrap_or(f64::NAN);
            steps.push((time, file.time.clone()));
        }
    }
    steps.sort_by(|left, right| left.0.total_cmp(&right.0));
    grid.collect_time_steps(&steps, |grid, index| {
        let files: Vec<&FieldFile<'_>> = case
            .fields
            .iter()
            .filter(|file| file.time == steps[index].1)
            .collect();
        merge_fields(grid, &case.regions, &files, &region_offsets);
    });
    Ok(grid)
}

/// 同名场跨区域合并为一个单元场；缺该场的区域填 NaN（作为缺失值统计）。
fn merge_fields(
    grid: &mut RawGrid,
    regions: &[RegionMesh],
    files: &[&FieldFile<'_>],
    offsets: &[usize],
) {
    let total = grid.cell_count();
    let mut names: Vec<&str> = files.iter().map(|file| file.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    for name in names {
        let mut merged: Option<ParsedField> = None;
        let mut failure = None;
        for (index, region) in regions.iter().enumerate() {
            let Some(file) = files
                .iter()
                .find(|file| file.name == name && file.region == region.name)
            else {
                continue;
            };
            let parsed = match decompress(file.relative, file.bytes)
                .and_then(|bytes| parse_field(&bytes, region.cell_count))
            {
                Ok(parsed) if parsed.values.len() == region.cell_count * parsed.components => {
                    parsed
                }
                Ok(parsed) => {
                    failure = Some(format!(
                        "{} values in region '{}', expected {} x {}",
                        parsed.values.len(),
                        region.name,
                        region.cell_count,
                        parsed.components
                    ));
                    break;
                }
                Err(error) => {
                    failure = Some(error.0);
                    break;
                }
            };
            let target = merged.get_or_insert_with(|| ParsedField {
                components: parsed.components,
                units: parsed.units.clone(),
                values: vec![f64::NAN; total * parsed.components],
            });
            if target.components != parsed.components {
                failure = Some("component count differs between regions".to_owned());
                break;
            }
            let start = offsets[index] * parsed.components;
            target.values[start..start + parsed.values.len()].copy_from_slice(&parsed.values);
        }
        let label = format!("{name} (cell)");
        match (failure, merged) {
            (Some(reason), _) => grid.skipped_fields.push(format!("{label}: {reason}")),
            (None, Some(field)) => {
                let before = grid.fields.len();
                grid.offer_field(
                    name,
                    FieldAssociation::Cell,
                    field.components,
                    Ok(field.values),
                );
                if let Some(added) = grid.fields.get_mut(before) {
                    added.units = field.units;
                }
            }
            (None, None) => {}
        }
    }
}

/// 表面模式：全部区域的边界 patch 面扇形三角化。
pub fn parse_openfoam_surface(snapshot: &[u8]) -> FormatResult<SurfaceMesh> {
    let case = read_case(snapshot)?;
    let mut mesh = SurfaceMesh {
        triangles: Vec::new(),
    };
    let mut nodes = Vec::new();
    for region in &case.regions {
        let indexed = IndexedPoints {
            points: &region.points,
        };
        for patch in &region.patches {
            for face in patch.start..patch.start + patch.count {
                nodes.clear();
                nodes.extend(region.face(face).iter().map(|&node| u64::from(node)));
                indexed.polygon(&nodes, &mut mesh)?;
            }
        }
    }
    super::finish_mesh(mesh)
}

#[cfg(test)]
mod tests;
