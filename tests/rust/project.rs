//! 工程服务和 STL 导入的行为回归。
use panta_core::project::{
    FillSettings, IMPORT_RECORD_VERSION, PROJECT_SCHEMA_VERSION, ProjectCommand, ProjectError,
    ProjectService, STL_IMPORT_PARSER_VERSION,
};
use std::fs;
use std::path::Path;

#[path = "../support/rust/temp_directory.rs"]
mod temp_directory;
use temp_directory::Fixture;

#[test]
fn create_command_save_open_and_model_command_round_trip() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    let created = service.create(&fixture.root, "Demo")?;
    assert_eq!(created.name, "Demo");
    assert!(!created.dirty);
    assert_eq!(created.path, fixture.root.join("Demo/Demo.panta"));
    assert!(created.path.is_file());

    let changed = service.execute(ProjectCommand::Rename {
        name: "Renamed".to_owned(),
    })?;
    assert_eq!(changed.name, "Renamed");
    assert!(changed.dirty);
    let saved = service.save()?;
    assert!(!saved.dirty);

    let mut reopened = ProjectService::new();
    let opened = reopened.open(&created.path)?;
    assert_eq!(opened.name, "Renamed");
    assert_eq!(opened.revision, 1);
    assert!(!opened.dirty);
    Ok(())
}

#[test]
fn rejects_invalid_names_and_existing_targets() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    for name in [
        "",
        "../escape",
        "CON",
        "bad/child",
        "trailing.",
        "Demo.panta",
    ] {
        let error = match service.create(&fixture.root, name) {
            Ok(snapshot) => panic!("invalid name unexpectedly created: {snapshot:?}"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            ProjectError::EmptyName | ProjectError::InvalidName(_)
        ));
    }
    service.create(&fixture.root, "Demo")?;
    let error = match service.create(&fixture.root, "Demo") {
        Ok(snapshot) => panic!("existing target unexpectedly created: {snapshot:?}"),
        Err(error) => error,
    };
    assert!(matches!(error, ProjectError::AlreadyExists(_)));
    Ok(())
}

#[test]
fn rejects_unsupported_manifest_schema() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("Demo");
    fs::create_dir_all(&project)?;
    let project_file = project.join("Demo.panta");
    fs::write(
        &project_file,
        format!(
            r#"{{"schema":{},"name":"Demo","revision":0}}"#,
            PROJECT_SCHEMA_VERSION + 1
        ),
    )?;
    let error = match ProjectService::new().open(&project_file) {
        Ok(snapshot) => panic!("unsupported schema opened: {snapshot:?}"),
        Err(error) => error,
    };
    assert!(matches!(error, ProjectError::UnsupportedSchema(_)));
    Ok(())
}

#[test]
fn accepts_only_panta_project_files() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("Demo");
    fs::create_dir_all(&project)?;
    let other_file = project.join("Demo.json");
    fs::write(other_file, br#"{"schema":1,"name":"Demo","revision":0}"#)?;
    let error = match ProjectService::new().open(&project.join("Demo.json")) {
        Ok(snapshot) => panic!("non-panta file opened: {snapshot:?}"),
        Err(error) => error,
    };
    assert!(matches!(error, ProjectError::InvalidFile(_)));
    Ok(())
}

#[test]
fn reports_empty_or_relative_locations_and_missing_project_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut service = ProjectService::new();
    assert!(matches!(service.current(), Err(ProjectError::NoProject)));
    assert!(matches!(service.save(), Err(ProjectError::NoProject)));
    assert!(matches!(
        service.execute(ProjectCommand::Rename {
            name: "Demo".to_owned(),
        }),
        Err(ProjectError::NoProject)
    ));

    assert!(matches!(
        service.create(Path::new(""), "Demo"),
        Err(ProjectError::LocationEmpty)
    ));
    assert!(matches!(
        service.create(Path::new("relative"), "Demo"),
        Err(ProjectError::LocationNotAbsolute(_))
    ));

    let fixture = Fixture::new()?;
    let file_location = fixture.root.join("location-file");
    fs::write(&file_location, b"not a directory")?;
    assert!(matches!(
        service.create(&file_location, "Demo"),
        Err(ProjectError::LocationCreateFailed(_))
    ));
    assert!(matches!(
        service.create(&file_location.join("child"), "Demo"),
        Err(ProjectError::LocationCreateFailed(_))
    ));
    Ok(())
}

#[test]
fn validates_platform_names_and_command_failures() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    let mut invalid_names = vec![
        ".".to_owned(),
        "..".to_owned(),
        "bad/child".to_owned(),
        r"bad\child".to_owned(),
        "bad:child".to_owned(),
        "trailing.".to_owned(),
        "trailing ".to_owned(),
        "Demo.PANTA".to_owned(),
        "CON.txt".to_owned(),
        "LPT9.log".to_owned(),
        "line\nfeed".to_owned(),
    ];
    invalid_names.push("x".repeat(256));
    for name in invalid_names {
        assert!(matches!(
            service.create(&fixture.root, &name),
            Err(ProjectError::InvalidName(_))
        ));
    }

    let created = service.create(&fixture.root, "Demo.v1")?;
    assert_eq!(service.current()?, created);
    assert!(matches!(
        service.execute(ProjectCommand::Rename {
            name: "Demo.v1".to_owned(),
        }),
        Err(ProjectError::CommandInvalid(_))
    ));
    assert!(matches!(
        service.execute(ProjectCommand::Rename {
            name: "bad/name".to_owned(),
        }),
        Err(ProjectError::InvalidName(_))
    ));
    Ok(())
}

#[test]
fn opens_invalid_manifests_and_accepts_case_insensitive_extension()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("Demo");
    fs::create_dir_all(&project)?;
    let mut source_service = ProjectService::new();
    let source_project = source_service.create(&fixture.root, "Upper")?;
    let valid_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&source_project.path)?)?;

    let mut service = ProjectService::new();
    assert!(matches!(
        service.open(Path::new("relative.panta")),
        Err(ProjectError::FileMissing(_))
    ));
    assert!(matches!(
        service.open(&fixture.root.join("missing.panta")),
        Err(ProjectError::FileMissing(_))
    ));
    let directory_with_extension = project.join("directory.panta");
    fs::create_dir(&directory_with_extension)?;
    assert!(matches!(
        service.open(&directory_with_extension),
        Err(ProjectError::FileMissing(_))
    ));

    let malformed = project.join("malformed.panta");
    fs::write(&malformed, b"not json")?;
    assert!(matches!(
        service.open(&malformed),
        Err(ProjectError::ManifestInvalid(_))
    ));

    let invalid_name = project.join("invalid-name.panta");
    let mut invalid_manifest = valid_manifest.clone();
    invalid_manifest["name"] = serde_json::json!("bad/name");
    fs::write(&invalid_name, serde_json::to_vec(&invalid_manifest)?)?;
    assert!(matches!(
        service.open(&invalid_name),
        Err(ProjectError::InvalidName(_))
    ));

    let uppercase = project.join("Upper.PANTA");
    let mut uppercase_manifest = valid_manifest;
    uppercase_manifest["revision"] = serde_json::json!(2);
    fs::write(&uppercase, serde_json::to_vec(&uppercase_manifest)?)?;
    let opened = service.open(&uppercase)?;
    assert_eq!(opened.name, "Upper");
    assert_eq!(opened.revision, 2);
    Ok(())
}

#[test]
fn rejects_analysis_sequences_with_missing_imports_or_unknown_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("sequence.stl");
    fs::write(
        &source,
        b"solid sequence\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 0\n   vertex 1 0 0\n   vertex 0 1 0\n  endloop\n endfacet\nendsolid sequence\n",
    )?;
    let mut service = ProjectService::new();
    let project = service.create(&fixture.root, "Invalid sequence")?;
    let imported = service.import_stl(&source, "solid-3d", "millimeters", true)?;
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&project.path)?)?;

    let mut manifest = original.clone();
    manifest["analysis_sequences"] = serde_json::json!({"missing-import": "fill"});
    fs::write(&project.path, serde_json::to_vec(&manifest)?)?;
    assert!(matches!(
        service.open(&project.path),
        Err(ProjectError::ManifestInvalid(_))
    ));

    let mut manifest = original;
    manifest["analysis_sequences"] = serde_json::json!({imported.id: "unknown-sequence"});
    fs::write(&project.path, serde_json::to_vec(&manifest)?)?;
    assert!(matches!(
        service.open(&project.path),
        Err(ProjectError::ManifestInvalid(_))
    ));
    Ok(())
}

#[test]
fn rejects_fill_settings_for_missing_import_without_replacing_current_project()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    let project = service.create(&fixture.root, "Invalid fill settings")?;
    let before = service.current()?;
    let mut manifest: serde_json::Value = serde_json::from_slice(&fs::read(&project.path)?)?;
    manifest["fill_settings"] = serde_json::json!({
        "missing-import": FillSettings::default()
    });
    fs::write(&project.path, serde_json::to_vec(&manifest)?)?;

    let error = service
        .open(&project.path)
        .err()
        .ok_or("fill settings for a missing import were accepted")?;

    assert!(matches!(error, ProjectError::ManifestInvalid(_)));
    assert!(
        error
            .detail()
            .contains("fill settings reference missing import missing-import")
    );
    assert_eq!(service.current()?, before);
    Ok(())
}

#[test]
fn rejects_materials_with_missing_imports_or_unknown_ids() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    let source = fixture.root.join("material.stl");
    fs::write(
        &source,
        b"solid material\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 0\n   vertex 1 0 0\n   vertex 0 1 0\n  endloop\n endfacet\nendsolid material\n",
    )?;
    let mut service = ProjectService::new();
    let project = service.create(&fixture.root, "Invalid material")?;
    let imported = service.import_stl(&source, "solid-3d", "millimeters", true)?;
    let before = service.current()?;
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&project.path)?)?;
    let default_material_id = panta_core::project::default_material_id()?;

    let mut manifest = original.clone();
    manifest["materials"] = serde_json::json!({"missing-import": default_material_id});
    fs::write(&project.path, serde_json::to_vec(&manifest)?)?;
    assert!(matches!(
        service.open(&project.path),
        Err(ProjectError::ManifestInvalid(_))
    ));
    assert_eq!(service.current()?, before);

    let mut manifest = original;
    manifest["materials"] = serde_json::json!({imported.id: "unknown-material"});
    fs::write(&project.path, serde_json::to_vec(&manifest)?)?;
    assert!(matches!(
        service.open(&project.path),
        Err(ProjectError::ManifestInvalid(_))
    ));
    assert_eq!(service.current()?, before);
    Ok(())
}

#[test]
fn imports_ascii_stl_and_round_trips_record_and_asset() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("Case.STL");
    fs::write(
            &source,
            b"solid case\n facet normal 0 0 1\n  outer loop\n   vertex 1 2 3\n   vertex 5 2 3\n   vertex 1 8 6\n  endloop\n endfacet\nendsolid case\n",
        )?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    let record = service.import_stl(&source, "dual-domain", "millimeters", true)?;
    assert_eq!(record.record_version, IMPORT_RECORD_VERSION);
    assert_eq!(record.parser_version, STL_IMPORT_PARSER_VERSION);
    assert_eq!(record.id, "import-1");
    assert_eq!(record.display_name, "Case");
    assert_eq!(record.source_name, "Case.STL");
    assert_eq!(record.mesh_asset_id, "mesh-import-1");
    assert_eq!(record.mesh_type, "dual-domain");
    assert_eq!(record.units, "millimeters");
    assert_eq!(record.triangle_count, 1);
    assert_eq!(record.dimensions, [4.0, 6.0, 3.0]);
    let mesh_asset_path = service.mesh_asset_path_for_import(&record.id)?;
    assert_eq!(mesh_asset_path, "assets/meshes/mesh-import-1-r1.pmesh");
    assert!(fixture.root.join("Demo").join(mesh_asset_path).is_file());
    assert!(!fixture.root.join("Demo/assets/imports").exists());
    assert!(!service.current()?.dirty);

    let project_path = fixture.root.join("Demo/Demo.panta");
    let mut reopened = ProjectService::new();
    reopened.open(&project_path)?;
    assert_eq!(reopened.imports()?, vec![record]);
    // 打开只读清单；已保存资产由只读激活按需恢复（见 activation.rs）。
    assert!(reopened.current_mesh().is_none());
    Ok(())
}

#[test]
fn rejects_legacy_schemas_without_mutating_project_or_assets()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("part.stl");
    fs::write(&source, b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    let project = service.create(&fixture.root, "No migration")?;
    let record = service.import_stl(&source, "dual-domain", "millimeters", false)?;
    let asset_path = fixture
        .root
        .join("No migration")
        .join(service.mesh_asset_path_for_import(&record.id)?);
    let current = service.current()?;
    let current_manifest: serde_json::Value = serde_json::from_slice(&fs::read(&project.path)?)?;

    for schema in 1..PROJECT_SCHEMA_VERSION {
        let mut manifest = current_manifest.clone();
        manifest["schema"] = serde_json::json!(schema);
        // 旧版记录可能缺少当前 schema 4 的必填字段；版本应先于记录解析被拒绝。
        manifest["imports"][0]
            .as_object_mut()
            .ok_or("import record was not an object")?
            .remove("mesh_asset_id");
        fs::write(&project.path, serde_json::to_vec(&manifest)?)?;
        let unsupported_bytes = fs::read(&project.path)?;

        assert!(matches!(
            service.open(&project.path),
            Err(ProjectError::UnsupportedSchema(found)) if found == schema
        ));
        assert_eq!(fs::read(&project.path)?, unsupported_bytes);
        assert!(asset_path.is_file());
        assert_eq!(service.current()?, current);
    }
    Ok(())
}

#[test]
fn rejects_invalid_current_import_records_without_replacing_current_project()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("part.stl");
    fs::write(&source, b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    let created = service.create(&fixture.root, "Invalid current manifest")?;
    service.import_stl(&source, "solid-3d", "millimeters", false)?;
    let before = service.current()?;
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&created.path)?)?;

    for invalid in ["record_version", "display_name", "duplicate_id"] {
        let mut manifest = original.clone();
        match invalid {
            "record_version" => manifest["imports"][0]["record_version"] = serde_json::json!(99),
            "display_name" => {
                manifest["imports"][0]
                    .as_object_mut()
                    .ok_or("import record was not an object")?
                    .remove("display_name");
            }
            "duplicate_id" => {
                let import = manifest["imports"][0].clone();
                manifest["imports"]
                    .as_array_mut()
                    .ok_or("imports were not an array")?
                    .push(import);
            }
            _ => unreachable!(),
        }
        fs::write(&created.path, serde_json::to_vec(&manifest)?)?;

        assert!(
            matches!(
                service.open(&created.path),
                Err(ProjectError::ManifestInvalid(_))
            ),
            "{invalid} unexpectedly opened"
        );
        assert_eq!(
            service.current()?,
            before,
            "{invalid} replaced active state"
        );
    }
    Ok(())
}

#[test]
fn imports_with_same_source_name_keep_the_same_display_name()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("part.stl");
    fs::write(&source, b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Duplicates")?;
    let first = service.import_stl(&source, "dual-domain", "millimeters", false)?;
    let second = service.import_stl(&source, "dual-domain", "millimeters", false)?;
    assert_eq!(first.display_name, "part");
    assert_eq!(second.display_name, "part");
    assert_ne!(first.id, second.id);
    assert_ne!(first.mesh_asset_id, second.mesh_asset_id);
    Ok(())
}

#[test]
fn import_asset_collision_uses_a_recovery_path_without_overwriting()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("part.stl");
    fs::write(&source, b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    let created = service.create(&fixture.root, "Demo")?;
    let asset_dir = fixture.root.join("Demo/assets/meshes");
    fs::create_dir_all(&asset_dir)?;
    let occupied_asset = asset_dir.join("mesh-import-1-r1.pmesh");
    fs::write(&occupied_asset, b"preserve this unrelated file")?;
    let imported = service.import_stl(&source, "solid-3d", "millimeters", false)?;
    assert_eq!(imported.mesh_asset_id, "mesh-import-1-recovery-1");
    assert_eq!(fs::read(&occupied_asset)?, b"preserve this unrelated file");
    assert_eq!(service.imports()?.len(), 1);
    assert!(service.current_mesh().is_some());
    assert_ne!(fs::read(&created.path)?, b"");
    Ok(())
}

#[test]
fn import_directory_failure_preserves_the_project() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("part.stl");
    fs::write(&source, b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    let created = service.create(&fixture.root, "Demo")?;
    let blocking_file = fixture.root.join("Demo/assets");
    fs::write(&blocking_file, b"preserve this file")?;
    let before = service.current()?;
    let manifest_before = fs::read(&created.path)?;

    assert!(matches!(
        service.import_stl(&source, "solid-3d", "millimeters", false),
        Err(ProjectError::ImportAssetWriteFailed(_))
    ));
    assert_eq!(fs::read(&blocking_file)?, b"preserve this file");
    assert_eq!(service.current()?, before);
    assert!(service.imports()?.is_empty());
    assert!(service.current_mesh().is_none());
    assert_eq!(fs::read(&created.path)?, manifest_before);
    Ok(())
}

#[cfg(unix)]
#[test]
fn import_rejects_mesh_directory_symlink_escape() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new()?;
    let source = fixture.root.join("part.stl");
    fs::write(&source, b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    let created = service.create(&fixture.root, "Demo")?;
    let project_root = fixture.root.join("Demo");
    let outside = fixture.root.join("outside-meshes");
    fs::create_dir_all(project_root.join("assets"))?;
    fs::create_dir(&outside)?;
    symlink(&outside, project_root.join("assets/meshes"))?;
    let manifest_before = fs::read(&created.path)?;

    assert!(
        service
            .import_stl(&source, "solid-3d", "millimeters", false)
            .is_err()
    );
    assert!(service.imports()?.is_empty());
    assert_eq!(fs::read(&created.path)?, manifest_before);
    assert!(fs::read_dir(&outside)?.next().is_none());
    Ok(())
}

#[test]
fn import_recovers_from_an_orphaned_sidecar_with_different_content()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let first_source = fixture.root.join("first.stl");
    fs::write(&first_source, b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let second_source = fixture.root.join("second.stl");
    fs::write(
        &second_source,
        b"vertex 0 0 0\nvertex 2 0 0\nvertex 0 1 0\n",
    )?;

    let mut service = ProjectService::new();
    let created = service.create(&fixture.root, "Orphan recovery")?;
    let original = service.import_stl(&first_source, "solid-3d", "millimeters", false)?;
    let project_root = fixture.root.join("Orphan recovery");
    let original_path = project_root.join("assets/meshes/mesh-import-1-r1.pmesh");
    assert!(original_path.is_file());
    drop(service);

    let mut manifest: serde_json::Value = serde_json::from_slice(&fs::read(&created.path)?)?;
    manifest["imports"] = serde_json::json!([]);
    manifest["mesh_assets"] = serde_json::json!({});
    fs::write(&created.path, serde_json::to_vec_pretty(&manifest)?)?;

    let mut reopened = ProjectService::new();
    reopened.open(&created.path)?;
    let recovered = reopened.import_stl(&second_source, "solid-3d", "millimeters", false)?;

    assert_eq!(recovered.id, original.id);
    assert_eq!(recovered.mesh_asset_id, "mesh-import-1-recovery-1");
    assert!(original_path.is_file());
    let persisted: serde_json::Value = serde_json::from_slice(&fs::read(&created.path)?)?;
    let relative = persisted["mesh_assets"][&recovered.mesh_asset_id]["relative_path"]
        .as_str()
        .ok_or("recovered mesh asset path missing")?;
    let recovered_path = project_root.join(relative);
    assert!(recovered_path.is_file());
    assert_ne!(fs::read(original_path)?, fs::read(recovered_path)?);
    Ok(())
}

#[test]
fn changed_preview_requires_review_and_failed_import_preserves_mesh()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("changing.stl");
    fs::write(&source, b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    assert_eq!(service.inspect_stl(&source)?.dimensions, [1.0, 1.0, 0.0]);
    fs::write(&source, b"vertex 0 0 0\nvertex 2 0 0\nvertex 0 1 0\n")?;
    assert!(matches!(
        service.import_stl(&source, "solid-3d", "millimeters", false),
        Err(ProjectError::ImportSourceChanged(_))
    ));
    assert_eq!(service.current()?.revision, 0);
    assert!(service.current_mesh().is_none());

    service.inspect_stl(&source)?;
    service.import_stl(&source, "solid-3d", "centimeters", false)?;
    assert_eq!(
        service.current_mesh().map(|mesh| mesh.summary().dimensions),
        Some([20.0, 10.0, 0.0])
    );
    let committed = service.current_mesh().cloned();
    let revision = service.current()?.revision;
    fs::write(&source, b"not an stl")?;
    assert!(matches!(
        service.import_stl(&source, "solid-3d", "millimeters", false),
        Err(ProjectError::ImportParseFailed(_))
    ));
    assert_eq!(service.current()?.revision, revision);
    assert_eq!(service.current_mesh().cloned(), committed);
    let mut reopened = ProjectService::new();
    reopened.open(&fixture.root.join("Demo/Demo.panta"))?;
    assert!(reopened.current_mesh().is_none());

    let second = fixture.root.join("second.stl");
    fs::write(&second, b"vertex 0 0 0\nvertex 3 0 0\nvertex 0 4 0\n")?;
    let second_record = service.import_stl(&second, "solid-3d", "millimeters", false)?;
    assert_eq!(
        service.current_mesh().map(|mesh| mesh.summary().dimensions),
        Some([3.0, 4.0, 0.0])
    );
    assert_eq!(service.imports()?.len(), 2);
    assert!(!fixture.root.join("Demo/assets/imports").exists());
    assert!(
        fixture
            .root
            .join("Demo")
            .join(service.mesh_asset_path_for_import(&second_record.id)?)
            .is_file()
    );
    reopened.open(&fixture.root.join("Demo/Demo.panta"))?;
    assert!(reopened.current_mesh().is_none());
    Ok(())
}

#[test]
fn converted_coordinate_overflow_leaves_project_unchanged() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    let source = fixture.root.join("large.stl");
    fs::write(
        &source,
        format!("vertex {} 0 0\nvertex 0 0 0\nvertex 0 1 0\n", f64::MAX),
    )?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    assert!(matches!(
        service.import_stl(&source, "solid-3d", "inches", false),
        Err(ProjectError::ImportParseFailed(_))
    ));
    assert_eq!(service.current()?.revision, 0);
    assert!(service.imports()?.is_empty());
    assert!(service.current_mesh().is_none());
    Ok(())
}

#[test]
fn inspects_binary_stl() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("Binary.STL");
    let vertices = [[0.0_f32, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 4.0]];
    let mut bytes = vec![0_u8; 84 + 50];
    bytes[80..84].copy_from_slice(&1_u32.to_le_bytes());
    for (index, vertex) in vertices.into_iter().enumerate() {
        let offset = 84 + 12 + index * 12;
        for (axis, value) in vertex.into_iter().enumerate() {
            let value_offset = offset + axis * 4;
            bytes[value_offset..value_offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    fs::write(&source, bytes)?;

    let preview = ProjectService::new().inspect_stl(&source)?;
    assert_eq!(preview.source_name, "Binary.STL");
    assert_eq!(preview.triangle_count, 1);
    assert_eq!(preview.dimensions, [2.0, 3.0, 4.0]);
    Ok(())
}

#[test]
fn failed_manifest_commit_preserves_previous_asset_and_mesh()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("part.stl");
    fs::write(&source, b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    let created = service.create(&fixture.root, "Demo")?;
    service.import_stl(&source, "solid-3d", "millimeters", false)?;
    let snapshot = service.current()?;
    let mesh = service.current_mesh().cloned();
    let manifest = fs::read(&created.path)?;
    fs::remove_dir(created.path.with_extension("panta.write"))?;
    fs::write(
        created.path.with_extension("panta.write"),
        b"blocked staging directory",
    )?;
    assert!(
        service
            .import_stl(&source, "solid-3d", "millimeters", false)
            .is_err()
    );
    assert_eq!(service.current()?, snapshot);
    assert_eq!(service.current_mesh().cloned(), mesh);
    assert_eq!(service.imports()?.len(), 1);
    assert_eq!(fs::read(&created.path)?, manifest);
    assert!(
        !fixture
            .root
            .join("Demo/assets/meshes/mesh-import-2-r1.pmesh")
            .exists()
    );
    Ok(())
}

#[test]
fn rejects_invalid_stl_import_requests_without_project_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("case.stl");
    fs::write(&source, b"not an stl")?;
    let valid_source = fixture.root.join("valid.stl");
    fs::write(
        &valid_source,
        b"solid part\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid\n",
    )?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    let revision = service.current()?.revision;
    assert!(matches!(
        service.import_stl(&source, "dual-domain", "millimeters", false),
        Err(ProjectError::ImportParseFailed(_))
    ));
    assert!(matches!(
        service.import_stl(&valid_source, "unknown", "millimeters", false),
        Err(ProjectError::ImportUnsupportedMeshType(_))
    ));
    assert!(matches!(
        service.import_stl(&valid_source, "dual-domain", "unknown", false),
        Err(ProjectError::ImportUnsupportedUnits(_))
    ));
    assert!(matches!(
        service.import_stl(
            &fixture.root.join("case.obj"),
            "dual-domain",
            "millimeters",
            false
        ),
        Err(ProjectError::ImportFileMissing(_))
    ));
    assert_eq!(service.current()?.revision, revision);
    assert!(service.imports()?.is_empty());
    Ok(())
}

#[test]
fn handles_manifest_revision_limits_and_save_io_failures() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    let project = fixture.root.join("Max");
    fs::create_dir_all(&project)?;
    let project_file = project.join("Max.panta");
    fs::write(
        &project_file,
        format!(
            r#"{{"schema":{},"name":"Max","revision":{}}}"#,
            PROJECT_SCHEMA_VERSION,
            u64::MAX - 1
        ),
    )?;
    let mut service = ProjectService::new();
    service.open(&project_file)?;
    let snapshot = service.execute(ProjectCommand::Rename {
        name: "MaxRenamed".to_owned(),
    })?;
    assert_eq!(snapshot.revision, u64::MAX);
    assert!(matches!(
        service.execute(ProjectCommand::Rename {
            name: "Again".to_owned()
        }),
        Err(ProjectError::CommandInvalid(_))
    ));
    assert_eq!(service.current()?, snapshot);

    fs::remove_file(&project_file)?;
    fs::create_dir(&project_file)?;
    assert!(matches!(service.save(), Err(ProjectError::Io(_))));
    Ok(())
}

#[test]
fn error_display_uses_stable_codes_and_details() {
    let errors = [
        (ProjectError::EmptyName, "project.empty_name"),
        (
            ProjectError::InvalidName("bad".to_owned()),
            "project.invalid_name: bad",
        ),
        (ProjectError::LocationEmpty, "project.location_empty"),
        (
            ProjectError::LocationNotAbsolute("relative".to_owned()),
            "project.location_not_absolute: relative",
        ),
        (
            ProjectError::LocationCreateFailed("location".to_owned()),
            "project.location_create_failed: location",
        ),
        (
            ProjectError::FileMissing("missing".to_owned()),
            "project.file_missing: missing",
        ),
        (
            ProjectError::InvalidFile("file".to_owned()),
            "project.invalid_file: file",
        ),
        (
            ProjectError::AlreadyExists("existing".to_owned()),
            "project.already_exists: existing",
        ),
        (
            ProjectError::ManifestInvalid("manifest".to_owned()),
            "project.manifest_invalid: manifest",
        ),
        (
            ProjectError::UnsupportedSchema(7),
            "project.unsupported_schema: 7",
        ),
        (ProjectError::NoProject, "project.no_project"),
        (
            ProjectError::CommandInvalid("unchanged".to_owned()),
            "project.command_invalid: unchanged",
        ),
        (
            ProjectError::ImportFileMissing("missing.stl".to_owned()),
            "project.import_file_missing: missing.stl",
        ),
        (
            ProjectError::ImportInvalidFile("mesh.obj".to_owned()),
            "project.import_invalid_file: mesh.obj",
        ),
        (
            ProjectError::ImportUnsupportedMeshType("shell".to_owned()),
            "project.import_unsupported_mesh_type: shell",
        ),
        (
            ProjectError::ImportUnsupportedUnits("unknown".to_owned()),
            "project.import_unsupported_units: unknown",
        ),
        (
            ProjectError::ImportParseFailed("invalid".to_owned()),
            "project.import_parse_failed: invalid",
        ),
        (
            ProjectError::ImportAssetWriteFailed("write".to_owned()),
            "project.import_asset_write_failed: write",
        ),
        (ProjectError::Io("disk".to_owned()), "project.io: disk"),
    ];
    for (error, expected) in errors {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn gate_location_manifest_rejects_missing_import_and_invalid_settings()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    let project = service.create(&fixture.root, "Gate")?;
    let source = fixture.root.join("part.stl");
    fs::write(
        &source,
        b"solid part\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid\n",
    )?;
    let imported = service.import_stl(&source, "solid-3d", "millimeters", false)?;
    let before = service.current()?;
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&project.path)?)?;
    let mut manifest = original.clone();
    manifest["gate_location_settings"] =
        serde_json::json!({"missing": panta_core::project::GateLocationSettings::default()});
    fs::write(&project.path, serde_json::to_vec(&manifest)?)?;
    assert!(service.open(&project.path).is_err());
    assert_eq!(service.current()?, before);
    let mut manifest = original;
    let invalid = panta_core::project::GateLocationSettings {
        number_of_gates: 0,
        ..Default::default()
    };
    manifest["gate_location_settings"] = serde_json::json!({imported.id: invalid});
    fs::write(&project.path, serde_json::to_vec(&manifest)?)?;
    assert!(service.open(&project.path).is_err());
    assert_eq!(service.current()?, before);
    Ok(())
}

fn finish_preview(
    service: &mut ProjectService,
    request: u64,
) -> Result<panta_core::project::StlImportPreview, ProjectError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(preview) = service.finish_stl_preview(request)? {
            return Ok(preview);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "STL preview timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn asynchronous_preview_retains_source_snapshot_and_preserves_committed_state()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let path = fixture.root.join("零件.stl");
    fs::write(&path, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    let project = service.create(&fixture.root, "Demo")?;
    let manifest = fs::read(&project.path)?;
    let request = service.begin_stl_preview(&path)?;
    // 即使 worker 已就绪，必须先消费预检结果，不能绕过已显示的来源快照。
    assert_eq!(
        service
            .import_stl(&path, "solid-3d", "millimeters", false)
            .err()
            .map(|error| error.code()),
        Some("project.command_invalid")
    );
    let preview = finish_preview(&mut service, request)?;
    assert_eq!(preview.source_name, "零件.stl");
    assert_eq!(preview.dimensions, [1.0, 1.0, 0.0]);
    assert_eq!(fs::read(&project.path)?, manifest);
    assert!(service.imports()?.is_empty());
    fs::write(&path, "vertex 0 0 0\nvertex 2 0 0\nvertex 0 1 0\n")?;
    assert_eq!(
        service
            .import_stl(&path, "solid-3d", "millimeters", false)
            .err()
            .map(|error| error.code()),
        Some("project.import_source_changed")
    );
    assert_eq!(fs::read(&project.path)?, manifest);
    let request = service.begin_stl_preview(&path)?;
    assert_eq!(
        finish_preview(&mut service, request)?.dimensions,
        [2.0, 1.0, 0.0]
    );
    service.import_stl(&path, "solid-3d", "millimeters", false)?;
    assert_eq!(service.imports()?.len(), 1);
    Ok(())
}

#[test]
fn asynchronous_preview_rejects_replaced_cancelled_and_old_project_requests()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let path = fixture.root.join("sample.stl");
    fs::write(&path, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    let first = service.begin_stl_preview(&path)?;
    assert!(service.cancel_stl_preview(first));
    assert!(service.finish_stl_preview(first).is_err());
    let second = service.begin_stl_preview(&path)?;
    let third = service.begin_stl_preview(&path)?;
    assert!(third > second);
    assert!(!service.cancel_stl_preview(second));
    assert!(service.finish_stl_preview(second).is_err());
    assert_eq!(finish_preview(&mut service, third)?.triangle_count, 1);
    let previous = service.begin_stl_preview(&path)?;
    service.create(&fixture.root, "Demo")?;
    assert!(service.finish_stl_preview(previous).is_err());
    let failed = service.begin_stl_preview(&fixture.root.join("missing.stl"))?;
    let error = finish_preview(&mut service, failed)
        .err()
        .ok_or("missing STL unexpectedly succeeded")?;
    assert_eq!(error.code(), "project.import_file_missing");
    assert_eq!(error.category(), "missing");
    assert!(service.imports()?.is_empty());
    let next = service.begin_stl_preview(&path)?;
    assert_eq!(finish_preview(&mut service, next)?.triangle_count, 1);
    assert!(service.cancel_stl_preview(next));
    Ok(())
}

#[test]
fn exhausted_revision_rejects_mutations_without_writing_assets_or_manifest()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("sample.stl");
    fs::write(&source, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    let project = service.create(&fixture.root, "Exhausted")?;
    service.import_stl(&source, "solid-3d", "millimeters", false)?;
    let mut manifest: serde_json::Value = serde_json::from_slice(&fs::read(&project.path)?)?;
    manifest["revision"] = serde_json::Value::from(u64::MAX);
    fs::write(&project.path, serde_json::to_vec(&manifest)?)?;
    service.open(&project.path)?;
    let before = service.current()?;
    let bytes = fs::read(&project.path)?;
    let assets = project
        .path
        .parent()
        .ok_or("missing package root")?
        .join("assets/meshes");
    let asset_files = || -> Result<Vec<_>, std::io::Error> {
        let mut files = fs::read_dir(&assets)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        files.sort();
        Ok(files)
    };
    let existing_assets = asset_files()?;
    for error in [
        service
            .execute(ProjectCommand::Rename {
                name: "Changed".into(),
            })
            .err(),
        service
            .import_stl(&source, "solid-3d", "millimeters", false)
            .err(),
    ] {
        let error = error.ok_or("exhausted revision accepted a mutation")?;
        assert_eq!(error.code(), "project.command_invalid");
        assert_eq!(error.detail(), "project revision exhausted");
    }
    assert_eq!(service.current()?, before);
    assert_eq!(fs::read(&project.path)?, bytes);
    assert_eq!(service.imports()?.len(), 1);
    assert_eq!(asset_files()?, existing_assets);
    // 保存没有变更的最大修订仍是合法操作。
    assert_eq!(service.save()?, before);
    Ok(())
}

#[test]
fn schema_four_metadata_survives_open_rename_and_background_save()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("sample.stl");
    fs::write(&source, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    let mut service = ProjectService::new();
    let project = service.create(&fixture.root, "Metadata")?;
    let record = service.import_stl(&source, "solid-3d", "millimeters", false)?;
    let mut original: serde_json::Value = serde_json::from_slice(&fs::read(&project.path)?)?;
    original["analysis_sequences"] = serde_json::json!({record.id.clone(): "gate-location"});
    original["materials"] =
        serde_json::json!({record.id.clone(): panta_core::project::default_material_id()?});
    original["fill_settings"] = serde_json::json!({record.id.clone(): FillSettings::default()});
    original["gate_location_settings"] =
        serde_json::json!({record.id: panta_core::project::GateLocationSettings::default()});
    fs::write(&project.path, serde_json::to_vec(&original)?)?;
    service.open(&project.path)?;
    service.execute(ProjectCommand::Rename {
        name: "Renamed".into(),
    })?;
    let completion = service
        .begin_project_operation(panta_core::project::ProjectOperation::Save)?
        .wait();
    service.finish_project_operation(completion)?;
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&project.path)?)?;
    for key in [
        "schema",
        "imports",
        "mesh_assets",
        "analysis_sequences",
        "materials",
        "fill_settings",
        "gate_location_settings",
    ] {
        assert_eq!(saved[key], original[key], "changed schema 4 field {key}");
    }
    assert_eq!(saved["name"], "Renamed");
    assert_eq!(
        saved["revision"].as_u64(),
        original["revision"].as_u64().map(|revision| revision + 1)
    );
    service.open(&project.path)?;
    assert_eq!(service.imports()?.len(), 1);
    Ok(())
}

#[test]
fn stale_services_and_parallel_saves_cannot_overwrite_a_committed_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut owner = ProjectService::new();
    let project = owner.create(&fixture.root, "Concurrent")?;
    let mut stale = ProjectService::new();
    stale.open(&project.path)?;
    owner.execute(ProjectCommand::Rename {
        name: "Owner".into(),
    })?;
    owner.save()?;
    let committed = fs::read(&project.path)?;
    stale.execute(ProjectCommand::Rename {
        name: "Stale".into(),
    })?;
    let before = stale.current()?;
    let result = stale.save();
    assert!(matches!(result, Err(ProjectError::CommandInvalid(_))));
    assert_eq!(stale.current()?, before);
    assert_eq!(fs::read(&project.path)?, committed);
    let source = fixture.root.join("part.stl");
    fs::write(&source, "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
    assert!(matches!(
        stale.import_stl(&source, "solid-3d", "millimeters", false),
        Err(ProjectError::CommandInvalid(_))
    ));
    assert!(
        !project
            .path
            .parent()
            .ok_or("no package root")?
            .join("assets")
            .exists()
    );

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut handles = Vec::new();
    for name in ["First", "Second"] {
        let mut service = ProjectService::new();
        service.open(&project.path)?;
        service.execute(ProjectCommand::Rename { name: name.into() })?;
        let barrier = std::sync::Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            service.save()
        }));
    }
    let mut successes = 0;
    for handle in handles {
        match handle.join().map_err(|_| "save worker panicked")? {
            Ok(_) => successes += 1,
            Err(ProjectError::CommandInvalid(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    assert_eq!(successes, 1);
    owner.open(&project.path)?;
    assert_eq!(owner.current()?.revision, 2);
    assert!(matches!(owner.current()?.name.as_str(), "First" | "Second"));
    assert_eq!(
        fs::read_dir(project.path.with_extension("panta.write"))?
            .collect::<Result<Vec<_>, _>>()?
            .len(),
        0
    );
    Ok(())
}

#[test]
fn project_lease_child() -> Result<(), Box<dyn std::error::Error>> {
    let Some(path) = std::env::var_os("PANTA_TEST_PROJECT_LEASE") else {
        return Ok(());
    };
    let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
    fs4::FileExt::try_lock(&file)?;
    use std::io::Write;
    println!("LEASE_HELD");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    // 不运行 Rust 析构，验证操作系统在进程退出时释放锁。
    std::process::exit(0);
}

#[test]
fn project_write_lock_is_nonblocking_and_released_after_process_exit()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    let project = service.create(&fixture.root, "Lease")?;
    service.execute(ProjectCommand::Rename {
        name: "Saved".into(),
    })?;
    let before = service.current()?;
    let mut child = Command::new(std::env::current_exe()?)
        .args(["--exact", "project_lease_child", "--nocapture"])
        .env(
            "PANTA_TEST_PROJECT_LEASE",
            project.path.with_extension("panta.lock"),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("missing child stdout")?;
    let mut ready = false;
    for line in BufReader::new(stdout).lines() {
        if line? == "LEASE_HELD" {
            ready = true;
            break;
        }
    }
    assert!(ready, "child did not acquire the lease");
    match service.save() {
        Err(error) => assert_eq!(error.detail(), "project write pending"),
        Ok(_) => panic!("save succeeded while another process held the lease"),
    }
    assert_eq!(service.current()?, before);
    drop(child.stdin.take());
    assert!(child.wait()?.success());
    service.save()?;
    let mut reopened = ProjectService::new();
    reopened.open(&project.path)?;
    assert_eq!(reopened.current()?.name, "Saved");
    Ok(())
}

#[test]
fn background_write_shutdown_child() -> Result<(), Box<dyn std::error::Error>> {
    let Some(root) = std::env::var_os("PANTA_TEST_WRITE_SHUTDOWN") else {
        return Ok(());
    };
    let root = std::path::PathBuf::from(root);
    let mut service = ProjectService::new();
    let project = service.create(&root, "Shutdown")?;
    service.import_stl(&root.join("shutdown.stl"), "solid-3d", "millimeters", false)?;
    service.execute(ProjectCommand::Rename {
        name: "Saved at shutdown".into(),
    })?;
    let expected = service.current()?;
    let _request = service.begin_project_operation(panta_core::project::ProjectOperation::Save)?;
    drop(service);
    panta_core::finish_background_writes();
    let mut reopened = ProjectService::new();
    reopened.open(&project.path)?;
    let saved = reopened.current()?;
    assert_eq!(saved.name, expected.name);
    assert_eq!(saved.revision, expected.revision);
    assert!(!saved.dirty);
    assert_eq!(reopened.imports()?.len(), 1);
    Ok(())
}

#[test]
fn application_shutdown_finishes_accepted_writes_after_service_teardown()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    fs::write(
        fixture.root.join("shutdown.stl"),
        "vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n",
    )?;
    let result = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "background_write_shutdown_child", "--nocapture"])
        .env("PANTA_TEST_WRITE_SHUTDOWN", &fixture.root)
        .output()?;
    assert!(
        result.status.success(),
        "shutdown child failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
