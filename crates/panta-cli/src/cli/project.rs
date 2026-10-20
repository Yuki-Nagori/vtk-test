//! CLI 项目命令；只编排公共 `ProjectService` 操作和查询。

use std::path::PathBuf;

use panta_core::project::{ImportRecord, ProjectCommand, ProjectService, ProjectSnapshot};

pub fn inspect(
    service: &mut ProjectService,
    path: PathBuf,
) -> Result<(ProjectSnapshot, Vec<ImportRecord>), String> {
    let snapshot = service
        .execute(ProjectCommand::Open { path })
        .map_err(|error| error.to_string())?;
    let imports = service.imports().map_err(|error| error.to_string())?;
    Ok((snapshot, imports))
}

pub fn create(
    service: &mut ProjectService,
    location: PathBuf,
    name: &str,
) -> Result<ProjectSnapshot, String> {
    service
        .create(&location, name)
        .map_err(|error| error.to_string())
}
