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

/// 在宿主后台线程把共享网格快照写成导出文件；成功返回空诊断。
#[expect(clippy::boxed_local, reason = "导出输入由 CXX Box 独占移交后台线程")]
pub(super) fn export_mesh_display(
    input: Box<MeshDisplayInput>,
    path: &str,
    progress: &crate::TaskProgress,
) -> bridge::ProjectDiagnostic {
    match input.mesh {
        Ok((mesh, _)) => {
            match panta_core::project::export_mesh_file_with_progress(
                &mesh,
                std::path::Path::new(path),
                &progress.0,
            ) {
                Ok(_) => bridge::ProjectDiagnostic::default(),
                Err(error) => error.into(),
            }
        }
        Err(diagnostic) => diagnostic,
    }
}

/// 在宿主后台线程诊断共享网格快照；不访问活跃工程服务。
#[expect(clippy::boxed_local, reason = "诊断输入由 CXX Box 独占移交后台线程")]
pub(super) fn diagnose_mesh_display(
    input: Box<MeshDisplayInput>,
    progress: &crate::TaskProgress,
) -> bridge::MeshQualityReportResult {
    let result = input.mesh.and_then(|(mesh, _)| {
        panta_mesh::diagnose_surface_with_progress(&mesh, Default::default(), &progress.0)
            .map(|report| bridge::MeshQualityReport {
                clean: report.is_clean(),
                weld_tolerance: report.weld_tolerance,
                triangle_count: report.triangle_count,
                raw_vertex_count: report.raw_vertex_count,
                welded_vertex_count: report.welded_vertex_count,
                near_duplicate_vertex_count: report.near_duplicate_vertex_count,
                degenerate_triangle_count: report.degenerate_triangle_count,
                boundary_edge_count: report.boundary_edge_count,
                hole_count: report.hole_count,
                non_manifold_edge_count: report.non_manifold_edge_count,
                inconsistent_orientation_edge_count: report.inconsistent_orientation_edge_count,
            })
            .map_err(|error| {
                let code = if error == panta_mesh::DiagnosticError::Cancelled {
                    "project.diagnostics_cancelled"
                } else {
                    "project.diagnostics_failed"
                };
                crate::project_response::diagnostic(
                    code,
                    "validation",
                    error.to_string(),
                )
            })
    });
    match result {
        Ok(value) => bridge::MeshQualityReportResult {
            value,
            error: Default::default(),
        },
        Err(error) => bridge::MeshQualityReportResult {
            value: Default::default(),
            error,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnoses_an_open_triangle_and_reports_missing_mesh() {
        let mesh = panta_mesh::SurfaceMesh {
            triangles: vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        };
        let report = diagnose_mesh_display(Box::new(MeshDisplayInput {
            mesh: Ok((Arc::new(mesh.clone()), 1)),
        }), &crate::TaskProgress::default());
        let cancelled = crate::TaskProgress::default();
        cancelled.0.cancel();
        let stopped = diagnose_mesh_display(
            Box::new(MeshDisplayInput {
                mesh: Ok((Arc::new(mesh), 1)),
            }),
            &cancelled,
        );
        assert_eq!(stopped.error.code, "project.diagnostics_cancelled");
        assert!(report.error.code.is_empty());
        assert_eq!(report.value.triangle_count, 1);
        assert_eq!(report.value.boundary_edge_count, 3);
        assert_eq!(report.value.hole_count, 1);
        assert!(!report.value.clean);

        let missing = diagnose_mesh_display(Box::new(MeshDisplayInput {
            mesh: Err(crate::project_response::diagnostic(
                "project.mesh_not_resident",
                "state",
                "x".into(),
            )),
        }), &crate::TaskProgress::default());
        assert_eq!(missing.error.code, "project.mesh_not_resident");
    }
}
