//! `panta-cli` 进程入口、命令分发与终端输出。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use panta_core::project::{ProjectService, ProjectSnapshot};

mod project;
mod stl;

const EXIT_COMMAND_FAILED: u8 = 2;
const USAGE: &str = concat!(
    "用法：panta-cli ",
    "project inspect <工程文件.panta> | ",
    "project create <父目录> <工程名> | ",
    "mesh inspect <网格文件> [--weld-tolerance <相对比例>] | ",
    "mesh import <工程文件.panta> <网格文件> ",
    "[--mesh-type <id>] [--units <unit>] | ",
    "mesh export <工程文件.panta> <导入 ID> <输出文件.stl|vtu|vtp|vtk|obj|ply>"
);

enum CommandOutput {
    ProjectInspect {
        snapshot: ProjectSnapshot,
        imports: Vec<panta_core::project::ImportRecord>,
    },
    ProjectCreate(ProjectSnapshot),
    StlInspect(
        panta_core::project::StlImportPreview,
        panta_core::project::SurfaceDiagnostics,
    ),
    StlImport(panta_core::project::ImportRecord),
    MeshExport {
        format: &'static str,
        target: PathBuf,
    },
}

pub fn run_process() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(&arguments) {
        Ok(CommandOutput::ProjectInspect { snapshot, imports }) => {
            println!("Project: {}", snapshot.name);
            println!("Path: {}", snapshot.path.display());
            println!("Revision: {}", snapshot.revision);
            println!("Imported assets: {}", imports.len());
            for record in imports {
                println!(
                    "Asset: {} | Import ID: {} | Mesh asset ID: {}",
                    record.display_name, record.id, record.mesh_asset_id
                );
                println!(
                    "Source: {} | Format: {} | Size: {} bytes | SHA-256: {}",
                    record.source_name,
                    record.format,
                    record.source_size_bytes,
                    record.source_digest.as_deref().unwrap_or("unavailable")
                );
            }
            ExitCode::SUCCESS
        }
        Ok(CommandOutput::ProjectCreate(project)) => {
            println!("Project: {}", project.name);
            println!("Path: {}", project.path.display());
            println!("Revision: {}", project.revision);
            ExitCode::SUCCESS
        }
        Ok(CommandOutput::StlInspect(preview, diagnostics)) => {
            println!("Mesh: {}", preview.source_name);
            println!("Triangles: {}", preview.triangle_count);
            println!(
                "Dimensions: {} × {} × {}",
                preview.dimensions[0], preview.dimensions[1], preview.dimensions[2]
            );
            println!(
                "Note: Mesh sources do not encode units; dimensions and weld tolerance use source coordinates."
            );
            print_diagnostics(&diagnostics);
            ExitCode::SUCCESS
        }
        Ok(CommandOutput::StlImport(record)) => {
            println!("Imported: {}", record.display_name);
            println!("Import ID: {}", record.id);
            println!("Mesh asset ID: {}", record.mesh_asset_id);
            println!(
                "Source: {} ({} bytes, {})",
                record.source_name,
                record.source_size_bytes,
                record
                    .source_digest
                    .as_deref()
                    .unwrap_or("SHA-256 unavailable")
            );
            ExitCode::SUCCESS
        }
        Ok(CommandOutput::MeshExport { format, target }) => {
            println!("Exported: {}", target.display());
            println!("Format: {format}");
            println!("Units: millimeters");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("panta-cli: {message}");
            ExitCode::from(EXIT_COMMAND_FAILED)
        }
    }
}

fn print_diagnostics(report: &panta_core::project::SurfaceDiagnostics) {
    println!(
        "Mesh quality: {}",
        if report.is_clean() {
            "no issues found"
        } else {
            "issues found"
        }
    );
    println!("Weld tolerance: {}", report.weld_tolerance);
    println!(
        "Vertices: {} welded from {} references ({} near-duplicate)",
        report.welded_vertex_count, report.raw_vertex_count, report.near_duplicate_vertex_count
    );
    println!("Degenerate triangles: {}", report.degenerate_triangle_count);
    println!(
        "Boundary edges: {} in {} hole(s)",
        report.boundary_edge_count, report.hole_count
    );
    println!("Non-manifold edges: {}", report.non_manifold_edge_count);
    println!(
        "Inconsistently oriented edges: {}",
        report.inconsistent_orientation_edge_count
    );
}

fn run(arguments: &[OsString]) -> Result<CommandOutput, String> {
    let current_directory =
        std::env::current_dir().map_err(|error| format!("无法读取当前目录：{error}"))?;
    let mut service = ProjectService::new();
    match arguments {
        [command, operation, path] if command == "project" && operation == "inspect" => {
            let path = resolve_project_path(PathBuf::from(path), &current_directory);
            project::inspect(&mut service, path)
                .map(|(snapshot, imports)| CommandOutput::ProjectInspect { snapshot, imports })
        }
        [command, operation, location, name] if command == "project" && operation == "create" => {
            let location = resolve_project_path(PathBuf::from(location), &current_directory);
            let name = name
                .to_str()
                .ok_or_else(|| "工程名必须是有效 UTF-8".to_owned())?;
            project::create(&mut service, location, name).map(CommandOutput::ProjectCreate)
        }
        [command, operation, path, options @ ..]
            if is_mesh_command(command) && operation == "inspect" =>
        {
            let options = stl::parse_inspect_options(options)?;
            let path = resolve_project_path(PathBuf::from(path), &current_directory);
            stl::inspect(&mut service, path, options)
                .map(|(preview, diagnostics)| CommandOutput::StlInspect(preview, diagnostics))
        }
        [command, operation, project_path, source_path, options @ ..]
            if is_mesh_command(command) && operation == "import" =>
        {
            let options = stl::parse_import_options(options)?;
            let project_path =
                resolve_project_path(PathBuf::from(project_path), &current_directory);
            let source_path = resolve_project_path(PathBuf::from(source_path), &current_directory);
            stl::import(&mut service, project_path, source_path, options)
                .map(CommandOutput::StlImport)
        }
        [command, operation, project_path, import_id, target]
            if is_mesh_command(command) && operation == "export" =>
        {
            let project_path =
                resolve_project_path(PathBuf::from(project_path), &current_directory);
            let target = resolve_project_path(PathBuf::from(target), &current_directory);
            let import_id = import_id
                .to_str()
                .ok_or_else(|| "导入 ID 必须是有效 UTF-8".to_owned())?;
            service
                .execute(panta_core::project::ProjectCommand::Open { path: project_path })
                .map_err(|error| error.to_string())?;
            let format = service
                .export_import_mesh(import_id, &target)
                .map_err(|error| error.to_string())?;
            Ok(CommandOutput::MeshExport {
                format: format.id(),
                target,
            })
        }
        _ => Err(USAGE.to_owned()),
    }
}

/// `mesh` 为通用入口，支持 STL / VTU / VTP / VTK / OBJ / PLY；`stl` 保留为同义别名。
fn is_mesh_command(command: &OsString) -> bool {
    command == "mesh" || command == "stl"
}

fn resolve_project_path(path: PathBuf, current_directory: &Path) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    current_directory.join(path)
}

#[cfg(test)]
#[path = "../../../tests/support/rust/temp_directory.rs"]
mod temp_directory;

#[cfg(test)]
mod tests {
    use super::temp_directory;
    use super::{CommandOutput, resolve_project_path, run};
    use panta_core::project::ProjectService;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    #[test]
    fn inspect_uses_the_shared_project_open_command() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let mut creator = ProjectService::new();
        let created = creator.create(&fixture.root, "CLI inspection")?;
        let arguments = [
            OsString::from("project"),
            OsString::from("inspect"),
            created.path.as_os_str().to_owned(),
        ];

        let CommandOutput::ProjectInspect { snapshot, imports } = run(&arguments)? else {
            return Err("project inspect returned an unexpected output".into());
        };

        assert_eq!(snapshot.path, created.path);
        assert_eq!(snapshot.name, "CLI inspection");
        assert_eq!(snapshot.revision, created.revision);
        assert!(!snapshot.dirty);
        assert!(imports.is_empty());
        Ok(())
    }

    #[test]
    fn inspect_reports_invalid_project_files_with_core_diagnostics()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let invalid = fixture.root.join("broken.panta");
        std::fs::write(&invalid, b"not a Panta project")?;
        let arguments = [
            OsString::from("project"),
            OsString::from("inspect"),
            invalid.into_os_string(),
        ];

        let error = run(&arguments)
            .err()
            .ok_or("invalid project was accepted")?;

        assert!(error.contains("project.manifest_invalid"));
        Ok(())
    }

    #[test]
    fn relative_path_is_resolved_against_the_cli_working_directory() {
        let current = Path::new("/workspace");
        let path = resolve_project_path(PathBuf::from("projects/demo.panta"), current);
        assert_eq!(path, current.join("projects/demo.panta"));
    }

    #[test]
    fn absolute_paths_are_preserved() -> Result<(), Box<dyn std::error::Error>> {
        let current = std::env::current_dir()?;
        let path = current.join("data/projects/demo");
        assert_eq!(resolve_project_path(path.clone(), &current), path);
        Ok(())
    }
}
