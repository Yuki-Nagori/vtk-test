//! 工程模块门面：领域数据、诊断与应用服务（057、063、107）。
//!
//! 工程目录由宿主选择，工程模型和清单事务由 Rust 持有。Qt/QML 只通过
//! CXX adapter 传递 UTF-8 路径、名称和命令，不直接读写清单文件。

use panta_io::import::StlImportSession;
mod activation_result;
mod error;
pub use activation_result::CompletedActivation;
mod mesh_cache;
mod model;
mod repository;
mod service;
mod workspace;
pub use crate::activation::{ActivationLimits, VolumeMode};
pub use error::ProjectError;
use mesh_cache::SurfaceMeshCache;
pub use model::{ProjectCommand, ProjectSnapshot};
use model::{ProjectManifest, ProjectState};
pub use panta_io::import::{
    DEFAULT_MESH_TYPE, FaceQualityRecord, IMPORT_RECORD_VERSION, ImportRecord, MESH_TYPES,
    STL_IMPORT_PARSER_VERSION, StlImportPreview, VolumeGeneration, default_display_name,
};
pub use panta_mesh::{DEFAULT_RELATIVE_WELD_TOLERANCE, DiagnosticOptions, SurfaceDiagnostics};
pub(crate) use service::resolve_asset_path;
pub use service::{ProjectService, export_mesh_file, export_mesh_file_with_progress};
pub use workspace::{
    ActivationRequest as WorkspaceActivationRequest, ActivationResultKind, CloseDocumentResult,
    DocumentDiagnostic, DocumentKind, DocumentState, DocumentWorkspace, WorkspaceDocument,
    WorkspaceSnapshot, activation_error_message, format_dimensions as format_project_dimensions,
    project_error_message,
};

mod command_job;
pub use command_job::{ProjectOperation, ProjectOperationCompletion, ProjectOperationRequest};

mod analysis_sequence;
pub use analysis_sequence::{ANALYSIS_SEQUENCES, AnalysisSequenceDefinition};
mod process_settings;
pub use process_settings::{
    FLOW_RATE_CONSTRAINT, FillSettings, HOLDING_DURATION_CONSTRAINT, HOLDING_PRESSURE_CONSTRAINT,
    HOLDING_PROFILE_MAX_POINTS, HoldingProfilePoint, NumericConstraint,
    SWITCH_OVER_VOLUME_CONSTRAINT, TEMPERATURE_CONSTRAINT,
};
mod gate_location;
pub use gate_location::{GateLocationSettings, GateLocatorAlgorithm, NUMBER_OF_GATES_CONSTRAINT};
mod material;
pub use material::default_material_id;
mod generated;
mod import;
mod mesh_assets;
mod preview;
mod results;
pub use results::ResultFieldSource;
mod volume;
pub use volume::{
    SharedChunks, StreamedAsset, StreamedKind, export_streamed_surface, export_streamed_volume_vtu,
    export_tet_mesh_vtu, section_plane, section_plane_field, streamed_section_plane,
    streamed_section_plane_field, streamed_volume_quality, volume_quality,
};

pub use crate::activation::{ActivationAttempt, Outcome, OutcomeKind};

/// 当前范例工程清单的 schema 版本。
pub const PROJECT_SCHEMA_VERSION: u32 = 4;
const PROJECT_FILE_EXTENSION: &str = "panta";

#[cfg(test)]
#[path = "../../../tests/support/rust/temp_directory.rs"]
pub(crate) mod temp_directory;

#[cfg(test)]
mod tests {
    use super::model::validate_name;
    use super::{
        DocumentState, ProjectCommand, ProjectError, ProjectService, SurfaceMeshCache,
        temp_directory,
    };
    use crate::activation::{ActivationCoordinator, ActivationRequest, OUTCOME_CAPACITY};
    use panta_io::import::ImportRecord;
    use panta_mesh::SurfaceMesh;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn mesh(triangle_count: usize) -> SurfaceMesh {
        SurfaceMesh {
            triangles: vec![[[0.0; 3]; 3]; triangle_count],
        }
    }

    #[test]
    fn mesh_cache_evicts_the_least_recently_used_inactive_asset() {
        let triangle_bytes = std::mem::size_of::<[[f64; 3]; 3]>();
        let mut service = ProjectService::with_mesh_cache_budget(triangle_bytes * 2);
        service.mesh_cache.insert_active("a".to_owned(), mesh(1));
        service.mesh_cache.insert_active("b".to_owned(), mesh(1));
        assert!(service.mesh_cache.activate("a"));
        service.mesh_cache.insert_active("c".to_owned(), mesh(1));

        let mut residents = service.mesh_cache.resident_ids();
        residents.sort();
        assert_eq!(residents, ["a", "c"]);
        assert_eq!(service.mesh_cache.retained_bytes(), triangle_bytes * 2);
    }

    #[test]
    fn mesh_cache_keeps_one_active_mesh_even_when_it_exceeds_budget() {
        let mut cache = SurfaceMeshCache::with_budget(0);
        cache.insert_active("large".to_owned(), mesh(2));

        assert_eq!(cache.resident_ids(), ["large"]);
        assert_eq!(
            cache.retained_bytes(),
            std::mem::size_of::<[[f64; 3]; 3]>() * 2
        );
        cache.deactivate();
        assert!(cache.resident_ids().is_empty());
    }

    #[test]
    fn open_command_uses_the_project_service_open_flow() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut creator = ProjectService::new();
        let created = creator.create(&fixture.root, "Opened by command")?;
        let mut service = ProjectService::new();

        let opened = service.execute(ProjectCommand::Open {
            path: created.path.clone(),
        })?;

        assert_eq!(opened, created);
        assert_eq!(service.current()?, opened);
        Ok(())
    }

    #[test]
    fn reopening_a_loading_document_replaces_cancelled_attempt_and_ignores_its_result()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let source = fixture.root.join("part.stl");
        std::fs::write(&source, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture.root, "Reopen loading")?;
        let record = service.import_stl(&source, "solid-3d", "millimeters", false)?;
        service.save()?;
        service.open(&project.path)?;

        // 单 worker 被真实队列占住，保证关闭和重开都发生在资产读取前。
        let executor = crate::execution::Executor::new("activation-reopen", 1, 3);
        let (release, blocked) = std::sync::mpsc::channel();
        executor.submit(move || {
            let _ = blocked.recv();
        })?;
        service.execution.reads = Arc::clone(&executor);
        service.open_import_document(&record.id, "part");
        let first = service.begin_asset_activation(&record.id)?;
        service.close_workspace_document(&record.id);
        service.open_import_document(&record.id, "part");
        let reopened = service.begin_asset_activation(&record.id)?;
        // 即使断言失败也先排空执行器，避免测试留下被阻塞的 worker。
        release.send(())?;
        executor.close_and_wait();
        assert_ne!(first.attempt, reopened.attempt);
        assert_eq!(first.generation, reopened.generation);

        let outcomes = service.drain_asset_activations();
        assert_eq!(outcomes.len(), 2);
        let stale = outcomes
            .iter()
            .find(|outcome| outcome.attempt == first.attempt)
            .ok_or("cancelled attempt outcome missing")?;
        assert_eq!(stale.kind, super::OutcomeKind::Expired);
        assert!(stale.mesh.is_none());
        let current = outcomes
            .iter()
            .find(|outcome| outcome.attempt == reopened.attempt)
            .ok_or("reopened attempt outcome missing")?;
        assert_eq!(current.kind, super::OutcomeKind::Succeeded);
        assert!(current.mesh.is_some());
        let workspace = service.document_workspace();
        assert_eq!(workspace.documents.len(), 1);
        assert_eq!(workspace.documents[0].state, DocumentState::Ready);
        assert!(service.mesh_for_import(&record.id).is_some());
        Ok(())
    }

    #[test]
    fn service_drop_invalidates_queued_activation_and_releases_worker_ownership()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let source = fixture.root.join("part.stl");
        std::fs::write(&source, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture.root, "Lifetime")?;
        let record = service.import_stl(&source, "solid-3d", "millimeters", false)?;
        service.save()?;
        service.open(&project.path)?;

        let executor = crate::execution::Executor::new("activation-lifetime", 1, 2);
        let (release, blocked) = std::sync::mpsc::channel();
        executor.submit(move || {
            let _ = blocked.recv();
        })?;
        service.execution.reads = Arc::clone(&executor);
        let coordinator = Arc::downgrade(&service.activation);
        let attempt = service.begin_asset_activation(&record.id)?;

        drop(service);
        let retained = coordinator.upgrade().ok_or("worker lost its coordinator")?;
        assert!(!retained.cancel(attempt.attempt));
        assert!(retained.drain().is_empty());
        release.send(())?;
        executor.close_and_wait();
        assert!(retained.drain().is_empty());
        drop(retained);
        assert!(coordinator.upgrade().is_none());
        Ok(())
    }

    #[test]
    fn activating_resident_mesh_updates_the_service_current_mesh_view() {
        let mut service = ProjectService::new();
        service.cache_activated_mesh("a".to_owned(), mesh(1));
        service.cache_activated_mesh("b".to_owned(), mesh(2));

        assert!(service.activate_mesh_document("a"));
        assert_eq!(
            service.current_mesh().map(|mesh| mesh.triangles.len()),
            Some(1)
        );
    }

    #[test]
    fn activating_missing_mesh_keeps_the_current_mesh_unchanged() {
        let mut service = ProjectService::new();
        service.cache_activated_mesh("resident".to_owned(), mesh(1));

        assert!(!service.activate_mesh_document("missing"));
        assert_eq!(
            service.current_mesh().map(|mesh| mesh.triangles.len()),
            Some(1)
        );
        assert_eq!(service.resident_mesh_ids(), ["resident"]);
    }

    #[test]
    fn closing_mesh_document_releases_runtime_data_without_touching_imports() {
        let mut service = ProjectService::new();
        service
            .mesh_cache
            .insert_active("import-1".to_owned(), mesh(1));
        service.latest_mesh_id = Some("import-1".to_owned());
        service.activation_attempts.insert("import-1".to_owned(), 7);

        service.release_mesh_document("import-1");

        assert!(service.mesh_for_import("import-1").is_none());
        assert!(service.current_mesh().is_none());
        assert!(service.resident_mesh_ids().is_empty());
        assert!(!service.is_current_activation_attempt("import-1", 7));
    }

    #[test]
    fn superseded_activation_cannot_replace_or_finish_the_current_mesh() {
        let mut service = ProjectService::new();
        service.activation_attempts.insert("import-1".to_owned(), 2);

        assert!(!service.cache_activation_result("import-1", 1, mesh(1)));
        service.finish_asset_activation("import-1", 1);
        assert!(service.mesh_for_import("import-1").is_none());
        assert!(service.is_current_activation_attempt("import-1", 2));

        assert!(service.cache_activation_result("import-1", 2, mesh(2)));
        assert_eq!(
            service
                .mesh_for_import("import-1")
                .map(|mesh| mesh.triangles.len()),
            Some(2)
        );
        assert!(!service.is_current_activation_attempt("import-1", 2));
    }

    #[test]
    fn activation_admission_failure_is_mapped_without_recording_an_attempt()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        service.create(&fixture.root, "Activation backlog")?;
        let state = service
            .current
            .as_mut()
            .ok_or("created project state missing")?;
        state.imports.push(ImportRecord {
            record_version: super::IMPORT_RECORD_VERSION,
            parser_version: 1,
            id: "import-overflow".to_owned(),
            display_name: "part".to_owned(),
            source_name: "part.stl".to_owned(),
            source_path: String::new(),
            source_digest: None,
            source_size_bytes: 0,
            source_modified_unix_ms: None,
            mesh_asset_id: "mesh-overflow".to_owned(),
            format: "stl".to_owned(),
            mesh_type: "midplane".to_owned(),
            units: "millimeters".to_owned(),
            show_import_log: false,
            triangle_count: 1,
            dimensions: [1.0, 1.0, 0.0],
            generation: None,
            skipped_result_fields: Vec::new(),
            region_names: Vec::new(),
            boundary_group_names: Vec::new(),
            face_quality: None,
        });
        state.mesh_assets.insert(
            "mesh-overflow".to_owned(),
            super::model::MeshAssetRef {
                id: "mesh-overflow".to_owned(),
                revision: 1,
                format_version: panta_mesh::MESH_ASSET_FORMAT_VERSION,
                kind: "surface".to_owned(),
                relative_path: "assets/meshes/overflow.pmesh".to_owned(),
                digest: "sha256:test".to_owned(),
                units: "millimeters".to_owned(),
                source_revision: None,
                results: None,
            },
        );

        let coordinator = Arc::new(ActivationCoordinator::default());
        for index in 0..OUTCOME_CAPACITY {
            let import_id = format!("queued-{index}");
            assert!(
                coordinator
                    .begin_with(
                        &import_id,
                        ActivationRequest {
                            project_root: PathBuf::from("unused-project"),
                            relative_path: "unused.pmesh".to_owned(),
                            expected_digest: "sha256:test".to_owned(),
                            results: None,
                        },
                        |_, _, _, _| Ok(()),
                    )
                    .is_ok(),
                "coordinator rejected before reaching its configured capacity"
            );
        }
        service.activation = coordinator;

        // 元数据投影和准入不探测文件系统；路径校验只由已准入 worker 执行。
        std::fs::remove_dir_all(fixture.root.join("Activation backlog"))?;
        assert_eq!(
            service.mesh_asset_path_for_import("import-overflow")?,
            "assets/meshes/overflow.pmesh"
        );

        let error = service
            .begin_asset_activation("import-overflow")
            .err()
            .ok_or("full activation coordinator unexpectedly admitted a request")?;

        assert!(matches!(
            error,
            ProjectError::CommandInvalid(detail) if detail == "asset activation backlog full"
        ));
        assert!(service.activation_attempts.is_empty());
        Ok(())
    }

    #[test]
    fn workspace_display_ordinals_include_closed_import_records()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        service.create(&fixture.root, "Duplicate names")?;
        let state = service
            .current
            .as_mut()
            .ok_or("created project state missing")?;
        for id in ["import-1", "import-2"] {
            state.imports.push(ImportRecord {
                record_version: super::IMPORT_RECORD_VERSION,
                parser_version: super::STL_IMPORT_PARSER_VERSION,
                id: id.to_owned(),
                display_name: "model_a".to_owned(),
                source_name: "model_a.stl".to_owned(),
                source_path: String::new(),
                source_digest: None,
                source_size_bytes: 0,
                source_modified_unix_ms: None,
                mesh_asset_id: format!("mesh-{id}"),
                format: "stl".to_owned(),
                mesh_type: super::DEFAULT_MESH_TYPE.to_owned(),
                units: "millimeters".to_owned(),
                show_import_log: false,
                triangle_count: 0,
                dimensions: [0.0; 3],
                generation: None,
                skipped_result_fields: Vec::new(),
                region_names: Vec::new(),
                boundary_group_names: Vec::new(),
                face_quality: None,
            });
        }
        assert!(service.workspace.open_import("import-2", "model_a"));

        let snapshot = service.document_workspace();
        let document = snapshot
            .documents
            .iter()
            .find(|document| document.id == "import-2")
            .ok_or("open import missing from workspace snapshot")?;
        assert_eq!(document.display_ordinal, 2);
        assert_eq!(document.display_count, 2);
        Ok(())
    }

    #[test]
    fn opening_saved_import_starts_a_loading_attempt() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        service.create(&fixture.root, "Open saved import")?;
        let state = service
            .current
            .as_mut()
            .ok_or("created project state missing")?;
        state.imports.push(ImportRecord {
            record_version: super::IMPORT_RECORD_VERSION,
            parser_version: super::STL_IMPORT_PARSER_VERSION,
            id: "saved-import".to_owned(),
            display_name: "part".to_owned(),
            source_name: "part.stl".to_owned(),
            source_path: String::new(),
            source_digest: None,
            source_size_bytes: 0,
            source_modified_unix_ms: None,
            mesh_asset_id: "mesh-saved-import".to_owned(),
            format: "stl".to_owned(),
            mesh_type: super::DEFAULT_MESH_TYPE.to_owned(),
            units: "millimeters".to_owned(),
            show_import_log: false,
            triangle_count: 1,
            dimensions: [1.0, 1.0, 0.0],
            generation: None,
            skipped_result_fields: Vec::new(),
            region_names: Vec::new(),
            boundary_group_names: Vec::new(),
            face_quality: None,
        });
        state.mesh_assets.insert(
            "mesh-saved-import".to_owned(),
            super::model::MeshAssetRef {
                id: "mesh-saved-import".to_owned(),
                revision: 1,
                format_version: panta_mesh::MESH_ASSET_FORMAT_VERSION,
                kind: "surface".to_owned(),
                relative_path: "assets/meshes/missing.pmesh".to_owned(),
                digest: "sha256:test".to_owned(),
                units: "millimeters".to_owned(),
                source_revision: None,
                results: None,
            },
        );

        service.open_import_document("saved-import", "part");

        assert!(service.activation_attempts.contains_key("saved-import"));
        let document = service
            .document_workspace()
            .documents
            .into_iter()
            .find(|document| document.id == "saved-import")
            .ok_or("opened import missing from workspace")?;
        assert_eq!(document.state, DocumentState::Loading);
        Ok(())
    }

    #[test]
    fn background_workspace_activation_failure_uses_saved_asset_message()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut service = ProjectService::new();
        service.execution.reads = crate::execution::Executor::new("asset-path-test", 1, 1);
        service.create(&fixture.root, "Invalid saved asset")?;
        let state = service
            .current
            .as_mut()
            .ok_or("created project state missing")?;
        state.imports.push(ImportRecord {
            record_version: super::IMPORT_RECORD_VERSION,
            parser_version: super::STL_IMPORT_PARSER_VERSION,
            id: "invalid-saved-import".to_owned(),
            display_name: "part".to_owned(),
            source_name: "part.stl".to_owned(),
            source_path: String::new(),
            source_digest: None,
            source_size_bytes: 0,
            source_modified_unix_ms: None,
            mesh_asset_id: "mesh-invalid-saved-import".to_owned(),
            format: "stl".to_owned(),
            mesh_type: super::DEFAULT_MESH_TYPE.to_owned(),
            units: "millimeters".to_owned(),
            show_import_log: false,
            triangle_count: 1,
            dimensions: [1.0, 1.0, 0.0],
            generation: None,
            skipped_result_fields: Vec::new(),
            region_names: Vec::new(),
            boundary_group_names: Vec::new(),
            face_quality: None,
        });
        state.mesh_assets.insert(
            "mesh-invalid-saved-import".to_owned(),
            super::model::MeshAssetRef {
                id: "mesh-invalid-saved-import".to_owned(),
                revision: 1,
                format_version: panta_mesh::MESH_ASSET_FORMAT_VERSION,
                kind: "surface".to_owned(),
                relative_path: "../outside.pmesh".to_owned(),
                digest: "sha256:test".to_owned(),
                units: "millimeters".to_owned(),
                source_revision: None,
                results: None,
            },
        );

        service.open_import_document("invalid-saved-import", "part");
        service.execution.reads.close_and_wait();
        let outcomes = service.drain_asset_activations();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, super::OutcomeKind::Failed);
        assert_eq!(outcomes[0].code, "project.manifest_invalid");

        let document = service
            .document_workspace()
            .documents
            .into_iter()
            .find(|document| document.id == "invalid-saved-import")
            .ok_or("opened import missing from workspace")?;
        assert_eq!(document.state, DocumentState::Failed);
        assert_eq!(document.diagnostic.code, "project.manifest_invalid");
        assert_eq!(document.message, "无法打开已保存的网格资产。");
        Ok(())
    }

    #[test]
    fn deactivating_over_budget_mesh_updates_workspace_residency() {
        let mut service = ProjectService::with_mesh_cache_budget(0);
        service.reset_document_workspace();
        assert!(service.workspace.open_ready_import("large", "large"));
        service.cache_activated_mesh("large".to_owned(), mesh(1));
        service.sync_workspace_residency();
        assert_eq!(
            service.document_workspace().documents[0].state,
            DocumentState::Ready
        );

        service.deactivate_mesh_document();
        service.sync_workspace_residency();

        assert_eq!(
            service.document_workspace().documents[0].state,
            DocumentState::Unloaded
        );
    }

    #[test]
    fn project_name_rules_accept_unicode_and_reject_platform_reserved_stems() {
        assert!(validate_name("模拟件.v1").is_ok());
        assert!(matches!(
            validate_name("COM1.log"),
            Err(ProjectError::InvalidName(_))
        ));
        assert!(matches!(
            validate_name("name. "),
            Err(ProjectError::InvalidName(_))
        ));
    }

    #[test]
    fn activation_limits_are_validated_before_replacing_the_service_setting() {
        let mut service = ProjectService::new();
        assert_eq!(
            service.activation_limits(),
            super::ActivationLimits::DEFAULT
        );
        let custom = super::ActivationLimits {
            surface_triangles: 50_000_000,
            surface_budget: 5_000_000,
            volume_cells: 2_000_000,
            ..super::ActivationLimits::DEFAULT
        };
        assert!(service.set_activation_limits(custom).is_ok());
        assert_eq!(service.activation_limits(), custom);
        let invalid = super::ActivationLimits {
            surface_budget: 60_000_000,
            ..custom
        };
        assert!(matches!(
            service.set_activation_limits(invalid),
            Err(ProjectError::LodSettingsInvalid(field)) if field == "surface_budget"
        ));
        assert_eq!(service.activation_limits(), custom);
    }
}
