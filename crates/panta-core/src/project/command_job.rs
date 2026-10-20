//! 后台工程命令只准备拥有式结果，服务线程校验请求身份后发布。

use super::model::validate_name;
use super::repository::write_manifest;
use super::service::{resolve_asset_path, validate_schema_four_mesh_assets};
use super::{
    PROJECT_FILE_EXTENSION, PROJECT_SCHEMA_VERSION, ProjectError, ProjectManifest, ProjectService,
    ProjectSnapshot, ProjectState, analysis_sequence, gate_location, material, process_settings,
};
use crate::path::{PathService, RootCategory};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

/// GUI 工程用例；路径检查、清单解析与原子写入全部在后台执行。
#[derive(Debug)]
pub enum ProjectOperation {
    Create { location: PathBuf, name: String },
    Open { path: PathBuf },
    Save,
}

/// 唯一结果接收者；宿主须在独立线程等待，不借用活跃服务。
#[derive(Debug)]
pub struct ProjectOperationRequest {
    identity: Arc<()>,
    receiver: mpsc::Receiver<Result<PreparedOperation, ProjectError>>,
}

/// 后台已完成的候选，不会自动改变当前工程。
#[derive(Debug)]
pub struct ProjectOperationCompletion {
    identity: Arc<()>,
    result: Result<PreparedOperation, ProjectError>,
}

impl ProjectOperationRequest {
    pub fn wait(self) -> ProjectOperationCompletion {
        ProjectOperationCompletion {
            identity: self.identity,
            result: self.receiver.recv().unwrap_or_else(|_| {
                Err(ProjectError::Io(
                    "project operation worker disconnected".to_owned(),
                ))
            }),
        }
    }
}

#[derive(Debug)]
pub(super) struct PendingProjectOperation {
    identity: Arc<()>,
    expected: Option<ProjectSnapshot>,
}

#[derive(Debug)]
pub(super) struct PreparedReplacement {
    pub(super) state: ProjectState,
}

#[derive(Debug)]
enum PreparedOperation {
    Replace(PreparedReplacement),
    Update(ProjectState),
}

impl ProjectService {
    /// 准入失败不冻结工程；已接受写入在宿主销毁后仍由执行通道完成。
    pub fn begin_project_operation(
        &mut self,
        operation: ProjectOperation,
    ) -> Result<ProjectOperationRequest, ProjectError> {
        self.ensure_project_writable()?;
        let state = match operation {
            ProjectOperation::Save => Some(
                self.current
                    .as_ref()
                    .ok_or(ProjectError::NoProject)?
                    .clone(),
            ),
            _ => None,
        };
        let executor = if matches!(operation, ProjectOperation::Open { .. }) {
            &self.execution.reads
        } else {
            &self.execution.writes
        };
        let expected = self.current().ok();
        let identity = Arc::new(());
        let (sender, receiver) = mpsc::channel();
        executor
            .submit(move || {
                let result = prepare_operation(operation, state);
                if let Err(mpsc::SendError(Err(error))) = sender.send(result) {
                    eprintln!("panta: project operation failed after service teardown: {error}");
                }
            })
            .map_err(|error| ProjectError::Io(format!("start project operation: {error}")))?;
        self.pending_project = Some(PendingProjectOperation {
            identity: Arc::clone(&identity),
            expected,
        });
        Ok(ProjectOperationRequest { identity, receiver })
    }

    /// 只发布本服务准入的当前候选；外来或旧请求不能解锁其他在途命令。
    pub fn finish_project_operation(
        &mut self,
        completion: ProjectOperationCompletion,
    ) -> Result<ProjectSnapshot, ProjectError> {
        let pending = self
            .pending_project
            .as_ref()
            .filter(|pending| Arc::ptr_eq(&pending.identity, &completion.identity))
            .ok_or_else(|| ProjectError::CommandInvalid("stale project operation".to_owned()))?;
        let expected = pending.expected.clone();
        self.pending_project = None;
        if self.current().ok() != expected {
            return Err(ProjectError::CommandInvalid(
                "project operation target changed".to_owned(),
            ));
        }
        match completion.result? {
            PreparedOperation::Replace(prepared) => self.publish_project_replacement(prepared),
            PreparedOperation::Update(state) => {
                self.current = Some(state);
                self.snapshot()
            }
        }
    }
}

fn prepare_operation(
    operation: ProjectOperation,
    state: Option<ProjectState>,
) -> Result<PreparedOperation, ProjectError> {
    match operation {
        ProjectOperation::Create { location, name } => {
            prepare_create(&location, &name).map(PreparedOperation::Replace)
        }
        ProjectOperation::Open { path } => prepare_open(&path).map(PreparedOperation::Replace),
        ProjectOperation::Save => {
            let mut state = state.ok_or(ProjectError::NoProject)?;
            write_manifest(&mut state)?;
            state.dirty = false;
            Ok(PreparedOperation::Update(state))
        }
    }
}

fn validate_project_root(root: &Path) -> Result<(), ProjectError> {
    let mut paths = PathService::new();
    paths
        .set_root(RootCategory::Project, root)
        .map_err(|error| ProjectError::Io(error.to_string()))?;
    Ok(())
}

pub(super) fn prepare_create(
    location: &Path,
    name: &str,
) -> Result<PreparedReplacement, ProjectError> {
    validate_name(name)?;
    if location.as_os_str().is_empty() {
        return Err(ProjectError::LocationEmpty);
    }
    if !location.is_absolute() {
        return Err(ProjectError::LocationNotAbsolute(
            location.display().to_string(),
        ));
    }
    if !location.exists() {
        fs::create_dir_all(location).map_err(|error| {
            ProjectError::LocationCreateFailed(format!("{}: {error}", location.display()))
        })?;
    }
    if !location.is_dir() {
        return Err(ProjectError::LocationCreateFailed(
            location.display().to_string(),
        ));
    }

    let project_root = location.join(name);
    if project_root.exists() {
        return Err(ProjectError::AlreadyExists(
            project_root.display().to_string(),
        ));
    }
    fs::create_dir(&project_root)
        .map_err(|error| ProjectError::Io(format!("{}: {error}", project_root.display())))?;

    let project_path = project_root.join(format!("{name}.{PROJECT_FILE_EXTENSION}"));

    let mut state = ProjectState {
        path: project_path,
        name: name.to_owned(),
        revision: 0,
        persisted_revision: None,
        dirty: false,
        imports: Vec::new(),
        mesh_assets: BTreeMap::new(),
        analysis_sequences: BTreeMap::new(),
        materials: BTreeMap::new(),
        fill_settings: BTreeMap::new(),
        gate_location_settings: BTreeMap::new(),
    };
    let prepared = (|| {
        validate_project_root(&project_root)?;
        write_manifest(&mut state)?;
        Ok(())
    })();
    match prepared {
        Ok(()) => (),
        Err(error) => {
            if let Some(project_root) = state.path.parent() {
                let _ = fs::remove_dir_all(project_root);
            }
            return Err(error);
        }
    };
    Ok(PreparedReplacement { state })
}

pub(super) fn prepare_open(path: &Path) -> Result<PreparedReplacement, ProjectError> {
    if !path.is_absolute() || !path.is_file() {
        return Err(ProjectError::FileMissing(path.display().to_string()));
    }
    let is_panta_file = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(PROJECT_FILE_EXTENSION));
    if !is_panta_file {
        return Err(ProjectError::InvalidFile(path.display().to_string()));
    }
    let bytes =
        fs::read(path).map_err(|error| ProjectError::Io(format!("{}: {error}", path.display())))?;
    // 先只读取版本号，确保旧 schema 不会因其记录形状与当前模型不同而
    // 被误报为当前清单损坏。
    #[derive(serde::Deserialize)]
    struct SchemaEnvelope {
        schema: u32,
    }

    let envelope: SchemaEnvelope = serde_json::from_slice(&bytes)
        .map_err(|error| ProjectError::ManifestInvalid(format!("{}: {error}", path.display())))?;
    if envelope.schema != PROJECT_SCHEMA_VERSION {
        return Err(ProjectError::UnsupportedSchema(envelope.schema));
    }
    let manifest: ProjectManifest = serde_json::from_slice(&bytes)
        .map_err(|error| ProjectError::ManifestInvalid(format!("{}: {error}", path.display())))?;
    validate_schema_four_mesh_assets(&manifest.imports, &manifest.mesh_assets)?;
    validate_name(&manifest.name)?;
    analysis_sequence::validate_sequences(&manifest)?;
    material::validate_materials(&manifest)?;
    process_settings::validate_settings(&manifest.imports, &manifest.fill_settings)?;
    gate_location::validate_settings(&manifest.imports, &manifest.gate_location_settings)?;
    let project_root = path
        .parent()
        .ok_or_else(|| ProjectError::Io("project has no package directory".to_owned()))?;
    for asset in manifest.mesh_assets.values() {
        resolve_asset_path(project_root, &asset.relative_path)?;
    }
    let state = ProjectState {
        path: path.to_path_buf(),
        name: manifest.name,
        revision: manifest.revision,
        persisted_revision: Some(manifest.revision),
        dirty: false,
        imports: manifest.imports,
        mesh_assets: manifest.mesh_assets,
        analysis_sequences: manifest.analysis_sequences,
        materials: manifest.materials,
        fill_settings: manifest.fill_settings,
        gate_location_settings: manifest.gate_location_settings,
    };
    validate_project_root(project_root)?;
    Ok(PreparedReplacement { state })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::Executor;
    use crate::project::ProjectCommand;
    use crate::project::temp_directory::Fixture;
    use std::time::Duration;

    #[test]
    fn create_publishes_only_after_wait_and_rejects_conflicting_commands()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let mut service = ProjectService::new();
        let before = service.create(&fixture.root, "Old")?;
        let generation = service.activation.generation();
        let request = service.begin_project_operation(ProjectOperation::Create {
            location: fixture.root.clone(),
            name: "New".into(),
        })?;
        assert_eq!(service.current()?, before);
        assert!(service.save().is_err());
        assert!(service.open(&before.path).is_err());
        assert!(
            service
                .execute(ProjectCommand::Rename {
                    name: "Wrong".into()
                })
                .is_err()
        );
        assert!(
            service
                .begin_project_operation(ProjectOperation::Save)
                .is_err()
        );
        let completion = std::thread::spawn(move || request.wait())
            .join()
            .map_err(|_| "wait panicked")?;
        assert_eq!(service.current()?, before);
        let created = service.finish_project_operation(completion)?;
        assert_eq!(created.name, "New");
        assert_eq!(service.activation.generation(), generation + 1);
        assert!(service.pending_project.is_none());
        Ok(())
    }

    #[test]
    fn failed_open_preserves_current_project_and_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let mut service = ProjectService::new();
        let before = service.create(&fixture.root, "Kept")?;
        let generation = service.activation.generation();
        let completion = service
            .begin_project_operation(ProjectOperation::Open {
                path: fixture.root.join("missing.panta"),
            })?
            .wait();
        assert!(service.finish_project_operation(completion).is_err());
        assert_eq!(service.current()?, before);
        assert_eq!(service.activation.generation(), generation);
        assert!(service.pending_project.is_none());
        service.save()?;
        Ok(())
    }

    #[test]
    fn open_reuses_schema_and_asset_checks_and_replaces_only_after_finish()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let mut source = ProjectService::new();
        let saved = source.create(&fixture.root, "Opened")?;
        let mut service = ProjectService::new();
        let before = service.create(&fixture.root, "Before")?;
        let completion = service
            .begin_project_operation(ProjectOperation::Open {
                path: saved.path.clone(),
            })?
            .wait();
        assert_eq!(service.current()?, before);
        assert_eq!(service.finish_project_operation(completion)?, saved);
        fs::write(&saved.path, "{\"schema\": 3}")?;
        let completion = service
            .begin_project_operation(ProjectOperation::Open { path: saved.path })?
            .wait();
        assert!(matches!(
            service.finish_project_operation(completion),
            Err(ProjectError::UnsupportedSchema(3))
        ));
        Ok(())
    }

    #[test]
    fn save_failure_retains_dirty_candidate_and_previous_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture.root, "Save")?;
        let original = fs::read(&project.path)?;
        service.execute(ProjectCommand::Rename {
            name: "Dirty".into(),
        })?;
        let before = service.current()?;
        let staging = project.path.with_extension("panta.write");
        fs::remove_dir(&staging)?;
        fs::write(&staging, "blocked")?;
        let completion = service
            .begin_project_operation(ProjectOperation::Save)?
            .wait();
        assert!(service.finish_project_operation(completion).is_err());
        assert_eq!(service.current()?, before);
        assert_eq!(fs::read(&project.path)?, original);
        assert!(service.pending_project.is_none());
        fs::remove_file(staging)?;
        let completion = service
            .begin_project_operation(ProjectOperation::Save)?
            .wait();
        assert!(!service.finish_project_operation(completion)?.dirty);
        let mut opened = ProjectService::new();
        assert_eq!(opened.open(&project.path)?.name, "Dirty");
        Ok(())
    }

    #[test]
    fn foreign_completion_does_not_unlock_matching_request_and_stale_target_is_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let mut first = ProjectService::new();
        first.create(&fixture.root, "First")?;
        let mut second = ProjectService::new();
        let before = second.create(&fixture.root, "Second")?;
        let foreign = first
            .begin_project_operation(ProjectOperation::Save)?
            .wait();
        let local = second
            .begin_project_operation(ProjectOperation::Save)?
            .wait();
        assert!(second.finish_project_operation(foreign).is_err());
        assert!(second.pending_project.is_some());
        assert_eq!(second.finish_project_operation(local)?, before);
        let local = second
            .begin_project_operation(ProjectOperation::Save)?
            .wait();
        second.current.as_mut().ok_or("missing state")?.revision += 1;
        assert!(second.finish_project_operation(local).is_err());
        assert_eq!(second.current()?.revision, before.revision + 1);
        assert!(second.pending_project.is_none());
        Ok(())
    }

    #[test]
    fn capacity_or_closed_executor_rejects_without_sticking_pending_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let mut service = ProjectService::new();
        let before = service.create(&fixture.root, "Capacity")?;
        let executor = Executor::new("project-command-capacity", 1, 1);
        let (started, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        executor.submit(move || {
            let _ = started.send(());
            let _ = gate.recv();
        })?;
        ready.recv_timeout(Duration::from_secs(2))?;
        service.execution.writes = Arc::clone(&executor);
        assert!(
            service
                .begin_project_operation(ProjectOperation::Save)
                .is_err()
        );
        assert!(service.pending_project.is_none());
        assert_eq!(service.current()?, before);
        release.send(())?;
        executor.close_and_wait();
        assert!(
            service
                .begin_project_operation(ProjectOperation::Save)
                .is_err()
        );
        assert!(service.pending_project.is_none());
        Ok(())
    }

    #[test]
    fn accepted_save_finishes_after_service_drop_without_joining_gui()
    -> Result<(), Box<dyn std::error::Error>> {
        for block_staging in [false, true] {
            let fixture = Fixture::new()?;
            let mut service = ProjectService::new();
            let project = service.create(&fixture.root, "Drop")?;
            service.execute(ProjectCommand::Rename {
                name: "Saved".into(),
            })?;
            let executor = Executor::new("project-command-drop", 1, 2);
            let (started, ready) = mpsc::channel();
            let (release, gate) = mpsc::channel();
            executor.submit(move || {
                let _ = started.send(());
                let _ = gate.recv();
            })?;
            ready.recv_timeout(Duration::from_secs(2))?;
            service.execution.writes = executor;
            if block_staging {
                let staging = project.path.with_extension("panta.write");
                fs::remove_dir(&staging)?;
                fs::write(staging, "blocked")?;
            }
            let request = service.begin_project_operation(ProjectOperation::Save)?;
            drop(service);
            release.send(())?;
            assert_eq!(request.wait().result.is_ok(), !block_staging);
            let mut opened = ProjectService::new();
            let reopened = opened.open(&project.path)?;
            assert_eq!(reopened.name, if block_staging { "Drop" } else { "Saved" });
            assert_eq!(reopened.revision, if block_staging { 0 } else { 1 });
        }
        Ok(())
    }

    #[test]
    fn disconnected_worker_clears_admission_without_changing_the_project()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let mut service = ProjectService::new();
        let before = service.create(&fixture.root, "Disconnect")?;
        let identity = Arc::new(());
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        service.pending_project = Some(PendingProjectOperation {
            identity: Arc::clone(&identity),
            expected: Some(before.clone()),
        });
        let completion = ProjectOperationRequest { identity, receiver }.wait();
        let error = service
            .finish_project_operation(completion)
            .err()
            .ok_or("missing error")?;
        assert_eq!(error.detail(), "project operation worker disconnected");
        assert_eq!(service.current()?, before);
        assert!(service.pending_project.is_none());
        service.save()?;
        Ok(())
    }

    #[test]
    fn absent_project_or_failed_creation_never_replaces_current_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let mut service = ProjectService::new();
        assert!(matches!(
            service.begin_project_operation(ProjectOperation::Save),
            Err(ProjectError::NoProject)
        ));
        assert!(service.pending_project.is_none());
        let completion = service
            .begin_project_operation(ProjectOperation::Create {
                location: fixture.root.clone(),
                name: "invalid/name".into(),
            })?
            .wait();
        assert!(service.finish_project_operation(completion).is_err());
        assert!(matches!(service.current(), Err(ProjectError::NoProject)));
        assert_eq!(service.activation.generation(), 0);
        let before = service.create(&fixture.root, "Existing")?;
        let manifest = fs::read(&before.path)?;
        let completion = service
            .begin_project_operation(ProjectOperation::Create {
                location: fixture.root.clone(),
                name: "Existing".into(),
            })?
            .wait();
        assert!(matches!(
            service.finish_project_operation(completion),
            Err(ProjectError::AlreadyExists(_))
        ));
        assert_eq!(service.current()?, before);
        assert_eq!(fs::read(before.path)?, manifest);
        Ok(())
    }
}
