//! 体网格摘要的 FFI 适配：GUI 线程同步调用，只读节点/单元计数（task 021/022）。
//! 质量统计、切面与 VTU 导出走 `display` 的后台快照入口（task 022）。
use crate::bridge;
use crate::project::ProjectService;

pub(super) fn project_service_volume_summary(
    service: &ProjectService,
    import_id: &str,
) -> bridge::VolumeInfoResult {
    let mut result = bridge::VolumeInfoResult::default();
    let streamed = service
        .service
        .streamed_asset_for_import(import_id)
        .and_then(|asset| match asset.kind {
            panta_core::project::StreamedKind::Volume(counts) => Some(counts),
            panta_core::project::StreamedKind::Surface { .. } => None,
        });
    match (service.service.volume_for_import(import_id), streamed) {
        (Some(volume), _) => {
            result.available = true;
            result.node_count = volume.nodes.len() as u64;
            result.tet_count = volume.tets.len() as u64;
            result.cell_count = volume.cell_count() as u64;
            result.cell_type_summary = cell_type_summary(volume.cell_type_counts());
        }
        // 单元未驻留的体网格（task 025）：计数来自资产，命令按需流式读取。
        (None, Some(counts)) => {
            result.available = true;
            result.streamed = true;
            result.node_count = counts.nodes as u64;
            result.tet_count = counts.tets as u64;
            result.cell_count = counts.cells as u64;
        }
        (None, None) => {
            result.error =
                panta_core::project::ProjectError::VolumeUnavailable(import_id.to_owned()).into();
        }
    }
    result
}

/// 中文单元类型名（task 030），顺序同 [`panta_mesh::CellType::ALL`]。
pub(crate) fn cell_type_label(kind: panta_mesh::CellType) -> &'static str {
    match kind {
        panta_mesh::CellType::Tetra => "四面体",
        panta_mesh::CellType::Hexahedron => "六面体",
        panta_mesh::CellType::Wedge => "三棱柱",
        panta_mesh::CellType::Pyramid => "四棱锥",
        panta_mesh::CellType::Polyhedron => "多面体",
    }
}

/// “六面体 8 · 四面体 4”形式的类型计数，跳过 0。
pub(crate) fn cell_type_summary(counts: [u64; panta_mesh::CELL_TYPE_COUNT]) -> String {
    panta_mesh::CellType::ALL
        .iter()
        .zip(counts)
        .filter(|(_, count)| *count > 0)
        .map(|(&kind, count)| format!("{} {count}", cell_type_label(kind)))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// 每类一行：“三棱柱 12：缩放雅可比 最小 0.83 / 平均 0.97，劣质 0”；多面体为正交质量。
pub(crate) fn cell_quality_summary(quality: &panta_mesh::TetQuality) -> String {
    panta_mesh::CellType::ALL
        .iter()
        .zip(&quality.by_type)
        .filter(|(_, summary)| summary.count > 0)
        .map(|(&kind, summary)| {
            // 多面体的指标是正交质量（task 031），其余类型为缩放雅可比。
            let metric = if kind == panta_mesh::CellType::Polyhedron {
                "正交质量"
            } else {
                "缩放雅可比"
            };
            format!(
                "{} {}：{metric} 最小 {:.3} / 平均 {:.3}，劣质 {}",
                cell_type_label(kind),
                summary.count,
                summary.min_scaled_jacobian,
                summary.mean_scaled_jacobian,
                summary.poor_count
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 结果场摘要（task 029）：GUI 线程同步调用，只读 sidecar 头部与字段表（KiB 级）。
pub(super) fn project_service_result_fields(
    service: &ProjectService,
    import_id: &str,
) -> bridge::ResultFieldListResult {
    let mut result = bridge::ResultFieldListResult::default();
    let summaries = match service.service.result_fields_for_import(import_id) {
        Ok(summaries) => summaries,
        Err(error) => {
            result.error = error.into();
            return result;
        }
    };
    let resident = service.service.volume_for_import(import_id).is_some();
    let chunks = service
        .service
        .streamed_asset_for_import(import_id)
        .and_then(|asset| asset.chunks);
    result.fields = summaries
        .into_iter()
        .map(|field| {
            let sampled = field.components == 1 || field.components == 3;
            let sectionable = sampled
                && (resident
                    || chunks.as_ref().is_some_and(|chunks| {
                        chunks
                            .0
                            .field_index(&field.name, field.association)
                            .is_some()
                    }));
            let [min, max] = field.range.unwrap_or([0.0, 0.0]);
            bridge::ResultFieldInfo {
                association: field.association.as_str().to_owned(),
                name: field.name,
                components: field.components,
                units: field.units,
                missing: field.missing,
                has_range: field.range.is_some(),
                min,
                max,
                sectionable,
            }
        })
        .collect();
    result
}
