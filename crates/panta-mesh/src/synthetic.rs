//! 合成测试网格（task 019 / 025 / 030 基准）：参数化起伏圆环面、四面体立方体与混合单元边界层块，
//! 按下标确定性生成任意规模，可按块并行生成而无需持有整个网格。

use crate::asset::Triangle;
use std::f64::consts::TAU;
use std::ops::Range;

/// 起伏圆环面：主半径 100 mm、管半径 30 mm，叠加小幅波纹使相邻面不共面。
#[derive(Debug, Clone, Copy)]
pub struct SyntheticTorus {
    triangle_count: usize,
    around: usize,
    along: usize,
}

impl SyntheticTorus {
    /// `triangle_count` 恰为生成的面数；参数网格约为 3:1 的四边形网格，每格两面。
    pub fn new(triangle_count: usize) -> Self {
        let quads = triangle_count.div_ceil(2).max(1);
        let around = ((quads as f64 / 3.0).sqrt().round() as usize).max(3);
        let along = quads.div_ceil(around).max(3);
        Self {
            triangle_count,
            around,
            along,
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.triangle_count
    }

    fn point(&self, i: usize, j: usize) -> [f64; 3] {
        let u = TAU * (i % self.along) as f64 / self.along as f64;
        let v = TAU * (j % self.around) as f64 / self.around as f64;
        let tube = 30.0 + 1.5 * (7.0 * u).sin() * (5.0 * v).cos();
        let radius = 100.0 + tube * v.cos();
        [radius * u.cos(), radius * u.sin(), tube * v.sin()]
    }

    /// 第 `index` 个三角面（`index < triangle_count`）。
    pub fn triangle(&self, index: usize) -> Triangle {
        let quad = index / 2;
        let (i, j) = (quad / self.around, quad % self.around);
        let (a, b, c, d) = (
            self.point(i, j),
            self.point(i + 1, j),
            self.point(i + 1, j + 1),
            self.point(i, j + 1),
        );
        if index.is_multiple_of(2) {
            [a, b, c]
        } else {
            [a, c, d]
        }
    }

    /// 生成 `range` 内的面，追加到 `out`。
    pub fn fill(&self, range: Range<usize>, out: &mut Vec<Triangle>) {
        out.extend(range.map(|index| self.triangle(index)));
    }
}

/// 合成四面体体网格（task 025 基准）：边长 100 mm 的立方体剖分为 `n³` 个小立方体，
/// 每个小立方体按 Kuhn 剖分为 6 个正向四面体；内部节点有确定性小扰动，使质量统计非平凡。
/// 节点、单元、边界面都按下标即时计算，可分块并行生成，无需持有整个网格。
#[derive(Debug, Clone, Copy)]
pub struct SyntheticTetBlock {
    cells_per_axis: usize,
    /// Kuhn 剖分的 6 个四面体，角点为单位立方体顶点编号（bit0 = x，bit1 = y，bit2 = z），已按正向排序。
    local: [[usize; 4]; 6],
}

const BLOCK_EDGE_MM: f64 = 100.0;

impl SyntheticTetBlock {
    /// 取 `n = round(cbrt(target / 6))`（至少 1），实际单元数为 `6n³`。
    pub fn new(target_tets: usize) -> Self {
        let n = ((target_tets as f64 / 6.0).cbrt().round() as usize).max(1);
        Self::with_cells_per_axis(n)
    }

    pub fn with_cells_per_axis(cells_per_axis: usize) -> Self {
        let corner = |bits: usize| {
            [
                (bits & 1) as f64,
                ((bits >> 1) & 1) as f64,
                ((bits >> 2) & 1) as f64,
            ]
        };
        let permutations = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let local = permutations.map(|order| {
            let first = 1 << order[0];
            let second = first | (1 << order[1]);
            let mut tet = [0, first, second, 7];
            let [a, b, c, d] = tet.map(corner);
            let sub = |p: [f64; 3], q: [f64; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
            let (u, v, w) = (sub(b, a), sub(c, a), sub(d, a));
            let volume6 = u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0])
                + u[2] * (v[0] * w[1] - v[1] * w[0]);
            if volume6 < 0.0 {
                tet.swap(2, 3);
            }
            tet
        });
        Self {
            cells_per_axis,
            local,
        }
    }

    pub fn cells_per_axis(&self) -> usize {
        self.cells_per_axis
    }

    pub fn node_count(&self) -> usize {
        (self.cells_per_axis + 1).pow(3)
    }

    pub fn tet_count(&self) -> usize {
        6 * self.cells_per_axis.pow(3)
    }

    /// 6 个面 × n² 个四边形 × 2 个三角形。
    pub fn boundary_count(&self) -> usize {
        12 * self.cells_per_axis.pow(2)
    }

    pub fn counts(&self) -> crate::VolumeStreamCounts {
        crate::VolumeStreamCounts {
            nodes: self.node_count(),
            tets: self.tet_count(),
            cells: self.tet_count(),
            boundary: self.boundary_count(),
            region_count: 1,
            boundary_group_count: 6,
            polyhedra: 0,
        }
    }

    fn node_index(&self, i: usize, j: usize, k: usize) -> u32 {
        let side = self.cells_per_axis + 1;
        ((k * side + j) * side + i) as u32
    }

    /// 第 `index` 个节点坐标。
    pub fn node(&self, index: usize) -> [f64; 3] {
        let side = self.cells_per_axis + 1;
        let (i, j, k) = (index % side, (index / side) % side, index / (side * side));
        let h = BLOCK_EDGE_MM / self.cells_per_axis as f64;
        let interior = |value: usize| value > 0 && value < self.cells_per_axis;
        let mut point = [i as f64 * h, j as f64 * h, k as f64 * h];
        if interior(i) && interior(j) && interior(k) {
            // 确定性扰动，幅度 2% 单元边长，不会翻转 Kuhn 四面体。
            let seed = index as f64;
            for (axis, value) in point.iter_mut().enumerate() {
                *value += 0.02 * h * (seed * (1.7 + axis as f64) + axis as f64).sin();
            }
        }
        point
    }

    /// 第 `index` 个四面体。
    pub fn tet(&self, index: usize) -> crate::Tetrahedron {
        let n = self.cells_per_axis;
        let cube = index / 6;
        let (i, j, k) = (cube % n, (cube / n) % n, cube / (n * n));
        let nodes = self.local[index % 6].map(|bits| {
            self.node_index(i + (bits & 1), j + ((bits >> 1) & 1), k + ((bits >> 2) & 1))
        });
        crate::Tetrahedron { nodes, region: 0 }
    }

    /// 第 `index` 个边界三角形：组号 0..6 依次为 x−、x+、y−、y+、z−、z+，法向朝外。
    pub fn boundary_face(&self, index: usize) -> crate::SurfaceTriangle {
        let n = self.cells_per_axis;
        let group = index / (2 * n * n);
        let quad = (index / 2) % (n * n);
        let (u, v) = (quad % n, quad / n);
        let axis = group / 2;
        let high = group % 2 == 1;
        let plane = if high { n } else { 0 };
        // (axis, 第一切向轴, 第二切向轴) 为右手系时 (a,b) 叉乘指向 +axis。
        let (t1, t2) = ((axis + 1) % 3, (axis + 2) % 3);
        let point = |du: usize, dv: usize| {
            let mut ijk = [0; 3];
            ijk[axis] = plane;
            ijk[t1] = u + du;
            ijk[t2] = v + dv;
            ijk
        };
        // Kuhn 剖分在每个面上的对角线：经过该面上 (0,0,0) 方向最小角与最大角。
        let (p00, p10, p11, p01) = (point(0, 0), point(1, 0), point(1, 1), point(0, 1));
        let mut corners = if index.is_multiple_of(2) {
            [p00, p10, p11]
        } else {
            [p00, p11, p01]
        };
        if !high {
            corners.swap(1, 2);
        }
        crate::SurfaceTriangle {
            nodes: corners.map(|[i, j, k]| self.node_index(i, j, k)),
            group: group as u32,
        }
    }
}

impl crate::VolumeSource for SyntheticTetBlock {
    fn nodes(&self, range: Range<usize>, out: &mut Vec<[f64; 3]>) {
        out.extend(range.map(|index| self.node(index)));
    }

    fn tets(&self, range: Range<usize>, out: &mut Vec<crate::Tetrahedron>) {
        out.extend(range.map(|index| self.tet(index)));
    }

    fn boundary(&self, range: Range<usize>, out: &mut Vec<crate::SurfaceTriangle>) {
        out.extend(range.map(|index| self.boundary_face(index)));
    }
}

/// 合成混合单元体网格（task 030 基准）：模拟 CFD 常见的“棱柱边界层 + 六面体核心”。
///
/// 边长 100 mm 的 `n × n` 平面网格沿 z 分层（自下而上）：
/// - `prism_layers` 层渐厚棱柱边界层（首层 0.05h，增长率 1.5；每个平面格按 00–11 对角线分成 2 个三角形后拉伸为楔形）；
/// - 1 层过渡层：每格加一个中心节点，顶面四边形与 4 个侧面各成一个金字塔，底面 2 个三角形各成一个四面体，
///   使三角形底面与上方六面体的四边形面共形；
/// - `core_layers` 层六面体核心；
/// - 镜像的过渡层与棱柱边界层。
///
/// 区域 0 为棱柱边界层，区域 1 为过渡层与核心；边界组 0..6 依次为 x−、x+、y−、y+、z−、z+。
/// 四边形边界面按与单元拆分相同的最小下标对角线规则剖分，因此边界三角形与拆分后的四面体网格精确一致。
/// 内部列的 xy 坐标有确定性小扰动（各层相同，楔形保持竖直），使质量统计非平凡。
/// 节点、单元、边界面都按下标即时计算，可分块并行生成。
#[derive(Debug, Clone)]
pub struct SyntheticMixedBlock {
    n: usize,
    prism_layers: usize,
    core_layers: usize,
    /// 各节点层的 z 坐标（mm）。
    levels: Vec<f64>,
    /// 各单元层：(类型, 底部节点层, 首个单元下标)。
    layers: Vec<(MixedLayer, usize, usize)>,
    cells: usize,
    tets: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MixedLayer {
    Prism,
    /// 底面三角形、顶面四边形。
    TransitionUp,
    Hex,
    /// 底面四边形、顶面三角形。
    TransitionDown,
}

impl MixedLayer {
    fn cells_per_square(self) -> usize {
        match self {
            Self::Prism => 2,
            Self::TransitionUp | Self::TransitionDown => 7,
            Self::Hex => 1,
        }
    }
}

/// 按最小下标规则把有向四边形分成两个同向三角形（与 `decompose_cell` 的对角线一致）。
fn split_quad(quad: [u32; 4]) -> [[u32; 3]; 2] {
    let minimum = (0..4).min_by_key(|&corner| quad[corner]).unwrap_or(0);
    let [a, b, c, d] = quad;
    if minimum % 2 == 0 {
        [[a, b, c], [a, c, d]]
    } else {
        [[b, c, d], [b, d, a]]
    }
}

impl SyntheticMixedBlock {
    /// 3 层棱柱边界层、核心层数约为 `n / 2`；取使原始单元数最接近 `target_cells` 的 `n`（至少 2）。
    pub fn new(target_cells: usize) -> Self {
        let cells = |n: usize| n * n * (4 * 3 + 14 + (n / 2).max(1));
        let mut n = 2;
        while cells(n + 1) <= target_cells
            || cells(n + 1).abs_diff(target_cells) < cells(n).abs_diff(target_cells)
        {
            n += 1;
        }
        Self::with_layers(n, 3, (n / 2).max(1))
    }

    pub fn with_layers(n: usize, prism_layers: usize, core_layers: usize) -> Self {
        let n = n.max(1);
        let core_layers = core_layers.max(1);
        let h = BLOCK_EDGE_MM / n as f64;
        let prism: Vec<f64> = (0..prism_layers)
            .map(|layer| 0.05 * h * 1.5_f64.powi(layer as i32))
            .collect();
        let mut thickness = Vec::new();
        let mut kinds = Vec::new();
        for &t in &prism {
            thickness.push(t);
            kinds.push(MixedLayer::Prism);
        }
        thickness.push(h);
        kinds.push(MixedLayer::TransitionUp);
        for _ in 0..core_layers {
            thickness.push(h);
            kinds.push(MixedLayer::Hex);
        }
        thickness.push(h);
        kinds.push(MixedLayer::TransitionDown);
        for &t in prism.iter().rev() {
            thickness.push(t);
            kinds.push(MixedLayer::Prism);
        }
        let mut levels = vec![0.0];
        for t in &thickness {
            let top = levels[levels.len() - 1] + t;
            levels.push(top);
        }
        let mut layers = Vec::with_capacity(kinds.len());
        let mut cells = 0;
        for (level, &kind) in kinds.iter().enumerate() {
            layers.push((kind, level, cells));
            cells += kind.cells_per_square() * n * n;
        }
        let mut block = Self {
            n,
            prism_layers,
            core_layers,
            levels,
            layers,
            cells,
            tets: 0,
        };
        block.tets = block.count_tets();
        block
    }

    /// 楔形 3、金字塔 2、四面体 1 个拆分四面体是确定的；六面体 5～6 个取决于对角线，逐个计算。
    fn count_tets(&self) -> usize {
        let mut tets = 0;
        let mut scratch = Vec::with_capacity(6);
        for &(kind, _, first) in &self.layers {
            let squares = self.n * self.n;
            tets += match kind {
                MixedLayer::Prism => 6 * squares,
                MixedLayer::TransitionUp | MixedLayer::TransitionDown => 12 * squares,
                MixedLayer::Hex => (first..first + squares)
                    .map(|cell| {
                        let (kind, _, nodes) = self.cell(cell);
                        scratch.clear();
                        let _ = crate::cells::decompose_cell(
                            kind,
                            &nodes[..kind.fixed_node_count().unwrap_or(0)],
                            &mut scratch,
                        );
                        scratch.len()
                    })
                    .sum(),
            };
        }
        tets
    }

    pub fn cells_per_axis(&self) -> usize {
        self.n
    }

    pub fn prism_layers(&self) -> usize {
        self.prism_layers
    }

    pub fn core_layers(&self) -> usize {
        self.core_layers
    }

    fn grid_nodes(&self) -> usize {
        (self.n + 1).pow(2) * self.levels.len()
    }

    pub fn node_count(&self) -> usize {
        self.grid_nodes() + 2 * self.n * self.n
    }

    pub fn cell_count(&self) -> usize {
        self.cells
    }

    /// 拆分后的四面体数。
    pub fn tet_count(&self) -> usize {
        self.tets
    }

    /// 各类型原始单元数，顺序同 [`crate::CellType::ALL`]（四面体、六面体、楔形、金字塔、多面体）。
    pub fn type_counts(&self) -> [u64; crate::CELL_TYPE_COUNT] {
        let squares = (self.n * self.n) as u64;
        let prism = 2 * self.prism_layers as u64;
        [
            4 * squares,
            self.core_layers as u64 * squares,
            2 * prism * squares,
            10 * squares,
            0,
        ]
    }

    fn wall_faces(&self) -> usize {
        2 * self.n * self.layers.len()
    }

    pub fn boundary_count(&self) -> usize {
        4 * self.wall_faces() + 4 * self.n * self.n
    }

    pub fn counts(&self) -> crate::VolumeStreamCounts {
        crate::VolumeStreamCounts {
            nodes: self.node_count(),
            tets: self.tet_count(),
            cells: self.cell_count(),
            boundary: self.boundary_count(),
            region_count: 2,
            boundary_group_count: 6,
            polyhedra: 0,
        }
    }

    fn grid(&self, i: usize, j: usize, level: usize) -> u32 {
        let side = self.n + 1;
        ((level * side + j) * side + i) as u32
    }

    fn center(&self, transition: usize, i: usize, j: usize) -> u32 {
        (self.grid_nodes() + transition * self.n * self.n + j * self.n + i) as u32
    }

    fn column_xy(&self, i: usize, j: usize) -> [f64; 2] {
        let h = BLOCK_EDGE_MM / self.n as f64;
        let mut xy = [i as f64 * h, j as f64 * h];
        if i > 0 && i < self.n && j > 0 && j < self.n {
            let seed = (j * (self.n + 1) + i) as f64;
            for (axis, value) in xy.iter_mut().enumerate() {
                *value += 0.03 * h * (seed * (1.3 + axis as f64) + axis as f64).sin();
            }
        }
        xy
    }

    /// 第 `index` 个节点坐标。
    pub fn node(&self, index: usize) -> [f64; 3] {
        let side = self.n + 1;
        if index < self.grid_nodes() {
            let (i, j, level) = (index % side, (index / side) % side, index / (side * side));
            let [x, y] = self.column_xy(i, j);
            return [x, y, self.levels[level]];
        }
        let local = index - self.grid_nodes();
        let squares = self.n * self.n;
        let (transition, square) = (local / squares, local % squares);
        let (i, j) = (square % self.n, square / self.n);
        let level = self
            .layers
            .iter()
            .filter(|(kind, _, _)| {
                matches!(kind, MixedLayer::TransitionUp | MixedLayer::TransitionDown)
            })
            .nth(transition)
            .map_or(0, |&(_, level, _)| level);
        let mut sum = [0.0; 3];
        for (di, dj, dl) in (0..8).map(|bits| (bits & 1, (bits >> 1) & 1, bits >> 2)) {
            let [x, y] = self.column_xy(i + di, j + dj);
            sum[0] += x;
            sum[1] += y;
            sum[2] += self.levels[level + dl];
        }
        sum.map(|value| value / 8.0)
    }

    /// 第 `index` 个原始单元：类型、区域、节点（类型节点数之后为 `u32::MAX`）。
    pub fn cell(&self, index: usize) -> (crate::CellType, u32, [u32; 8]) {
        use crate::CellType;
        let layer = self
            .layers
            .partition_point(|&(_, _, first)| first <= index)
            .saturating_sub(1);
        let (kind, lo, first) = self.layers[layer];
        let per = kind.cells_per_square();
        let local = index - first;
        let (square, slot) = (local / per, local % per);
        let (i, j) = (square % self.n, square / self.n);
        let hi = lo + 1;
        let g = |di: usize, dj: usize, level: usize| self.grid(i + di, j + dj, level);
        let pad = |nodes: &[u32]| {
            let mut out = [u32::MAX; 8];
            out[..nodes.len()].copy_from_slice(nodes);
            out
        };
        // xy 方格按 00–11 对角线分成的两个三角形（自 +z 看逆时针）。
        let triangles = [[(0, 0), (1, 0), (1, 1)], [(0, 0), (1, 1), (0, 1)]];
        match kind {
            MixedLayer::Prism => {
                let [a, b, c] = triangles[slot];
                let nodes = [
                    g(a.0, a.1, lo),
                    g(b.0, b.1, lo),
                    g(c.0, c.1, lo),
                    g(a.0, a.1, hi),
                    g(b.0, b.1, hi),
                    g(c.0, c.1, hi),
                ];
                (CellType::Wedge, 0, pad(&nodes))
            }
            MixedLayer::Hex => {
                let nodes = [
                    g(0, 0, lo),
                    g(1, 0, lo),
                    g(1, 1, lo),
                    g(0, 1, lo),
                    g(0, 0, hi),
                    g(1, 0, hi),
                    g(1, 1, hi),
                    g(0, 1, hi),
                ];
                (CellType::Hexahedron, 1, pad(&nodes))
            }
            MixedLayer::TransitionUp | MixedLayer::TransitionDown => {
                let up = kind == MixedLayer::TransitionUp;
                let center = self.center(usize::from(!up), i, j);
                // 各面按右手法向指向单元内部排列。
                let quad_face = |face: usize| -> [u32; 4] {
                    match face {
                        0 if up => [g(0, 0, hi), g(0, 1, hi), g(1, 1, hi), g(1, 0, hi)],
                        0 => [g(0, 0, lo), g(1, 0, lo), g(1, 1, lo), g(0, 1, lo)],
                        1 => [g(0, 0, lo), g(0, 1, lo), g(0, 1, hi), g(0, 0, hi)],
                        2 => [g(1, 0, lo), g(1, 0, hi), g(1, 1, hi), g(1, 1, lo)],
                        3 => [g(0, 0, lo), g(0, 0, hi), g(1, 0, hi), g(1, 0, lo)],
                        _ => [g(0, 1, lo), g(1, 1, lo), g(1, 1, hi), g(0, 1, hi)],
                    }
                };
                if slot < 5 {
                    let [a, b, c, d] = quad_face(slot);
                    (CellType::Pyramid, 1, pad(&[a, b, c, d, center]))
                } else {
                    let [a, b, c] = triangles[slot - 5];
                    let (level, tri) = if up { (lo, [a, b, c]) } else { (hi, [a, c, b]) };
                    let nodes = tri.map(|(di, dj)| g(di, dj, level));
                    (
                        CellType::Tetra,
                        1,
                        pad(&[nodes[0], nodes[1], nodes[2], center]),
                    )
                }
            }
        }
    }

    /// 第 `index` 个边界三角形，法向朝外。
    pub fn boundary_face(&self, index: usize) -> crate::SurfaceTriangle {
        let walls = 4 * self.wall_faces();
        if index >= walls {
            let local = index - walls;
            let squares = 2 * self.n * self.n;
            let (top, local) = (local / squares, local % squares);
            let (square, half) = (local / 2, local % 2);
            let (i, j) = (square % self.n, square / self.n);
            let level = if top == 1 { self.levels.len() - 1 } else { 0 };
            let g = |(di, dj): (usize, usize)| self.grid(i + di, j + dj, level);
            let triangle = [[(0, 0), (1, 0), (1, 1)], [(0, 0), (1, 1), (0, 1)]][half];
            let mut nodes = triangle.map(g);
            if top == 0 {
                nodes.swap(1, 2);
            }
            return crate::SurfaceTriangle {
                nodes,
                group: 4 + top as u32,
            };
        }
        let group = index / self.wall_faces();
        let local = index % self.wall_faces();
        let (quad, half) = (local / 2, local % 2);
        let (u, lo) = (quad % self.n, quad / self.n);
        let hi = lo + 1;
        let side = self.n;
        // 自外侧看逆时针（右手法向朝外）。
        let outward = match group {
            0 => [
                self.grid(0, u, lo),
                self.grid(0, u, hi),
                self.grid(0, u + 1, hi),
                self.grid(0, u + 1, lo),
            ],
            1 => [
                self.grid(side, u, lo),
                self.grid(side, u + 1, lo),
                self.grid(side, u + 1, hi),
                self.grid(side, u, hi),
            ],
            2 => [
                self.grid(u, 0, lo),
                self.grid(u + 1, 0, lo),
                self.grid(u + 1, 0, hi),
                self.grid(u, 0, hi),
            ],
            _ => [
                self.grid(u, side, lo),
                self.grid(u, side, hi),
                self.grid(u + 1, side, hi),
                self.grid(u + 1, side, lo),
            ],
        };
        crate::SurfaceTriangle {
            nodes: split_quad(outward)[half],
            group: group as u32,
        }
    }
}

impl crate::VolumeSource for SyntheticMixedBlock {
    fn nodes(&self, range: Range<usize>, out: &mut Vec<[f64; 3]>) {
        out.extend(range.map(|index| self.node(index)));
    }

    /// 混合单元源不提供四面体流（写入端改用 [`Self::cell`]）。
    fn tets(&self, _range: Range<usize>, _out: &mut Vec<crate::Tetrahedron>) {}

    fn boundary(&self, range: Range<usize>, out: &mut Vec<crate::SurfaceTriangle>) {
        out.extend(range.map(|index| self.boundary_face(index)));
    }

    fn cells(&self, range: Range<usize>, out: &mut crate::CellBlock) {
        for index in range {
            let (kind, region, nodes) = self.cell(index);
            out.push(kind, region, &nodes[..kind.fixed_node_count().unwrap_or(0)]);
        }
    }
}
