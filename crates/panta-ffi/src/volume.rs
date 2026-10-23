//! 体网格只读命令的 FFI 适配（task 021）：GUI 线程同步调用，返回拥有式结果。
use crate::bridge;
use crate::project::ProjectService;

pub(super) fn project_service_volume_info(
    service: &ProjectService,
    import_id: &str,
) -> bridge::VolumeInfoResult {
    let mut result = bridge::VolumeInfoResult::default();
    match service.service.volume_quality(import_id) {
        Ok(quality) => {
            result.available = true;
            result.node_count = service
                .service
                .volume_for_import(import_id)
                .map_or(0, |volume| volume.nodes.len() as u64);
            result.tet_count = quality.tet_count as u64;
            result.min_dihedral_degrees = quality.min_dihedral_degrees;
            result.max_aspect_ratio = quality.max_aspect_ratio;
            result.mean_aspect_ratio = quality.mean_aspect_ratio;
            result.poor_count = quality.poor_count as u64;
        }
        Err(error) => result.error = error.into(),
    }
    result
}

pub(super) fn project_service_volume_section(
    service: &ProjectService,
    import_id: &str,
    axis: u32,
    fraction: f64,
) -> bridge::AnimationFrameResult {
    let mut result = bridge::AnimationFrameResult::default();
    match service
        .service
        .volume_section(import_id, axis as usize, fraction)
    {
        Ok(mesh) => match crate::mesh_coordinates(&mesh) {
            Ok(coordinates) => {
                result.ready = true;
                result.coordinates = coordinates;
            }
            Err(detail) => {
                result.error = crate::project_response::diagnostic(
                    "project.volume_unavailable",
                    "resource",
                    detail,
                );
            }
        },
        Err(error) => result.error = error.into(),
    }
    result
}

pub(super) fn project_service_export_volume_vtu(
    service: &ProjectService,
    import_id: &str,
    path: &str,
) -> bridge::ProjectDiagnostic {
    match service
        .service
        .export_volume_vtu(import_id, std::path::Path::new(path))
    {
        Ok(()) => bridge::ProjectDiagnostic::default(),
        Err(error) => error.into(),
    }
}
