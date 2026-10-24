//! 原生混合单元（task 030）：VTK 风格的单元类型、局部节点顺序与共形四面体拆分。
//!
//! 局部节点顺序与 VTK 线性单元一致（见 `ai-docs/architecture/mesh.md`）：
//! - Tetra（10）：0,1,2 逆时针朝向节点 3；参考坐标 (0,0,0)(1,0,0)(0,1,0)(0,0,1)。
//! - Hexahedron（12）：底面 0,1,2,3 逆时针朝向顶面，顶面 4..7 依次位于 0..3 上方。
//! - Wedge（13）：底面三角形 0,1,2 的右手法向指向顶面 3,4,5，3..5 依次位于 0..2 上方。
//! - Pyramid（14）：底面 0,1,2,3 的右手法向指向锥顶 4。
//! - Polyhedron（42）：预留编码，导入时以明确错误拒绝。
//!
//! 拆分只用于切面、体积与质量计算等内部算法；单元 ID 始终是原始单元下标。
//! 四边形面的对角线取经过面上全局节点号最小者的那一条（Dompierre 等的最小下标规则），
//! 相邻单元因此在共享面上选同一条对角线，拆分结果共形且只由全局节点号决定，
//! 加载时可确定性重算，不写入资产。

use crate::{MeshIndex, Tetrahedron};

/// VTK 多面体单元编码：格式预留，当前不支持。
pub const VTK_POLYHEDRON: u8 = 42;

/// 线性体单元类型，判别值即 VTK 单元编码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum CellType {
    Tetra = 10,
    Hexahedron = 12,
    Wedge = 13,
    Pyramid = 14,
}

impl CellType {
    /// 按 VTK 编码顺序列出全部受支持的类型。
    pub const ALL: [Self; 4] = [Self::Tetra, Self::Hexahedron, Self::Wedge, Self::Pyramid];

    pub fn from_vtk(code: u8) -> Option<Self> {
        Some(match code {
            10 => Self::Tetra,
            12 => Self::Hexahedron,
            13 => Self::Wedge,
            14 => Self::Pyramid,
            _ => return None,
        })
    }

    pub fn vtk_code(self) -> u8 {
        self as u8
    }

    pub fn node_count(self) -> usize {
        match self {
            Self::Tetra => 4,
            Self::Hexahedron => 8,
            Self::Wedge => 6,
            Self::Pyramid => 5,
        }
    }

    /// 在 [`Self::ALL`] 中的下标，便于按类型计数。
    pub fn ordinal(self) -> usize {
        match self {
            Self::Tetra => 0,
            Self::Hexahedron => 1,
            Self::Wedge => 2,
            Self::Pyramid => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Tetra => "tetra",
            Self::Hexahedron => "hexahedron",
            Self::Wedge => "wedge",
            Self::Pyramid => "pyramid",
        }
    }

    /// 镜像置换：把朝向相反（负体积）的单元改写为正朝向的局部节点顺序。
    fn mirror(self) -> &'static [usize] {
        match self {
            Self::Tetra => &[0, 1, 3, 2],
            Self::Hexahedron => &[0, 3, 2, 1, 4, 7, 6, 5],
            Self::Wedge => &[0, 2, 1, 3, 5, 4],
            Self::Pyramid => &[0, 3, 2, 1, 4],
        }
    }

    /// 四边形面（局部节点号）；对角线位 i 对应第 i 个面。
    fn quad_faces(self) -> &'static [[usize; 4]] {
        match self {
            Self::Tetra => &[],
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
        }
    }
}

/// 原生单元布局：VTK 风格的类型数组 + offsets（`len + 1` 项，首项 0）+ 连接表，
/// 以及派生的四面体拆分归属 `tet_cells`（第 i 个四面体所属原始单元，非递减）。
///
/// 纯四面体网格不携带布局（`TetMesh::cells == None`），此时单元 ID 即四面体下标。
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

    /// 第 `cell` 个单元的节点（局部顺序）。
    pub fn cell_nodes(&self, cell: usize) -> &[MeshIndex] {
        &self.connectivity[self.offsets[cell] as usize..self.offsets[cell + 1] as usize]
    }

    /// 各类型单元数，顺序同 [`CellType::ALL`]。
    pub fn type_counts(&self) -> [u64; 4] {
        let mut counts = [0; 4];
        for kind in &self.types {
            counts[kind.ordinal()] += 1;
        }
        counts
    }
}

/// 由类型与节点数组成 VTK 风格 offsets；节点数与类型不符时返回出错单元下标。
pub fn cell_offsets(types: &[CellType], connectivity_len: usize) -> Result<Vec<u64>, usize> {
    let mut offsets = Vec::with_capacity(types.len() + 1);
    let mut cursor = 0_u64;
    offsets.push(0);
    for kind in types {
        cursor += kind.node_count() as u64;
        offsets.push(cursor);
    }
    if cursor != connectivity_len as u64 {
        return Err(types.len());
    }
    Ok(offsets)
}

/// 单元节点数与类型不符或含重复节点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidCell;

impl std::fmt::Display for InvalidCell {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("cell has the wrong node count or repeats a node")
    }
}

impl std::error::Error for InvalidCell {}

/// 共形拆分一个单元，按局部顺序追加正朝向（对参考单元而言）的四面体节点。
/// 节点数不符或节点重复的单元返回 `Err(InvalidCell)`。
pub fn decompose_cell(
    kind: CellType,
    nodes: &[MeshIndex],
    out: &mut Vec<[MeshIndex; 4]>,
) -> Result<(), InvalidCell> {
    if nodes.len() != kind.node_count()
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

/// 单元局部节点镜像为正朝向顺序（原地）。
pub fn mirror_cell(kind: CellType, nodes: &mut [MeshIndex]) {
    let original: Vec<MeshIndex> = nodes.to_vec();
    for (slot, &source) in nodes.iter_mut().zip(kind.mirror()) {
        *slot = original[source];
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
        assert_eq!(CellType::from_vtk(VTK_POLYHEDRON), None);
        assert_eq!(
            cell_offsets(&[CellType::Tetra, CellType::Pyramid], 9),
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
