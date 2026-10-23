//! CLI STL 命令；预览和提交均委托给 `ProjectService`。

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use panta_core::project::{
    DEFAULT_MESH_TYPE, DiagnosticOptions, ImportRecord, ProjectCommand, ProjectService,
    StlImportPreview, SurfaceDiagnostics,
};

#[derive(Debug, PartialEq, Eq)]
pub struct ImportOptions {
    mesh_type: String,
    units: String,
}

pub fn inspect(
    service: &mut ProjectService,
    path: PathBuf,
    options: DiagnosticOptions,
) -> Result<(StlImportPreview, SurfaceDiagnostics), String> {
    service
        .inspect_stl_with_diagnostics(&path, options)
        .map_err(|error| error.to_string())
}

/// 解析 `stl inspect` 选项；`--weld-tolerance` 为相对包围盒对角线的有限非负比例。
pub(super) fn parse_inspect_options(arguments: &[OsString]) -> Result<DiagnosticOptions, String> {
    let mut options = DiagnosticOptions::default();
    match arguments {
        [] => {}
        [option, value] if option == OsStr::new("--weld-tolerance") => {
            let ratio = value
                .to_str()
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|ratio| ratio.is_finite() && *ratio >= 0.0)
                .ok_or_else(|| {
                    format!(
                        "--weld-tolerance 需要有限非负数：{}",
                        value.to_string_lossy()
                    )
                })?;
            options.relative_weld_tolerance = ratio;
        }
        _ => return Err("stl inspect 仅接受 --weld-tolerance <相对比例>".to_owned()),
    }
    Ok(options)
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
    use super::{DEFAULT_MESH_TYPE, ImportOptions, parse_import_options, parse_inspect_options};
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

    #[test]
    fn inspect_options_default_and_validate_the_relative_tolerance() {
        let default = parse_inspect_options(&[]).map(|options| options.relative_weld_tolerance);
        assert_eq!(
            default,
            Ok(panta_core::project::DEFAULT_RELATIVE_WELD_TOLERANCE)
        );
        let explicit =
            parse_inspect_options(&[OsString::from("--weld-tolerance"), OsString::from("1e-4")])
                .map(|options| options.relative_weld_tolerance);
        assert_eq!(explicit, Ok(1e-4));
        for bad in ["-1", "NaN", "inf", "abc"] {
            assert!(
                parse_inspect_options(&[OsString::from("--weld-tolerance"), OsString::from(bad)])
                    .is_err()
            );
        }
        assert!(parse_inspect_options(&[OsString::from("--weld-tolerance")]).is_err());
    }
}
