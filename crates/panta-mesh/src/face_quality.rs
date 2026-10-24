//! 基于面邻接（owner / neighbour）的面质量（task 032 PR 2），口径与 OpenFOAM checkMesh 一致：
//!
//! - 面中心 / 面积向量：三角形直接取重心与半叉积；多边形以顶点平均点为扇心，按子三角形面积
//!   加权求中心，面积向量为子三角形面积向量之和（`primitiveMesh::makeFaceCentresAndAreas`）。
//! - 单元中心：以面中心平均点为估计中心，按各面棱锥体积加权棱锥质心（`0.75 面中心 + 0.25 估计
//!   中心`）；体积接近 0 时退回估计中心（`makeCellCentresAndVols`）。
//! - 非正交角：内部面面积向量与 owner → neighbour 中心向量的夹角；最大值取最小余弦的反余弦，
//!   平均值取余弦平均后的反余弦（checkMesh 的报告方式），大于 70° 记为严重。
//! - 偏斜度：面中心到「owner–neighbour 连线与面交点」的距离，按 `0.2 |d|` 与面顶点沿偏斜方向
//!   的最大距离归一化；边界面以面法向投影代替 neighbour 方向、系数 0.4。大于 4 记为偏高。
//! - 正交质量（Fluent 定义）：逐单元取各面「面积向量 vs 中心 → 面中心」与「面积向量 vs 中心 →
//!   邻居中心」余弦的最小值，再取全网格最小值。

use crate::volume::{cross, dot, sub};

/// 非正交角超过该值记为严重（checkMesh 默认 `nonOrthThreshold`）。
pub const SEVERE_NON_ORTHOGONALITY_DEGREES: f64 = 70.0;
/// 偏斜度超过该值记为偏高（checkMesh 默认 `skewThreshold`）。
pub const HIGH_SKEWNESS: f64 = 4.0;

const ROOT_VSMALL: f64 = 1.0e-150;
const VSMALL: f64 = 1.0e-300;

/// 面质量汇总；可跨区域用 [`FaceQuality::merge`] 合并。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceQuality {
    pub internal_faces: u64,
    pub boundary_faces: u64,
    /// 内部面余弦的最小值与总和（用于最大 / 平均非正交角）。
    pub min_cosine: f64,
    pub sum_cosine: f64,
    /// 非正交角大于 [`SEVERE_NON_ORTHOGONALITY_DEGREES`] 的内部面数（含余弦非正的面）。
    pub severe_non_orthogonal_faces: u64,
    pub max_skewness: f64,
    /// 偏斜度大于 [`HIGH_SKEWNESS`] 的面数（内部面与边界面）。
    pub high_skew_faces: u64,
    /// 全网格最小正交质量；无单元时为 1。
    pub min_orthogonal_quality: f64,
    /// 正交质量低于 [`crate::POOR_ORTHOGONAL_QUALITY`] 的单元数。
    pub poor_orthogonal_cells: u64,
}

impl Default for FaceQuality {
    fn default() -> Self {
        Self {
            internal_faces: 0,
            boundary_faces: 0,
            min_cosine: 1.0,
            sum_cosine: 0.0,
            severe_non_orthogonal_faces: 0,
            max_skewness: 0.0,
            high_skew_faces: 0,
            min_orthogonal_quality: 1.0,
            poor_orthogonal_cells: 0,
        }
    }
}

impl FaceQuality {
    /// 最大非正交角（度）；无内部面时为 0。
    pub fn max_non_orthogonality_degrees(&self) -> f64 {
        self.min_cosine.clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// 平均非正交角（度，checkMesh 口径：余弦平均后取反余弦）；无内部面时为 0。
    pub fn mean_non_orthogonality_degrees(&self) -> f64 {
        if self.internal_faces == 0 {
            return 0.0;
        }
        (self.sum_cosine / self.internal_faces as f64)
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    }

    pub fn merge(&mut self, other: &Self) {
        self.internal_faces += other.internal_faces;
        self.boundary_faces += other.boundary_faces;
        self.min_cosine = self.min_cosine.min(other.min_cosine);
        self.sum_cosine += other.sum_cosine;
        self.severe_non_orthogonal_faces += other.severe_non_orthogonal_faces;
        self.max_skewness = self.max_skewness.max(other.max_skewness);
        self.high_skew_faces += other.high_skew_faces;
        self.min_orthogonal_quality = self
            .min_orthogonal_quality
            .min(other.min_orthogonal_quality);
        self.poor_orthogonal_cells += other.poor_orthogonal_cells;
    }
}

/// 多面体网格（OpenFOAM polyMesh 布局）的面质量。
///
/// `face_ends[f]` 为第 `f` 个面在 `face_nodes` 中的结束偏移；面 `f < neighbour.len()` 为内部面，
/// 其余为边界面；面按 owner 侧朝外定向。调用方保证下标有效（越界节点 / 单元会 panic 前被
/// 导入校验拒绝）；越界时该面被跳过。
pub fn poly_mesh_face_quality(
    points: &[[f64; 3]],
    face_ends: &[usize],
    face_nodes: &[u32],
    owner: &[u32],
    neighbour: &[u32],
    cell_count: usize,
) -> FaceQuality {
    let face_count = face_ends.len().min(owner.len());
    let face = |index: usize| -> &[u32] {
        let start = if index == 0 { 0 } else { face_ends[index - 1] };
        face_nodes.get(start..face_ends[index]).unwrap_or(&[])
    };
    let point = |node: u32| points.get(node as usize).copied().unwrap_or([0.0; 3]);
    let mut centres = Vec::with_capacity(face_count);
    let mut areas = Vec::with_capacity(face_count);
    for index in 0..face_count {
        let (centre, area) = face_centre_and_area(face(index), &point);
        centres.push(centre);
        areas.push(area);
    }
    let cell_of = |cell: u32| (cell as usize) < cell_count;
    // 估计中心：面中心平均。
    let mut estimate = vec![[0.0; 3]; cell_count];
    let mut face_counts = vec![0usize; cell_count];
    let sides = |index: usize| {
        let own = Some(owner[index]).filter(|&cell| cell_of(cell));
        let nei = neighbour.get(index).copied().filter(|&cell| cell_of(cell));
        (own, nei)
    };
    for (index, &centre) in centres.iter().enumerate() {
        let (own, nei) = sides(index);
        for cell in own.into_iter().chain(nei) {
            add_to(&mut estimate[cell as usize], centre, 1.0);
            face_counts[cell as usize] += 1;
        }
    }
    for (centre, &count) in estimate.iter_mut().zip(&face_counts) {
        if count > 0 {
            *centre = centre.map(|value| value / count as f64);
        }
    }
    let mut weighted = vec![[0.0; 3]; cell_count];
    let mut volumes = vec![0.0; cell_count];
    for index in 0..face_count {
        let (own, nei) = sides(index);
        for (cell, sign) in own
            .map(|cell| (cell, 1.0))
            .into_iter()
            .chain(nei.map(|cell| (cell, -1.0)))
        {
            let slot = cell as usize;
            let pyramid = sign * dot(areas[index], sub(centres[index], estimate[slot]));
            let centroid =
                [0, 1, 2].map(|axis| 0.75 * centres[index][axis] + 0.25 * estimate[slot][axis]);
            add_to(&mut weighted[slot], centroid, pyramid);
            volumes[slot] += pyramid;
        }
    }
    let cell_centres: Vec<[f64; 3]> = (0..cell_count)
        .map(|slot| {
            if volumes[slot].abs() > VSMALL {
                weighted[slot].map(|value| value / volumes[slot])
            } else {
                estimate[slot]
            }
        })
        .collect();

    let severe_cosine = SEVERE_NON_ORTHOGONALITY_DEGREES.to_radians().cos();
    let mut quality = FaceQuality::default();
    let mut orthogonal = vec![1.0_f64; cell_count];
    for index in 0..face_count {
        let (Some(own), nei) = sides(index) else {
            continue;
        };
        let area = areas[index];
        let centre = centres[index];
        let own_centre = cell_centres[own as usize];
        let area_length = norm(area);
        let to_face = sub(centre, own_centre);
        let skewness = match nei {
            Some(nei) => {
                let d = sub(cell_centres[nei as usize], own_centre);
                let cosine = dot(d, area) / (norm(d) * area_length + VSMALL);
                quality.internal_faces += 1;
                quality.min_cosine = quality.min_cosine.min(cosine);
                quality.sum_cosine += cosine;
                if cosine < severe_cosine {
                    quality.severe_non_orthogonal_faces += 1;
                }
                let clamped = cosine.clamp(-1.0, 1.0);
                let own_value = clamped.min(cos_angle(area, to_face));
                let nei_value =
                    clamped.min(cos_angle(area, sub(cell_centres[nei as usize], centre)));
                orthogonal[own as usize] = orthogonal[own as usize].min(own_value);
                orthogonal[nei as usize] = orthogonal[nei as usize].min(nei_value);
                skew(face(index), &point, centre, area, to_face, d, 0.2)
            }
            None => {
                quality.boundary_faces += 1;
                orthogonal[own as usize] = orthogonal[own as usize].min(cos_angle(area, to_face));
                let normal = area.map(|value| value / (area_length + ROOT_VSMALL));
                let d = normal.map(|value| value * dot(normal, to_face));
                skew(face(index), &point, centre, area, to_face, d, 0.4)
            }
        };
        quality.max_skewness = quality.max_skewness.max(skewness);
        if skewness > HIGH_SKEWNESS {
            quality.high_skew_faces += 1;
        }
    }
    for value in orthogonal {
        quality.min_orthogonal_quality = quality.min_orthogonal_quality.min(value);
        if value < crate::POOR_ORTHOGONAL_QUALITY {
            quality.poor_orthogonal_cells += 1;
        }
    }
    quality
}

fn skew(
    nodes: &[u32],
    point: &impl Fn(u32) -> [f64; 3],
    centre: [f64; 3],
    area: [f64; 3],
    to_face: [f64; 3],
    d: [f64; 3],
    factor: f64,
) -> f64 {
    let ratio = dot(area, to_face) / (dot(area, d) + ROOT_VSMALL);
    let sv = [0, 1, 2].map(|axis| to_face[axis] - ratio * d[axis]);
    let sv_length = norm(sv);
    let sv_hat = sv.map(|value| value / (sv_length + ROOT_VSMALL));
    let mut distance = factor * norm(d) + ROOT_VSMALL;
    for &node in nodes {
        distance = distance.max(dot(sv_hat, sub(point(node), centre)).abs());
    }
    sv_length / distance
}

fn face_centre_and_area(nodes: &[u32], point: &impl Fn(u32) -> [f64; 3]) -> ([f64; 3], [f64; 3]) {
    match nodes {
        [] | [_] | [_, _] => ([0.0; 3], [0.0; 3]),
        &[a, b, c] => {
            let (a, b, c) = (point(a), point(b), point(c));
            let centre = [0, 1, 2].map(|axis| (a[axis] + b[axis] + c[axis]) / 3.0);
            (centre, cross(sub(b, a), sub(c, a)).map(|value| 0.5 * value))
        }
        _ => {
            let mut mean = [0.0; 3];
            for &node in nodes {
                add_to(&mut mean, point(node), 1.0);
            }
            let mean = mean.map(|value| value / nodes.len() as f64);
            let mut sum_normal = [0.0; 3];
            let mut sum_area = 0.0;
            let mut sum_moment = [0.0; 3];
            for (index, &node) in nodes.iter().enumerate() {
                let here = point(node);
                let next = point(nodes[(index + 1) % nodes.len()]);
                let centroid = [0, 1, 2].map(|axis| here[axis] + next[axis] + mean[axis]);
                let normal = cross(sub(next, here), sub(mean, here));
                let size = norm(normal);
                add_to(&mut sum_normal, normal, 1.0);
                sum_area += size;
                add_to(&mut sum_moment, centroid, size);
            }
            if sum_area < ROOT_VSMALL {
                (mean, [0.0; 3])
            } else {
                (
                    sum_moment.map(|value| value / (3.0 * sum_area)),
                    sum_normal.map(|value| 0.5 * value),
                )
            }
        }
    }
}

fn cos_angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    let lengths = norm(a) * norm(b);
    if lengths > 0.0 && lengths.is_finite() {
        (dot(a, b) / lengths).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn norm(value: [f64; 3]) -> f64 {
    dot(value, value).sqrt()
}

fn add_to(target: &mut [f64; 3], value: [f64; 3], weight: f64) {
    for axis in 0..3 {
        target[axis] += weight * value[axis];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (点, 面结束偏移, 面节点, owner, neighbour)。
    type PolyMesh = (Vec<[f64; 3]>, Vec<usize>, Vec<u32>, Vec<u32>, Vec<u32>);

    /// 两个单位立方体沿 x 并排（OpenFOAM 布局：1 个内部面 + 10 个边界面）。
    fn two_cubes(shift: f64) -> PolyMesh {
        let mut points = Vec::new();
        for x in 0..3 {
            for (y, z) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                let offset = if x == 2 { shift } else { 0.0 };
                points.push([x as f64, y + offset, z]);
            }
        }
        let node = |x: u32, corner: u32| x * 4 + corner;
        // 内部面 x=1，法向 +x（owner 0 → neighbour 1）。
        let mut faces: Vec<Vec<u32>> = vec![vec![node(1, 0), node(1, 1), node(1, 2), node(1, 3)]];
        let mut owners = vec![0];
        for cell in 0..2_u32 {
            let (a, b) = (cell, cell + 1);
            // 侧面按外法向定向。
            faces.push(vec![node(b, 0), node(b, 3), node(a, 3), node(a, 0)]); // y=0
            faces.push(vec![node(a, 2), node(b, 2), node(b, 1), node(a, 1)]); // y=1
            faces.push(vec![node(a, 1), node(b, 1), node(b, 0), node(a, 0)]); // z=0
            faces.push(vec![node(b, 3), node(b, 2), node(a, 2), node(a, 3)]); // z=1
            owners.extend([cell; 4]);
        }
        faces.push(vec![node(0, 3), node(0, 2), node(0, 1), node(0, 0)]);
        owners.push(0);
        faces.push(vec![node(2, 0), node(2, 1), node(2, 2), node(2, 3)]);
        owners.push(1);
        let mut ends = Vec::new();
        let mut flat = Vec::new();
        for face in &faces {
            flat.extend(face);
            ends.push(flat.len());
        }
        (points, ends, flat, owners, vec![1])
    }

    #[test]
    fn aligned_cubes_are_orthogonal_and_unskewed() {
        let (points, ends, nodes, owner, neighbour) = two_cubes(0.0);
        let quality = poly_mesh_face_quality(&points, &ends, &nodes, &owner, &neighbour, 2);
        assert_eq!((quality.internal_faces, quality.boundary_faces), (1, 10));
        assert!(quality.max_non_orthogonality_degrees() < 1e-6);
        assert!(quality.max_skewness < 1e-9, "{}", quality.max_skewness);
        assert!((quality.min_orthogonal_quality - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sheared_outer_cell_tilts_the_internal_face() {
        let (points, ends, nodes, owner, neighbour) = two_cubes(0.5);
        let quality = poly_mesh_face_quality(&points, &ends, &nodes, &owner, &neighbour, 2);
        assert!(quality.max_non_orthogonality_degrees() > 1.0);
        assert!(quality.max_skewness > 0.0);
        assert!(quality.min_orthogonal_quality < 1.0);
        let mut merged = FaceQuality::default();
        merged.merge(&quality);
        merged.merge(&quality);
        assert_eq!(merged.internal_faces, 2);
        assert!(
            (merged.mean_non_orthogonality_degrees() - quality.mean_non_orthogonality_degrees())
                .abs()
                < 1e-12
        );
    }
}
