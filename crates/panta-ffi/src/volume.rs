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
        }
        // 单元未驻留的体网格（task 025）：计数来自资产，命令按需流式读取。
        (None, Some(counts)) => {
            result.available = true;
            result.streamed = true;
            result.node_count = counts.nodes as u64;
            result.tet_count = counts.tets as u64;
        }
        (None, None) => {
            result.error =
                panta_core::project::ProjectError::VolumeUnavailable(import_id.to_owned()).into();
        }
    }
    result
}
