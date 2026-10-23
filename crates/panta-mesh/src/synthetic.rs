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
