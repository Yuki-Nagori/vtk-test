//! 按单元类型的质量指标（task 030）：Verdict 风格的缩放雅可比（scaled Jacobian）。
//!
//! 每个角点取三条出边的单位向量组成的行列式（三重积），全单元取最小值，并按
//! 理想单元归一化到 1：
//! - Tetra：`√2 × 6V / max(角点三边长乘积)`，正四面体为 1（与 Verdict `tet_scaled_jacobian` 相同）。
//! - Hexahedron：8 个角点加中心主轴三个方向，立方体为 1（同 Verdict `hex_scaled_jacobian`）。
//! - Wedge：6 个角点，乘 `2/√3`，正三棱柱为 1（同 Verdict `wedge_scaled_jacobian`）。
//! - Pyramid：4 个底面角点（两条底边 + 指向锥顶的边），乘 `√2`，所有棱等长的正四棱锥为 1。
//!
//! 取值范围 [-1, 1]，非正值表示角点处反转或退化；边长为 0 的角点按 0 计。

use crate::CellType;
use crate::volume::{cross, dot, sub};

/// 缩放雅可比低于该值记为劣质单元（Verdict 对四类单元给出的可接受下限多在 0.2–0.5，取宽松的 0.2）。
pub const POOR_SCALED_JACOBIAN: f64 = 0.2;

const HEX_CORNERS: [[usize; 4]; 8] = [
    [0, 1, 3, 4],
    [1, 2, 0, 5],
    [2, 3, 1, 6],
    [3, 0, 2, 7],
    [4, 7, 5, 0],
    [5, 4, 6, 1],
    [6, 5, 7, 2],
    [7, 6, 4, 3],
];
const WEDGE_CORNERS: [[usize; 4]; 6] = [
    [0, 1, 2, 3],
    [1, 2, 0, 4],
    [2, 0, 1, 5],
    [3, 5, 4, 0],
    [4, 3, 5, 1],
    [5, 4, 3, 2],
];
const PYRAMID_CORNERS: [[usize; 4]; 4] = [[0, 1, 3, 4], [1, 2, 0, 4], [2, 3, 1, 4], [3, 0, 2, 4]];
const TET_CORNERS: [[usize; 4]; 4] = [[0, 1, 2, 3], [1, 2, 0, 3], [2, 0, 1, 3], [3, 2, 1, 0]];

/// 单元的缩放雅可比；`points` 按 VTK 局部顺序给出（长度至少为节点数）。
pub fn scaled_jacobian(kind: CellType, points: &[[f64; 3]]) -> f64 {
    match kind {
        CellType::Tetra => corner_min(points, &TET_CORNERS) * std::f64::consts::SQRT_2,
        CellType::Hexahedron => {
            let axis = |plus: [usize; 4], minus: [usize; 4]| {
                let mut sum = [0.0; 3];
                for (a, b) in plus.into_iter().zip(minus) {
                    let edge = sub(points[a], points[b]);
                    sum = [sum[0] + edge[0], sum[1] + edge[1], sum[2] + edge[2]];
                }
                sum
            };
            let x1 = axis([1, 2, 5, 6], [0, 3, 4, 7]);
            let x2 = axis([3, 2, 7, 6], [0, 1, 4, 5]);
            let x3 = axis([4, 5, 6, 7], [0, 1, 2, 3]);
            corner_min(points, &HEX_CORNERS).min(unit_triple(x1, x2, x3))
        }
        CellType::Wedge => corner_min(points, &WEDGE_CORNERS) * 2.0 / 3.0_f64.sqrt(),
        CellType::Pyramid => corner_min(points, &PYRAMID_CORNERS) * std::f64::consts::SQRT_2,
    }
}

fn corner_min(points: &[[f64; 3]], corners: &[[usize; 4]]) -> f64 {
    corners
        .iter()
        .map(|&[corner, a, b, c]| {
            unit_triple(
                sub(points[a], points[corner]),
                sub(points[b], points[corner]),
                sub(points[c], points[corner]),
            )
        })
        .fold(f64::INFINITY, f64::min)
}

/// 三个向量单位化后的三重积；任一向量长度为 0 时返回 0。
fn unit_triple(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let lengths = dot(a, a).sqrt() * dot(b, b).sqrt() * dot(c, c).sqrt();
    if lengths.is_nan() || lengths <= 0.0 || !lengths.is_finite() {
        return 0.0;
    }
    (dot(a, cross(b, c)) / lengths).clamp(-1.0, 1.0)
}

/// 某一单元类型的质量汇总。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellTypeQuality {
    pub count: usize,
    /// 最小缩放雅可比；`count == 0` 时为 0。
    pub min_scaled_jacobian: f64,
    pub mean_scaled_jacobian: f64,
    /// 缩放雅可比低于 [`POOR_SCALED_JACOBIAN`] 的单元数。
    pub poor_count: usize,
}

impl Default for CellTypeQuality {
    fn default() -> Self {
        Self {
            count: 0,
            min_scaled_jacobian: 0.0,
            mean_scaled_jacobian: 0.0,
            poor_count: 0,
        }
    }
}

/// 按 [`CellType::ALL`] 顺序累加的部分和，可跨线程/分批合并。
#[derive(Debug, Clone, Copy)]
pub(crate) struct TypeAccumulator {
    count: [usize; 4],
    min: [f64; 4],
    sum: [f64; 4],
    poor: [usize; 4],
}

impl Default for TypeAccumulator {
    fn default() -> Self {
        Self {
            count: [0; 4],
            min: [f64::INFINITY; 4],
            sum: [0.0; 4],
            poor: [0; 4],
        }
    }
}

impl TypeAccumulator {
    /// 计入一个单元并返回其缩放雅可比。
    pub(crate) fn add(&mut self, kind: CellType, points: &[[f64; 3]]) -> f64 {
        let value = scaled_jacobian(kind, points);
        let slot = kind.ordinal();
        self.count[slot] += 1;
        self.min[slot] = self.min[slot].min(value);
        self.sum[slot] += value;
        if value.is_nan() || value < POOR_SCALED_JACOBIAN {
            self.poor[slot] += 1;
        }
        value
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        for slot in 0..4 {
            self.count[slot] += other.count[slot];
            self.min[slot] = self.min[slot].min(other.min[slot]);
            self.sum[slot] += other.sum[slot];
            self.poor[slot] += other.poor[slot];
        }
    }

    pub(crate) fn finish(&self) -> [CellTypeQuality; 4] {
        std::array::from_fn(|slot| {
            let count = self.count[slot];
            if count == 0 {
                return CellTypeQuality::default();
            }
            CellTypeQuality {
                count,
                min_scaled_jacobian: self.min[slot],
                mean_scaled_jacobian: self.sum[slot] / count as f64,
                poor_count: self.poor[slot],
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn ideal_cells_score_one() {
        let s3 = 3.0_f64.sqrt() / 2.0;
        let tet = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.5, s3, 0.0],
            [0.5, s3 / 3.0, (2.0_f64 / 3.0).sqrt()],
        ];
        assert!(close(scaled_jacobian(CellType::Tetra, &tet), 1.0));
        let cube: Vec<[f64; 3]> = [0, 1, 3, 2, 4, 5, 7, 6]
            .iter()
            .map(|&bits: &usize| {
                [
                    (bits & 1) as f64,
                    ((bits >> 1) & 1) as f64,
                    (bits >> 2) as f64,
                ]
            })
            .collect();
        assert!(close(scaled_jacobian(CellType::Hexahedron, &cube), 1.0));
        let wedge = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.5, s3, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [0.5, s3, 1.0],
        ];
        assert!(close(scaled_jacobian(CellType::Wedge, &wedge), 1.0));
        let pyramid = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.5, 0.5, 0.5_f64.sqrt()],
        ];
        assert!(close(scaled_jacobian(CellType::Pyramid, &pyramid), 1.0));
    }

    #[test]
    fn distorted_and_inverted_cells_score_lower() {
        // 高宽比 100 的薄边界层棱柱：角点仍正交，缩放雅可比保持 1（Verdict 同样如此）。
        let s3 = 3.0_f64.sqrt() / 2.0;
        let thin = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.5, s3, 0.0],
            [0.0, 0.0, 0.01],
            [1.0, 0.0, 0.01],
            [0.5, s3, 0.01],
        ];
        assert!(close(scaled_jacobian(CellType::Wedge, &thin), 1.0));
        // 顶面整体剪切的六面体：角点不再正交。
        let mut sheared: Vec<[f64; 3]> = [0, 1, 3, 2, 4, 5, 7, 6]
            .iter()
            .map(|&bits: &usize| {
                [
                    (bits & 1) as f64,
                    ((bits >> 1) & 1) as f64,
                    (bits >> 2) as f64,
                ]
            })
            .collect();
        for point in &mut sheared[4..] {
            point[0] += 3.0;
        }
        let value = scaled_jacobian(CellType::Hexahedron, &sheared);
        assert!(value > 0.0 && value < POOR_SCALED_JACOBIAN + 0.2, "{value}");
        // 锥顶压到底面以下：反转。
        let flipped = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.5, 0.5, -0.5],
        ];
        assert!(scaled_jacobian(CellType::Pyramid, &flipped) < 0.0);
        let mut total = TypeAccumulator::default();
        total.add(CellType::Pyramid, &flipped);
        let mut other = TypeAccumulator::default();
        other.add(CellType::Wedge, &thin);
        total.merge(&other);
        let summary = total.finish();
        assert_eq!(summary[CellType::Pyramid.ordinal()].poor_count, 1);
        assert_eq!(summary[CellType::Wedge.ordinal()].poor_count, 0);
        assert_eq!(
            summary[CellType::Hexahedron.ordinal()],
            CellTypeQuality::default()
        );
    }
}
