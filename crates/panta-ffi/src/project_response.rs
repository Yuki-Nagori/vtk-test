//! 工程服务的 CXX 返回值适配；不解析错误文本，不持有另一份工程状态。
use crate::{ProjectService, bridge};
use panta_core::project::ProjectError;

pub(crate) fn diagnostic(code: &str, category: &str, detail: String) -> bridge::ProjectDiagnostic {
    bridge::ProjectDiagnostic {
        code: code.to_owned(),
        category: category.to_owned(),
        detail,
    }
}

impl From<ProjectError> for bridge::ProjectDiagnostic {
    fn from(error: ProjectError) -> Self {
        diagnostic(error.code(), error.category(), error.detail())
    }
}

// 只在此处构造结果，保证成功诊断为空、失败载荷为缺省值。
macro_rules! response_conversion {
    ($response:ty, $value:ty) => {
        impl<E: Into<bridge::ProjectDiagnostic>> From<Result<$value, E>> for $response {
            fn from(result: Result<$value, E>) -> Self {
                match result {
                    Ok(value) => Self {
                        value,
                        ..Self::default()
                    },
                    Err(error) => Self {
                        error: error.into(),
                        ..Self::default()
                    },
                }
            }
        }
    };
}

response_conversion!(bridge::ProjectSnapshotResult, bridge::ProjectSnapshot);
response_conversion!(bridge::ProjectImportResult, bridge::ProjectImport);
response_conversion!(bridge::ProjectImportsResult, Vec<bridge::ProjectImport>);
response_conversion!(
    bridge::SurfaceMeshSnapshotResult,
    bridge::SurfaceMeshSnapshot
);
response_conversion!(bridge::StlPreviewRequestResult, u64);

pub(super) fn project_service_execute_response(
    service: &mut ProjectService,
    command: bridge::ProjectCommand,
) -> bridge::ProjectSnapshotResult {
    crate::project_service_execute(service, command).into()
}

pub(super) fn project_service_current_response(
    service: &ProjectService,
) -> bridge::ProjectSnapshotResult {
    crate::project_service_current(service).into()
}

pub(super) fn project_service_import_stl_response(
    service: &mut ProjectService,
    source: String,
    mesh_type: String,
    units: String,
    show_import_log: bool,
) -> bridge::ProjectImportResult {
    crate::project_service_import_stl(service, source, mesh_type, units, show_import_log).into()
}

pub(super) fn project_service_imports_response(
    service: &ProjectService,
) -> bridge::ProjectImportsResult {
    crate::project_service_imports(service).into()
}

pub(super) fn project_service_begin_stl_preview_response(
    service: &mut ProjectService,
    source: String,
) -> bridge::StlPreviewRequestResult {
    service
        .service
        .begin_stl_preview(std::path::Path::new(&source))
        .into()
}

pub(super) fn project_service_finish_stl_preview_response(
    service: &mut ProjectService,
    request: u64,
) -> bridge::StlPreviewPoll {
    match service.service.finish_stl_preview(request) {
        Ok(Some(preview)) => bridge::StlPreviewPoll {
            ready: true,
            value: crate::stl_import_preview(preview),
            ..Default::default()
        },
        Ok(None) => bridge::StlPreviewPoll::default(),
        Err(error) => bridge::StlPreviewPoll {
            error: error.into(),
            ..Default::default()
        },
    }
}

pub(super) fn project_service_cancel_stl_preview(
    service: &mut ProjectService,
    request: u64,
) -> bool {
    service.service.cancel_stl_preview(request)
}

pub(super) fn project_service_begin_stl_import_response(
    service: &mut ProjectService,
    source: String,
    mesh_type: String,
    units: String,
    show_import_log: bool,
) -> bridge::StlPreviewRequestResult {
    crate::begin_stl_import(service, source, mesh_type, units, show_import_log).into()
}

pub(super) fn project_service_finish_stl_import_response(
    service: &mut ProjectService,
    request: u64,
) -> bridge::StlImportPoll {
    match crate::project_service_finish_stl_import(service, request) {
        Ok(Some(record)) => bridge::StlImportPoll {
            ready: true,
            value: record,
            ..Default::default()
        },
        Ok(None) => bridge::StlImportPoll::default(),
        Err(error) => bridge::StlImportPoll {
            ready: true,
            error: error.into(),
            ..Default::default()
        },
    }
}

pub(super) fn project_service_cancel_stl_import_response(
    service: &mut ProjectService,
    request: u64,
) -> bool {
    crate::cancel_stl_import(service, request)
}

impl std::fmt::Display for bridge::ProjectDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.detail.is_empty() {
            write!(formatter, "{}", self.code)
        } else {
            write!(formatter, "{}: {}", self.code, self.detail)
        }
    }
}

impl std::error::Error for bridge::ProjectDiagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_revision_crosses_cxx_as_diagnostic_without_changing_the_project()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = crate::temp_directory::Fixture::new()?;
        let mut service = crate::project_service_new();
        let created = service.service.create(&fixture.root, "Max")?;
        let path = created.path;
        let mut manifest = String::from_utf8(std::fs::read(&path)?)?;
        let revision_key = manifest
            .find("\"revision\"")
            .ok_or("revision field missing")?;
        let value_start = revision_key
            + manifest[revision_key..]
                .find(':')
                .ok_or("revision field has no value")?
            + 1;
        let value_start = value_start
            + manifest[value_start..]
                .find(|character: char| !character.is_whitespace())
                .ok_or("revision value missing")?;
        let value_end = value_start
            + manifest[value_start..]
                .find(|character: char| !character.is_ascii_digit())
                .ok_or("revision value has no terminator")?;
        manifest.replace_range(value_start..value_end, &u64::MAX.to_string());
        std::fs::write(&path, &manifest)?;
        service.service.open(&path)?;
        let result = project_service_execute_response(
            &mut service,
            bridge::ProjectCommand {
                kind: bridge::ProjectCommandKind::Rename,
                value: "Next".into(),
            },
        );
        assert_eq!(result.error.code, "project.command_invalid");
        assert_eq!(result.error.category, "validation");
        assert_eq!(result.error.detail, "project revision exhausted");
        assert!(result.value.path.is_empty());
        let current = project_service_current_response(&service);
        assert!(current.error.code.is_empty());
        assert_eq!(current.value.name, "Max");
        assert_eq!(current.value.revision, u64::MAX);
        assert!(!current.value.dirty);
        assert_eq!(std::fs::read(&path)?, manifest.as_bytes());
        Ok(())
    }

    fn poll_preview(service: &mut ProjectService, request: u64) -> bridge::StlPreviewPoll {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let result = project_service_finish_stl_preview_response(service, request);
            if result.ready || !result.error.code.is_empty() {
                return result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "bridge preview timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn poll_import(service: &mut ProjectService, request: u64) -> bridge::StlImportPoll {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let result = project_service_finish_stl_import_response(service, request);
            if result.ready {
                return result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "bridge import timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn confirmed_import_bridge_returns_committed_record() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = crate::temp_directory::Fixture::new()?;
        let source = fixture.root.join("bridge.stl");
        std::fs::write(&source, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
        let mut service = crate::project_service_new();
        service.service.create(&fixture.root, "Bridge")?;
        let preview =
            project_service_begin_stl_preview_response(&mut service, source.display().to_string());
        assert!(preview.error.code.is_empty());
        assert!(poll_preview(&mut service, preview.value).ready);
        let request = project_service_begin_stl_import_response(
            &mut service,
            source.display().to_string(),
            "solid-3d".into(),
            "millimeters".into(),
            false,
        );
        assert!(request.error.code.is_empty());
        let imported = poll_import(&mut service, request.value);
        assert!(imported.error.code.is_empty());
        assert!(imported.ready);
        assert_eq!(imported.value.id, "import-1");
        assert_eq!(imported.value.source_name, "bridge.stl");
        assert_eq!(project_service_current_response(&service).value.revision, 1);
        Ok(())
    }

    #[test]
    fn preview_bridge_distinguishes_pending_success_failure_and_cancelled_requests()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = crate::temp_directory::Fixture::new()?;
        let source = fixture.root.join("preview.stl");
        std::fs::write(&source, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
        let mut service = crate::project_service_new();
        let request =
            project_service_begin_stl_preview_response(&mut service, source.display().to_string());
        assert_eq!(request.error, bridge::ProjectDiagnostic::default());
        assert!(request.value > 0);
        let result = poll_preview(&mut service, request.value);
        assert!(result.ready);
        assert_eq!(result.error, bridge::ProjectDiagnostic::default());
        assert_eq!(result.value.source_name, "preview.stl");
        assert_eq!(result.value.triangle_count, 1);
        assert!(project_service_cancel_stl_preview(
            &mut service,
            request.value
        ));
        let invalid = project_service_finish_stl_preview_response(&mut service, request.value);
        assert!(!invalid.ready);
        assert_eq!(invalid.error.code, "project.command_invalid");
        assert!(invalid.value.source_name.is_empty());
        let request = project_service_begin_stl_preview_response(
            &mut service,
            fixture.root.join("missing.stl").display().to_string(),
        );
        let failed = poll_preview(&mut service, request.value);
        assert!(!failed.ready);
        assert_eq!(failed.error.code, "project.import_file_missing");
        assert_eq!(failed.error.category, "missing");
        assert!(failed.error.detail.ends_with("missing.stl"));
        assert!(failed.value.source_name.is_empty());
        Ok(())
    }

    #[test]
    fn domain_diagnostics_preserve_codes_categories_and_verbatim_context() {
        let context = "参数: 中文 / C:\\零件.stl";
        let errors = [
            (
                ProjectError::EmptyName,
                "project.empty_name",
                "validation",
                "",
            ),
            (
                ProjectError::InvalidName(context.into()),
                "project.invalid_name",
                "validation",
                context,
            ),
            (
                ProjectError::LocationEmpty,
                "project.location_empty",
                "validation",
                "",
            ),
            (
                ProjectError::LocationNotAbsolute(context.into()),
                "project.location_not_absolute",
                "validation",
                context,
            ),
            (
                ProjectError::LocationCreateFailed(context.into()),
                "project.location_create_failed",
                "io",
                context,
            ),
            (
                ProjectError::FileMissing(context.into()),
                "project.file_missing",
                "missing",
                context,
            ),
            (
                ProjectError::InvalidFile(context.into()),
                "project.invalid_file",
                "format",
                context,
            ),
            (
                ProjectError::AlreadyExists(context.into()),
                "project.already_exists",
                "conflict",
                context,
            ),
            (
                ProjectError::ManifestInvalid(context.into()),
                "project.manifest_invalid",
                "format",
                context,
            ),
            (
                ProjectError::UnsupportedSchema(999),
                "project.unsupported_schema",
                "format",
                "999",
            ),
            (ProjectError::NoProject, "project.no_project", "state", ""),
            (
                ProjectError::CommandInvalid(context.into()),
                "project.command_invalid",
                "validation",
                context,
            ),
            (
                ProjectError::ImportFileMissing(context.into()),
                "project.import_file_missing",
                "missing",
                context,
            ),
            (
                ProjectError::ImportInvalidFile(context.into()),
                "project.import_invalid_file",
                "format",
                context,
            ),
            (
                ProjectError::ImportUnsupportedMeshType(context.into()),
                "project.import_unsupported_mesh_type",
                "format",
                context,
            ),
            (
                ProjectError::ImportUnsupportedUnits(context.into()),
                "project.import_unsupported_units",
                "format",
                context,
            ),
            (
                ProjectError::ImportParseFailed(context.into()),
                "project.import_parse_failed",
                "format",
                context,
            ),
            (
                ProjectError::ImportSourceChanged(context.into()),
                "project.import_source_changed",
                "conflict",
                context,
            ),
            (
                ProjectError::ImportAssetWriteFailed(context.into()),
                "project.import_asset_write_failed",
                "io",
                context,
            ),
            (
                ProjectError::ImportRecordMissing(context.into()),
                "project.import_record_missing",
                "missing",
                context,
            ),
            (
                ProjectError::Io(context.into()),
                "project.io",
                "io",
                context,
            ),
        ];
        for (error, code, category, detail) in errors {
            let diagnostic = bridge::ProjectDiagnostic::from(error);
            assert_eq!(diagnostic.code, code);
            assert_eq!(diagnostic.category, category);
            assert_eq!(diagnostic.detail, detail);
        }
    }

    #[test]
    fn all_response_shapes_distinguish_failure_from_empty_success() {
        macro_rules! check {
            ($response:ty, $value:ty) => {
                let success = <$response>::from(Ok::<$value, ProjectError>(<$value>::default()));
                assert_eq!(success.error, bridge::ProjectDiagnostic::default());
                let failure = <$response>::from(Err::<$value, _>(ProjectError::NoProject));
                assert_eq!(failure.error.code, "project.no_project");
                assert_eq!(failure.error.category, "state");
                assert!(failure.error.detail.is_empty());
            };
        }
        check!(bridge::ProjectSnapshotResult, bridge::ProjectSnapshot);
        check!(bridge::ProjectImportResult, bridge::ProjectImport);
        check!(bridge::ProjectImportsResult, Vec<bridge::ProjectImport>);
        check!(bridge::StlPreviewRequestResult, u64);
        check!(
            bridge::SurfaceMeshSnapshotResult,
            bridge::SurfaceMeshSnapshot
        );
    }

    #[test]
    fn service_responses_reject_requests_without_publishing_placeholder_values() {
        let mut service = crate::project_service_new();
        assert_eq!(
            project_service_current_response(&service).error.code,
            "project.no_project"
        );
        assert_eq!(
            project_service_imports_response(&service).error.code,
            "project.no_project"
        );
        let command = bridge::ProjectCommand {
            kind: bridge::ProjectCommandKind { repr: 99 },
            value: String::new(),
        };
        let result = project_service_execute_response(&mut service, command);
        assert_eq!(result.error.code, "project.command_invalid");
        assert_eq!(result.error.detail, "unknown command");
    }

    #[test]
    fn importing_without_a_project_returns_a_diagnostic() {
        let mut service = crate::project_service_new();
        let imported = project_service_import_stl_response(
            &mut service,
            "part.stl".into(),
            "solid-3d".into(),
            "millimeters".into(),
            false,
        );
        assert_eq!(imported.error.code, "project.no_project");
        assert!(imported.value.id.is_empty());

        let empty = bridge::ProjectDiagnostic::default();
        assert_eq!(empty.to_string(), "");
        let detailed = diagnostic("project.io", "io", "disk full".into());
        assert_eq!(detailed.to_string(), "project.io: disk full");
    }
}
