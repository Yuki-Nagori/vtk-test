//! 显示缓冲适配：服务线程只捕获共享快照，大型坐标展开由宿主后台调用。
use crate::{bridge, project::ProjectService};
use std::sync::Arc;

pub struct MeshDisplayInput {
    mesh: Result<(Arc<panta_mesh::SurfaceMesh>, u64), bridge::ProjectDiagnostic>,
    /// 非 0：驻留网格是显示 LOD，值为源面数；诊断与导出拒绝 LOD（task 019）。
    lod_source_triangles: u64,
    /// 体网格文档的四面体快照（task 022）：诊断追加质量统计、`.vtu` 导出写四面体、切面在后台计算。
    volume: Option<Arc<panta_mesh::TetMesh>>,
}

impl MeshDisplayInput {
    /// 诊断 / 导出需要完整网格；LOD 输入返回明确诊断。
    fn full_mesh(self) -> Result<Arc<panta_mesh::SurfaceMesh>, bridge::ProjectDiagnostic> {
        let (mesh, _) = self.mesh?;
        if self.lod_source_triangles > 0 {
            return Err(crate::project_response::diagnostic(
                "project.mesh_lod_only",
                "resource",
                format!("{} source triangles", self.lod_source_triangles),
            ));
        }
        Ok(mesh)
    }
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
    let lod_source_triangles = service.service.mesh_lod_source_for_import(import_id);
    let volume = service.service.volume_for_import(import_id);
    Box::new(MeshDisplayInput {
        mesh,
        lod_source_triangles,
        volume,
    })
}

/// 只访问拥有式不可变输入；不访问活跃工程服务，可以移交宿主工作线程。
#[expect(
    clippy::boxed_local,
    reason = "显示输入由 CXX Box 独占移交后台适配线程"
)]
pub(super) fn prepare_mesh_display(
    input: Box<MeshDisplayInput>,
) -> bridge::SurfaceMeshSnapshotResult {
    let lod_source_triangles = input.lod_source_triangles;
    let result = input.mesh.and_then(|(mesh, revision)| {
        crate::mesh_coordinates(&mesh)
            .map(|coordinates| bridge::SurfaceMeshSnapshot {
                coordinates,
                revision,
                lod_source_triangles,
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
    let target = std::path::Path::new(path);
    let is_vtu = target
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("vtu"));
    if let (true, Some(volume)) = (is_vtu, input.volume.as_ref()) {
        // 体网格导出 .vtu 写四面体单元；其他格式仍导出外表面。
        return match panta_core::project::export_tet_mesh_vtu(volume, target, &progress.0) {
            Ok(()) => bridge::ProjectDiagnostic::default(),
            Err(error) => error.into(),
        };
    }
    match input.full_mesh() {
        Ok(mesh) => {
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
    let cancelled = || {
        crate::project_response::diagnostic(
            "project.diagnostics_cancelled",
            "validation",
            "Cancelled".into(),
        )
    };
    let volume = input.volume.clone();
    if volume.is_some() {
        // 外表面诊断 0–80%，体网格质量统计 80–100%。
        progress.0.set_window(0.0, 0.8);
    }
    let result = input.full_mesh().and_then(|mesh| {
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
                ..Default::default()
            })
            .map_err(|error| {
                let code = if error == panta_mesh::DiagnosticError::Cancelled {
                    "project.diagnostics_cancelled"
                } else {
                    "project.diagnostics_failed"
                };
                crate::project_response::diagnostic(code, "validation", error.to_string())
            })
    });
    let result = result.and_then(|mut value| {
        if let Some(volume) = volume {
            progress.0.set_window(0.8, 1.0);
            let quality =
                panta_core::project::volume_quality(&volume, &progress.0).ok_or_else(cancelled)?;
            value.volume_available = true;
            value.node_count = volume.nodes.len() as u64;
            value.tet_count = quality.tet_count as u64;
            value.min_dihedral_degrees = quality.min_dihedral_degrees;
            value.max_aspect_ratio = quality.max_aspect_ratio;
            value.mean_aspect_ratio = quality.mean_aspect_ratio;
            value.poor_count = quality.poor_count as u64;
            value.clean = value.clean && quality.poor_count == 0;
        }
        Ok(value)
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

/// 在宿主后台线程计算体网格轴向切面显示坐标（task 022）；取消时 `ready` 为假且无错误。
#[expect(clippy::boxed_local, reason = "切面输入由 CXX Box 独占移交后台线程")]
pub(super) fn section_mesh_display(
    input: Box<MeshDisplayInput>,
    axis: u32,
    fraction: f64,
    progress: &crate::TaskProgress,
) -> bridge::AnimationFrameResult {
    let mut result = bridge::AnimationFrameResult::default();
    let Some(volume) = input.volume else {
        result.error = panta_core::project::ProjectError::VolumeUnavailable(String::new()).into();
        return result;
    };
    match panta_core::project::section_along_axis(&volume, axis as usize, fraction, &progress.0) {
        Ok(Some(mesh)) => match crate::mesh_coordinates(&mesh) {
            Ok(coordinates) => {
                result.ready = true;
                result.coordinates = coordinates;
            }
            Err(detail) => {
                result.error = crate::project_response::diagnostic(
                    "project.mesh_allocation_failed",
                    "resource",
                    detail,
                );
            }
        },
        Ok(None) => {}
        Err(error) => result.error = error.into(),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnoses_an_open_triangle_and_reports_missing_mesh() {
        let mesh = panta_mesh::SurfaceMesh {
            triangles: vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        };
        let report = diagnose_mesh_display(
            Box::new(MeshDisplayInput {
                mesh: Ok((Arc::new(mesh.clone()), 1)),
                lod_source_triangles: 0,
                volume: None,
            }),
            &crate::TaskProgress::default(),
        );
        let cancelled = crate::TaskProgress::default();
        cancelled.0.cancel();
        let stopped = diagnose_mesh_display(
            Box::new(MeshDisplayInput {
                mesh: Ok((Arc::new(mesh.clone()), 1)),
                lod_source_triangles: 0,
                volume: None,
            }),
            &cancelled,
        );
        assert_eq!(stopped.error.code, "project.diagnostics_cancelled");
        assert!(report.error.code.is_empty());
        assert_eq!(report.value.triangle_count, 1);
        assert_eq!(report.value.boundary_edge_count, 3);
        assert_eq!(report.value.hole_count, 1);
        assert!(!report.value.clean);

        let missing = diagnose_mesh_display(
            Box::new(MeshDisplayInput {
                mesh: Err(crate::project_response::diagnostic(
                    "project.mesh_not_resident",
                    "state",
                    "x".into(),
                )),
                lod_source_triangles: 0,
                volume: None,
            }),
            &crate::TaskProgress::default(),
        );
        assert_eq!(missing.error.code, "project.mesh_not_resident");

        let lod = diagnose_mesh_display(
            Box::new(MeshDisplayInput {
                mesh: Ok((Arc::new(mesh), 1)),
                lod_source_triangles: 30_000_000,
                volume: None,
            }),
            &crate::TaskProgress::default(),
        );
        assert_eq!(lod.error.code, "project.mesh_lod_only");
    }

    fn volume_input() -> Result<Box<MeshDisplayInput>, Box<dyn std::error::Error>> {
        let nodes = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
        ];
        let volume = panta_mesh::tet_mesh_from_cells(nodes, vec![[0, 1, 2, 3], [1, 2, 3, 4]])?;
        let surface = panta_mesh::boundary_surface(&volume);
        Ok(Box::new(MeshDisplayInput {
            mesh: Ok((Arc::new(surface), 1)),
            lod_source_triangles: 0,
            volume: Some(Arc::new(volume)),
        }))
    }

    #[test]
    fn volume_diagnostics_section_and_vtu_export_run_on_snapshots_and_cancel()
    -> Result<(), Box<dyn std::error::Error>> {
        let progress = crate::TaskProgress::default();
        let report = diagnose_mesh_display(volume_input()?, &progress);
        assert!(report.error.code.is_empty());
        assert!(report.value.volume_available);
        assert_eq!(report.value.tet_count, 2);
        assert_eq!(report.value.node_count, 5);
        assert_eq!(report.value.triangle_count, 6);
        assert_eq!(progress.0.fraction(), 1.0);

        let section = section_mesh_display(volume_input()?, 0, 0.5, &Default::default());
        assert!(section.ready && !section.coordinates.is_empty());
        let invalid = section_mesh_display(volume_input()?, 5, 0.5, &Default::default());
        assert_eq!(invalid.error.code, "project.command_invalid");

        let cancelled = crate::TaskProgress::default();
        cancelled.0.cancel();
        let stopped = section_mesh_display(volume_input()?, 0, 0.5, &cancelled);
        assert!(!stopped.ready && stopped.error.code.is_empty());
        let stopped = diagnose_mesh_display(volume_input()?, &cancelled);
        assert_eq!(stopped.error.code, "project.diagnostics_cancelled");

        let directory =
            std::env::temp_dir().join(format!("panta-ffi-volume-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let target = directory.join("volume.VTU");
        let target_text = target.to_str().ok_or("path")?;
        let error = export_mesh_display(volume_input()?, target_text, &cancelled);
        assert_eq!(error.code, "project.export_cancelled");
        assert!(!target.exists());
        let error = export_mesh_display(volume_input()?, target_text, &Default::default());
        assert!(error.code.is_empty(), "{}", error.detail);
        let grid = vtu_tet_count(&std::fs::read(&target)?)?;
        assert_eq!(grid, 2);
        std::fs::remove_dir_all(&directory)?;
        Ok(())
    }

    /// 只数 VTU 中的四面体类型标记（10），避免 FFI crate 依赖导入解析器。
    fn vtu_tet_count(bytes: &[u8]) -> Result<usize, Box<dyn std::error::Error>> {
        let text = std::str::from_utf8(bytes)?;
        let types = text.split("Name=\"types\"").nth(1).ok_or("types")?;
        let body = types.split('>').nth(1).ok_or("body")?;
        let body = body.split('<').next().ok_or("body")?;
        Ok(body
            .split_whitespace()
            .filter(|value| *value == "10")
            .count())
    }
}
