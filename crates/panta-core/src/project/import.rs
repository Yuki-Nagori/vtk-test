//! STL 导入的工程事务；读取、预览和解析由 panta-import 负责。
use super::model::{MeshAssetRef, next_revision};
use super::{ImportRecord, ProjectError, ProjectService, StlImportPreview, mesh_assets};
use panta_import::{CheckedReadError, StlImportSession};
use panta_import::{ImportError, ImportOptions};
use panta_mesh::{DiagnosticOptions, Progress, SurfaceDiagnostics, diagnose_surface};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::{fs, path::Path};

const IMPORT_RUNNING: u8 = 0;
const IMPORT_COMMITTING: u8 = 1;
const IMPORT_CANCELLED: u8 = 2;
const IMPORT_COMMITTED: u8 = 3;

#[derive(Debug)]
pub(super) struct PendingImport {
    request: u64,
    phase: Arc<AtomicU8>,
    progress: Arc<Progress>,
    receiver: Receiver<Result<ImportCommit, ProjectError>>,
}

impl PendingImport {
    fn request_cancel(&self) -> bool {
        self.phase
            .compare_exchange(
                IMPORT_RUNNING,
                IMPORT_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }
}

impl Drop for PendingImport {
    fn drop(&mut self) {
        self.request_cancel();
    }
}

impl ProjectService {
    /// 启动确认导入事务。源复核、Mesh sidecar 写入和清单提交都由 Rust 写入 worker 执行。
    /// 成功提交只有在 `finish_stl_import` 消费结果后才发布到内存模型。
    pub fn begin_stl_import(
        &mut self,
        source: &Path,
        mesh_type: &str,
        units: &str,
        show_import_log: bool,
    ) -> Result<u64, ProjectError> {
        self.ensure_project_writable()?;
        if self.pending_preview.is_some() {
            return Err(ProjectError::CommandInvalid(
                "STL preview pending".to_owned(),
            ));
        }
        if self.current.is_none() {
            return Err(ProjectError::NoProject);
        }
        if !self.import_session.has_preview_for(source) {
            return Err(ProjectError::CommandInvalid(
                "STL preview is not ready for the selected source".to_owned(),
            ));
        }
        let request = self
            .import_request
            .checked_add(1)
            .ok_or_else(|| ProjectError::CommandInvalid("import request exhausted".to_owned()))?;
        let options =
            ImportOptions::new(mesh_type, units, show_import_log).map_err(map_import_error)?;
        let import_number = self
            .current
            .as_ref()
            .ok_or(ProjectError::NoProject)?
            .imports
            .len()
            + 1;
        let revision = next_revision(
            self.current
                .as_ref()
                .ok_or(ProjectError::NoProject)?
                .revision,
        )?;
        let mut candidate = self
            .current
            .as_ref()
            .ok_or(ProjectError::NoProject)?
            .clone();
        candidate.revision = revision;
        candidate.dirty = false;

        // Keep ownership recoverable if bounded admission rejects the job.
        let session = Arc::new(Mutex::new(Some(std::mem::take(&mut self.import_session))));
        let worker_session = Arc::clone(&session);
        let phase = Arc::new(AtomicU8::new(IMPORT_RUNNING));
        let worker_phase = Arc::clone(&phase);
        let progress = Arc::new(Progress::new());
        let worker_progress = Arc::clone(&progress);
        let source = source.to_path_buf();
        let (sender, receiver) = mpsc::channel();
        let submitted = self.execution.writes.submit(move || {
            let session = worker_session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            let result = match session {
                Some(mut session) => commit_import(
                    &mut candidate,
                    &mut session,
                    &source,
                    options,
                    import_number,
                    &worker_phase,
                    &worker_progress,
                ),
                None => Err(ProjectError::Io(
                    "STL import session unavailable".to_owned(),
                )),
            };
            if let Err(mpsc::SendError(Ok(_))) = sender.send(result) {
                eprintln!("panta: STL import committed after service teardown");
            }
        });
        if let Err(error) = submitted {
            self.import_session = session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .unwrap_or_default();
            return Err(ProjectError::Io(format!("start STL import: {error}")));
        }

        self.import_request = request;
        self.pending_import = Some(PendingImport {
            request,
            phase,
            progress,
            receiver,
        });
        Ok(request)
    }

    /// `None` 表示仍在执行；成功结果发布已提交修订和 Mesh，错误保留旧状态。
    pub fn finish_stl_import(
        &mut self,
        request: u64,
    ) -> Result<Option<ImportRecord>, ProjectError> {
        let pending = self
            .pending_import
            .as_ref()
            .filter(|pending| pending.request == request)
            .ok_or_else(|| {
                ProjectError::CommandInvalid("no matching STL import pending".to_owned())
            })?;
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) => Err(ProjectError::Io(
                "STL import worker disconnected".to_owned(),
            )),
        };
        self.pending_import = None;
        let (state, record, mesh, volume) = result?;
        self.latest_mesh_id = Some(record.id.clone());
        self.mesh_cache.insert_active(record.id.clone(), mesh);
        self.mesh_lod_sources.remove(&record.id);
        self.set_volume_mesh(&record.id, volume);
        self.current = Some(state);
        Ok(Some(record))
    }

    /// 导入进度（0..=1，按阶段估算）；请求不匹配或已结束时返回 `None`。
    pub fn stl_import_progress(&self, request: u64) -> Option<f64> {
        self.pending_import
            .as_ref()
            .filter(|pending| pending.request == request)
            .map(|pending| pending.progress.fraction())
    }

    /// 取消只在 manifest 替换前接受；到达提交阶段后调用方必须消费结果。
    pub fn cancel_stl_import(&mut self, request: u64) -> bool {
        let Some(pending) = self
            .pending_import
            .as_ref()
            .filter(|pending| pending.request == request)
        else {
            return false;
        };
        if !pending.request_cancel() {
            return false;
        }
        self.pending_import = None;
        true
    }
}

/// 提交结果：候选状态、导入记录、显示网格，以及体网格导入时的四面体数据（task 021）。
type ImportCommit = (
    super::ProjectState,
    ImportRecord,
    panta_mesh::SurfaceMesh,
    Option<panta_mesh::TetMesh>,
);

fn commit_import(
    candidate: &mut super::ProjectState,
    session: &mut StlImportSession,
    source: &Path,
    options: ImportOptions,
    import_number: usize,
    phase: &AtomicU8,
    progress: &Progress,
) -> Result<ImportCommit, ProjectError> {
    // 细粒度进度：复核读取/解析 5–50%（按字节 / 三角面），缩放 60%，资产写出 60–90%（按块），
    // 清单提交 100%。
    progress.set_fraction(0.05);
    let mut prepared = session
        .prepare_with_progress(
            source,
            || phase.load(Ordering::Acquire) == IMPORT_CANCELLED,
            &|value| progress.set_fraction(0.05 + value * 0.45),
        )
        .map_err(|error| match error {
            CheckedReadError::Failed(error) => map_import_error(error),
            CheckedReadError::Cancelled(_) => import_cancelled(),
        })?;
    ensure_import_running(phase)?;
    progress.set_fraction(0.5);
    let mut record = prepared.record(import_number, options);
    let volume = volume_from_prepared(&prepared, options)?;
    drop(std::mem::take(&mut prepared.bytes));
    match &volume {
        Some(volume) => prepared.mesh = panta_mesh::boundary_surface(volume),
        None => prepared.scale_mesh_mm(options).map_err(map_import_error)?,
    }
    progress.set_fraction(0.6);
    let lease = super::repository::WriteLease::acquire(candidate)?;
    let project_root = candidate
        .path
        .parent()
        .ok_or_else(|| ProjectError::Io("project has no package directory".to_owned()))?;
    let (asset_reference, asset_path, created) = write_import_mesh_asset(
        project_root,
        &mut record,
        match &volume {
            Some(volume) => panta_mesh::MeshAssetRef::Volume(volume),
            None => panta_mesh::MeshAssetRef::Surface(&prepared.mesh),
        },
        Some(phase),
        &|value| progress.set_fraction(0.6 + value * 0.3),
    )?;
    progress.set_fraction(0.9);
    candidate
        .mesh_assets
        .insert(asset_reference.id.clone(), asset_reference);
    candidate.imports.push(record.clone());
    let mut asset = ImportAsset {
        path: asset_path,
        created,
        committed: false,
    };
    let commit_result = lease.commit_with_before_replace(candidate, || {
        phase
            .compare_exchange(
                IMPORT_RUNNING,
                IMPORT_COMMITTING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|_| import_cancelled())
    });
    commit_result?;
    asset.committed = true;
    phase.store(IMPORT_COMMITTED, Ordering::Release);
    progress.set_fraction(1.0);
    Ok((candidate.clone(), record, prepared.mesh, volume))
}

/// 「实体 3D」类型的 VTU / 旧版 VTK 按线性四面体体网格导入：坐标换算到毫米后构建并校验；
/// 含其他单元类型时整体拒绝（task 021 保守决策）。其他组合仍按表面导入。
fn volume_from_prepared(
    prepared: &panta_import::PreparedStl,
    options: ImportOptions,
) -> Result<Option<panta_mesh::TetMesh>, ProjectError> {
    if !options.is_solid()
        || !matches!(
            prepared.format,
            panta_import::ImportFormat::Vtu | panta_import::ImportFormat::VtkLegacy
        )
    {
        return Ok(None);
    }
    let snapshot = panta_import::SourceSnapshot {
        path: prepared.path.clone(),
        source_name: prepared.source_name.clone(),
        format: prepared.format,
        bytes: prepared.bytes.clone(),
        modified_unix_ms: prepared.modified_unix_ms,
    };
    let grid = panta_import::parse_volume_snapshot(&snapshot).map_err(|error| match error {
        ImportError::Format(detail) => ProjectError::ImportVolumeUnsupported(detail.to_string()),
        other => map_import_error(other),
    })?;
    let factor = options.factor_mm();
    let mut points = grid.points;
    for point in &mut points {
        *point = point.map(|value| value * factor);
        if point.iter().any(|value| !value.is_finite()) {
            return Err(ProjectError::ImportParseFailed(
                "millimeter coordinate overflow".to_owned(),
            ));
        }
    }
    panta_mesh::tet_mesh_from_cells(points, grid.tets)
        .map(Some)
        .map_err(|error| ProjectError::ImportVolumeUnsupported(error.to_string()))
}

struct ImportAsset {
    path: std::path::PathBuf,
    created: bool,
    committed: bool,
}

impl Drop for ImportAsset {
    fn drop(&mut self) {
        if self.created && !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn write_import_mesh_asset(
    project_root: &Path,
    record: &mut ImportRecord,
    mesh: panta_mesh::MeshAssetRef<'_>,
    phase: Option<&AtomicU8>,
    on_progress: &dyn Fn(f64),
) -> Result<(MeshAssetRef, std::path::PathBuf, bool), ProjectError> {
    let base_id = record.mesh_asset_id.clone();
    for attempt in 0..32 {
        if let Some(phase) = phase {
            ensure_import_running(phase)?;
        }
        let asset_id = if attempt == 0 {
            base_id.clone()
        } else {
            format!("{base_id}-recovery-{attempt}")
        };
        record.mesh_asset_id = asset_id;
        let mut asset_reference = new_surface_asset_ref(record, "millimeters");
        if matches!(mesh, panta_mesh::MeshAssetRef::Volume(_)) {
            asset_reference.kind = "volume".to_owned();
        }
        let asset_path =
            super::service::resolve_asset_path(project_root, &asset_reference.relative_path)
                .map_err(|error| ProjectError::ImportAssetWriteFailed(error.to_string()))?;
        let write_result = mesh_assets::write_mesh_asset_or_reuse(
            &asset_path,
            match &mesh {
                panta_mesh::MeshAssetRef::Surface(surface) => {
                    panta_mesh::MeshAssetRef::Surface(surface)
                }
                panta_mesh::MeshAssetRef::Volume(volume) => {
                    panta_mesh::MeshAssetRef::Volume(volume)
                }
            },
            on_progress,
        )
        .map_err(|error| ProjectError::ImportAssetWriteFailed(error.to_string()))?;
        if let Some((digest, created)) = write_result {
            asset_reference.digest = digest;
            return Ok((asset_reference, asset_path, created));
        }
    }
    record.mesh_asset_id = base_id;
    Err(ProjectError::ImportAssetWriteFailed(
        "mesh asset recovery path limit reached".to_owned(),
    ))
}

fn ensure_import_running(phase: &AtomicU8) -> Result<(), ProjectError> {
    if phase.load(Ordering::Acquire) == IMPORT_RUNNING {
        Ok(())
    } else {
        Err(import_cancelled())
    }
}

fn import_cancelled() -> ProjectError {
    ProjectError::CommandInvalid("STL import cancelled".to_owned())
}

impl ProjectService {
    /// 校验来源快照并将 Mesh sidecar 与导入记录提交到当前工程。
    /// 返回成功前先写入清单，重新打开工程时无需依赖后续显式保存即可恢复。
    pub fn import_stl(
        &mut self,
        source: &Path,
        mesh_type: &str,
        units: &str,
        show_import_log: bool,
    ) -> Result<ImportRecord, ProjectError> {
        self.ensure_project_writable()?;
        if self.pending_preview.is_some() {
            return Err(ProjectError::CommandInvalid(
                "STL preview pending".to_owned(),
            ));
        }
        if self.current.is_none() {
            return Err(ProjectError::NoProject);
        }
        let options =
            ImportOptions::new(mesh_type, units, show_import_log).map_err(map_import_error)?;
        let import_number = self
            .current
            .as_ref()
            .ok_or(ProjectError::NoProject)?
            .imports
            .len()
            + 1;
        let revision = next_revision(
            self.current
                .as_ref()
                .ok_or(ProjectError::NoProject)?
                .revision,
        )?;
        let mut candidate = self
            .current
            .as_ref()
            .ok_or(ProjectError::NoProject)?
            .clone();
        candidate.revision = revision;
        candidate.dirty = false;
        let phase = AtomicU8::new(IMPORT_RUNNING);
        let (candidate, record, mesh, volume) = commit_import(
            &mut candidate,
            &mut self.import_session,
            source,
            options,
            import_number,
            &phase,
            &Progress::new(),
        )?;
        self.current = Some(candidate);
        self.latest_mesh_id = Some(record.id.clone());
        self.mesh_cache.insert_active(record.id.clone(), mesh);
        self.set_volume_mesh(&record.id, volume);
        Ok(record)
    }

    /// 同步领域入口，供无 GUI 的调用方使用；Qt 使用后台 begin / finish 入口。
    pub fn inspect_stl(&mut self, source: &Path) -> Result<StlImportPreview, ProjectError> {
        self.clear_stl_preview();
        self.import_session
            .preview(source)
            .map_err(map_import_error)
    }

    /// 预检 STL 并对同一解析结果做只读质量诊断；坐标与容差均为来源单位。
    pub fn inspect_stl_with_diagnostics(
        &mut self,
        source: &Path,
        options: DiagnosticOptions,
    ) -> Result<(StlImportPreview, SurfaceDiagnostics), ProjectError> {
        let preview = self.inspect_stl(source)?;
        let mesh = self
            .import_session
            .preview_mesh()
            .ok_or_else(|| ProjectError::CommandInvalid("STL preview missing".to_owned()))?;
        let diagnostics = diagnose_surface(mesh, options)
            .map_err(|error| ProjectError::CommandInvalid(format!("STL diagnostics: {error}")))?;
        Ok((preview, diagnostics))
    }
}

fn new_surface_asset_ref(record: &ImportRecord, units: &str) -> MeshAssetRef {
    MeshAssetRef {
        id: record.mesh_asset_id.clone(),
        revision: 1,
        format_version: panta_mesh::MESH_ASSET_FORMAT_VERSION,
        kind: "surface".to_owned(),
        relative_path: format!("assets/meshes/{}-r1.pmesh", record.mesh_asset_id),
        digest: String::new(),
        units: units.to_owned(),
        source_revision: record.source_digest.clone(),
    }
}

pub(super) fn map_import_error(error: ImportError) -> ProjectError {
    match error {
        ImportError::Missing(path) => ProjectError::ImportFileMissing(path),
        ImportError::UnsupportedFormat(path) => ProjectError::ImportInvalidFile(path),
        ImportError::Read(detail) => ProjectError::ImportParseFailed(detail),
        ImportError::Parse(detail) => ProjectError::ImportParseFailed(detail.to_string()),
        ImportError::Format(detail) => ProjectError::ImportParseFailed(detail.to_string()),
        ImportError::SourceChanged(path) => ProjectError::ImportSourceChanged(path),
        ImportError::UnsupportedMeshType(kind) => ProjectError::ImportUnsupportedMeshType(kind),
        ImportError::UnsupportedUnits(units) => ProjectError::ImportUnsupportedUnits(units),
        ImportError::CoordinateOverflow => {
            ProjectError::ImportParseFailed("millimeter coordinate overflow".to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IMPORT_CANCELLED, IMPORT_COMMITTING, IMPORT_RUNNING, PendingImport, map_import_error,
    };
    use crate::execution::Executor;
    use crate::project::ProjectService;
    use crate::project::temp_directory;
    use panta_import::ImportError;
    use panta_mesh::StlError;
    use std::fs;
    use std::sync::{Arc, atomic::Ordering, mpsc};
    use std::time::Duration;

    fn write_triangle(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        fs::write(path, b"vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n")?;
        Ok(())
    }

    fn finish_preview(
        service: &mut ProjectService,
        request: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while service.finish_stl_preview(request)?.is_none() {
            assert!(std::time::Instant::now() < deadline, "preview timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    fn finish_import(
        service: &mut ProjectService,
        request: u64,
    ) -> Result<super::ImportRecord, Box<dyn std::error::Error>> {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(record) = service.finish_stl_import(request)? {
                return Ok(record);
            }
            assert!(std::time::Instant::now() < deadline, "import timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn block_executor(
        executor: &Executor,
    ) -> Result<(mpsc::Sender<()>, mpsc::Receiver<()>), Box<dyn std::error::Error>> {
        let (started, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        executor.submit(move || {
            let _ = started.send(());
            let _ = gate.recv();
        })?;
        ready.recv_timeout(Duration::from_secs(2))?;
        Ok((release, ready))
    }

    #[test]
    fn import_failures_map_to_stable_project_diagnostics() {
        let cases = [
            (
                ImportError::Missing("part.stl".into()),
                "project.import_file_missing",
                "missing",
                "part.stl",
            ),
            (
                ImportError::UnsupportedFormat("part.step".into()),
                "project.import_invalid_file",
                "format",
                "part.step",
            ),
            (
                ImportError::Read("disk full".into()),
                "project.import_parse_failed",
                "format",
                "disk full",
            ),
            (
                ImportError::Parse(StlError::InvalidData),
                "project.import_parse_failed",
                "format",
                "InvalidData",
            ),
            (
                ImportError::SourceChanged("part.stl".into()),
                "project.import_source_changed",
                "conflict",
                "part.stl",
            ),
            (
                ImportError::UnsupportedMeshType("unknown".into()),
                "project.import_unsupported_mesh_type",
                "format",
                "unknown",
            ),
            (
                ImportError::UnsupportedUnits("yards".into()),
                "project.import_unsupported_units",
                "format",
                "yards",
            ),
            (
                ImportError::CoordinateOverflow,
                "project.import_parse_failed",
                "format",
                "millimeter coordinate overflow",
            ),
        ];

        for (error, code, category, detail) in cases {
            let mapped = map_import_error(error);
            assert_eq!(mapped.code(), code);
            assert_eq!(mapped.category(), category);
            assert_eq!(mapped.detail(), detail);
        }
    }

    #[test]
    fn async_import_publishes_only_after_atomic_manifest_commit()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let source = fixture.root.join("part.stl");
        write_triangle(&source)?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture.root, "Async")?;
        let manifest = fs::read(&project.path)?;
        let preview = service.begin_stl_preview(&source)?;
        finish_preview(&mut service, preview)?;
        let executor = Executor::new("import-commit-test", 1, 2);
        let (release, _) = block_executor(&executor)?;
        service.execution.writes = Arc::clone(&executor);

        let request = service.begin_stl_import(&source, "solid-3d", "millimeters", false)?;
        assert_eq!(fs::read(&project.path)?, manifest);
        assert!(service.imports()?.is_empty());
        assert_eq!(service.current()?.revision, 0);
        assert!(service.current_mesh().is_none());
        assert!(
            service
                .execute(crate::project::ProjectCommand::Rename {
                    name: "Blocked".into(),
                })
                .is_err()
        );

        release.send(())?;
        let record = finish_import(&mut service, request)?;
        assert_eq!(record.id, "import-1");
        assert_eq!(service.current()?.revision, 1);
        assert_eq!(service.imports()?, vec![record.clone()]);
        assert!(service.current_mesh().is_some());
        let asset_path = service
            .current
            .as_ref()
            .and_then(|state| state.mesh_assets.get(&record.mesh_asset_id))
            .map(|asset| asset.relative_path.clone())
            .ok_or("mesh asset index entry missing")?;
        assert!(fixture.root.join("Async").join(asset_path).is_file());
        assert!(!fixture.root.join("Async/assets/imports").exists());

        let mut reopened = ProjectService::new();
        reopened.open(&project.path)?;
        assert_eq!(reopened.imports()?, vec![record]);
        executor.close_and_wait();
        Ok(())
    }

    #[test]
    fn imported_mesh_does_not_depend_on_source_file_after_commit()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let source = fixture.root.join("external-part.stl");
        write_triangle(&source)?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture.root, "Portable mesh")?;
        let record = service.import_stl(&source, "solid-3d", "millimeters", false)?;
        let sidecar_path = service
            .current
            .as_ref()
            .and_then(|state| state.mesh_assets.get(&record.mesh_asset_id))
            .map(|asset| asset.relative_path.clone())
            .ok_or("mesh asset index entry missing")?;
        let sidecar = fixture.root.join("Portable mesh").join(sidecar_path);
        assert!(sidecar.is_file());
        assert!(!fixture.root.join("Portable mesh/assets/imports").exists());

        fs::remove_file(&source)?;
        let mut reopened = ProjectService::new();
        reopened.open(&project.path)?;
        let attempt = reopened.begin_asset_activation(&record.id)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let outcomes = reopened.drain_asset_activations();
            if let Some(outcome) = outcomes.into_iter().next() {
                assert_eq!(outcome.attempt, attempt.attempt);
                assert_eq!(outcome.kind, crate::activation::OutcomeKind::Succeeded);
                assert_eq!(
                    outcome
                        .mesh
                        .ok_or("activated sidecar has no mesh")?
                        .triangles
                        .len(),
                    1
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "sidecar activation timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }

    #[test]
    fn corrupt_mesh_sidecar_fails_activation_without_publishing_a_mesh()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let source = fixture.root.join("part.stl");
        write_triangle(&source)?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture.root, "Corrupt sidecar")?;
        let record = service.import_stl(&source, "solid-3d", "millimeters", false)?;
        let asset_path = service
            .current
            .as_ref()
            .and_then(|state| state.mesh_assets.get(&record.mesh_asset_id))
            .map(|asset| {
                fixture
                    .root
                    .join("Corrupt sidecar")
                    .join(&asset.relative_path)
            })
            .ok_or("mesh asset index entry missing")?;
        drop(service);

        let mut bytes = fs::read(&asset_path)?;
        *bytes.last_mut().ok_or("mesh sidecar is empty")? ^= 1;
        fs::write(&asset_path, bytes)?;

        let mut reopened = ProjectService::new();
        reopened.open(&project.path)?;
        let attempt = reopened.begin_asset_activation(&record.id)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let outcomes = reopened.drain_asset_activations();
            if let Some(outcome) = outcomes.into_iter().next() {
                assert_eq!(outcome.attempt, attempt.attempt);
                assert_eq!(outcome.kind, crate::activation::OutcomeKind::Failed);
                assert_eq!(outcome.code, "project.asset_parse_failed");
                assert!(outcome.mesh.is_none());
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "corrupt sidecar activation timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }

    #[test]
    fn queued_async_import_can_be_cancelled_without_asset_or_manifest_changes()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let source = fixture.root.join("part.stl");
        write_triangle(&source)?;
        let mut service = ProjectService::new();
        let project = service.create(&fixture.root, "Cancel")?;
        let manifest = fs::read(&project.path)?;
        let preview = service.begin_stl_preview(&source)?;
        finish_preview(&mut service, preview)?;
        let executor = Executor::new("import-cancel-test", 1, 2);
        let (release, _) = block_executor(&executor)?;
        service.execution.writes = Arc::clone(&executor);

        let request = service.begin_stl_import(&source, "solid-3d", "millimeters", false)?;
        // 排队中的导入尚未开始：进度可查询且为 0。
        assert_eq!(service.stl_import_progress(request), Some(0.0));
        assert_eq!(service.stl_import_progress(request + 1), None);
        assert!(service.cancel_stl_import(request));
        assert_eq!(service.stl_import_progress(request), None);
        assert!(service.finish_stl_import(request).is_err());
        release.send(())?;
        executor.close_and_wait();
        assert_eq!(fs::read(&project.path)?, manifest);
        assert!(
            !fixture
                .root
                .join("Cancel/assets/imports/0001-part.stl")
                .exists()
        );
        assert!(
            !fixture
                .root
                .join("Cancel/assets/meshes/import-1-r1.pmesh")
                .exists()
        );
        assert!(service.imports()?.is_empty());
        assert!(service.current_mesh().is_none());
        Ok(())
    }

    #[test]
    fn import_cancel_is_rejected_after_the_manifest_commit_gate() {
        let phase = Arc::new(std::sync::atomic::AtomicU8::new(IMPORT_RUNNING));
        assert!(
            phase
                .compare_exchange(
                    IMPORT_RUNNING,
                    IMPORT_COMMITTING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
        );
        let pending = PendingImport {
            request: 1,
            phase,
            progress: Arc::default(),
            receiver: mpsc::channel().1,
        };
        assert!(!pending.request_cancel());
        assert_ne!(IMPORT_CANCELLED, pending.phase.load(Ordering::Acquire));
        assert_eq!(IMPORT_COMMITTING, pending.phase.load(Ordering::Acquire));
    }

    #[test]
    fn rejected_import_admission_restores_preview_for_retry()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = temp_directory::Fixture::new()?;
        let source = fixture.root.join("part.stl");
        write_triangle(&source)?;
        let mut service = ProjectService::new();
        service.create(&fixture.root, "Retry")?;
        let preview = service.begin_stl_preview(&source)?;
        finish_preview(&mut service, preview)?;
        let executor = Executor::new("import-admission-test", 1, 1);
        let (release, _) = block_executor(&executor)?;
        service.execution.writes = Arc::clone(&executor);

        assert!(
            service
                .begin_stl_import(&source, "solid-3d", "millimeters", false)
                .is_err()
        );
        assert!(service.import_session.has_preview_for(&source));
        release.send(())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let request = loop {
            match service.begin_stl_import(&source, "solid-3d", "millimeters", false) {
                Ok(request) => break request,
                Err(error) if error.detail().contains("background capacity exhausted") => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "executor did not drain"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error.into()),
            }
        };
        finish_import(&mut service, request)?;
        executor.close_and_wait();
        assert_eq!(service.imports()?.len(), 1);
        Ok(())
    }
}
