//! 工程后台请求的独占所有权与 DTO 映射；提交裁决由 core 完成。
use crate::{bridge, project::ProjectService};
use panta_core::project::{ProjectError, ProjectOperation};
use std::path::PathBuf;

pub struct ProjectOperationRequest {
    request: Result<panta_core::project::ProjectOperationRequest, ProjectError>,
}

pub struct ProjectOperationCompletion {
    completion: Result<panta_core::project::ProjectOperationCompletion, ProjectError>,
}

pub(super) fn project_service_begin_operation(
    service: &mut ProjectService,
    input: bridge::ProjectOperationInput,
) -> Box<ProjectOperationRequest> {
    let operation = match input.kind {
        bridge::ProjectOperationKind::Create => Ok(ProjectOperation::Create {
            location: PathBuf::from(input.path),
            name: input.name,
        }),
        bridge::ProjectOperationKind::Open => Ok(ProjectOperation::Open {
            path: PathBuf::from(input.path),
        }),
        bridge::ProjectOperationKind::Save => Ok(ProjectOperation::Save),
        _ => Err(ProjectError::CommandInvalid(
            "unknown project operation".into(),
        )),
    };
    Box::new(ProjectOperationRequest {
        request: operation.and_then(|operation| service.service.begin_project_operation(operation)),
    })
}

pub(super) fn project_operation_accepted(request: &ProjectOperationRequest) -> bool {
    request.request.is_ok()
}

#[expect(clippy::boxed_local, reason = "CXX opaque 请求需要独占移交等待线程")]
pub(super) fn project_operation_wait(
    request: Box<ProjectOperationRequest>,
) -> Box<ProjectOperationCompletion> {
    Box::new(ProjectOperationCompletion {
        completion: request
            .request
            .map(panta_core::project::ProjectOperationRequest::wait),
    })
}

pub(super) fn project_service_finish_operation(
    service: &mut ProjectService,
    completion: Box<ProjectOperationCompletion>,
) -> bridge::ProjectSnapshotResult {
    completion
        .completion
        .and_then(|completion| service.service.finish_project_operation(completion))
        .map(crate::project::project_snapshot)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_operation_crosses_waiter_and_returns_to_service()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = crate::temp_directory::Fixture::new()?;
        let mut service = crate::project_service_new();
        let request = project_service_begin_operation(
            &mut service,
            bridge::ProjectOperationInput {
                kind: bridge::ProjectOperationKind::Create,
                path: fixture.root.display().to_string(),
                name: "Background".into(),
            },
        );
        assert!(project_operation_accepted(&request));
        let completion = std::thread::spawn(move || project_operation_wait(request))
            .join()
            .map_err(|_| "waiter panicked")?;
        let result = project_service_finish_operation(&mut service, completion);
        assert!(result.error.code.is_empty(), "{}", result.error.detail);
        assert_eq!(result.value.name, "Background");
        Ok(())
    }
}
