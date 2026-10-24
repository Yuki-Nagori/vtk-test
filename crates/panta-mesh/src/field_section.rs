//! 结果场切面取值（task 029 PR 2）：在切面几何上按节点场线性插值、单元场取单元值。
//!
//! 几何与 [`crate::section_surface`] / 分块精细切面逐位一致（同一裁剪参数 `t`、同一排序与扇形剖分），
//! 每个输出三角形额外带三个顶点标量。节点场先对分量插值再取标量（矢量模长在插值后计算，
//! 与 ParaView 的点数据切面一致）；单元场在切面多边形和被裁剪的外表面三角形上取所属单元的值。
//! 非有限值（缺失）原样传播为 NaN，由显示层画成缺失色。

use std::collections::HashMap;

use crate::volume::{
    Lerp, VolumeError, clip_triangle_with, fan_by, order_around_by, plane_distances,
};
use crate::{FieldAssociation, ResultField, SurfaceMesh, TetMesh};

/// 取标量的方式：矢量取模长或某一分量；标量场只允许 `Magnitude` 或 `Component(0)`（都等于值本身）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldComponent {
    Magnitude,
    Component(u8),
}

impl FieldComponent {
    /// 校验分量号不越界；场只支持 1 或 3 个分量。
    pub fn validate(self, components: u8) -> Result<(), String> {
        if components != 1 && components != 3 {
            return Err(format!(
                "fields with {components} components cannot be sampled"
            ));
        }
        match self {
            Self::Component(index) if index >= components => Err(format!(
                "component {index} out of range for a {components}-component field"
            )),
            _ => Ok(()),
        }
    }
}

/// 带场值的切面：`values[i]` 是 `surface.triangles[i]` 三个顶点的标量。
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSection {
    pub surface: SurfaceMesh,
    pub values: Vec<[f64; 3]>,
}

impl FieldSection {
    /// 有限顶点值的区间；没有有限值时为 `None`。
    pub fn value_range(&self) -> Option<[f64; 2]> {
        self.values
            .iter()
            .flatten()
            .filter(|value| value.is_finite())
            .fold(None, |range, &value| match range {
                Some([low, high]) => Some([f64::min(low, value), f64::max(high, value)]),
                None => Some([value, value]),
            })
    }

    /// 值缺失（非有限）的顶点数（按三角形顶点计，不去重）。
    pub fn missing_vertices(&self) -> usize {
        self.values
            .iter()
            .flatten()
            .filter(|value| !value.is_finite())
            .count()
    }
}

/// 切面累加器：几何与未归约的分量元组。
#[derive(Default)]
pub(crate) struct FieldOutput {
    pub triangles: Vec<[[f64; 3]; 3]>,
    pub tuples: Vec<[[f64; 3]; 3]>,
}

impl FieldOutput {
    pub fn append(&mut self, mut other: Self) {
        self.triangles.append(&mut other.triangles);
        self.tuples.append(&mut other.tuples);
    }

    pub fn finish(self, components: u8, component: FieldComponent) -> FieldSection {
        FieldSection {
            surface: SurfaceMesh {
                triangles: self.triangles,
            },
            values: self
                .tuples
                .into_iter()
                .map(|corners| corners.map(|tuple| reduce(tuple, components, component)))
                .collect(),
        }
    }
}

/// 第 `index` 个元组（1 或 3 分量，补零到 3）。
pub(crate) fn tuple(values: &[f64], components: usize, index: usize) -> [f64; 3] {
    let start = index * components;
    std::array::from_fn(|slot| {
        if slot < components {
            values[start + slot]
        } else {
            0.0
        }
    })
}

/// 元组 → 标量；任一分量非有限则为 NaN。
pub(crate) fn reduce(tuple: [f64; 3], components: u8, component: FieldComponent) -> f64 {
    let used = &tuple[..usize::from(components.clamp(1, 3))];
    if used.iter().any(|value| !value.is_finite()) {
        return f64::NAN;
    }
    match component {
        FieldComponent::Component(index) => tuple[usize::from(index)],
        FieldComponent::Magnitude if components == 1 => tuple[0],
        FieldComponent::Magnitude => used.iter().map(|value| value * value).sum::<f64>().sqrt(),
    }
}

/// 元组的标量区间键（标量取值、矢量取模长；与 [`ResultField::summary`] 一致），用于块级值区间。
pub(crate) fn range_key(tuple: [f64; 3], components: u8) -> f64 {
    reduce(tuple, components, FieldComponent::Magnitude)
}

/// 四面体截面多边形（与 `section_polygon` 同一几何），顶点带 `attribute(局部节点)` 插值元组。
pub(crate) fn section_polygon_with(
    nodes: &[[f64; 3]],
    distance: &[f64],
    corners: [u32; 4],
    normal: [f64; 3],
    attribute: impl Fn(u32) -> [f64; 3],
    output: &mut FieldOutput,
) {
    let kept = corners
        .iter()
        .filter(|&&node| distance[node as usize] <= 0.0)
        .count();
    if kept == 0 || kept == 4 {
        return;
    }
    let mut points: Vec<([f64; 3], [f64; 3])> = Vec::with_capacity(4);
    for i in 0..4 {
        for j in i + 1..4 {
            let (a, b) = (corners[i], corners[j]);
            let (da, db) = (distance[a as usize], distance[b as usize]);
            if (da <= 0.0) != (db <= 0.0) {
                let t = da / (da - db);
                points.push((
                    nodes[a as usize].lerp(nodes[b as usize], t),
                    attribute(a).lerp(attribute(b), t),
                ));
            }
        }
    }
    order_around_by(&mut points, |item| item.0, normal);
    emit_fan(&points, output);
}

/// 一个外表面三角形按平面裁剪，顶点带元组。
pub(crate) fn clip_with(corners: [([f64; 3], f64, [f64; 3]); 3], output: &mut FieldOutput) {
    let polygon = clip_triangle_with(corners);
    emit_fan(&polygon, output);
}

fn emit_fan(polygon: &[([f64; 3], [f64; 3])], output: &mut FieldOutput) {
    fan_by(
        polygon,
        |item| item.0,
        |items| {
            output.triangles.push(items.map(|item| item.0));
            output.tuples.push(items.map(|item| item.1));
        },
    );
}

/// 校验场与体网格匹配。
pub(crate) fn check_field(
    field: &ResultField,
    node_count: u64,
    cell_count: u64,
    component: FieldComponent,
) -> Result<(), String> {
    component.validate(field.components)?;
    let tuples = match field.association {
        FieldAssociation::Node => node_count,
        FieldAssociation::Cell => cell_count,
    };
    if field.values.len() as u64 != tuples * u64::from(field.components) {
        return Err(format!(
            "field {} has {} values, expected {tuples} x {}",
            field.name,
            field.values.len(),
            field.components
        ));
    }
    Ok(())
}

/// 每个外表面三角形所属的四面体（全局下标）；找不到时为 `u32::MAX`。
pub(crate) fn boundary_owners(mesh: &TetMesh) -> Vec<u32> {
    let mut on_boundary = vec![0u64; mesh.nodes.len().div_ceil(64)];
    let mut faces = HashMap::with_capacity(mesh.boundary.len());
    for (index, face) in mesh.boundary.iter().enumerate() {
        for node in face.nodes {
            on_boundary[node as usize / 64] |= 1 << (node % 64);
        }
        let mut key = face.nodes;
        key.sort_unstable();
        faces.insert(key, index);
    }
    let mut owners = vec![u32::MAX; mesh.boundary.len()];
    for (tet_index, tet) in mesh.tets.iter().enumerate() {
        for skip in 0..4 {
            let mut key = [0u32; 3];
            let mut slot = 0;
            for (corner, &node) in tet.nodes.iter().enumerate() {
                if corner != skip {
                    key[slot] = node;
                    slot += 1;
                }
            }
            if key
                .iter()
                .any(|&node| on_boundary[node as usize / 64] & (1 << (node % 64)) == 0)
            {
                continue;
            }
            key.sort_unstable();
            if let Some(&face) = faces.get(&key) {
                owners[face] = tet_index as u32;
            }
        }
    }
    owners
}

/// 驻留体网格的结果场切面（几何与 [`crate::section_surface`] 逐位一致，三角形顺序也一致）。
/// 取消返回 `Ok(None)`。
pub fn section_surface_field(
    mesh: &TetMesh,
    field: &ResultField,
    component: FieldComponent,
    origin: [f64; 3],
    normal: [f64; 3],
    progress: &crate::Progress,
) -> Result<Option<FieldSection>, VolumeError> {
    let length = crate::volume::dot(normal, normal).sqrt();
    if !length.is_finite() || length == 0.0 || origin.iter().any(|value| !value.is_finite()) {
        return Err(VolumeError("section plane is degenerate".to_owned()));
    }
    check_field(
        field,
        mesh.nodes.len() as u64,
        mesh.tets.len() as u64,
        component,
    )
    .map_err(VolumeError)?;
    let normal = normal.map(|value| value / length);
    let distance = plane_distances(&mesh.nodes, origin, normal);
    let components = usize::from(field.components);
    let values = &field.values;
    let mut output = FieldOutput::default();
    match field.association {
        FieldAssociation::Node => {
            for face in &mesh.boundary {
                clip_with(
                    face.nodes.map(|node| {
                        (
                            mesh.nodes[node as usize],
                            distance[node as usize],
                            tuple(values, components, node as usize),
                        )
                    }),
                    &mut output,
                );
            }
        }
        FieldAssociation::Cell => {
            let owners = boundary_owners(mesh);
            for (face, &owner) in mesh.boundary.iter().zip(&owners) {
                let value = if owner == u32::MAX {
                    [f64::NAN; 3]
                } else {
                    tuple(values, components, owner as usize)
                };
                clip_with(
                    face.nodes
                        .map(|node| (mesh.nodes[node as usize], distance[node as usize], value)),
                    &mut output,
                );
            }
        }
    }
    let total = mesh.tets.len() as u64;
    for (index, tet) in mesh.tets.iter().enumerate() {
        if index.is_multiple_of(crate::PROGRESS_STRIDE) {
            if progress.is_cancelled() {
                return Ok(None);
            }
            progress.report(0.0, 1.0, index as u64, total);
        }
        match field.association {
            FieldAssociation::Node => section_polygon_with(
                &mesh.nodes,
                &distance,
                tet.nodes,
                normal,
                |node| tuple(values, components, node as usize),
                &mut output,
            ),
            FieldAssociation::Cell => {
                let value = tuple(values, components, index);
                section_polygon_with(
                    &mesh.nodes,
                    &distance,
                    tet.nodes,
                    normal,
                    |_| value,
                    &mut output,
                );
            }
        }
    }
    progress.set_fraction(1.0);
    Ok(Some(output.finish(field.components, component)))
}
