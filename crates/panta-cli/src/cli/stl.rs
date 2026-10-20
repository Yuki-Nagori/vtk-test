//! CLI STL 命令；预览和提交均委托给 `ProjectService`。

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use panta_core::project::{
    DEFAULT_MESH_TYPE, ImportRecord, ProjectCommand, ProjectService, StlImportPreview,
};

#[derive(Debug, PartialEq, Eq)]
pub struct ImportOptions {
    mesh_type: String,
    units: String,
}

pub fn inspect(service: &mut ProjectService, path: PathBuf) -> Result<StlImportPreview, String> {
    service
        .inspect_stl(&path)
        .map_err(|error| error.to_string())
}

pub fn import(
    service: &mut ProjectService,
    project_path: PathBuf,
    source_path: PathBuf,
    options: ImportOptions,
) -> Result<ImportRecord, String> {
    service
        .execute(ProjectCommand::Open { path: project_path })
        .map_err(|error| error.to_string())?;
    let preview = service
        .inspect_stl(&source_path)
        .map_err(|error| error.to_string())?;
    if preview.large_mesh_warning {
        eprintln!(
            "Warning: expanded mesh data is estimated to exceed 2 GiB. Import will continue, but it may require substantial memory and disk space."
        );
    }
    service
        .import_stl(&source_path, &options.mesh_type, &options.units, false)
        .map_err(|error| error.to_string())
}

pub(super) fn parse_import_options(arguments: &[OsString]) -> Result<ImportOptions, String> {
    let mut options = ImportOptions {
        mesh_type: DEFAULT_MESH_TYPE.to_owned(),
        units: "millimeters".to_owned(),
    };
    let mut mesh_type_seen = false;
    let mut units_seen = false;
    let mut arguments = arguments.iter();
    while let Some(option) = arguments.next() {
        let value = arguments
            .next()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("缺少有效的 UTF-8 参数值：{}", option.to_string_lossy()))?;
        if option == OsStr::new("--mesh-type") && !mesh_type_seen {
            options.mesh_type = value.to_owned();
            mesh_type_seen = true;
        } else if option == OsStr::new("--units") && !units_seen {
            options.units = value.to_owned();
            units_seen = true;
        } else {
            return Err(format!(
                "未知或重复的导入选项：{}",
                option.to_string_lossy()
            ));
        }
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MESH_TYPE, ImportOptions, parse_import_options};
    use std::ffi::OsString;

    #[test]
    fn import_options_reuse_the_domain_default_values() {
        assert_eq!(
            parse_import_options(&[]),
            Ok(ImportOptions {
                mesh_type: DEFAULT_MESH_TYPE.to_owned(),
                units: "millimeters".to_owned(),
            })
        );
    }

    #[test]
    fn import_options_accept_explicit_values_and_reject_bad_syntax() {
        assert_eq!(
            parse_import_options(&[
                OsString::from("--units"),
                OsString::from("inches"),
                OsString::from("--mesh-type"),
                OsString::from("midplane"),
            ]),
            Ok(ImportOptions {
                mesh_type: "midplane".to_owned(),
                units: "inches".to_owned(),
            })
        );
        assert!(parse_import_options(&[OsString::from("--unknown"), OsString::from("x")]).is_err());
        assert!(parse_import_options(&[OsString::from("--units")]).is_err());
    }
}
