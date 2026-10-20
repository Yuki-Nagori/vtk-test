//! 工程服务与显示快照的 CXX 适配；业务校验和提交由 core 拥有。
use crate::bridge;

/// Rust 工程服务的 CXX 适配器：只负责 DTO 映射与结构化诊断，
/// 工程校验、清单事务及模型状态均保留在 panta-core。
pub struct ProjectService {
    pub(super) service: panta_core::project::ProjectService,
}

pub(super) fn project_service_new() -> Box<ProjectService> {
    Box::new(ProjectService {
        service: panta_core::project::ProjectService::new(),
    })
}

pub(super) fn project_service_execute(
    service: &mut ProjectService,
    command: bridge::ProjectCommand,
) -> Result<bridge::ProjectSnapshot, panta_core::project::ProjectError> {
    let command = match command.kind {
        bridge::ProjectCommandKind::Rename => panta_core::project::ProjectCommand::Rename {
            name: command.value,
        },
        _ => {
            return Err(panta_core::project::ProjectError::CommandInvalid(
                "unknown command".to_owned(),
            ));
        }
    };
    service.service.execute(command).map(project_snapshot)
}

pub(super) fn project_service_current(
    service: &ProjectService,
) -> Result<bridge::ProjectSnapshot, panta_core::project::ProjectError> {
    service.service.current().map(project_snapshot)
}

pub(super) fn project_service_import_stl(
    service: &mut ProjectService,
    source: String,
    mesh_type: String,
    units: String,
    show_import_log: bool,
) -> Result<bridge::ProjectImport, panta_core::project::ProjectError> {
    service
        .service
        .import_stl(
            std::path::Path::new(&source),
            &mesh_type,
            &units,
            show_import_log,
        )
        .and_then(|import| project_import(&service.service, import))
}

pub(super) fn project_service_begin_stl_import(
    service: &mut ProjectService,
    source: String,
    mesh_type: String,
    units: String,
    show_import_log: bool,
) -> Result<u64, panta_core::project::ProjectError> {
    service.service.begin_stl_import(
        std::path::Path::new(&source),
        &mesh_type,
        &units,
        show_import_log,
    )
}

pub(super) fn project_service_finish_stl_import(
    service: &mut ProjectService,
    request: u64,
) -> Result<Option<bridge::ProjectImport>, panta_core::project::ProjectError> {
    service
        .service
        .finish_stl_import(request)
        .and_then(|record| {
            record
                .map(|record| project_import(&service.service, record))
                .transpose()
        })
}

pub(super) fn project_service_cancel_stl_import(
    service: &mut ProjectService,
    request: u64,
) -> bool {
    service.service.cancel_stl_import(request)
}

pub(super) fn project_service_imports(
    service: &ProjectService,
) -> Result<Vec<bridge::ProjectImport>, panta_core::project::ProjectError> {
    service.service.imports().and_then(|imports| {
        imports
            .into_iter()
            .map(|import| project_import(&service.service, import))
            .collect()
    })
}

pub(super) fn project_service_workspace_snapshot(
    service: &ProjectService,
) -> bridge::WorkspaceSnapshot {
    let snapshot = service.service.document_workspace();
    bridge::WorkspaceSnapshot {
        documents: snapshot
            .documents
            .into_iter()
            .map(|document| bridge::WorkspaceDocument {
                id: document.id,
                kind: document.kind.as_str().to_owned(),
                state: document.state.as_str().to_owned(),
                title: document.title,
                message: document.message,
                diagnostic_code: document.diagnostic.code,
                diagnostic_category: document.diagnostic.category,
                diagnostic_detail: document.diagnostic.detail,
                display_ordinal: document.display_ordinal,
                display_count: document.display_count,
            })
            .collect(),
        active_document_id: snapshot.active_document_id,
    }
}

pub(super) fn project_service_workspace_reset(service: &mut ProjectService) {
    service.service.reset_document_workspace();
}

pub(super) fn project_service_workspace_open_import(
    service: &mut ProjectService,
    import_id: &str,
    title: &str,
) {
    service.service.open_import_document(import_id, title);
}

pub(super) fn project_service_workspace_publish_import(
    service: &mut ProjectService,
    import_id: &str,
    title: &str,
) {
    service.service.publish_import_document(import_id, title);
}

pub(super) fn project_service_workspace_activate(service: &mut ProjectService, document_id: &str) {
    service.service.activate_workspace_document(document_id);
}

pub(super) fn project_service_workspace_close(service: &mut ProjectService, document_id: &str) {
    service.service.close_workspace_document(document_id);
}

pub(super) fn project_service_workspace_move(service: &mut ProjectService, from: u64, to: u64) {
    let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) else {
        return;
    };
    service.service.move_workspace_document(from, to);
}

pub(super) fn project_error_message(code: &str) -> String {
    panta_core::project::project_error_message(code).to_owned()
}

pub(super) fn format_project_dimensions(
    size_x: f64,
    size_y: f64,
    size_z: f64,
    unit: &str,
) -> String {
    panta_core::project::format_project_dimensions(size_x, size_y, size_z, unit)
}

pub(super) fn mesh_type_catalog() -> Vec<bridge::ChoiceDefinition> {
    panta_core::project::MESH_TYPES
        .iter()
        .map(|entry| bridge::ChoiceDefinition {
            id: entry.id.to_owned(),
            source_text: entry.source_text.to_owned(),
        })
        .collect()
}

pub(super) fn default_mesh_type() -> String {
    panta_core::project::DEFAULT_MESH_TYPE.to_owned()
}

pub(super) fn project_service_drain_workspace_activations(service: &mut ProjectService) {
    // 领域层在 drain 时发布缓存和文档状态；显示载荷由独立输入快照通道读取。
    service.service.drain_asset_activations();
}

pub(super) fn project_snapshot(
    snapshot: panta_core::project::ProjectSnapshot,
) -> bridge::ProjectSnapshot {
    bridge::ProjectSnapshot {
        path: snapshot.path.display().to_string(),
        name: snapshot.name,
        revision: snapshot.revision,
        dirty: snapshot.dirty,
    }
}

fn project_import(
    service: &panta_core::project::ProjectService,
    import: panta_core::project::ImportRecord,
) -> Result<bridge::ProjectImport, panta_core::project::ProjectError> {
    let mesh_asset_path = service.mesh_asset_path_for_import(&import.id)?;
    Ok(bridge::ProjectImport {
        record_version: import.record_version,
        parser_version: import.parser_version,
        id: import.id,
        display_name: import.display_name,
        source_name: import.source_name,
        mesh_asset_path,
        format: import.format,
        mesh_type: import.mesh_type,
        units: import.units,
        show_import_log: import.show_import_log,
        triangle_count: import.triangle_count,
        size_x: import.dimensions[0],
        size_y: import.dimensions[1],
        size_z: import.dimensions[2],
    })
}

pub(super) fn stl_import_preview(
    preview: panta_core::project::StlImportPreview,
) -> bridge::StlImportPreview {
    bridge::StlImportPreview {
        source_name: preview.source_name,
        triangle_count: preview.triangle_count,
        expanded_mesh_bytes: preview.expanded_mesh_bytes,
        large_mesh_warning: preview.large_mesh_warning,
        size_x: preview.dimensions[0],
        size_y: preview.dimensions[1],
        size_z: preview.dimensions[2],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn workspace_drain_publishes_ready_mesh_and_failure_diagnostics()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = crate::temp_directory::Fixture::new()?;
        let source = fixture.root.join("part.stl");
        std::fs::write(&source, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 2 0\n")?;
        let mut service = project_service_new();
        let project = service.service.create(&fixture.root, "Workspace")?;
        let imported = service
            .service
            .import_stl(&source, "solid-3d", "centimeters", false)?;
        let asset = service.service.mesh_asset_path_for_import(&imported.id)?;
        service.service.open(&project.path)?;
        project_service_workspace_open_import(&mut service, &imported.id, "part");
        let ready = wait_for_activation(&mut service)?;
        assert_eq!(ready.documents[0].state, "ready");
        let display = crate::prepare_mesh_display(crate::project_service_mesh_display_input(
            &service,
            &imported.id,
        ));
        assert!(display.error.code.is_empty());
        assert_eq!(
            display.value.coordinates,
            [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 20.0, 0.0]
        );
        project_service_workspace_close(&mut service, &imported.id);
        std::fs::remove_file(
            project
                .path
                .parent()
                .ok_or("project has no parent")?
                .join(asset),
        )?;
        project_service_workspace_open_import(&mut service, &imported.id, "part");
        let failed = wait_for_activation(&mut service)?;
        assert_eq!(failed.documents[0].state, "failed");
        assert!(!failed.documents[0].diagnostic_code.is_empty());
        assert!(!failed.documents[0].diagnostic_category.is_empty());
        assert!(!failed.documents[0].diagnostic_detail.is_empty());
        let display = crate::prepare_mesh_display(crate::project_service_mesh_display_input(
            &service,
            &imported.id,
        ));
        assert_eq!(display.error.code, "project.mesh_not_resident");
        assert!(display.value.coordinates.is_empty());
        Ok(())
    }

    fn wait_for_activation(
        service: &mut ProjectService,
    ) -> Result<bridge::WorkspaceSnapshot, Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            project_service_drain_workspace_activations(service);
            let snapshot = project_service_workspace_snapshot(service);
            if snapshot
                .documents
                .iter()
                .all(|document| document.state != "loading")
            {
                return Ok(snapshot);
            }
            if Instant::now() >= deadline {
                return Err("workspace activation timed out".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
