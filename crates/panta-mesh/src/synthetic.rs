//! 合成测试网格（task 019 基准）：参数化起伏圆环面，按下标确定性生成任意面数，
//! 可按块并行生成而无需持有整个网格。

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
