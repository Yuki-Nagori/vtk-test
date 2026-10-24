//! 原生混合单元（task 030 / 031）：VTK 风格的单元类型、局部节点顺序与共形四面体拆分。
//!
//! 局部节点顺序与 VTK 线性单元一致（见 `ai-docs/architecture/mesh.md`）：
//! - Tetra（10）：0,1,2 逆时针朝向节点 3；参考坐标 (0,0,0)(1,0,0)(0,1,0)(0,0,1)。
//! - Hexahedron（12）：底面 0,1,2,3 逆时针朝向顶面，顶面 4..7 依次位于 0..3 上方。
//! - Wedge（13）：底面三角形 0,1,2 的右手法向指向顶面 3,4,5，3..5 依次位于 0..2 上方。
//! - Pyramid（14）：底面 0,1,2,3 的右手法向指向锥顶 4。
//! - Polyhedron（42，task 031）：规范连接为 `[apex, nFaces, n0, ids…, n1, ids…]`，即 VTK
//!   旧版序列化面流前加辅助中心节点 `apex`；各面按右手法则朝外，面集合是闭合、一致定向的曲面。
//!
//! 拆分只用于切面、体积与质量计算等内部算法；单元 ID 始终是原始单元下标。
//! 四边形面的对角线取经过面上全局节点号最小者的那一条（Dompierre 等的最小下标规则），
//! 相邻单元因此在共享面上选同一条对角线，拆分结果共形且只由全局节点号决定，
//! 加载时可确定性重算，不写入资产。多面体的每个面从最小全局节点号出发扇形三角化
//! （四边形面与上述规则一致），每个三角形与 `apex` 组成一个四面体。

use crate::{MeshIndex, Tetrahedron};

/// VTK 多面体单元编码。
pub const VTK_POLYHEDRON: u8 = 42;

/// 受支持的单元类型数，即 [`CellType::ALL`] 的长度。
pub const CELL_TYPE_COUNT: usize = 5;

/// 线性体单元类型，判别值即 VTK 单元编码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum CellType {
    Tetra = 10,
    Hexahedron = 12,
    Wedge = 13,
    Pyramid = 14,
    Polyhedron = 42,
}

impl CellType {
    /// 按 VTK 编码顺序列出全部受支持的类型。
    pub const ALL: [Self; CELL_TYPE_COUNT] = [
        Self::Tetra,
        Self::Hexahedron,
        Self::Wedge,
        Self::Pyramid,
        Self::Polyhedron,
    ];

    pub fn from_vtk(code: u8) -> Option<Self> {
        Some(match code {
            10 => Self::Tetra,
            12 => Self::Hexahedron,
            13 => Self::Wedge,
            14 => Self::Pyramid,
            VTK_POLYHEDRON => Self::Polyhedron,
            _ => return None,
        })
    }

    pub fn vtk_code(self) -> u8 {
        self as u8
    }

    /// 固定节点数；多面体节点数可变，返回 `None`。
    pub fn fixed_node_count(self) -> Option<usize> {
        match self {
            Self::Tetra => Some(4),
            Self::Hexahedron => Some(8),
            Self::Wedge => Some(6),
            Self::Pyramid => Some(5),
            Self::Polyhedron => None,
        }
    }

    /// 在 [`Self::ALL`] 中的下标，便于按类型计数。
    pub fn ordinal(self) -> usize {
        match self {
            Self::Tetra => 0,
            Self::Hexahedron => 1,
            Self::Wedge => 2,
            Self::Pyramid => 3,
            Self::Polyhedron => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Tetra => "tetra",
            Self::Hexahedron => "hexahedron",
            Self::Wedge => "wedge",
            Self::Pyramid => "pyramid",
            Self::Polyhedron => "polyhedron",
        }
    }

    /// 镜像置换：把朝向相反（负体积）的单元改写为正朝向的局部节点顺序。
    fn mirror(self) -> &'static [usize] {
        match self {
            Self::Tetra => &[0, 1, 3, 2],
            Self::Hexahedron => &[0, 3, 2, 1, 4, 7, 6, 5],
            Self::Wedge => &[0, 2, 1, 3, 5, 4],
            Self::Pyramid => &[0, 3, 2, 1, 4],
            Self::Polyhedron => &[],
        }
    }

    /// 四边形面（局部节点号）；对角线位 i 对应第 i 个面。
    fn quad_faces(self) -> &'static [[usize; 4]] {
        match self {
            Self::Tetra | Self::Polyhedron => &[],
            Self::Hexahedron => &[
                [0, 1, 2, 3],
                [4, 5, 6, 7],
                [0, 1, 5, 4],
                [1, 2, 6, 5],
                [2, 3, 7, 6],
                [3, 0, 4, 7],
            ],
            Self::Wedge => &[[0, 1, 4, 3], [1, 2, 5, 4], [2, 0, 3, 5]],
            Self::Pyramid => &[[0, 1, 2, 3]],
        }
    }

    fn tet_table(self) -> &'static [&'static [[u8; 4]]] {
        match self {
            Self::Tetra => &[&[[0, 1, 2, 3]]],
            Self::Hexahedron => &HEX_TETS,
            Self::Wedge => &WEDGE_TETS,
            Self::Pyramid => &PYRAMID_TETS,
            Self::Polyhedron => &[],
        }
    }
}

/// 原生单元布局：VTK 风格的类型数组 + offsets（`len + 1` 项，首项 0）+ 连接表，
/// 以及派生的四面体拆分归属 `tet_cells`（第 i 个四面体所属原始单元，非递减）。
///
/// 纯四面体网格不携带布局（`TetMesh::cells == None`），此时单元 ID 即四面体下标。
/// 多面体的 `apex` 是追加在节点数组末尾的辅助中心节点：第 k 个多面体（按单元顺序）的
/// `apex` 为 `源节点数 + k`，源节点数 = 节点数 − 多面体数（task 031）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellLayout {
    pub types: Vec<CellType>,
    pub offsets: Vec<u64>,
    pub connectivity: Vec<MeshIndex>,
    pub tet_cells: Vec<MeshIndex>,
}

impl CellLayout {
    pub fn cell_count(&self) -> usize {
        self.types.len()
    }

    /// 第 `cell` 个单元的节点（局部顺序；多面体为规范面流）。
    pub fn cell_nodes(&self, cell: usize) -> &[MeshIndex] {
        &self.connectivity[self.offsets[cell] as usize..self.offsets[cell + 1] as usize]
    }

    /// 各类型单元数，顺序同 [`CellType::ALL`]。
    pub fn type_counts(&self) -> [u64; CELL_TYPE_COUNT] {
        let mut counts = [0; CELL_TYPE_COUNT];
        for kind in &self.types {
            counts[kind.ordinal()] += 1;
        }
        counts
    }

    /// 多面体单元数，即辅助中心节点数。
    pub fn polyhedron_count(&self) -> usize {
        self.types
            .iter()
            .filter(|&&kind| kind == CellType::Polyhedron)
            .count()
    }
}

/// 面流（`[nFaces, n0, ids…, …]`）的总字数；格式不完整时返回 `None`。
pub fn face_stream_len(words: &[MeshIndex]) -> Option<usize> {
    let faces = *words.first()? as usize;
    let mut cursor = 1_usize;
    for _ in 0..faces {
        let count = *words.get(cursor)? as usize;
        cursor = cursor.checked_add(1 + count)?;
        if cursor > words.len() {
            return None;
        }
    }
    Some(cursor)
}

/// 遍历格式完整的面流中的各个面（不含面节点数）。
pub fn faces(words: &[MeshIndex]) -> impl Iterator<Item = &[MeshIndex]> {
    let faces = words.first().map_or(0, |&count| count as usize);
    let mut cursor = 1_usize;
    (0..faces).map_while(move |_| {
        let count = *words.get(cursor)? as usize;
        let face = words.get(cursor + 1..cursor + 1 + count)?;
        cursor += 1 + count;
        Some(face)
    })
}

/// 由类型与连接表组成 VTK 风格 offsets（多面体按规范面流解析长度）；
/// 节点数与类型不符时返回出错单元下标（总长不符时为单元数）。
pub fn cell_offsets(types: &[CellType], connectivity: &[MeshIndex]) -> Result<Vec<u64>, usize> {
    let mut offsets = Vec::with_capacity(types.len() + 1);
    let mut cursor = 0_usize;
    offsets.push(0);
    for (cell, kind) in types.iter().enumerate() {
        cursor += match kind.fixed_node_count() {
            Some(count) => count,
            None => connectivity
                .get(cursor + 1..)
                .and_then(face_stream_len)
                .map(|len| len + 1)
                .ok_or(cell)?,
        };
        offsets.push(cursor as u64);
    }
    if cursor != connectivity.len() {
        return Err(types.len());
    }
    Ok(offsets)
}

/// 单元节点数与类型不符、含重复节点，或多面体面流不是闭合一致定向曲面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidCell;

impl std::fmt::Display for InvalidCell {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "cell has the wrong node count, repeats a node, or (polyhedron) its faces do not form a closed consistently oriented surface",
        )
    }
}

impl std::error::Error for InvalidCell {}

/// 共形拆分一个单元，按局部顺序追加正朝向（对参考单元而言）的四面体节点。
/// 节点数不符或节点重复的单元返回 `Err(InvalidCell)`；多面体见 [`decompose_polyhedron`]。
pub fn decompose_cell(
    kind: CellType,
    nodes: &[MeshIndex],
    out: &mut Vec<[MeshIndex; 4]>,
) -> Result<(), InvalidCell> {
    let Some(count) = kind.fixed_node_count() else {
        return decompose_polyhedron(nodes, out);
    };
    if nodes.len() != count
        || nodes
            .iter()
            .enumerate()
            .any(|(index, node)| nodes[..index].contains(node))
    {
        return Err(InvalidCell);
    }
    let mut bits = 0_usize;
    for (face_index, face) in kind.quad_faces().iter().enumerate() {
        let minimum = (0..4)
            .min_by_key(|&corner| nodes[face[corner]])
            .unwrap_or(0);
        if minimum % 2 == 1 {
            bits |= 1 << face_index;
        }
    }
    let table = kind.tet_table()[bits];
    if table.is_empty() {
        // 最小下标规则下不可达的对角线组合（仅在节点重复时出现）。
        return Err(InvalidCell);
    }
    out.extend(
        table
            .iter()
            .map(|tet| tet.map(|corner| nodes[corner as usize])),
    );
    Ok(())
}

/// 多面体拆分（task 031）：`stream` 为规范面流 `[apex, nFaces, n0, ids…, …]`。
///
/// 拓扑校验：至少 4 个面，每面至少 3 个互不相同的节点且不含 `apex`，每条有向边恰好出现
/// 一次且其反向边也恰好出现一次（闭合、一致定向）。每个面从最小节点号起扇形三角化，
/// 三角形 `(m, a, b)` 朝外，对应四面体 `[m, b, a, apex]`，面朝外时体积为正。
pub fn decompose_polyhedron(
    stream: &[MeshIndex],
    out: &mut Vec<[MeshIndex; 4]>,
) -> Result<(), InvalidCell> {
    let (&apex, words) = stream.split_first().ok_or(InvalidCell)?;
    if face_stream_len(words) != Some(words.len()) || words[0] < 4 {
        return Err(InvalidCell);
    }
    let start = out.len();
    let mut edges = Vec::with_capacity(words.len());
    for face in faces(words) {
        if face.len() < 3
            || face.contains(&apex)
            || face
                .iter()
                .enumerate()
                .any(|(index, node)| face[..index].contains(node))
        {
            out.truncate(start);
            return Err(InvalidCell);
        }
        for index in 0..face.len() {
            edges.push((face[index], face[(index + 1) % face.len()]));
        }
        let minimum = (0..face.len()).min_by_key(|&i| face[i]).unwrap_or(0);
        for step in 1..face.len() - 1 {
            let a = face[(minimum + step) % face.len()];
            let b = face[(minimum + step + 1) % face.len()];
            out.push([face[minimum], b, a, apex]);
        }
    }
    edges.sort_unstable();
    let closed = edges.windows(2).all(|pair| pair[0] != pair[1])
        && edges
            .iter()
            .all(|&(a, b)| edges.binary_search(&(b, a)).is_ok());
    if !closed {
        out.truncate(start);
        return Err(InvalidCell);
    }
    Ok(())
}

/// 单元局部节点镜像为正朝向顺序（原地）；多面体为翻转每个面的节点顺序。
pub fn mirror_cell(kind: CellType, nodes: &mut [MeshIndex]) {
    if kind == CellType::Polyhedron {
        let Some(words) = nodes.get_mut(1..) else {
            return;
        };
        let faces = words.first().map_or(0, |&count| count as usize);
        let mut cursor = 1_usize;
        for _ in 0..faces {
            let Some(&count) = words.get(cursor) else {
                return;
            };
            let Some(face) = words.get_mut(cursor + 1..cursor + 1 + count as usize) else {
                return;
            };
            face.reverse();
            cursor += 1 + count as usize;
        }
        return;
    }
    let original: Vec<MeshIndex> = nodes.to_vec();
    for (slot, &source) in nodes.iter_mut().zip(kind.mirror()) {
        *slot = original[source];
    }
}

/// 单元的源节点（多面体为面流中出现的去重节点，升序；其余类型为局部节点）。
pub fn cell_source_nodes(kind: CellType, nodes: &[MeshIndex]) -> Vec<MeshIndex> {
    if kind != CellType::Polyhedron {
        return nodes.to_vec();
    }
    let mut unique: Vec<MeshIndex> = nodes
        .get(1..)
        .map(|words| faces(words).flatten().copied().collect())
        .unwrap_or_default();
    unique.sort_unstable();
    unique.dedup();
    unique
}

/// 流式读写的一批原始单元（task 031）：与 [`CellLayout`] 同样的类型 + offsets + 连接表，
/// 但 offsets 从 0 起只覆盖本批，另带区域号；多面体连接为规范面流（含全局 `apex`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellBlock {
    pub types: Vec<CellType>,
    pub regions: Vec<MeshIndex>,
    offsets: Vec<usize>,
    pub connectivity: Vec<MeshIndex>,
}

impl CellBlock {
    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    pub fn clear(&mut self) {
        self.types.clear();
        self.regions.clear();
        self.offsets.clear();
        self.connectivity.clear();
    }

    pub fn push(&mut self, kind: CellType, region: MeshIndex, nodes: &[MeshIndex]) {
        if self.offsets.is_empty() {
            self.offsets.push(self.connectivity.len());
        }
        self.types.push(kind);
        self.regions.push(region);
        self.connectivity.extend_from_slice(nodes);
        self.offsets.push(self.connectivity.len());
    }

    /// 第 `index` 个单元的（类型，区域，节点）。
    pub fn cell(&self, index: usize) -> (CellType, MeshIndex, &[MeshIndex]) {
        (
            self.types[index],
            self.regions[index],
            &self.connectivity[self.offsets[index]..self.offsets[index + 1]],
        )
    }

    pub fn iter(&self) -> impl Iterator<Item = (CellType, MeshIndex, &[MeshIndex])> {
        (0..self.len()).map(|index| self.cell(index))
    }
}

/// 按布局确定性地拆分全部单元；`region(cell)` 给出单元区域号。
/// 返回四面体与 `tet_cells`；失败返回出错单元下标。
pub(crate) fn decompose_cells(
    types: &[CellType],
    offsets: &[u64],
    connectivity: &[MeshIndex],
    region: impl Fn(usize) -> MeshIndex,
) -> Result<(Vec<Tetrahedron>, Vec<MeshIndex>), usize> {
    let mut tets = Vec::with_capacity(types.len().saturating_mul(3));
    let mut tet_cells = Vec::with_capacity(types.len().saturating_mul(3));
    let mut scratch = Vec::with_capacity(6);
    for (cell, &kind) in types.iter().enumerate() {
        let (Some(&start), Some(&end)) = (offsets.get(cell), offsets.get(cell + 1)) else {
            return Err(cell);
        };
        let nodes = usize::try_from(start)
            .ok()
            .zip(usize::try_from(end).ok())
            .and_then(|(start, end)| connectivity.get(start..end))
            .ok_or(cell)?;
        scratch.clear();
        decompose_cell(kind, nodes, &mut scratch).map_err(|InvalidCell| cell)?;
        let cell_id = MeshIndex::try_from(cell).map_err(|_| cell)?;
        let region = region(cell);
        for &tet in &scratch {
            tets.push(Tetrahedron { nodes: tet, region });
            tet_cells.push(cell_id);
        }
    }
    Ok((tets, tet_cells))
}

/// 金字塔：底面对角线 0-2 或 1-3。
const PYRAMID_TETS: [&[[u8; 4]]; 2] =
    [&[[0, 1, 2, 4], [0, 2, 3, 4]], &[[0, 1, 3, 4], [1, 2, 3, 4]]];

// 下列表由穷举脚本生成：对每种四边形面对角线组合（位 i = 1 表示第 i 个四边形面取
// f[1]-f[3]，否则取 f[0]-f[2]），在参考单元上搜索不加内点、面上三角形与对角线一致、
// 体积守恒的 5～6（楔形 3）个四面体。最小下标规则可达 42 种六面体组合与 6 种楔形组合，
// 全部有解；空项为不可达组合。
const HEX_TETS: [&[[u8; 4]]; 64] = [
    &[],
    &[],
    &[],
    &[],
    &[
        [0, 1, 2, 4],
        [0, 2, 3, 4],
        [1, 2, 4, 6],
        [1, 4, 5, 6],
        [2, 3, 4, 7],
        [2, 4, 6, 7],
    ],
    &[
        [0, 1, 3, 4],
        [1, 2, 3, 4],
        [1, 2, 4, 6],
        [1, 4, 5, 6],
        [2, 3, 4, 7],
        [2, 4, 6, 7],
    ],
    &[],
    &[
        [0, 1, 3, 4],
        [1, 2, 3, 7],
        [1, 2, 7, 6],
        [1, 3, 4, 7],
        [1, 4, 5, 7],
        [1, 5, 6, 7],
    ],
    &[
        [0, 1, 2, 5],
        [0, 2, 3, 4],
        [0, 2, 4, 5],
        [2, 3, 4, 7],
        [2, 4, 5, 6],
        [2, 4, 6, 7],
    ],
    &[],
    &[
        [0, 1, 2, 5],
        [0, 2, 3, 4],
        [0, 2, 4, 5],
        [2, 3, 4, 7],
        [2, 4, 5, 7],
        [2, 5, 6, 7],
    ],
    &[
        [0, 1, 3, 5],
        [0, 3, 4, 5],
        [1, 2, 3, 5],
        [2, 3, 5, 7],
        [2, 5, 6, 7],
        [3, 4, 5, 7],
    ],
    &[
        [0, 1, 2, 4],
        [0, 2, 3, 4],
        [1, 2, 4, 5],
        [2, 3, 4, 7],
        [2, 4, 5, 6],
        [2, 4, 6, 7],
    ],
    &[
        [0, 1, 3, 4],
        [1, 2, 3, 4],
        [1, 2, 4, 5],
        [2, 3, 4, 7],
        [2, 4, 5, 6],
        [2, 4, 6, 7],
    ],
    &[
        [0, 1, 2, 4],
        [0, 2, 3, 4],
        [1, 2, 4, 5],
        [2, 3, 4, 7],
        [2, 4, 5, 7],
        [2, 5, 6, 7],
    ],
    &[],
    &[
        [0, 1, 2, 6],
        [0, 1, 6, 5],
        [0, 2, 3, 6],
        [0, 3, 4, 6],
        [0, 4, 5, 6],
        [3, 4, 6, 7],
    ],
    &[
        [0, 1, 3, 5],
        [0, 3, 4, 5],
        [1, 2, 3, 6],
        [1, 3, 5, 6],
        [3, 4, 5, 6],
        [3, 4, 6, 7],
    ],
    &[],
    &[
        [0, 1, 3, 5],
        [0, 3, 4, 5],
        [1, 2, 3, 6],
        [1, 3, 5, 6],
        [3, 4, 5, 7],
        [3, 5, 6, 7],
    ],
    &[
        [0, 1, 2, 4],
        [0, 2, 3, 4],
        [1, 2, 4, 6],
        [1, 4, 5, 6],
        [2, 3, 4, 6],
        [3, 4, 6, 7],
    ],
    &[
        [0, 1, 3, 4],
        [1, 2, 3, 6],
        [1, 3, 4, 6],
        [1, 4, 5, 6],
        [3, 4, 6, 7],
    ],
    &[],
    &[
        [0, 1, 3, 4],
        [1, 2, 3, 6],
        [1, 3, 4, 5],
        [1, 3, 5, 6],
        [3, 4, 5, 7],
        [3, 5, 6, 7],
    ],
    &[],
    &[
        [0, 1, 3, 5],
        [0, 3, 4, 5],
        [1, 2, 3, 5],
        [2, 3, 5, 6],
        [3, 4, 5, 6],
        [3, 4, 6, 7],
    ],
    &[
        [0, 1, 2, 5],
        [0, 2, 3, 5],
        [0, 3, 4, 5],
        [2, 3, 5, 6],
        [3, 4, 5, 7],
        [3, 5, 6, 7],
    ],
    &[
        [0, 1, 3, 5],
        [0, 3, 4, 5],
        [1, 2, 3, 5],
        [2, 3, 5, 6],
        [3, 4, 5, 7],
        [3, 5, 6, 7],
    ],
    &[
        [0, 1, 2, 4],
        [0, 2, 3, 4],
        [1, 2, 4, 5],
        [2, 3, 4, 6],
        [2, 4, 5, 6],
        [3, 4, 6, 7],
    ],
    &[
        [0, 1, 3, 4],
        [1, 2, 3, 4],
        [1, 2, 4, 5],
        [2, 3, 4, 6],
        [2, 4, 5, 6],
        [3, 4, 6, 7],
    ],
    &[],
    &[
        [0, 1, 3, 4],
        [1, 2, 3, 5],
        [1, 3, 4, 5],
        [2, 3, 5, 6],
        [3, 4, 5, 7],
        [3, 5, 6, 7],
    ],
    &[
        [0, 1, 2, 6],
        [0, 1, 6, 5],
        [0, 2, 3, 7],
        [0, 2, 7, 6],
        [0, 4, 5, 6],
        [0, 4, 6, 7],
    ],
    &[],
    &[
        [0, 1, 2, 6],
        [0, 1, 6, 5],
        [0, 2, 3, 7],
        [0, 2, 7, 6],
        [0, 4, 5, 7],
        [0, 5, 6, 7],
    ],
    &[
        [0, 1, 3, 7],
        [0, 1, 7, 5],
        [0, 4, 5, 7],
        [1, 2, 3, 7],
        [1, 2, 7, 6],
        [1, 5, 6, 7],
    ],
    &[],
    &[
        [0, 1, 3, 7],
        [0, 1, 7, 4],
        [1, 2, 3, 7],
        [1, 2, 7, 6],
        [1, 4, 5, 6],
        [1, 4, 6, 7],
    ],
    &[
        [0, 1, 2, 7],
        [0, 1, 7, 4],
        [0, 2, 3, 7],
        [1, 2, 7, 6],
        [1, 4, 5, 7],
        [1, 5, 6, 7],
    ],
    &[
        [0, 1, 3, 7],
        [0, 1, 7, 4],
        [1, 2, 3, 7],
        [1, 2, 7, 6],
        [1, 4, 5, 7],
        [1, 5, 6, 7],
    ],
    &[
        [0, 1, 2, 5],
        [0, 2, 3, 7],
        [0, 2, 4, 5],
        [0, 2, 7, 4],
        [2, 4, 5, 6],
        [2, 4, 6, 7],
    ],
    &[],
    &[
        [0, 1, 2, 5],
        [0, 2, 3, 7],
        [0, 2, 7, 5],
        [0, 4, 5, 7],
        [2, 5, 6, 7],
    ],
    &[
        [0, 1, 3, 5],
        [0, 3, 7, 5],
        [0, 4, 5, 7],
        [1, 2, 3, 5],
        [2, 3, 5, 7],
        [2, 5, 6, 7],
    ],
    &[
        [0, 1, 2, 4],
        [0, 2, 3, 7],
        [0, 2, 7, 4],
        [1, 2, 4, 5],
        [2, 4, 5, 6],
        [2, 4, 6, 7],
    ],
    &[],
    &[
        [0, 1, 2, 4],
        [0, 2, 3, 7],
        [0, 2, 7, 4],
        [1, 2, 4, 5],
        [2, 4, 5, 7],
        [2, 5, 6, 7],
    ],
    &[
        [0, 1, 3, 7],
        [0, 1, 7, 4],
        [1, 2, 3, 7],
        [1, 2, 7, 5],
        [1, 4, 5, 7],
        [2, 5, 6, 7],
    ],
    &[
        [0, 1, 2, 6],
        [0, 1, 6, 5],
        [0, 2, 3, 6],
        [0, 3, 7, 6],
        [0, 4, 5, 6],
        [0, 4, 6, 7],
    ],
    &[
        [0, 1, 3, 6],
        [0, 1, 6, 5],
        [0, 3, 7, 6],
        [0, 4, 5, 6],
        [0, 4, 6, 7],
        [1, 2, 3, 6],
    ],
    &[
        [0, 1, 2, 6],
        [0, 1, 6, 5],
        [0, 2, 3, 6],
        [0, 3, 7, 6],
        [0, 4, 5, 7],
        [0, 5, 6, 7],
    ],
    &[],
    &[
        [0, 1, 2, 6],
        [0, 1, 6, 4],
        [0, 2, 3, 6],
        [0, 3, 7, 6],
        [0, 4, 6, 7],
        [1, 4, 5, 6],
    ],
    &[
        [0, 1, 3, 6],
        [0, 1, 6, 4],
        [0, 3, 7, 6],
        [0, 4, 6, 7],
        [1, 2, 3, 6],
        [1, 4, 5, 6],
    ],
    &[],
    &[
        [0, 1, 3, 7],
        [0, 1, 7, 4],
        [1, 2, 3, 6],
        [1, 3, 7, 6],
        [1, 4, 5, 7],
        [1, 5, 6, 7],
    ],
    &[
        [0, 1, 2, 5],
        [0, 2, 3, 6],
        [0, 2, 6, 5],
        [0, 3, 7, 6],
        [0, 4, 5, 6],
        [0, 4, 6, 7],
    ],
    &[],
    &[
        [0, 1, 2, 5],
        [0, 2, 3, 5],
        [0, 3, 7, 5],
        [0, 4, 5, 7],
        [2, 3, 5, 6],
        [3, 5, 6, 7],
    ],
    &[
        [0, 1, 3, 5],
        [0, 3, 7, 5],
        [0, 4, 5, 7],
        [1, 2, 3, 5],
        [2, 3, 5, 6],
        [3, 5, 6, 7],
    ],
    &[],
    &[],
    &[],
    &[],
];
const WEDGE_TETS: [&[[u8; 4]]; 8] = [
    &[],
    &[[0, 1, 2, 3], [1, 2, 3, 5], [1, 3, 4, 5]],
    &[[0, 1, 2, 4], [0, 2, 3, 4], [2, 3, 4, 5]],
    &[[0, 1, 2, 3], [1, 2, 3, 4], [2, 3, 4, 5]],
    &[[0, 1, 2, 5], [0, 1, 5, 4], [0, 3, 4, 5]],
    &[[0, 1, 2, 5], [0, 1, 5, 3], [1, 3, 4, 5]],
    &[[0, 1, 2, 4], [0, 2, 5, 4], [0, 3, 4, 5]],
    &[],
];

#[cfg(test)]
mod tests {
    use super::*;

    const HEX_REF: [[f64; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];

    fn volume6(points: &[[f64; 3]], tet: [MeshIndex; 4]) -> f64 {
        let [a, b, c, d] = tet.map(|node| points[node as usize]);
        let u: [f64; 3] = std::array::from_fn(|k| b[k] - a[k]);
        let v: [f64; 3] = std::array::from_fn(|k| c[k] - a[k]);
        let w: [f64; 3] = std::array::from_fn(|k| d[k] - a[k]);
        u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0])
            + u[2] * (v[0] * w[1] - v[1] * w[0])
    }

    /// 所有节点编号排列下，立方体都拆成体积和为 1 的正四面体。
    #[test]
    fn hexahedron_decomposes_for_every_labelling() {
        let mut labels: Vec<MeshIndex> = (0..8).collect();
        let mut checked = 0;
        permute(&mut labels, 0, &mut |labels| {
            let mut points = [[0.0; 3]; 8];
            for (corner, &label) in labels.iter().enumerate() {
                points[label as usize] = HEX_REF[corner];
            }
            let mut tets = Vec::new();
            decompose_cell(CellType::Hexahedron, labels, &mut tets)
                .unwrap_or_else(|_| panic!("decomposes"));
            assert!(matches!(tets.len(), 5 | 6));
            let total: f64 = tets
                .iter()
                .map(|&tet| {
                    let volume = volume6(&points, tet);
                    assert!(volume > 0.0);
                    volume
                })
                .sum();
            assert!((total - 6.0).abs() < 1e-12);
            checked += 1;
        });
        assert_eq!(checked, 40320);
    }

    #[test]
    fn wedge_and_pyramid_decompose_for_every_labelling() {
        let wedge = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let pyramid = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.5, 0.5, 1.0],
        ];
        for (kind, reference, expected6) in [
            (CellType::Wedge, &wedge[..], 3.0),
            (CellType::Pyramid, &pyramid[..], 2.0),
        ] {
            let mut labels: Vec<MeshIndex> = (0..reference.len() as MeshIndex).collect();
            permute(&mut labels, 0, &mut |labels| {
                let mut points = vec![[0.0; 3]; labels.len()];
                for (corner, &label) in labels.iter().enumerate() {
                    points[label as usize] = reference[corner];
                }
                let mut tets = Vec::new();
                decompose_cell(kind, labels, &mut tets).unwrap_or_else(|_| panic!("decomposes"));
                let total: f64 = tets.iter().map(|&tet| volume6(&points, tet)).sum();
                assert!(tets.iter().all(|&tet| volume6(&points, tet) > 0.0));
                assert!((total - expected6).abs() < 1e-12);
            });
        }
    }

    #[test]
    fn mirrored_cells_flip_orientation_and_repeated_nodes_fail() {
        let mut nodes: Vec<MeshIndex> = (0..8).collect();
        mirror_cell(CellType::Hexahedron, &mut nodes);
        assert_eq!(nodes, [0, 3, 2, 1, 4, 7, 6, 5]);
        let mut tets = Vec::new();
        decompose_cell(CellType::Hexahedron, &nodes, &mut tets)
            .unwrap_or_else(|_| panic!("decomposes"));
        assert!(tets.iter().all(|&tet| volume6(&HEX_REF, tet) < 0.0));
        assert!(decompose_cell(CellType::Pyramid, &[0, 1, 2, 2, 4], &mut tets).is_err());
        assert!(decompose_cell(CellType::Wedge, &[0, 1, 2], &mut tets).is_err());
        assert_eq!(
            CellType::from_vtk(VTK_POLYHEDRON),
            Some(CellType::Polyhedron)
        );
        assert_eq!(
            cell_offsets(&[CellType::Tetra, CellType::Pyramid], &[0; 9]),
            Ok(vec![0, 4, 9])
        );
    }

    fn permute(labels: &mut Vec<MeshIndex>, start: usize, visit: &mut impl FnMut(&[MeshIndex])) {
        if start == labels.len() {
            visit(labels);
            return;
        }
        for index in start..labels.len() {
            labels.swap(start, index);
            permute(labels, start + 1, visit);
            labels.swap(start, index);
        }
    }
}
