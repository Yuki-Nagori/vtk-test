//! 体网格摘要的 FFI 适配：GUI 线程同步调用，只读节点/单元计数（task 021/022）。
//! 质量统计、切面与 VTU 导出走 `display` 的后台快照入口（task 022）。
use crate::bridge;
use crate::project::ProjectService;

pub(super) fn project_service_volume_summary(
    service: &ProjectService,
    import_id: &str,
) -> bridge::VolumeInfoResult {
    let mut result = bridge::VolumeInfoResult::default();
    match service.service.volume_for_import(import_id) {
        Some(volume) => {
            result.available = true;
            result.node_count = volume.nodes.len() as u64;
            result.tet_count = volume.tets.len() as u64;
        }
        None => {
            result.error =
                panta_core::project::ProjectError::VolumeUnavailable(import_id.to_owned()).into();
        }
    }
    result
}
