//! 只读资产激活：后台路径约束、分块读取、摘要校验与网格解码。
//! 取消、工程代次与一次性请求身份共同控制结果发布；过期网格就地释放。
//! 激活不修改工程修订、dirty 状态或存储。

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, PoisonError};

use panta_import::ImportError;
use panta_mesh::{
    MeshAsset, MeshAssetError, MeshAssetReader, SurfaceMesh, TetMesh, VolumeStreamCounts,
};

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
    /// 非 0 表示 `mesh` 是显示 LOD，值为源资产面数（task 019）。
    pub lod_source_triangles: u64,
    /// 体网格资产：`mesh` 为外表面，此处为原始四面体（task 021）。
    pub volume: Option<Arc<TetMesh>>,
    /// 体网格单元数超过激活阈值：四面体不驻留，`mesh` 为流式提取的精确外表面（task 025）。
    pub streamed_volume: Option<VolumeStreamCounts>,
}

/// 超过此面数的表面资产激活时改为流式构建显示 LOD，不整体读入（约 1.4 GiB 展开体积）。
pub const LOD_ACTIVATION_TRIANGLES: u64 = 20_000_000;

/// 超过此四面体数的体网格资产激活时不驻留单元（task 025）：只流式提取外表面显示，
/// 质量统计、切面与 `.vtu` 导出按需从资产流式读取。驻留读取含完整拓扑校验，
/// 实测 1010 万四面体峰值 1.8 GiB、15.8 s，而流式质量/切面同规模各约 0.3 s，
/// 默认值取 1000 万以 8 GiB 内存机器为保守上限（基准见 task 025 文档）。
pub const VOLUME_STREAM_ACTIVATION_CELLS: u64 = 10_000_000;

/// 大网格激活设置（task 026）：表面 LOD 触发面数与 LOD 输出预算、体网格流式激活单元数。
/// 只影响之后的激活；已驻留文档不重算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivationLimits {
    /// 表面资产面数超过此值时只驻留显示 LOD。
    pub surface_triangles: u64,
    /// 显示 LOD 的面数上限。
    pub surface_budget: u64,
    /// 体网格四面体数超过此值时单元不驻留、命令按需流式读取。
    pub volume_cells: u64,
}

impl Default for ActivationLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl ActivationLimits {
    pub const DEFAULT: Self = Self {
        surface_triangles: LOD_ACTIVATION_TRIANGLES,
        surface_budget: panta_mesh::DEFAULT_LOD_TRIANGLE_BUDGET as u64,
        volume_cells: VOLUME_STREAM_ACTIVATION_CELLS,
    };
    /// 表面触发阈值范围（面）。
    pub const SURFACE_TRIANGLES_RANGE: std::ops::RangeInclusive<u64> = 1_000_000..=2_000_000_000;
    /// LOD 预算范围（面），且须小于触发阈值。
    pub const SURFACE_BUDGET_RANGE: std::ops::RangeInclusive<u64> = 100_000..=20_000_000;
    /// 体网格流式激活阈值范围（四面体）。
    pub const VOLUME_CELLS_RANGE: std::ops::RangeInclusive<u64> = 1_000_000..=10_000_000_000;

    /// 范围校验；违反时 detail 指出字段，供界面显示。
    pub fn validate(&self) -> Result<(), crate::project::ProjectError> {
        let invalid = |field: &str| {
            Err(crate::project::ProjectError::LodSettingsInvalid(
                field.to_owned(),
            ))
        };
        if !Self::SURFACE_TRIANGLES_RANGE.contains(&self.surface_triangles) {
            return invalid("surface_triangles");
        }
        if !Self::SURFACE_BUDGET_RANGE.contains(&self.surface_budget)
            || self.surface_budget >= self.surface_triangles
        {
            return invalid("surface_budget");
        }
        if !Self::VOLUME_CELLS_RANGE.contains(&self.volume_cells) {
            return invalid("volume_cells");
        }
        Ok(())
    }
    /// 完整读取（导出等只读命令）：不走 LOD 或流式激活。
    pub(crate) const RESIDENT: Self = Self {
        surface_triangles: u64::MAX,
        surface_budget: u64::MAX,
        volume_cells: u64::MAX,
    };
}

/// 激活成功的负载。
pub(crate) struct LoadedMesh {
    pub(crate) mesh: SurfaceMesh,
    /// 非 0：`mesh` 是显示 LOD，值为源资产面数（task 019）。
    pub(crate) lod_source: u64,
    /// 体网格资产的驻留四面体数据（task 021），显示网格为其外表面。
    pub(crate) volume: Option<Arc<TetMesh>>,
    /// 流式体网格（task 025）：单元未驻留，值为资产计数。
    pub(crate) streamed_volume: Option<VolumeStreamCounts>,
}

impl LoadedMesh {
    fn surface(mesh: SurfaceMesh) -> Self {
        Self {
            mesh,
            lod_source: 0,
            volume: None,
            streamed_volume: None,
        }
    }
}

/// 资产读取、校验或后台调度的失败诊断。
pub(crate) struct ActivationFailure {
    pub(crate) code: &'static str,
    pub(crate) category: &'static str,
    pub(crate) detail: String,
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
    /// 已校验的大网格设置；worker 开始读取时取值（task 026）。
    limits: Mutex<ActivationLimits>,
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

    /// 替换大网格设置（调用方已校验）；在飞与已驻留的激活不受影响。
    pub(crate) fn set_limits(&self, limits: ActivationLimits) {
        *self.limits.lock().unwrap_or_else(PoisonError::into_inner) = limits;
    }

    pub(crate) fn limits(&self) -> ActivationLimits {
        *self.limits.lock().unwrap_or_else(PoisonError::into_inner)
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
    fn complete(&self, attempt: u64, result: Result<Option<LoadedMesh>, ActivationFailure>) {
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
    result: Result<Option<LoadedMesh>, ActivationFailure>,
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
        lod_source_triangles: 0,
        volume: None,
        streamed_volume: None,
    };
    match result {
        Ok(Some(loaded)) if !entry.cancel_requested.load(Ordering::SeqCst) => {
            outcome.kind = OutcomeKind::Succeeded;
            outcome.mesh = Some(loaded.mesh);
            outcome.lod_source_triangles = loaded.lod_source;
            outcome.volume = loaded.volume;
            outcome.streamed_volume = loaded.streamed_volume;
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
    let limits = coordinator.limits();
    let result = load_asset(request, || cancel_requested.load(Ordering::SeqCst), limits);
    coordinator.complete(attempt, result);
}

/// 同步读取已提交 Surface 资产（导出等只读命令复用激活的校验路径，不可取消）。
pub(crate) fn load_committed_surface(
    request: ActivationRequest,
) -> Result<SurfaceMesh, ActivationFailure> {
    load_asset(request, || false, ActivationLimits::RESIDENT)?
        .map(|loaded| loaded.mesh)
        .ok_or(ActivationFailure {
            code: "project.asset_read_failed",
            category: "io",
            detail: "asset read did not complete".to_owned(),
        })
}

/// `limits`：v3 表面资产面数超过阈值时只构建显示 LOD，v3 体网格单元数超过阈值时流式激活
/// （导出等完整读取传 [`ActivationLimits::RESIDENT`]）。
fn load_asset(
    request: ActivationRequest,
    cancelled: impl Fn() -> bool,
    limits: ActivationLimits,
) -> Result<Option<LoadedMesh>, ActivationFailure> {
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
    if let Some(loaded) = load_lod(
        &asset_path,
        &request.expected_digest,
        &cancelled,
        limits.surface_triangles,
        limits.surface_budget,
    )? {
        return Ok(loaded);
    }
    if let Some(loaded) = load_streamed_volume(
        &asset_path,
        &request.expected_digest,
        &cancelled,
        limits.volume_cells,
    )? {
        return Ok(loaded);
    }
    let Some((asset, digest)) = read_mesh_asset(&asset_path, &cancelled)? else {
        return Ok(None);
    };
    match asset {
        MeshAsset::Surface(mesh) if digest == request.expected_digest => {
            Ok((!cancelled()).then_some(LoadedMesh::surface(mesh)))
        }
        MeshAsset::Surface(_) => Err(ActivationFailure {
            code: "project.asset_parse_failed",
            category: "format",
            detail: asset_path.display().to_string(),
        }),
        MeshAsset::Volume(volume) if digest == request.expected_digest => {
            let surface = panta_mesh::boundary_surface(&volume);
            Ok((!cancelled()).then(|| LoadedMesh {
                volume: Some(Arc::new(volume)),
                ..LoadedMesh::surface(surface)
            }))
        }
        MeshAsset::Volume(_) => Err(ActivationFailure {
            code: "project.asset_parse_failed",
            category: "format",
            detail: asset_path.display().to_string(),
        }),
    }
}

/// 大资产走流式 LOD：外层 `None` 表示不适用（非 v3 表面或未超阈值），交给整体读取。
/// 摘要（目录 SHA-256）在扫描前核对；每块 SHA-256 在扫描中校验。
#[allow(clippy::type_complexity)]
fn load_lod(
    path: &std::path::Path,
    expected_digest: &str,
    cancelled: &impl Fn() -> bool,
    lod_threshold: u64,
    budget: u64,
) -> Result<Option<Option<LoadedMesh>>, ActivationFailure> {
    let Ok(chunks) = panta_mesh::SurfaceAssetChunks::open(path) else {
        return Ok(None);
    };
    let source = chunks.triangle_count();
    if source <= lod_threshold {
        return Ok(None);
    }
    if chunks.digest() != expected_digest {
        return Err(ActivationFailure {
            code: "project.asset_parse_failed",
            category: "format",
            detail: path.display().to_string(),
        });
    }
    drop(chunks);
    // 校验保证预算 < 阈值；仍取阈值一半为上限，保证 LOD 显著小于触发阈值。
    let budget = usize::try_from(budget.min(lod_threshold / 2)).unwrap_or(usize::MAX);
    let result = with_cancellation(cancelled, |progress| {
        panta_mesh::build_surface_lod(path, budget, Some(progress))
    })?;
    match result {
        Ok(Some(lod)) if !cancelled() => Ok(Some(Some(LoadedMesh {
            lod_source: source,
            ..LoadedMesh::surface(lod.mesh)
        }))),
        Ok(_) => Ok(Some(None)),
        Err(error) => Err(stream_failure(path, error)),
    }
}

/// 超过单元阈值的 v3 体网格：流式提取精确外表面（只驻留节点与外表面），单元不驻留。
/// 外层 `None` 表示不适用（非 v3 体网格或未超阈值）。
#[allow(clippy::type_complexity)]
fn load_streamed_volume(
    path: &std::path::Path,
    expected_digest: &str,
    cancelled: &impl Fn() -> bool,
    cell_threshold: u64,
) -> Result<Option<Option<LoadedMesh>>, ActivationFailure> {
    let Ok(mut chunks) = panta_mesh::VolumeAssetChunks::open(path) else {
        return Ok(None);
    };
    let counts = chunks.counts();
    if counts.tets as u64 <= cell_threshold {
        return Ok(None);
    }
    if chunks.digest() != expected_digest {
        return Err(ActivationFailure {
            code: "project.asset_parse_failed",
            category: "format",
            detail: path.display().to_string(),
        });
    }
    let result = with_cancellation(cancelled, |progress| {
        panta_mesh::boundary_surface_streaming(&mut chunks, Some(progress))
    })?;
    match result {
        Ok(Some(surface)) if !cancelled() => Ok(Some(Some(LoadedMesh {
            streamed_volume: Some(counts),
            ..LoadedMesh::surface(surface)
        }))),
        Ok(_) => Ok(Some(None)),
        Err(panta_mesh::VolumeStreamError::Asset(error)) => Err(stream_failure(path, error)),
        Err(panta_mesh::VolumeStreamError::Invalid(detail)) => Err(ActivationFailure {
            code: "project.asset_parse_failed",
            category: "format",
            detail,
        }),
    }
}

/// 在作用域线程中运行流式任务，当前线程轮询宿主取消并置位进度句柄，任务在批次边界停止。
fn with_cancellation<T: Send>(
    cancelled: &impl Fn() -> bool,
    work: impl FnOnce(&panta_mesh::Progress) -> T + Send,
) -> Result<T, ActivationFailure> {
    let progress = panta_mesh::Progress::new();
    let result = std::thread::scope(|scope| {
        let worker = scope.spawn(|| work(&progress));
        while !worker.is_finished() {
            if cancelled() {
                progress.cancel();
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        worker.join()
    });
    result.map_err(|_| ActivationFailure {
        code: "project.activation_worker_failed",
        category: "internal",
        detail: "streaming worker panicked".to_owned(),
    })
}

fn stream_failure(path: &std::path::Path, error: MeshAssetError) -> ActivationFailure {
    match error {
        MeshAssetError::Io(kind) => ActivationFailure {
            code: "project.asset_read_failed",
            category: "io",
            detail: format!("{}: {kind}", path.display()),
        },
        other => ActivationFailure {
            code: "project.asset_parse_failed",
            category: "format",
            detail: other.to_string(),
        },
    }
}

/// 先验证头部、目录与声明长度，再按数据块批量读取、并行解码并在批次间响应关闭 / 取消；
/// 峰值内存约为解码结果加一个批次的压缩块。格式不设单文件大小上限。
fn read_mesh_asset(
    path: &std::path::Path,
    cancelled: impl Fn() -> bool,
) -> Result<Option<(MeshAsset, String)>, ActivationFailure> {
    let import_failure = |error: ImportError| {
        let (code, category, detail) = activation_failure(&error);
        ActivationFailure {
            code,
            category,
            detail,
        }
    };
    let read_failure = |detail: String| import_failure(ImportError::Read(detail));
    let file = File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            import_failure(ImportError::Missing(path.display().to_string()))
        } else {
            read_failure(format!("{}: {error}", path.display()))
        }
    })?;
    let file_len = file
        .metadata()
        .map_err(|error| read_failure(format!("{}: {error}", path.display())))?
        .len();
    let reader =
        MeshAssetReader::open(BufReader::new(file), file_len).map_err(|error| match error {
            MeshAssetError::Io(kind) => read_failure(format!("{}: {kind}", path.display())),
            other => import_failure(ImportError::UnsupportedFormat(format!(
                "{}: {other}",
                path.display()
            ))),
        })?;
    if cancelled() {
        return Ok(None);
    }
    reader.read(cancelled).map_err(|error| match error {
        MeshAssetError::Io(kind) => read_failure(format!("{}: {kind}", path.display())),
        other => ActivationFailure {
            code: "project.asset_parse_failed",
            category: "format",
            detail: other.to_string(),
        },
    })
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
        ImportError::Format(error) => ("project.asset_parse_failed", "format", error.to_string()),
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
        // v3 头部 + 单条目录：一个 2 GiB 的稀疏数据块，偏移按 64 B 对齐到 128。
        let chunk_len = 2_u64 * 1024 * 1024 * 1024;
        let mut toc = Vec::new();
        toc.extend_from_slice(&1_u64.to_le_bytes());
        let mut entry = [0_u8; 64];
        entry[0] = 1; // Surface triangles.
        entry[1] = 1; // Zstandard codec.
        entry[8..16].copy_from_slice(&128_u64.to_le_bytes());
        entry[16..24].copy_from_slice(&chunk_len.to_le_bytes());
        entry[24..32].copy_from_slice(&1_u64.to_le_bytes());
        toc.extend_from_slice(&entry);
        let mut header = [0_u8; 56];
        header[..8].copy_from_slice(b"PNTMESH\0");
        header[8..12].copy_from_slice(&panta_mesh::MESH_ASSET_FORMAT_VERSION.to_le_bytes());
        header[12] = 1;
        header[13] = 1;
        header[16..24].copy_from_slice(&(128 - 56 + chunk_len).to_le_bytes());
        use sha2::Digest;
        header[24..56].copy_from_slice(&sha2::Sha256::digest(&toc));
        let file = File::create(&path)?;
        use std::io::Write;
        (&file).write_all(&header)?;
        (&file).write_all(&toc)?;
        file.set_len(128 + chunk_len)?;
        drop(file);

        // Cancellation after header and directory validation proves the read path accepts
        // the size without allocating or touching the sparse chunk.
        let result = read_mesh_asset(&path, || true);
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
    fn large_surface_assets_activate_as_display_lod_and_exports_read_in_full()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = crate::project::temp_directory::Fixture::new()?;
        let count = 60_000;
        let torus = panta_mesh::SyntheticTorus::new(count);
        let file = File::create(fixture.root.join("big.pmesh"))?;
        let (_, digest) =
            panta_mesh::write_surface_asset_streaming(file, count, |r, o| torus.fill(r, o), None)?
                .ok_or("cancelled")?;
        let request = || ActivationRequest {
            project_root: fixture.root.clone(),
            relative_path: "big.pmesh".to_owned(),
            expected_digest: digest.clone(),
        };
        let small = ActivationLimits {
            surface_triangles: 1_000,
            surface_budget: 500,
            volume_cells: 1_000,
        };
        let lod = load_asset(request(), || false, small)
            .map_err(|f| f.detail)?
            .ok_or("cancelled")?;
        assert_eq!(lod.lod_source, count as u64);
        assert!(lod.mesh.triangles.len() < count);
        let full = load_asset(request(), || false, ActivationLimits::RESIDENT)
            .map_err(|f| f.detail)?
            .ok_or("cancelled")?;
        assert_eq!((full.mesh.triangles.len(), full.lod_source), (count, 0));
        assert_eq!(
            load_committed_surface(request())
                .ok()
                .map(|m| m.triangles.len()),
            Some(count)
        );
        assert!(matches!(load_asset(request(), || true, small), Ok(None)));
        let mut wrong = request();
        wrong.expected_digest = "sha256:other".to_owned();
        assert!(load_asset(wrong, || false, small).is_err());
        Ok(())
    }

    #[test]
    fn large_volume_assets_activate_streamed_with_the_exact_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = crate::project::temp_directory::Fixture::new()?;
        let block = panta_mesh::SyntheticTetBlock::with_cells_per_axis(6);
        let file = File::create(fixture.root.join("volume.pmesh"))?;
        let (_, digest) =
            panta_mesh::write_volume_asset_streaming(file, block.counts(), &block, None)?
                .ok_or("cancelled")?;
        let request = || ActivationRequest {
            project_root: fixture.root.clone(),
            relative_path: "volume.pmesh".to_owned(),
            expected_digest: digest.clone(),
        };
        let limits = ActivationLimits {
            surface_triangles: u64::MAX,
            surface_budget: 500,
            volume_cells: 1_000,
        };
        let streamed = load_asset(request(), || false, limits)
            .map_err(|f| f.detail)?
            .ok_or("cancelled")?;
        assert!(streamed.volume.is_none());
        assert_eq!(streamed.lod_source, 0);
        assert_eq!(streamed.streamed_volume, Some(block.counts()));
        let resident = load_asset(request(), || false, ActivationLimits::DEFAULT)
            .map_err(|f| f.detail)?
            .ok_or("cancelled")?;
        assert!(resident.streamed_volume.is_none());
        assert_eq!(
            resident.volume.map(|v| v.tets.len()),
            Some(block.tet_count())
        );
        // 流式外表面与驻留外表面逐面一致。
        assert_eq!(streamed.mesh, resident.mesh);
        assert!(matches!(load_asset(request(), || true, limits), Ok(None)));
        let mut wrong = request();
        wrong.expected_digest = "sha256:other".to_owned();
        assert!(load_asset(wrong, || false, limits).is_err());

        // worker 使用协调器中的设置（task 026）：默认驻留，调低阈值后流式。
        let (coordinator, attempt) = coordinator_with_request();
        assert_eq!(coordinator.limits(), ActivationLimits::DEFAULT);
        coordinator.set_limits(limits);
        run_attempt(
            Arc::clone(&coordinator),
            attempt.attempt,
            request(),
            Arc::new(AtomicBool::new(false)),
        );
        let outcomes = coordinator.drain();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].streamed_volume, Some(block.counts()));
        Ok(())
    }

    #[test]
    fn activation_limits_validate_ranges_and_budget_below_threshold() {
        let field = |limits: ActivationLimits| match limits.validate() {
            Ok(()) => String::new(),
            Err(error) => {
                assert_eq!(error.code(), "project.lod_settings_invalid");
                error.detail()
            }
        };
        let base = ActivationLimits::DEFAULT;
        assert_eq!(field(base), "");
        let lowest = ActivationLimits {
            surface_triangles: 1_000_000,
            surface_budget: 100_000,
            volume_cells: 1_000_000,
        };
        assert_eq!(field(lowest), "");
        let highest = ActivationLimits {
            surface_triangles: 2_000_000_000,
            surface_budget: 20_000_000,
            volume_cells: 10_000_000_000,
        };
        assert_eq!(field(highest), "");
        let cases = [
            (
                ActivationLimits {
                    surface_triangles: 999_999,
                    ..lowest
                },
                "surface_triangles",
            ),
            (
                ActivationLimits {
                    surface_triangles: 2_000_000_001,
                    ..highest
                },
                "surface_triangles",
            ),
            (
                ActivationLimits {
                    surface_budget: 99_999,
                    ..base
                },
                "surface_budget",
            ),
            (
                ActivationLimits {
                    surface_budget: 20_000_001,
                    ..highest
                },
                "surface_budget",
            ),
            (
                ActivationLimits {
                    surface_budget: 1_000_000,
                    ..lowest
                },
                "surface_budget",
            ),
            (
                ActivationLimits {
                    volume_cells: 999_999,
                    ..base
                },
                "volume_cells",
            ),
            (
                ActivationLimits {
                    volume_cells: 10_000_000_001,
                    ..base
                },
                "volume_cells",
            ),
        ];
        for (limits, expected) in cases {
            assert_eq!(field(limits), expected, "{limits:?}");
        }
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
            Ok(Some(LoadedMesh::surface(SurfaceMesh {
                triangles: Vec::new(),
            }))),
        );
        // 请求已消费后，迟到结果不再发布。
        coordinator.complete(
            attempt.attempt,
            Ok(Some(LoadedMesh::surface(SurfaceMesh {
                triangles: Vec::new(),
            }))),
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
            Ok(Some(LoadedMesh::surface(SurfaceMesh {
                triangles: Vec::new(),
            }))),
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
            Ok(Some(LoadedMesh::surface(SurfaceMesh {
                triangles: Vec::new(),
            }))),
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
            Ok(Some(LoadedMesh::surface(SurfaceMesh {
                triangles: Vec::new(),
            }))),
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
