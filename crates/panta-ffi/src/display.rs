//! 显示缓冲适配：服务线程只捕获共享快照，大型坐标展开由宿主后台调用。
use crate::{bridge, project::ProjectService};
use std::sync::Arc;

pub struct MeshDisplayInput {
    mesh: Result<(Arc<panta_mesh::SurfaceMesh>, u64), bridge::ProjectDiagnostic>,
}

pub(super) fn project_service_mesh_display_input(
    service: &ProjectService,
    import_id: &str,
) -> Box<MeshDisplayInput> {
    let mesh = (|| {
        let snapshot = service
            .service
            .current()
            .map_err(bridge::ProjectDiagnostic::from)?;
        let mesh = service
            .service
            .mesh_snapshot_for_import(import_id)
            .ok_or_else(|| {
                crate::project_response::diagnostic(
                    "project.mesh_not_resident",
                    "state",
                    import_id.into(),
                )
            })?;
        Ok((mesh, snapshot.revision))
    })();
    Box::new(MeshDisplayInput { mesh })
}

/// 只访问拥有式不可变输入；不访问活跃工程服务，可以移交宿主工作线程。
#[expect(
    clippy::boxed_local,
    reason = "显示输入由 CXX Box 独占移交后台适配线程"
)]
pub(super) fn prepare_mesh_display(
    input: Box<MeshDisplayInput>,
) -> bridge::SurfaceMeshSnapshotResult {
    let result = input.mesh.and_then(|(mesh, revision)| {
        crate::mesh_coordinates(&mesh)
            .map(|coordinates| bridge::SurfaceMeshSnapshot {
                coordinates,
                revision,
            })
            .map_err(|detail| {
                crate::project_response::diagnostic(
                    "project.mesh_allocation_failed",
                    "resource",
                    detail,
                )
            })
    });
    result.into()
}
