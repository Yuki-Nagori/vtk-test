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

/// 面邻接质量（task 032 PR 2）：非正交角 / 偏斜度按 checkMesh 口径，正交质量按 Fluent 定义。
pub(crate) fn face_quality_summary(quality: &panta_core::project::FaceQualityRecord) -> String {
    format!(
        "内部面 {} / 边界面 {}\n非正交角 最大 {:.2}° / 平均 {:.2}°，超过 {:.0}° {}\n偏斜度 最大 {:.3}，超过 {:.0} {}\n正交质量 最小 {:.3}，低于 {:.2} {}",
        quality.internal_faces,
        quality.boundary_faces,
        quality.max_non_orthogonality_degrees,
        quality.mean_non_orthogonality_degrees,
        panta_mesh::SEVERE_NON_ORTHOGONALITY_DEGREES,
        quality.severe_non_orthogonal_faces,
        quality.max_skewness,
        panta_mesh::HIGH_SKEWNESS,
        quality.high_skew_faces,
        quality.min_orthogonal_quality,
        panta_mesh::POOR_ORTHOGONAL_QUALITY,
        quality.poor_orthogonal_cells
    )
}

pub(crate) fn face_quality_warning(quality: &panta_core::project::FaceQualityRecord) -> bool {
    quality.severe_non_orthogonal_faces > 0
        || quality.high_skew_faces > 0
        || quality.poor_orthogonal_cells > 0
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

/// 结果场时间步（task 033）：GUI 线程同步调用，只读清单。
pub(super) fn project_service_result_time_steps(
    service: &ProjectService,
    import_id: &str,
) -> bridge::ResultTimeStepListResult {
    match service.service.result_time_steps(import_id) {
        Ok(steps) => bridge::ResultTimeStepListResult {
            steps: steps
                .into_iter()
                .map(|step| bridge::ResultTimeStepInfo {
                    time: step.time,
                    label: step.label,
                    field_count: step.field_count as u64,
                })
                .collect(),
            error: Default::default(),
        },
        Err(error) => bridge::ResultTimeStepListResult {
            steps: Vec::new(),
            error: error.into(),
        },
    }
}

/// 字段在全部时间步上的显示范围（task 033 全局色标）：只读各步 sidecar 头部的范围摘要，
/// 不读字段数值。摘要范围为标量值或多分量的模；取单个分量时用 [-最大模, 最大模] 作保守包络。
/// 缺少该字段的步跳过；无时间步时只看最新结果。
pub(super) fn project_service_result_field_range(
    service: &ProjectService,
    import_id: &str,
    name: &str,
    association: &str,
    component: i32,
) -> bridge::FieldRangeResult {
    let mut result = bridge::FieldRangeResult::default();
    let Some(association) = crate::display::field_association(association) else {
        result.error = panta_core::project::ProjectError::CommandInvalid(
            "field association is invalid".to_owned(),
        )
        .into();
        return result;
    };
    let sources = (|| -> Result<Vec<_>, panta_core::project::ProjectError> {
        let steps = service.service.result_time_steps(import_id)?;
        if steps.is_empty() {
            return Ok(service
                .service
                .result_field_source(import_id)?
                .into_iter()
                .collect::<Vec<_>>());
        }
        (0..steps.len())
            .map(|step| service.service.result_field_source_at(import_id, step))
            .collect()
    })();
    let mut range: Option<[f64; 2]> = None;
    let outcome = sources.and_then(|sources| {
        for source in sources {
            let reader = source.open()?;
            let Some([min, max]) = reader
                .summaries()
                .iter()
                .find(|field| field.name == name && field.association == association)
                .and_then(|field| field.range)
            else {
                continue;
            };
            let [min, max] = if component >= 0 {
                let bound = min.abs().max(max.abs());
                [-bound, bound]
            } else {
                [min, max]
            };
            range = Some(range.map_or([min, max], |[low, high]| [low.min(min), high.max(max)]));
        }
        Ok(())
    });
    match outcome {
        Ok(()) => {
            if let Some([min, max]) = range {
                result.has_range = true;
                result.min = min;
                result.max = max;
            }
        }
        Err(error) => result.error = error.into(),
    }
    result
}
