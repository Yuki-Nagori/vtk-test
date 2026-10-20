//! 只读资产激活：后台路径约束、分块读取、摘要校验与网格解码。
//! 取消、工程代次与一次性请求身份共同控制结果发布；过期网格就地释放。
//! 激活不修改工程修订、dirty 状态或存储。

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, PoisonError};

use panta_import::ImportError;
use panta_mesh::{MeshAsset, SurfaceMesh, decode_mesh_asset_with_digest, mesh_asset_encoded_len};

pub(crate) const OUTCOME_CAPACITY: usize = 256;

/// begin 返回的运行期相关性句柄；generation 是会话级计数，非工程 revision。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivationAttempt {
    pub attempt: u64,
    pub generation: u64,
}

/// 一次激活请求的工作数据；提交后归 worker 线程所有。
pub(crate) struct ActivationRequest {
    pub project_root: PathBuf,
    pub relative_path: String,
    pub expected_digest: String,
}

/// 每个进程内 attempt 至多发布一次的终态结果；`Expired` 无网格负载。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeKind {
    Succeeded,
    Failed,
    Cancelled,
    Expired,
}

/// 穿越所有权的终态结果：`mesh` 由接收方释放，drain 过滤的过期快照就地释放。
/// code / category / detail 仅 Failed 非空；分类由类型化错误映射，不依赖文案。
pub struct Outcome {
    pub attempt: u64,
    pub generation: u64,
    pub import_id: String,
    pub kind: OutcomeKind,
    pub code: String,
    pub category: &'static str,
    pub detail: String,
    pub mesh: Option<SurfaceMesh>,
}

/// 资产读取、校验或后台调度的失败诊断。
struct ActivationFailure {
    code: &'static str,
    category: &'static str,
    detail: String,
}

struct AttemptEntry {
    generation: u64,
    import_id: String,
    cancel_requested: Arc<AtomicBool>,
}

#[derive(Default)]
struct CoordinatorState {
    next_attempt: u64,
    attempts: HashMap<u64, AttemptEntry>,
    outcomes: VecDeque<Outcome>,
}

/// 激活协调器：在飞请求、取消标志与结果队列的唯一所有者。
/// ProjectService（FFI/GUI 线程）与 worker 线程共享 `Arc<Self>`；所有
/// 请求完成与失效都在入口锁内串行，worker 与工程切换不会交错写状态。
#[derive(Default)]
pub struct ActivationCoordinator {
    state: Mutex<CoordinatorState>,
    generation: AtomicU64,
}

impl std::fmt::Debug for ActivationCoordinator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 不展开 attempt 与结果队列（含网格负载），只暴露会话代次。
        formatter
            .debug_struct("ActivationCoordinator")
            .field("generation", &self.generation.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl ActivationCoordinator {
    fn lock(&self) -> std::sync::MutexGuard<'_, CoordinatorState> {
        // 锁中毒只可能来自 panic 传播；恢复内层数据继续串行语义。
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 提交一次激活。同会话同记录的在飞 attempt 直接复用（去重）；
    /// 调用方已查证工程记录存在；调度失败也消费请求并发布失败结果。
    pub(crate) fn begin(
        self: &Arc<Self>,
        import_id: &str,
        request: ActivationRequest,
        executor: Arc<crate::execution::Executor>,
    ) -> Result<ActivationAttempt, &'static str> {
        self.begin_with(
            import_id,
            request,
            move |coordinator, attempt, request, cancelled| {
                spawn_activation_worker(executor, coordinator, attempt, request, cancelled)
            },
        )
    }

    /// begin 的可注入入口：spawner 可替换以覆盖 worker 启动失败。
    pub(crate) fn begin_with(
        self: &Arc<Self>,
        import_id: &str,
        request: ActivationRequest,
        spawn: impl FnOnce(Arc<Self>, u64, ActivationRequest, Arc<AtomicBool>) -> std::io::Result<()>,
    ) -> Result<ActivationAttempt, &'static str> {
        let mut state = self.lock();
        let generation = self.generation.load(Ordering::SeqCst);
        let existing = state.attempts.iter().find_map(|(attempt, entry)| {
            (entry.generation == generation
                && entry.import_id == import_id
                && !entry.cancel_requested.load(Ordering::SeqCst))
            .then_some(*attempt)
        });
        if let Some(attempt) = existing {
            return Ok(ActivationAttempt {
                attempt,
                generation,
            });
        }
        if state.attempts.len() + state.outcomes.len() >= OUTCOME_CAPACITY {
            return Err("asset activation backlog full");
        }
        state.next_attempt = state
            .next_attempt
            .checked_add(1)
            .ok_or("activation attempt exhausted")?;
        let attempt = state.next_attempt;
        let cancel_requested = Arc::new(AtomicBool::new(false));
        state.attempts.insert(
            attempt,
            AttemptEntry {
                generation,
                import_id: import_id.to_owned(),
                cancel_requested: Arc::clone(&cancel_requested),
            },
        );
        if let Err(error) = spawn(Arc::clone(self), attempt, request, cancel_requested) {
            publish(
                &mut state,
                &self.generation,
                attempt,
                Err(ActivationFailure {
                    code: "project.activation_spawn_failed",
                    category: "resource",
                    detail: error.to_string(),
                }),
            );
        }
        Ok(ActivationAttempt {
            attempt,
            generation,
        })
    }

    /// 请求取消：只置位标志；`Cancelled` 终态由 worker 在安全检查点确认。
    pub(crate) fn cancel(&self, attempt: u64) -> bool {
        let state = self.lock();
        match state.attempts.get(&attempt) {
            Some(entry) => {
                entry.cancel_requested.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    /// 推进会话代次并使所有旧代次在飞 attempt 失效（create/open 调用）。
    pub(crate) fn advance_generation(&self) -> u64 {
        let mut state = self.lock();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        for (_, entry) in state.attempts.drain() {
            entry.cancel_requested.store(true, Ordering::SeqCst);
        }
        state
            .outcomes
            .retain(|outcome| outcome.generation == generation);
        generation
    }

    /// 取出当前代次的终态结果；旧代次残留结果（含快照）就地释放，不跨 FFI。
    pub(crate) fn drain(&self) -> Vec<Outcome> {
        let mut state = self.lock();
        let generation = self.generation.load(Ordering::SeqCst);
        let mut results = Vec::new();
        for outcome in state.outcomes.drain(..) {
            if outcome.generation == generation {
                results.push(outcome);
            }
        }
        results
    }

    /// 一次性消费在飞请求；重复完成或旧会话结果在锁内丢弃。
    fn complete(&self, attempt: u64, result: Result<Option<SurfaceMesh>, ActivationFailure>) {
        publish(&mut self.lock(), &self.generation, attempt, result);
    }

    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

fn publish(
    state: &mut CoordinatorState,
    generation: &AtomicU64,
    attempt: u64,
    result: Result<Option<SurfaceMesh>, ActivationFailure>,
) {
    let Some(entry) = state.attempts.remove(&attempt) else {
        return;
    };
    if entry.generation != generation.load(Ordering::SeqCst) {
        return;
    }
    let mut outcome = Outcome {
        attempt,
        generation: entry.generation,
        import_id: entry.import_id,
        kind: OutcomeKind::Cancelled,
        code: String::new(),
        category: "",
        detail: String::new(),
        mesh: None,
    };
    match result {
        Ok(Some(mesh)) if !entry.cancel_requested.load(Ordering::SeqCst) => {
            outcome.kind = OutcomeKind::Succeeded;
            outcome.mesh = Some(mesh);
        }
        Ok(_) => {}
        Err(error) => {
            outcome.kind = OutcomeKind::Failed;
            outcome.code = error.code.to_owned();
            outcome.category = error.category;
            outcome.detail = error.detail;
        }
    }
    state.outcomes.push_back(outcome);
}

/// worker 入口：分块读取（可取消）→ 校验 / 解码（完成边界检查会话代次）。
/// GUI 线程不等待 worker；一次性结果由协调器相关性检查后发布。
fn run_attempt(
    coordinator: Arc<ActivationCoordinator>,
    attempt: u64,
    request: ActivationRequest,
    cancel_requested: Arc<AtomicBool>,
) {
    let result = load_asset(request, || cancel_requested.load(Ordering::SeqCst));
    coordinator.complete(attempt, result);
}

fn load_asset(
    request: ActivationRequest,
    cancelled: impl Fn() -> bool,
) -> Result<Option<SurfaceMesh>, ActivationFailure> {
    if cancelled() {
        return Ok(None);
    }
    let asset_path =
        crate::project::resolve_asset_path(&request.project_root, &request.relative_path).map_err(
            |error| ActivationFailure {
                code: error.code(),
                category: error.category(),
                detail: error.detail(),
            },
        )?;
    let Some(bytes) = read_mesh_bytes(&asset_path, &cancelled).map_err(|error| {
        let (code, category, detail) = activation_failure(&error);
        ActivationFailure {
            code,
            category,
            detail,
        }
    })?
    else {
        return Ok(None);
    };
    if cancelled() {
        return Ok(None);
    }
    let (asset, digest) =
        decode_mesh_asset_with_digest(&bytes).map_err(|error| ActivationFailure {
            code: "project.asset_parse_failed",
            category: "format",
            detail: error.to_string(),
        })?;
    match asset {
        MeshAsset::Surface(mesh) if digest == request.expected_digest => {
            Ok((!cancelled()).then_some(mesh))
        }
        MeshAsset::Surface(_) => Err(ActivationFailure {
            code: "project.asset_parse_failed",
            category: "format",
            detail: asset_path.display().to_string(),
        }),
        MeshAsset::Volume(_) => Err(ActivationFailure {
            code: "project.asset_unsupported_format",
            category: "format",
            detail: asset_path.display().to_string(),
        }),
    }
}

/// 先验证头部与声明长度，再分块读取并响应关闭 / 取消；格式不设单文件大小上限。
fn read_mesh_bytes(
    path: &std::path::Path,
    cancelled: impl Fn() -> bool,
) -> Result<Option<Vec<u8>>, ImportError> {
    let mut file = File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ImportError::Missing(path.display().to_string())
        } else {
            ImportError::Read(format!("{}: {error}", path.display()))
        }
    })?;
    let file_len = file
        .metadata()
        .map_err(|error| ImportError::Read(format!("{}: {error}", path.display())))?
        .len();
    let mut header = [0_u8; 56];
    file.read_exact(&mut header)
        .map_err(|error| ImportError::UnsupportedFormat(format!("{}: {error}", path.display())))?;
    let expected = mesh_asset_encoded_len(&header)
        .map_err(|error| ImportError::UnsupportedFormat(format!("{}: {error}", path.display())))?;
    let expected_u64 = u64::try_from(expected)
        .map_err(|_| ImportError::UnsupportedFormat(path.display().to_string()))?;
    if expected_u64 != file_len {
        return Err(ImportError::UnsupportedFormat(format!(
            "{}: sidecar length does not match its header",
            path.display()
        )));
    }
    if cancelled() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(expected).map_err(|error| {
        ImportError::Read(format!("{}: allocation failed: {error}", path.display()))
    })?;
    bytes.extend_from_slice(&header);
    let mut remaining = expected - header.len();
    let mut chunk = [0_u8; 256 * 1024];
    while remaining > 0 {
        if cancelled() {
            return Ok(None);
        }
        let capacity = remaining.min(chunk.len());
        let read = file
            .read(&mut chunk[..capacity])
            .map_err(|error| ImportError::Read(format!("{}: {error}", path.display())))?;
        if read == 0 {
            return Err(ImportError::Read(format!(
                "{}: sidecar ended before its declared size",
                path.display()
            )));
        }
        bytes.extend_from_slice(&chunk[..read]);
        remaining -= read;
    }
    let mut trailing = [0_u8; 1];
    if file
        .read(&mut trailing)
        .map_err(|error| ImportError::Read(format!("{}: {error}", path.display())))?
        != 0
    {
        return Err(ImportError::Read(format!(
            "{}: sidecar grew while being read",
            path.display()
        )));
    }
    Ok(Some(bytes))
}

/// 共享读取通道限制并发；调度失败仍发布结构化诊断。
fn spawn_activation_worker(
    executor: Arc<crate::execution::Executor>,
    coordinator: Arc<ActivationCoordinator>,
    attempt: u64,
    request: ActivationRequest,
    cancel_requested: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let failed = Arc::clone(&coordinator);
    executor.submit_reported(
        move || run_attempt(coordinator, attempt, request, cancel_requested),
        move || worker_panicked(&failed, attempt),
    )
}

fn worker_panicked(coordinator: &ActivationCoordinator, attempt: u64) {
    coordinator.complete(
        attempt,
        Err(ActivationFailure {
            code: "project.activation_worker_failed",
            category: "internal",
            detail: "asset activation worker panicked".to_owned(),
        }),
    );
}

fn activation_failure(error: &ImportError) -> (&'static str, &'static str, String) {
    match error {
        ImportError::Missing(path) => ("project.asset_missing", "missing", path.clone()),
        ImportError::UnsupportedFormat(path) => {
            ("project.asset_unsupported_format", "format", path.clone())
        }
        ImportError::Read(detail) => ("project.asset_read_failed", "io", detail.clone()),
        ImportError::Parse(error) => ("project.asset_parse_failed", "format", error.to_string()),
        ImportError::SourceChanged(path) => {
            ("project.asset_source_changed", "conflict", path.clone())
        }
        ImportError::UnsupportedUnits(units) => {
            ("project.import_unsupported_units", "format", units.clone())
        }
        ImportError::UnsupportedMeshType(kind) => (
            "project.import_unsupported_mesh_type",
            "format",
            kind.clone(),
        ),
        ImportError::CoordinateOverflow => (
            "project.asset_coordinate_overflow",
            "format",
            "millimeter coordinate overflow".to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accept_activation_request(
        _: Arc<ActivationCoordinator>,
        _: u64,
        _: ActivationRequest,
        _: Arc<AtomicBool>,
    ) -> std::io::Result<()> {
        Ok(())
    }

    #[test]
    fn sidecar_header_above_two_gib_is_accepted_by_format_policy()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!(
            "panta-large-mesh-{}-{}.pmesh",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let mut header = [0_u8; 56];
        header[..8].copy_from_slice(b"PNTMESH\0");
        header[8..12].copy_from_slice(&panta_mesh::MESH_ASSET_FORMAT_VERSION.to_le_bytes());
        header[12] = 1;
        header[13] = 1;
        header[14] = 1; // Zstandard codec.
        header[16..24].copy_from_slice(&(2_u64 * 1024 * 1024 * 1024).to_le_bytes());
        let file = File::create(&path)?;
        use std::io::Write;
        (&file).write_all(&header)?;
        file.set_len(2_u64 * 1024 * 1024 * 1024 + header.len() as u64)?;
        drop(file);

        // Cancellation after header validation proves the read path accepts the size
        // without allocating or touching the sparse payload.
        let result = read_mesh_bytes(&path, || true);
        std::fs::remove_file(&path)?;
        assert!(matches!(result, Ok(None)));
        Ok(())
    }

    #[test]
    fn activation_diagnostics_preserve_typed_categories_and_context() {
        let context = "参数: 中文 / C:\\零件.stl";
        let cases = [
            (
                ImportError::Missing(context.into()),
                "project.asset_missing",
                "missing",
                context,
            ),
            (
                ImportError::UnsupportedFormat(context.into()),
                "project.asset_unsupported_format",
                "format",
                context,
            ),
            (
                ImportError::Read(context.into()),
                "project.asset_read_failed",
                "io",
                context,
            ),
            (
                ImportError::SourceChanged(context.into()),
                "project.asset_source_changed",
                "conflict",
                context,
            ),
            (
                ImportError::UnsupportedUnits(context.into()),
                "project.import_unsupported_units",
                "format",
                context,
            ),
            (
                ImportError::UnsupportedMeshType(context.into()),
                "project.import_unsupported_mesh_type",
                "format",
                context,
            ),
            (
                ImportError::CoordinateOverflow,
                "project.asset_coordinate_overflow",
                "format",
                "millimeter coordinate overflow",
            ),
        ];
        for (error, code, category, detail) in cases {
            assert_eq!(activation_failure(&error), (code, category, detail.into()));
        }
        for error in [
            panta_mesh::StlError::Empty,
            panta_mesh::StlError::InvalidEncoding,
            panta_mesh::StlError::InvalidData,
            panta_mesh::StlError::NonFiniteCoordinate,
            panta_mesh::StlError::SizeOverflow,
        ] {
            let detail = error.to_string();
            assert_eq!(
                activation_failure(&ImportError::Parse(error)),
                ("project.asset_parse_failed", "format", detail)
            );
        }
    }

    fn coordinator_with_request() -> (Arc<ActivationCoordinator>, ActivationAttempt) {
        let coordinator = Arc::new(ActivationCoordinator::default());
        let attempt = coordinator
            .begin_with(
                "import-1",
                ActivationRequest {
                    project_root: PathBuf::from("unused-project"),
                    relative_path: "unused.pmesh".to_owned(),
                    expected_digest: "sha256:test".to_owned(),
                },
                accept_activation_request,
            )
            .unwrap_or_else(|error| panic!("activation admission failed: {error}"));
        (coordinator, attempt)
    }

    fn run_path_request(root: PathBuf, relative_path: &str) -> Outcome {
        let (coordinator, attempt) = coordinator_with_request();
        run_attempt(
            Arc::clone(&coordinator),
            attempt.attempt,
            ActivationRequest {
                project_root: root,
                relative_path: relative_path.to_owned(),
                expected_digest: "sha256:test".to_owned(),
            },
            Arc::new(AtomicBool::new(false)),
        );
        let mut outcomes = coordinator.drain();
        assert_eq!(outcomes.len(), 1);
        outcomes.remove(0)
    }

    #[test]
    fn worker_validates_relative_asset_paths_and_preserves_missing_diagnostics()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = crate::project::temp_directory::Fixture::new()?;
        let missing = run_path_request(fixture.root.clone(), "missing.pmesh");
        assert_eq!(missing.kind, OutcomeKind::Failed);
        assert_eq!(missing.code, "project.asset_missing");
        assert_eq!(missing.category, "missing");
        assert!(missing.mesh.is_none());

        for path in ["../outside.pmesh", "/outside.pmesh"] {
            let invalid = run_path_request(fixture.root.clone(), path);
            assert_eq!(invalid.kind, OutcomeKind::Failed);
            assert_eq!(invalid.code, "project.manifest_invalid");
            assert_eq!(invalid.category, "format");
            assert!(invalid.mesh.is_none());
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn worker_rejects_leaf_and_parent_symlinks_that_escape_the_project()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = crate::project::temp_directory::Fixture::new()?;
        let project_root = fixture.root.join("project");
        let external_root = fixture.root.join("external");
        std::fs::create_dir(&project_root)?;
        std::fs::create_dir(&external_root)?;
        let external_asset = external_root.join("asset.pmesh");
        std::fs::write(&external_asset, b"must not be read")?;
        std::os::unix::fs::symlink(&external_asset, project_root.join("leaf.pmesh"))?;
        std::os::unix::fs::symlink(&external_root, project_root.join("directory"))?;
        for path in ["leaf.pmesh", "directory/asset.pmesh"] {
            let invalid = run_path_request(project_root.clone(), path);
            assert_eq!(invalid.kind, OutcomeKind::Failed);
            assert_eq!(invalid.code, "project.manifest_invalid");
            assert_eq!(invalid.category, "format");
            assert!(invalid.mesh.is_none());
        }
        Ok(())
    }

    #[test]
    fn success_publishes_snapshot_once_and_consumes_attempt() {
        let (coordinator, attempt) = coordinator_with_request();

        coordinator.complete(
            attempt.attempt,
            Ok(Some(SurfaceMesh {
                triangles: Vec::new(),
            })),
        );
        // 请求已消费后，迟到结果不再发布。
        coordinator.complete(
            attempt.attempt,
            Ok(Some(SurfaceMesh {
                triangles: Vec::new(),
            })),
        );
        let outcomes = coordinator.drain();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, OutcomeKind::Succeeded);
        assert_eq!(outcomes[0].import_id, "import-1");
        assert!(outcomes[0].mesh.is_some());
        assert!(coordinator.drain().is_empty(), "结果只发布一次");
        assert!(!coordinator.cancel(attempt.attempt), "终态后不可取消");
    }

    #[test]
    fn cancellation_at_publication_discards_the_decoded_mesh() {
        let (coordinator, attempt) = coordinator_with_request();
        assert!(coordinator.cancel(attempt.attempt));
        coordinator.complete(
            attempt.attempt,
            Ok(Some(SurfaceMesh {
                triangles: Vec::new(),
            })),
        );
        let outcomes = coordinator.drain();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, OutcomeKind::Cancelled);
        assert!(outcomes[0].mesh.is_none());
    }

    #[test]
    fn fail_and_cancel_paths_publish_distinct_outcomes() {
        let (coordinator, attempt) = coordinator_with_request();
        coordinator.complete(
            attempt.attempt,
            Err(ActivationFailure {
                code: "project.asset_missing",
                category: "missing",
                detail: "part.stl".to_owned(),
            }),
        );
        let outcomes = coordinator.drain();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, OutcomeKind::Failed);
        assert_eq!(outcomes[0].code, "project.asset_missing");
        assert_eq!(outcomes[0].category, "missing");

        let (coordinator, attempt) = coordinator_with_request();
        coordinator.complete(attempt.attempt, Ok(None));
        let outcomes = coordinator.drain();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, OutcomeKind::Cancelled);
    }

    #[test]
    fn generation_invalidation_drops_late_snapshot() {
        let (coordinator, attempt) = coordinator_with_request();
        coordinator.advance_generation();
        // 旧代次结果（含无负载的 Expired）不得跨会话发布：drain 为空，
        // 且 attempt 已终态化（不可再取消）。
        assert!(coordinator.drain().is_empty());
        assert!(!coordinator.cancel(attempt.attempt));
        // 旧请求已移除，迟到网格就地释放。
        coordinator.complete(
            attempt.attempt,
            Ok(Some(SurfaceMesh {
                triangles: Vec::new(),
            })),
        );
        assert!(coordinator.drain().is_empty(), "迟到成功不得发布");
    }

    #[test]
    fn generation_advance_without_attempts_is_a_noop() {
        let coordinator = Arc::new(ActivationCoordinator::default());
        assert_eq!(coordinator.advance_generation(), 1);
        assert!(coordinator.drain().is_empty());
        assert_eq!(coordinator.advance_generation(), 2);
        assert!(coordinator.drain().is_empty());
        let diagnostic = format!("{coordinator:?}");
        assert!(diagnostic.contains("generation: 2"));
        assert!(!diagnostic.contains("outcomes"));
    }

    #[test]
    fn drain_drops_results_queued_for_stale_generations() {
        let (coordinator, attempt) = coordinator_with_request();

        coordinator.complete(
            attempt.attempt,
            Ok(Some(SurfaceMesh {
                triangles: Vec::new(),
            })),
        );
        // 成功结果已入队；随后代次推进（工程切换）：旧代次结果在 drain
        // 边界被过滤，坐标（含网格负载）就地释放，不跨会话发布。
        coordinator.advance_generation();
        assert!(coordinator.drain().is_empty());
    }

    #[test]
    fn duplicate_begin_reuses_in_flight_attempt() {
        let coordinator = Arc::new(ActivationCoordinator::default());
        let request = || ActivationRequest {
            project_root: PathBuf::from("unused-project"),
            relative_path: "unused.pmesh".to_owned(),
            expected_digest: "sha256:test".to_owned(),
        };
        let first = coordinator
            .begin_with("import-1", request(), accept_activation_request)
            .unwrap_or_else(|error| panic!("activation admission failed: {error}"));
        let second = coordinator
            .begin_with("import-1", request(), accept_activation_request)
            .unwrap_or_else(|error| panic!("activation admission failed: {error}"));
        assert_eq!(first.attempt, second.attempt);
        assert_eq!(first.generation, second.generation);
        // 不同记录仍创建独立 attempt。
        let other = coordinator
            .begin_with("import-2", request(), accept_activation_request)
            .unwrap_or_else(|error| panic!("activation admission failed: {error}"));
        assert_ne!(first.attempt, other.attempt);
    }

    #[test]
    fn spawn_failure_publishes_a_failure() {
        let coordinator = Arc::new(ActivationCoordinator::default());
        let attempt = coordinator
            .begin_with(
                "import-1",
                ActivationRequest {
                    project_root: PathBuf::from("unused-project"),
                    relative_path: "unused.pmesh".to_owned(),
                    expected_digest: "sha256:test".to_owned(),
                },
                |_coordinator, _attempt, _request, _cancel| {
                    Err(std::io::Error::other("thread pool exhausted"))
                },
            )
            .unwrap_or_else(|error| panic!("activation admission failed: {error}"));
        let outcomes = coordinator.drain();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, OutcomeKind::Failed);
        assert_eq!(outcomes[0].code, "project.activation_spawn_failed");
        assert_eq!(outcomes[0].category, "resource");
        assert_eq!(outcomes[0].attempt, attempt.attempt);
    }

    #[test]
    fn worker_panic_publishes_a_structured_failure() {
        let (coordinator, attempt) = coordinator_with_request();

        worker_panicked(&coordinator, attempt.attempt);

        let outcomes = coordinator.drain();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, OutcomeKind::Failed);
        assert_eq!(outcomes[0].code, "project.activation_worker_failed");
        assert_eq!(outcomes[0].category, "internal");
        assert_eq!(outcomes[0].detail, "asset activation worker panicked");
        assert_eq!(outcomes[0].attempt, attempt.attempt);
    }

    #[test]
    fn parse_failure_publishes_without_a_mesh() {
        let (coordinator, attempt) = coordinator_with_request();

        // 资产读取后的中间态没有可观察结果。
        assert!(coordinator.drain().is_empty());
        coordinator.complete(
            attempt.attempt,
            Err(ActivationFailure {
                code: "project.asset_parse_failed",
                category: "format",
                detail: "invalid stl".to_owned(),
            }),
        );
        let outcomes = coordinator.drain();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, OutcomeKind::Failed);
        assert_eq!(outcomes[0].code, "project.asset_parse_failed");
        assert_eq!(outcomes[0].category, "format");
    }

    #[test]
    fn activation_backlog_and_identifiers_are_bounded_and_reclaimed() {
        let coordinator = Arc::new(ActivationCoordinator::default());
        let request = || ActivationRequest {
            project_root: PathBuf::from("unused-project"),
            relative_path: "unused.pmesh".to_owned(),
            expected_digest: "sha256:test".into(),
        };
        for id in 0..OUTCOME_CAPACITY {
            let attempt = coordinator
                .begin_with(
                    &format!("import-{id}"),
                    request(),
                    accept_activation_request,
                )
                .unwrap_or_else(|error| panic!("activation admission failed: {error}"));
            coordinator.complete(
                attempt.attempt,
                Err(ActivationFailure {
                    code: "test.failure",
                    category: "internal",
                    detail: String::new(),
                }),
            );
        }
        assert_eq!(
            coordinator
                .begin_with("overflow", request(), accept_activation_request)
                .err(),
            Some("asset activation backlog full")
        );
        assert_eq!(coordinator.drain().len(), OUTCOME_CAPACITY);
        coordinator.lock().next_attempt = u64::MAX;
        assert_eq!(
            coordinator
                .begin_with("exhausted", request(), accept_activation_request)
                .err(),
            Some("activation attempt exhausted")
        );
        assert!(coordinator.lock().attempts.is_empty());
    }

    #[test]
    fn generation_change_cancels_queued_reads_and_releases_old_outcomes() {
        let coordinator = Arc::new(ActivationCoordinator::default());
        let cancelled = Arc::new(Mutex::new(None));
        let saved = Arc::clone(&cancelled);
        coordinator
            .begin_with(
                "queued",
                ActivationRequest {
                    project_root: PathBuf::from("unused-project"),
                    relative_path: "unused.pmesh".to_owned(),
                    expected_digest: "sha256:test".into(),
                },
                move |_, _, _, token| {
                    *saved
                        .lock()
                        .unwrap_or_else(|error| panic!("token lock failed: {error}")) = Some(token);
                    Ok(())
                },
            )
            .unwrap_or_else(|error| panic!("activation admission failed: {error}"));
        coordinator.advance_generation();
        assert!(
            cancelled
                .lock()
                .unwrap_or_else(|error| panic!("token lock failed: {error}"))
                .as_ref()
                .unwrap_or_else(|| panic!("cancellation token missing"))
                .load(Ordering::SeqCst)
        );
        assert!(coordinator.lock().attempts.is_empty());
        assert!(coordinator.lock().outcomes.is_empty());
    }
}
