//! 只读 STL 资产激活的集成行为：成功、失败、取消、工程切换失效、
//! 去重与工程持久状态不变式。经 `ProjectService` 公共 API 驱动，worker 为
//! 真实线程；结果以 `drain_asset_activations` 轮询拉取。
use panta_core::project::{CompletedActivation, OutcomeKind, ProjectError, ProjectService};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[path = "../support/rust/temp_directory.rs"]
mod temp_directory;
use temp_directory::Fixture;

const SAMPLE_STL: &[u8] = b"vertex 0 0 0\nvertex 2 0 0\nvertex 0 3 0\n";
const IN_FLIGHT_TRIANGLES: usize = 350_000;

fn import_sample_stl(
    service: &mut ProjectService,
    root: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let source = root.join("part.stl");
    fs::write(&source, SAMPLE_STL)?;
    let record = service.import_stl(&source, "solid-3d", "millimeters", false)?;
    service.save()?;
    Ok(record.id)
}

/// 大型有效 sidecar 让校验和解码持续足够久，覆盖激活的取消与失效路径。
fn overwrite_asset_with_large_mesh(
    service: &ProjectService,
    record_id: &str,
    triangles: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let imports = service.imports()?;
    let record = imports
        .iter()
        .find(|record| record.id == record_id)
        .ok_or("import record not found")?;
    let path = asset_path(service, record_id)?;
    let mesh = panta_mesh::SurfaceMesh {
        triangles: vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]; triangles],
    };
    let (bytes, digest) = panta_mesh::encode_mesh_asset(&panta_mesh::MeshAsset::Surface(mesh))?;
    fs::write(&path, bytes)?;

    let project_path = &service.current()?.path;
    let mut manifest: serde_json::Value = serde_json::from_slice(&fs::read(project_path)?)?;
    let asset = manifest["mesh_assets"]
        .get_mut(&record.mesh_asset_id)
        .ok_or("mesh asset index entry missing")?;
    asset["digest"] = serde_json::Value::String(digest);
    let persisted_record = manifest["imports"]
        .as_array_mut()
        .and_then(|imports| imports.iter_mut().find(|value| value["id"] == record_id))
        .ok_or("persisted import record missing")?;
    persisted_record["triangle_count"] = serde_json::Value::from(triangles as u64);
    fs::write(project_path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}

fn asset_path(
    service: &ProjectService,
    record_id: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let root = service
        .current()?
        .path
        .parent()
        .ok_or("no project dir")?
        .to_path_buf();
    // 清单内相对路径使用 '/'；Path::join 在各平台均接受该分隔符。
    Ok(root.join(service.mesh_asset_path_for_import(record_id)?))
}

fn wait_for_outcome(
    service: &mut ProjectService,
    timeout: Duration,
) -> Result<CompletedActivation, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        let outcomes = service.drain_asset_activations();
        if let Some(outcome) = outcomes.into_iter().next() {
            return Ok(outcome);
        }
        if Instant::now() >= deadline {
            return Err("activation outcome did not arrive in time".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn activation_restores_saved_asset_without_touching_manifest()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    let record_id = import_sample_stl(&mut service, &fixture.root)?;
    let manifest_before = fs::read(service.current()?.path.clone())?;
    let revision_before = service.current()?.revision;
    drop(service);

    let mut reopened = ProjectService::new();
    reopened.open(&fixture.root.join("Demo/Demo.panta"))?;
    // 打开不再同步解析资产；网格只能经只读激活恢复。
    assert!(reopened.current_mesh().is_none());
    let attempt = reopened.begin_asset_activation(&record_id)?;
    let outcome = wait_for_outcome(&mut reopened, Duration::from_secs(10))?;
    assert_eq!(outcome.kind, OutcomeKind::Succeeded);
    assert_eq!(outcome.attempt, attempt.attempt);
    assert_eq!(outcome.generation, attempt.generation);
    assert_eq!(outcome.import_id, record_id);
    let mesh = outcome.mesh.ok_or("success must carry mesh")?;
    assert_eq!(mesh.summary().dimensions, [2.0, 3.0, 0.0]);

    // 加载路径不推进修订、不标脏、不改写清单。
    assert_eq!(reopened.current()?.revision, revision_before);
    assert!(!reopened.current()?.dirty);
    assert_eq!(fs::read(reopened.current()?.path.clone())?, manifest_before);
    Ok(())
}

#[test]
fn unknown_record_and_missing_project_fail_synchronously() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    assert!(matches!(
        service.begin_asset_activation("import-1"),
        Err(ProjectError::NoProject)
    ));
    service.create(&fixture.root, "Demo")?;
    assert!(matches!(
        service.begin_asset_activation("import-99"),
        Err(ProjectError::ImportRecordMissing(record)) if record == "import-99"
    ));
    Ok(())
}

#[test]
fn corrupt_asset_fails_with_stable_code() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    let record_id = import_sample_stl(&mut service, &fixture.root)?;
    let asset = asset_path(&service, &record_id)?;
    fs::write(&asset, b"this is not a stl payload")?;
    let mut reopened = ProjectService::new();
    reopened.open(&fixture.root.join("Demo/Demo.panta"))?;
    reopened.begin_asset_activation(&record_id)?;
    let outcome = wait_for_outcome(&mut reopened, Duration::from_secs(10))?;
    assert_eq!(outcome.kind, OutcomeKind::Failed);
    assert_eq!(outcome.code, "project.asset_unsupported_format");
    Ok(())
}

#[test]
fn missing_asset_file_fails_with_stable_code() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    let record_id = import_sample_stl(&mut service, &fixture.root)?;
    fs::remove_file(asset_path(&service, &record_id)?)?;
    let mut reopened = ProjectService::new();
    reopened.open(&fixture.root.join("Demo/Demo.panta"))?;
    reopened.begin_asset_activation(&record_id)?;
    let outcome = wait_for_outcome(&mut reopened, Duration::from_secs(10))?;
    assert_eq!(outcome.kind, OutcomeKind::Failed);
    assert_eq!(outcome.code, "project.asset_missing");
    Ok(())
}

#[test]
fn cancel_request_terminates_as_cancelled() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    let record_id = import_sample_stl(&mut service, &fixture.root)?;
    overwrite_asset_with_large_mesh(&service, &record_id, IN_FLIGHT_TRIANGLES)?;

    let mut reopened = ProjectService::new();
    reopened.open(&fixture.root.join("Demo/Demo.panta"))?;
    let attempt = reopened.begin_asset_activation(&record_id)?;
    assert!(reopened.cancel_asset_activation(attempt.attempt));
    let outcome = wait_for_outcome(&mut reopened, Duration::from_secs(10))?;
    assert_eq!(outcome.kind, OutcomeKind::Cancelled);
    assert!(outcome.mesh.is_none());
    assert!(
        !reopened.cancel_asset_activation(attempt.attempt),
        "终态后取消必须返回 false"
    );
    Ok(())
}

#[test]
fn project_switch_invalidates_in_flight_results() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    let record_id = import_sample_stl(&mut service, &fixture.root)?;
    overwrite_asset_with_large_mesh(&service, &record_id, IN_FLIGHT_TRIANGLES)?;

    let mut reopened = ProjectService::new();
    reopened.open(&fixture.root.join("Demo/Demo.panta"))?;
    reopened.begin_asset_activation(&record_id)?;
    // 切换工程（新会话代次）：旧 attempt 立即失效，结果不跨会话发布。
    reopened.create(&fixture.root, "Other")?;
    assert!(reopened.drain_asset_activations().is_empty());
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        reopened.drain_asset_activations().is_empty(),
        "迟到成功不得覆盖新工程会话"
    );
    Ok(())
}

#[test]
fn duplicate_begin_reuses_in_flight_attempt() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    let record_id = import_sample_stl(&mut service, &fixture.root)?;
    overwrite_asset_with_large_mesh(&service, &record_id, IN_FLIGHT_TRIANGLES)?;

    let mut reopened = ProjectService::new();
    reopened.open(&fixture.root.join("Demo/Demo.panta"))?;
    let first = reopened.begin_asset_activation(&record_id)?;
    let second = reopened.begin_asset_activation(&record_id)?;
    assert_eq!(first.attempt, second.attempt, "同会话同记录必须去重");
    let outcome = wait_for_outcome(&mut reopened, Duration::from_secs(10))?;
    assert_eq!(outcome.kind, OutcomeKind::Succeeded);
    assert_eq!(outcome.attempt, first.attempt);
    assert!(
        reopened.drain_asset_activations().is_empty(),
        "去重后只有一个终态结果"
    );
    Ok(())
}

#[test]
fn manifest_asset_traversal_is_rejected_at_activation_admission()
-> Result<(), Box<dyn std::error::Error>> {
    use panta_core::project::ProjectError;

    let fixture = Fixture::new()?;
    let mut service = ProjectService::new();
    service.create(&fixture.root, "Demo")?;
    let record_id = import_sample_stl(&mut service, &fixture.root)?;
    service.save()?;
    let record = service
        .imports()?
        .into_iter()
        .find(|record| record.id == record_id)
        .ok_or("import record not found")?;
    let manifest_path = fixture.root.join("Demo/Demo.panta");
    drop(service);

    // schema 4 的 mesh index 路径必须保持在工程包内。
    for evil in ["../../evil.pmesh", "/absolutely/external.pmesh"] {
        let mut manifest: serde_json::Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        manifest["mesh_assets"][&record.mesh_asset_id]["relative_path"] =
            serde_json::Value::String(evil.to_owned());
        fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
        let mut reopened = ProjectService::new();
        assert!(matches!(
            reopened.open(&manifest_path),
            Err(ProjectError::ManifestInvalid(_))
        ));
    }
    Ok(())
}
