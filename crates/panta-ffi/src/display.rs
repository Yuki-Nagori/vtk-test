//! 显示缓冲适配：服务线程只捕获共享快照，大型坐标展开由宿主后台调用。
use crate::{bridge, project::ProjectService};
use std::sync::Arc;

pub struct MeshDisplayInput {
    mesh: Result<(Arc<panta_mesh::SurfaceMesh>, u64), bridge::ProjectDiagnostic>,
    /// 非 0：驻留网格是显示 LOD，值为源面数；表面诊断拒绝 LOD（task 019），导出改为流式（task 025）。
    lod_source_triangles: u64,
    /// 体网格文档的四面体快照（task 022）：诊断追加质量统计、`.vtu` 导出写四面体、切面在后台计算。
    volume: Option<Arc<panta_mesh::TetMesh>>,
    /// 需要流式访问的完整资产（task 025）：表面 LOD 的源资产，或单元未驻留的体网格资产。
    /// 与 `results` 同为 `Arc`，使输入保持小体积（计数随单元类型增长，task 031）。
    streamed: Option<Arc<panta_core::project::StreamedAsset>>,
    /// 结果 sidecar 句柄（task 029）：驻留体网格场剖面在后台读取字段（单字段缓存）。
    /// `Arc` 保持输入体积小（与其他快照字段同为共享句柄）。
    results: Option<Arc<panta_core::project::ResultFieldSource>>,
    /// 导入记录中的面邻接质量（task 032 PR 2）；`None` 表示来源没有面邻接。
    face_quality: Option<panta_core::project::FaceQualityRecord>,
}

impl MeshDisplayInput {
    fn streamed_volume(&self) -> Option<&panta_core::project::StreamedAsset> {
        self.streamed
            .as_deref()
            .filter(|asset| matches!(asset.kind, panta_core::project::StreamedKind::Volume(_)))
    }

    fn streamed_surface(&self) -> Option<&panta_core::project::StreamedAsset> {
        self.streamed.as_deref().filter(|asset| {
            matches!(
                asset.kind,
                panta_core::project::StreamedKind::Surface { .. }
            )
        })
    }

    /// 表面诊断需要完整驻留网格；LOD 输入返回明确诊断（task 025 评估后保留，见任务文档）。
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
    let streamed = service.service.streamed_asset_for_import(import_id);
    // 只解析路径与核对参数，不做 IO；无效引用在场剖面时报告。
    let results = service
        .service
        .result_field_source(import_id)
        .ok()
        .flatten()
        .map(Arc::new);
    let face_quality = service.service.imports().ok().and_then(|records| {
        records
            .into_iter()
            .find(|record| record.id == import_id)
            .and_then(|record| record.face_quality)
    });
    Box::new(MeshDisplayInput {
        mesh,
        lod_source_triangles,
        volume,
        streamed: streamed.map(Arc::new),
        results,
        face_quality,
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
    if let (true, Some(asset)) = (is_vtu, input.streamed_volume()) {
        // 单元未驻留的体网格：从资产流式写出四面体（task 025）。
        return match panta_core::project::export_streamed_volume_vtu(asset, target, &progress.0) {
            Ok(()) => bridge::ProjectDiagnostic::default(),
            Err(error) => error.into(),
        };
    }
    if let Some(asset) = input.streamed_surface() {
        // 表面显示 LOD：从源资产流式写出完整网格（task 025）。
        return match panta_core::project::export_streamed_surface(asset, target, &progress.0) {
            Ok(_) => bridge::ProjectDiagnostic::default(),
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
    let streamed_volume = input.streamed_volume().cloned();
    let face_quality = input.face_quality;
    let volume_counts = match (&volume, &streamed_volume) {
        (Some(volume), _) => Some(volume.nodes.len() as u64),
        (None, Some(asset)) => match asset.kind {
            panta_core::project::StreamedKind::Volume(counts) => Some(counts.nodes as u64),
            panta_core::project::StreamedKind::Surface { .. } => None,
        },
        (None, None) => None,
    };
    if volume_counts.is_some() {
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
        if let Some(node_count) = volume_counts {
            progress.0.set_window(0.8, 1.0);
            let quality = match (&volume, &streamed_volume) {
                (Some(volume), _) => panta_core::project::volume_quality(volume, &progress.0),
                (None, Some(asset)) => {
                    panta_core::project::streamed_volume_quality(asset, &progress.0)
                        .map_err(bridge::ProjectDiagnostic::from)?
                }
                (None, None) => None,
            }
            .ok_or_else(cancelled)?;
            value.volume_available = true;
            value.node_count = node_count;
            value.tet_count = quality.tet_count as u64;
            value.min_dihedral_degrees = quality.min_dihedral_degrees;
            value.max_aspect_ratio = quality.max_aspect_ratio;
            value.mean_aspect_ratio = quality.mean_aspect_ratio;
            value.poor_count = quality.poor_count as u64;
            value.cell_count = quality.by_type.iter().map(|kind| kind.count as u64).sum();
            value.cell_quality_summary = crate::volume::cell_quality_summary(&quality);
            if let Some(face) = &face_quality {
                value.face_quality_summary = crate::volume::face_quality_summary(face);
                value.face_quality_warning = crate::volume::face_quality_warning(face);
                value.clean = value.clean && !value.face_quality_warning;
            }
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

/// 在宿主后台线程计算体网格任意平面切面显示坐标（task 022/024）；取消时 `ready` 为假且无错误。
#[expect(clippy::boxed_local, reason = "切面输入由 CXX Box 独占移交后台线程")]
pub(super) fn section_mesh_display(
    input: Box<MeshDisplayInput>,
    normal_x: f64,
    normal_y: f64,
    normal_z: f64,
    fraction: f64,
    progress: &crate::TaskProgress,
) -> bridge::AnimationFrameResult {
    let mut result = bridge::AnimationFrameResult::default();
    let normal = [normal_x, normal_y, normal_z];
    let section = match (&input.volume, input.streamed_volume()) {
        (Some(volume), _) => {
            panta_core::project::section_plane(volume, normal, fraction, &progress.0)
        }
        // 单元未驻留：每次切面重新扫描资产单元流（task 025）。
        (None, Some(asset)) => {
            panta_core::project::streamed_section_plane(asset, normal, fraction, &progress.0)
        }
        (None, None) => {
            result.error =
                panta_core::project::ProjectError::VolumeUnavailable(String::new()).into();
            return result;
        }
    };
    match section {
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

fn field_component(component: i32) -> Option<panta_mesh::FieldComponent> {
    match component {
        -1 => Some(panta_mesh::FieldComponent::Magnitude),
        0..=2 => Some(panta_mesh::FieldComponent::Component(component as u8)),
        _ => None,
    }
}

fn field_association(association: &str) -> Option<panta_mesh::FieldAssociation> {
    match association {
        "node" => Some(panta_mesh::FieldAssociation::Node),
        "cell" => Some(panta_mesh::FieldAssociation::Cell),
        _ => None,
    }
}

/// 在宿主后台线程计算带结果场数值的切面（task 029）：驻留体网格读整字段（服务单字段缓存），
/// 单元未驻留时只走派生分块文件；逐次流式回退返回 CommandInvalid。
#[expect(clippy::boxed_local, reason = "切面输入由 CXX Box 独占移交后台线程")]
pub(super) fn section_field_mesh_display(
    input: Box<MeshDisplayInput>,
    request: &bridge::FieldSectionRequest,
    progress: &crate::TaskProgress,
) -> bridge::FieldSectionResult {
    use panta_core::project::ProjectError;
    let mut result = bridge::FieldSectionResult::default();
    let normal = [request.normal_x, request.normal_y, request.normal_z];
    let (fraction, field_name) = (request.fraction, request.field_name.as_str());
    let (Some(association), Some(component)) = (
        field_association(&request.association),
        field_component(request.component),
    ) else {
        result.error =
            ProjectError::CommandInvalid("field association or component is invalid".to_owned())
                .into();
        return result;
    };
    let section = match (&input.volume, input.streamed_volume()) {
        (Some(volume), _) => input
            .results
            .as_ref()
            .ok_or_else(|| ProjectError::CommandInvalid("import has no result fields".to_owned()))
            .and_then(|source| source.read_field(field_name, association))
            .and_then(|field| {
                panta_core::project::section_plane_field(
                    volume,
                    &field,
                    component,
                    normal,
                    fraction,
                    &progress.0,
                )
            }),
        (None, Some(asset)) => panta_core::project::streamed_section_plane_field(
            asset,
            field_name,
            association,
            component,
            normal,
            fraction,
            &progress.0,
        ),
        (None, None) => Err(ProjectError::VolumeUnavailable(String::new())),
    };
    match section {
        Ok(Some(section)) => match crate::mesh_coordinates(&section.surface) {
            Ok(coordinates) => {
                if let Some([min, max]) = section.value_range() {
                    result.has_range = true;
                    result.min = min;
                    result.max = max;
                }
                result.missing_vertices = section.missing_vertices() as u64;
                result.scalars = section
                    .values
                    .iter()
                    .flatten()
                    .map(|&value| value as f32)
                    .collect();
                result.coordinates = coordinates;
                result.ready = true;
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
                streamed: None,
                results: None,
                face_quality: None,
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
                streamed: None,
                results: None,
                face_quality: None,
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
                streamed: None,
                results: None,
                face_quality: None,
            }),
            &crate::TaskProgress::default(),
        );
        assert_eq!(missing.error.code, "project.mesh_not_resident");

        let lod = diagnose_mesh_display(
            Box::new(MeshDisplayInput {
                mesh: Ok((Arc::new(mesh), 1)),
                lod_source_triangles: 30_000_000,
                volume: None,
                streamed: None,
                results: None,
                face_quality: None,
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
            streamed: None,
            results: None,
            face_quality: None,
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
        assert!(report.value.face_quality_summary.is_empty());

        // 导入记录带面邻接质量时追加摘要；严重非正交面使结论变为有问题（task 032 PR 2）。
        let mut input = volume_input()?;
        input.face_quality = Some(panta_core::project::FaceQualityRecord {
            internal_faces: 46,
            boundary_faces: 52,
            max_non_orthogonality_degrees: 75.0,
            mean_non_orthogonality_degrees: 21.6,
            severe_non_orthogonal_faces: 1,
            max_skewness: 1.42,
            high_skew_faces: 0,
            min_orthogonal_quality: 0.3,
            poor_orthogonal_cells: 0,
        });
        let face = diagnose_mesh_display(input, &Default::default()).value;
        assert!(face.face_quality_warning && !face.clean);
        assert!(
            face.face_quality_summary
                .contains("非正交角 最大 75.00° / 平均 21.60°，超过 70° 1"),
            "{}",
            face.face_quality_summary
        );

        let section =
            section_mesh_display(volume_input()?, 1.0, 0.0, 0.0, 0.5, &Default::default());
        assert!(section.ready && !section.coordinates.is_empty());
        let invalid =
            section_mesh_display(volume_input()?, 0.0, 0.0, 0.0, 0.5, &Default::default());
        assert_eq!(invalid.error.code, "project.command_invalid");

        let cancelled = crate::TaskProgress::default();
        cancelled.0.cancel();
        let stopped = section_mesh_display(volume_input()?, 1.0, 0.0, 0.0, 0.5, &cancelled);
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

    #[test]
    fn streamed_assets_export_diagnose_and_section_without_resident_cells()
    -> Result<(), Box<dyn std::error::Error>> {
        use panta_core::project::{StreamedAsset, StreamedKind};
        let directory =
            std::env::temp_dir().join(format!("panta-ffi-streamed-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let block = panta_mesh::SyntheticTetBlock::with_cells_per_axis(3);
        let volume_path = directory.join("volume.pmesh");
        let (_, digest) = panta_mesh::write_volume_asset_streaming(
            std::fs::File::create(&volume_path)?,
            block.counts(),
            &block,
            None,
        )?
        .ok_or("cancelled")?;
        let mut chunks = panta_mesh::VolumeAssetChunks::open(&volume_path)?;
        let boundary =
            panta_mesh::boundary_surface_streaming(&mut chunks, None)?.ok_or("cancelled")?;
        let volume_input = || {
            Box::new(MeshDisplayInput {
                mesh: Ok((Arc::new(boundary.clone()), 1)),
                lod_source_triangles: 0,
                volume: None,
                streamed: Some(Arc::new(StreamedAsset {
                    path: volume_path.clone(),
                    digest: digest.clone(),
                    kind: StreamedKind::Volume(block.counts()),
                    chunks: None,
                })),
                results: None,
                face_quality: None,
            })
        };
        let report = diagnose_mesh_display(volume_input(), &Default::default());
        assert!(report.error.code.is_empty(), "{}", report.error.detail);
        assert!(report.value.volume_available);
        assert_eq!(report.value.tet_count, block.tet_count() as u64);
        assert_eq!(report.value.node_count, block.node_count() as u64);
        let section = section_mesh_display(volume_input(), 0.0, 1.0, 0.0, 0.5, &Default::default());
        assert!(section.ready && !section.coordinates.is_empty());
        let target = directory.join("volume.vtu");
        let error = export_mesh_display(
            volume_input(),
            target.to_str().ok_or("path")?,
            &Default::default(),
        );
        assert!(error.code.is_empty(), "{}", error.detail);
        assert_eq!(vtu_tet_count(&std::fs::read(&target)?)?, block.tet_count());

        let torus = panta_mesh::SyntheticTorus::new(5_000);
        let surface_path = directory.join("surface.pmesh");
        let (_, surface_digest) = panta_mesh::write_surface_asset_streaming(
            std::fs::File::create(&surface_path)?,
            5_000,
            |range, out| torus.fill(range, out),
            None,
        )?
        .ok_or("cancelled")?;
        let lod = panta_mesh::SurfaceMesh {
            triangles: vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        };
        let surface_input = |digest: &str| {
            Box::new(MeshDisplayInput {
                mesh: Ok((Arc::new(lod.clone()), 1)),
                lod_source_triangles: 5_000,
                volume: None,
                streamed: Some(Arc::new(StreamedAsset {
                    path: surface_path.clone(),
                    digest: digest.to_owned(),
                    kind: StreamedKind::Surface { triangles: 5_000 },
                    chunks: None,
                })),
                results: None,
                face_quality: None,
            })
        };
        let stl = directory.join("full.stl");
        let error = export_mesh_display(
            surface_input(&surface_digest),
            stl.to_str().ok_or("path")?,
            &Default::default(),
        );
        assert!(error.code.is_empty(), "{}", error.detail);
        // 完整源资产而非 LOD：84 字节头 + 每面 50 字节。
        assert_eq!(std::fs::metadata(&stl)?.len(), 84 + 50 * 5_000);
        let stale = export_mesh_display(
            surface_input("sha256:stale"),
            directory.join("stale.stl").to_str().ok_or("path")?,
            &Default::default(),
        );
        assert_eq!(stale.code, "project.export_source_unavailable");
        let diagnose = diagnose_mesh_display(surface_input(&surface_digest), &Default::default());
        assert_eq!(diagnose.error.code, "project.mesh_lod_only");
        std::fs::remove_dir_all(&directory)?;
        Ok(())
    }

    #[test]
    fn field_sections_color_resident_volumes_and_report_invalid_requests()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = crate::temp_directory::Fixture::new()?;
        let source = fixture.root.join("fields.vtk");
        std::fs::write(
            &source,
            "# vtk DataFile Version 4.2\nresults\nASCII\nDATASET UNSTRUCTURED_GRID\n\
             POINTS 5 double\n0 0 0 1 0 0 0 1 0 0 0 1 1 1 1\n\
             CELLS 2 10\n4 0 1 2 3\n4 1 2 3 4\nCELL_TYPES 2\n10 10\n\
             POINT_DATA 5\nVECTORS velocity double\n1 0 0 0 2 0 0 0 3 0 0 0 1 1 1\n\
             CELL_DATA 2\nSCALARS material int\nLOOKUP_TABLE default\n1 2\n",
        )?;
        let mut service = crate::project::project_service_new();
        service.service.create(&fixture.root, "Fields")?;
        let record = service
            .service
            .import_stl(&source, "solid-3d", "millimeters", false)?;
        let fields = crate::volume::project_service_result_fields(&service, &record.id);
        assert!(fields.error.code.is_empty(), "{}", fields.error.detail);
        let names: Vec<_> = fields
            .fields
            .iter()
            .map(|field| {
                (
                    field.name.as_str(),
                    field.association.as_str(),
                    field.sectionable,
                )
            })
            .collect();
        assert_eq!(
            names,
            [("velocity", "node", true), ("material", "cell", true)]
        );
        assert!(fields.fields[0].has_range && fields.fields[0].max == 3.0);

        let request = |name: &str, association: &str, component: i32| bridge::FieldSectionRequest {
            field_name: name.to_owned(),
            association: association.to_owned(),
            component,
            normal_x: 1.0,
            normal_y: 1.0,
            normal_z: 1.0,
            fraction: 0.5,
        };
        let input = || project_service_mesh_display_input(&service, &record.id);
        let progress = crate::TaskProgress::default();
        let material =
            section_field_mesh_display(input(), &request("material", "cell", -1), &progress);
        assert!(material.error.code.is_empty(), "{}", material.error.detail);
        assert!(material.ready);
        assert_eq!(material.scalars.len() * 3, material.coordinates.len());
        assert!(
            material
                .scalars
                .iter()
                .all(|value| *value == 1.0 || *value == 2.0)
        );
        assert!(material.has_range && material.min >= 1.0 && material.max <= 2.0);
        assert_eq!(material.missing_vertices, 0);
        let plain = section_mesh_display(input(), 1.0, 1.0, 1.0, 0.5, &progress);
        assert_eq!(plain.coordinates, material.coordinates);
        let velocity_x =
            section_field_mesh_display(input(), &request("velocity", "node", 0), &progress);
        assert!(velocity_x.ready && velocity_x.max <= 1.0 + 1e-9);

        for invalid in [
            request("velocity", "edge", -1),
            request("velocity", "node", 3),
            request("missing", "node", -1),
            bridge::FieldSectionRequest {
                normal_x: 0.0,
                normal_y: 0.0,
                normal_z: 0.0,
                ..request("velocity", "node", -1)
            },
        ] {
            let result = section_field_mesh_display(input(), &invalid, &progress);
            assert_eq!(result.error.code, "project.command_invalid", "{invalid:?}");
            assert!(!result.ready);
        }
        let cancelled = crate::TaskProgress::default();
        cancelled.0.cancel();
        let stopped =
            section_field_mesh_display(input(), &request("velocity", "node", -1), &cancelled);
        assert!(!stopped.ready && stopped.error.code.is_empty());
        // 表面网格输入：无体网格。
        let surface = section_field_mesh_display(
            Box::new(MeshDisplayInput {
                mesh: Err(Default::default()),
                lod_source_triangles: 0,
                volume: None,
                streamed: None,
                results: None,
                face_quality: None,
            }),
            &request("velocity", "node", -1),
            &progress,
        );
        assert_eq!(surface.error.code, "project.volume_unavailable");
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
