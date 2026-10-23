//! panta-cli 公共行为黑盒测试；由 panta-cli manifest 显式注册。
use std::process::Command;

use panta_core::project::ProjectService;

#[path = "../support/rust/temp_directory.rs"]
mod temp_directory;

fn panta_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_panta-cli"))
}

#[test]
fn create_writes_a_project_that_inspect_can_read() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let relative_parent = std::path::Path::new("nested").join("projects");
    let parent = fixture.root.join(&relative_parent);
    let created = panta_cli()
        .current_dir(&fixture.root)
        .args(["project", "create"])
        .arg(&relative_parent)
        .arg("CLI created project")
        .output()?;

    assert_eq!(created.status.code(), Some(0), "{created:?}");
    let created_stdout = String::from_utf8(created.stdout)?;
    assert!(
        created_stdout.contains("Project: CLI created project"),
        "{created_stdout}"
    );
    assert!(created_stdout.contains("Revision: 0"), "{created_stdout}");

    let project_path = parent
        .join("CLI created project")
        .join("CLI created project.panta");
    assert!(project_path.is_file());
    let inspected = panta_cli()
        .current_dir(&fixture.root)
        .args(["project", "inspect"])
        .arg(&project_path)
        .output()?;

    assert_eq!(inspected.status.code(), Some(0), "{inspected:?}");
    assert!(String::from_utf8(inspected.stdout)?.contains("Project: CLI created project"));
    Ok(())
}

#[test]
fn create_rejects_an_existing_project_without_overwriting_it()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let parent = fixture.root.join("projects");
    let first = panta_cli()
        .args(["project", "create"])
        .arg(&parent)
        .arg("Existing project")
        .output()?;
    assert_eq!(first.status.code(), Some(0), "{first:?}");
    let manifest = parent
        .join("Existing project")
        .join("Existing project.panta");
    let original_contents = std::fs::read(&manifest)?;

    let conflict = panta_cli()
        .args(["project", "create"])
        .arg(&parent)
        .arg("Existing project")
        .output()?;

    assert_eq!(conflict.status.code(), Some(2), "{conflict:?}");
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("project.already_exists"));
    assert!(conflict.stdout.is_empty());
    assert_eq!(std::fs::read(manifest)?, original_contents);
    Ok(())
}

#[test]
fn create_rejects_invalid_names_with_a_domain_diagnostic() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = temp_directory::Fixture::new()?;
    let output = panta_cli()
        .args(["project", "create"])
        .arg(&fixture.root)
        .arg("../escape")
        .output()?;

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("project.invalid_name"));
    assert!(output.stdout.is_empty());
    assert!(!fixture.root.join("escape").exists());
    Ok(())
}

#[test]
fn create_reports_an_unusable_parent_directory() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let parent_file = fixture.root.join("not-a-directory");
    std::fs::write(&parent_file, b"occupied")?;
    let output = panta_cli()
        .args(["project", "create"])
        .arg(&parent_file)
        .arg("Unwritable project")
        .output()?;

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("project.location_create_failed"));
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn inspect_accepts_relative_paths_and_prints_the_project_summary()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let mut creator = ProjectService::new();
    let created = creator.create(&fixture.root, "CLI relative inspection")?;
    let relative_path = created.path.strip_prefix(&fixture.root)?;

    let output = panta_cli()
        .current_dir(&fixture.root)
        .args(["project", "inspect"])
        .arg(relative_path)
        .output()?;

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8(output.stdout)?;
    assert!(
        stdout.contains("Project: CLI relative inspection"),
        "{stdout}"
    );
    assert!(
        stdout.contains(&created.path.display().to_string()),
        "{stdout}"
    );
    assert!(stdout.contains("Revision: 0"), "{stdout}");
    Ok(())
}

#[test]
fn stl_inspect_prints_the_shared_preview_for_a_path_with_spaces_and_unicode()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let source = fixture.root.join("零件 sample.stl");
    let original = b"solid part\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 2 0\nendsolid\n";
    std::fs::write(&source, original)?;

    let output = panta_cli().args(["stl", "inspect"]).arg(&source).output()?;

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("STL: 零件 sample.stl"), "{stdout}");
    assert!(stdout.contains("Triangles: 1"), "{stdout}");
    assert!(stdout.contains("Dimensions: 1 × 2 × 0"), "{stdout}");
    assert!(stdout.contains("STL does not encode units"), "{stdout}");
    assert!(stdout.contains("Mesh quality: issues found"), "{stdout}");
    assert!(
        stdout.contains("Boundary edges: 3 in 1 hole(s)"),
        "{stdout}"
    );
    assert_eq!(std::fs::read(&source)?, original);
    assert_eq!(std::fs::read_dir(&fixture.root)?.count(), 1);
    Ok(())
}

fn mesh_fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/mesh")
        .join(name)
}

#[test]
fn stl_inspect_reports_mesh_quality_diagnostics_for_defect_fixtures()
-> Result<(), Box<dyn std::error::Error>> {
    let inspect = |name: &str, extra: &[&str]| -> Result<String, Box<dyn std::error::Error>> {
        let output = panta_cli()
            .args(["stl", "inspect"])
            .arg(mesh_fixture(name))
            .args(extra)
            .output()?;
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        Ok(String::from_utf8(output.stdout)?)
    };

    let clean = inspect("clean_tetrahedron.stl", &[])?;
    assert!(clean.contains("Mesh quality: no issues found"), "{clean}");
    let open = inspect("open_tetrahedron.stl", &[])?;
    assert!(open.contains("Boundary edges: 3 in 1 hole(s)"), "{open}");
    let flipped = inspect("flipped_tetrahedron.stl", &[])?;
    assert!(
        flipped.contains("Inconsistently oriented edges: 3"),
        "{flipped}"
    );
    let fins = inspect("non_manifold_fins.stl", &[])?;
    assert!(fins.contains("Non-manifold edges: 1"), "{fins}");
    let degenerate = inspect("degenerate_faces.stl", &[])?;
    assert!(
        degenerate.contains("Degenerate triangles: 2"),
        "{degenerate}"
    );
    let welded = inspect("unwelded_tetrahedron.stl", &[])?;
    assert!(welded.contains("(1 near-duplicate)"), "{welded}");
    let strict = inspect("unwelded_tetrahedron.stl", &["--weld-tolerance", "0"])?;
    assert!(strict.contains("Weld tolerance: 0"), "{strict}");
    assert!(strict.contains("Vertices: 5 welded"), "{strict}");

    let invalid = panta_cli()
        .args(["stl", "inspect"])
        .arg(mesh_fixture("clean_tetrahedron.stl"))
        .args(["--weld-tolerance", "-1"])
        .output()?;
    assert_eq!(invalid.status.code(), Some(2), "{invalid:?}");
    assert!(invalid.stdout.is_empty());
    Ok(())
}

#[test]
fn stl_inspect_rejects_missing_and_malformed_inputs_with_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let missing = fixture.root.join("missing.stl");
    let missing_output = panta_cli().args(["stl", "inspect"]).arg(missing).output()?;
    assert_eq!(missing_output.status.code(), Some(2), "{missing_output:?}");
    assert!(
        String::from_utf8_lossy(&missing_output.stderr).contains("project.import_file_missing")
    );
    assert!(missing_output.stdout.is_empty());

    let malformed = fixture.root.join("malformed.stl");
    std::fs::write(&malformed, b"not an STL")?;
    let malformed_output = panta_cli()
        .args(["stl", "inspect"])
        .arg(malformed)
        .output()?;
    assert_eq!(
        malformed_output.status.code(),
        Some(2),
        "{malformed_output:?}"
    );
    assert!(
        String::from_utf8_lossy(&malformed_output.stderr).contains("project.import_parse_failed")
    );
    assert!(malformed_output.stdout.is_empty());

    let empty = fixture.root.join("empty.stl");
    std::fs::write(&empty, [])?;
    let empty_output = panta_cli().args(["stl", "inspect"]).arg(empty).output()?;
    assert_eq!(empty_output.status.code(), Some(2), "{empty_output:?}");
    assert!(String::from_utf8_lossy(&empty_output.stderr).contains("project.import_parse_failed"));
    assert!(empty_output.stdout.is_empty());

    let unsupported = fixture.root.join("model.obj");
    std::fs::write(&unsupported, b"triangle data")?;
    let unsupported_output = panta_cli()
        .args(["stl", "inspect"])
        .arg(unsupported)
        .output()?;
    assert_eq!(
        unsupported_output.status.code(),
        Some(2),
        "{unsupported_output:?}"
    );
    assert!(
        String::from_utf8_lossy(&unsupported_output.stderr).contains("project.import_invalid_file")
    );
    assert!(unsupported_output.stdout.is_empty());
    Ok(())
}

#[test]
fn stl_inspect_accepts_a_non_utf8_unix_parent_path() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;

        let fixture = temp_directory::Fixture::new()?;
        let parent = fixture
            .root
            .join(std::ffi::OsString::from_vec(b"input-\xff".to_vec()));
        match std::fs::create_dir(&parent) {
            Ok(()) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.kind() == std::io::ErrorKind::InvalidInput
                    || (cfg!(target_os = "macos") && error.raw_os_error() == Some(92)) =>
            {
                eprintln!("skipping non-UTF-8 path fixture: {error}");
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }
        let source = parent.join(std::ffi::OsString::from_vec(b"part-\xff.stl".to_vec()));
        match std::fs::write(
            &source,
            b"solid part\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid\n",
        ) {
            Ok(()) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.kind() == std::io::ErrorKind::InvalidInput
                    || (cfg!(target_os = "macos") && error.raw_os_error() == Some(92)) =>
            {
                eprintln!("skipping non-UTF-8 file-name fixture: {error}");
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }

        let output = panta_cli().args(["stl", "inspect"]).arg(&source).output()?;

        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let stdout = String::from_utf8(output.stdout)?;
        assert!(stdout.contains("STL: part-\u{fffd}.stl"), "{stdout}");
        assert!(stdout.contains("Triangles: 1"), "{stdout}");
    }
    Ok(())
}

#[test]
fn invalid_project_returns_diagnostic_on_stderr_and_exit_code_two()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let invalid = fixture.root.join("invalid.panta");
    std::fs::write(&invalid, b"not a project")?;

    let output = panta_cli()
        .args(["project", "inspect"])
        .arg(&invalid)
        .output()?;

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("project.manifest_invalid"));
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn unsupported_arguments_print_usage_and_exit_two() -> Result<(), Box<dyn std::error::Error>> {
    let output = panta_cli().output()?;

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("用法：panta-cli project"));
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn stl_import_persists_asset_metadata_and_inspect_works_after_source_removal()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let mut creator = ProjectService::new();
    let project = creator.create(&fixture.root, "CLI import project")?;
    let source = fixture.root.join("inputs").join("cover sample.stl");
    std::fs::create_dir_all(source.parent().ok_or("STL has no parent directory")?)?;
    let original = b"solid part\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 2 0\nendsolid\n";
    std::fs::write(&source, original)?;

    let imported = panta_cli()
        .args(["stl", "import"])
        .arg(&project.path)
        .arg(&source)
        .args(["--mesh-type", "midplane", "--units", "centimeters"])
        .output()?;

    assert_eq!(imported.status.code(), Some(0), "{imported:?}");
    let stdout = String::from_utf8(imported.stdout)?;
    assert!(stdout.contains("Imported: cover sample"), "{stdout}");
    assert!(stdout.contains("Import ID: import-1"), "{stdout}");
    assert!(stdout.contains("Mesh asset ID: mesh-import-1"), "{stdout}");
    assert!(stdout.contains("cover sample.stl"), "{stdout}");
    assert_eq!(std::fs::read(&source)?, original);

    let mut reopened = ProjectService::new();
    reopened.execute(panta_core::project::ProjectCommand::Open {
        path: project.path.clone(),
    })?;
    let records = reopened.imports()?;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].display_name, "cover sample");
    assert_eq!(records[0].mesh_type, "midplane");
    assert_eq!(records[0].units, "centimeters");
    assert_eq!(records[0].source_size_bytes, original.len() as u64);
    let mesh_path = reopened.mesh_asset_path_for_import(&records[0].id)?;
    assert!(
        project
            .path
            .parent()
            .ok_or("project has no parent")?
            .join(mesh_path)
            .is_file()
    );

    std::fs::remove_file(&source)?;
    let inspected = panta_cli()
        .args(["project", "inspect"])
        .arg(&project.path)
        .output()?;
    assert_eq!(inspected.status.code(), Some(0), "{inspected:?}");
    let inspect_stdout = String::from_utf8(inspected.stdout)?;
    assert!(
        inspect_stdout.contains("Imported assets: 1"),
        "{inspect_stdout}"
    );
    assert!(
        inspect_stdout.contains("Import ID: import-1"),
        "{inspect_stdout}"
    );
    assert!(
        inspect_stdout.contains("Mesh asset ID: mesh-import-1"),
        "{inspect_stdout}"
    );
    assert!(
        inspect_stdout.contains("Source: cover sample.stl"),
        "{inspect_stdout}"
    );
    assert!(
        !inspect_stdout.contains(&source.display().to_string()),
        "{inspect_stdout}"
    );
    Ok(())
}

#[test]
fn stl_import_rejects_invalid_options_without_changing_the_project()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let mut creator = ProjectService::new();
    let project = creator.create(&fixture.root, "CLI invalid import")?;
    let source = fixture.root.join("part.stl");
    std::fs::write(
        &source,
        b"solid part\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid\n",
    )?;

    let unsupported_type = panta_cli()
        .args(["stl", "import"])
        .arg(&project.path)
        .arg(&source)
        .args(["--mesh-type", "unknown"])
        .output()?;
    assert_eq!(
        unsupported_type.status.code(),
        Some(2),
        "{unsupported_type:?}"
    );
    assert!(
        String::from_utf8_lossy(&unsupported_type.stderr)
            .contains("project.import_unsupported_mesh_type")
    );
    assert!(unsupported_type.stdout.is_empty());

    let unsupported_units = panta_cli()
        .args(["stl", "import"])
        .arg(&project.path)
        .arg(&source)
        .args(["--units", "meters"])
        .output()?;
    assert_eq!(
        unsupported_units.status.code(),
        Some(2),
        "{unsupported_units:?}"
    );
    assert!(
        String::from_utf8_lossy(&unsupported_units.stderr)
            .contains("project.import_unsupported_units")
    );
    assert!(unsupported_units.stdout.is_empty());

    let mut reopened = ProjectService::new();
    let snapshot =
        reopened.execute(panta_core::project::ProjectCommand::Open { path: project.path })?;
    assert_eq!(snapshot.revision, 0);
    assert!(reopened.imports()?.is_empty());
    Ok(())
}

#[test]
fn stl_import_rejects_invalid_sources_and_projects_without_partial_assets()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let mut creator = ProjectService::new();
    let project = creator.create(&fixture.root, "CLI failed imports")?;
    let missing = fixture.root.join("missing.stl");
    let malformed = fixture.root.join("malformed.stl");
    let unsupported = fixture.root.join("model.obj");
    let invalid_project = fixture.root.join("invalid.panta");
    std::fs::write(&malformed, b"not an STL")?;
    std::fs::write(&unsupported, b"triangle data")?;
    std::fs::write(&invalid_project, b"not a Panta project")?;
    let valid_source = fixture.root.join("valid.stl");
    std::fs::write(
        &valid_source,
        b"solid part\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid\n",
    )?;

    for (source, expected_code) in [
        (&missing, "project.import_file_missing"),
        (&malformed, "project.import_parse_failed"),
        (&unsupported, "project.import_invalid_file"),
    ] {
        let output = panta_cli()
            .args(["stl", "import"])
            .arg(&project.path)
            .arg(source)
            .output()?;
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected_code));
        assert!(output.stdout.is_empty());
    }

    let invalid_destination = panta_cli()
        .args(["stl", "import"])
        .arg(&invalid_project)
        .arg(&valid_source)
        .output()?;
    assert_eq!(
        invalid_destination.status.code(),
        Some(2),
        "{invalid_destination:?}"
    );
    assert!(
        String::from_utf8_lossy(&invalid_destination.stderr).contains("project.manifest_invalid")
    );
    assert!(invalid_destination.stdout.is_empty());

    let mut reopened = ProjectService::new();
    let snapshot =
        reopened.execute(panta_core::project::ProjectCommand::Open { path: project.path })?;
    assert_eq!(snapshot.revision, 0);
    assert!(reopened.imports()?.is_empty());
    Ok(())
}

#[test]
fn stl_import_allows_duplicate_display_names_and_assigns_distinct_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = temp_directory::Fixture::new()?;
    let mut creator = ProjectService::new();
    let project = creator.create(&fixture.root, "CLI duplicate names")?;
    let left = fixture.root.join("left").join("model.stl");
    let right = fixture.root.join("right").join("model.stl");
    for source in [&left, &right] {
        std::fs::create_dir_all(source.parent().ok_or("STL has no parent directory")?)?;
        std::fs::write(
            source,
            b"solid part\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid\n",
        )?;
    }

    for source in [&left, &right] {
        let output = panta_cli()
            .args(["stl", "import"])
            .arg(&project.path)
            .arg(source)
            .output()?;
        assert_eq!(output.status.code(), Some(0), "{output:?}");
    }

    let mut reopened = ProjectService::new();
    reopened.execute(panta_core::project::ProjectCommand::Open { path: project.path })?;
    let records = reopened.imports()?;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].display_name, "model");
    assert_eq!(records[1].display_name, "model");
    assert_ne!(records[0].id, records[1].id);
    assert_ne!(records[0].mesh_asset_id, records[1].mesh_asset_id);
    Ok(())
}
