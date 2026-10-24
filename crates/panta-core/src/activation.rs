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

use panta_io::import::ImportError;
use panta_mesh::chunked::ChunkedVolume;
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
    /// 结果场 sidecar（task 029）：派生分块文件带场构建时使用。
    pub results: Option<ResultRequest>,
}

/// 激活请求中的结果场引用：工程内相对路径与清单摘要。
#[derive(Debug, Clone)]
pub(crate) struct ResultRequest {
    pub relative_path: String,
    pub digest: String,
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
    /// 流式体网格的派生分块文件（task 028）。
    pub chunked_volume: Option<Arc<ChunkedVolume>>,
}

/// 超过此面数的表面资产激活时改为流式构建显示 LOD，不整体读入（约 1.4 GiB 展开体积）。
pub const LOD_ACTIVATION_TRIANGLES: u64 = 20_000_000;

/// 超过此四面体数的体网格资产激活时不驻留单元（task 025）：只流式提取外表面显示，
/// 质量统计、切面与 `.vtu` 导出按需从资产流式读取。驻留读取含完整拓扑校验，
/// 实测 1010 万四面体峰值 1.8 GiB、15.8 s，而流式质量/切面同规模各约 0.3 s，
/// 默认值取 1000 万以 8 GiB 内存机器为保守上限（基准见 task 025 文档）。
pub const VOLUME_STREAM_ACTIVATION_CELLS: u64 = 10_000_000;

/// 完整驻留体网格的估计内存（B / 四面体）：task 025 实测 1010 万四面体峰值 1.8 GiB。
pub const RESIDENT_BYTES_PER_TET: u64 = 190;
/// 分块块缓存预算默认值（MiB，task 028）：1 亿四面体斜切冷精细约读 500 MiB 块，1 GiB 可覆盖一次精细切面。
pub const DEFAULT_CHUNK_CACHE_MIB: u64 = 1024;
/// 完整驻留顺序读的缓冲大小：MiB 级单次读，减少 Windows 上的读调用（task 028）。
pub(crate) const RESIDENT_READ_BUFFER: usize = 8 << 20;

/// 体网格激活模式（task 028）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VolumeMode {
    /// 四面体数超过 [`ActivationLimits::volume_cells`] 时分块按需读取。
    #[default]
    Threshold,
    /// 按可用物理内存的一半推算阈值（[`RESIDENT_BYTES_PER_TET`] / 四面体）；宿主未提供内存时用手动阈值。
    AutoMemory,
    /// 禁用分块、完整驻留：始终整体读入进程内存（大块顺序读，读完即关闭文件，不映射）。
    Resident,
}

impl VolumeMode {
    /// FFI 编码：0 阈值、1 按可用内存自动、2 完整驻留。
    pub fn from_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(Self::Threshold),
            1 => Some(Self::AutoMemory),
            2 => Some(Self::Resident),
            _ => None,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            Self::Threshold => 0,
            Self::AutoMemory => 1,
            Self::Resident => 2,
        }
    }
}

/// 大网格激活设置（task 026 / 028）：表面 LOD 触发面数与 LOD 输出预算、体网格分块阈值、
/// 体网格模式与块缓存预算。只影响之后的激活；已驻留文档不重算（块缓存预算立即生效）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivationLimits {
    /// 表面资产面数超过此值时只驻留显示 LOD。
    pub surface_triangles: u64,
    /// 显示 LOD 的面数上限。
    pub surface_budget: u64,
    /// 体网格四面体数超过此值时单元不驻留、命令按需分块读取。
    pub volume_cells: u64,
    pub volume_mode: VolumeMode,
    /// 分块块缓存预算（MiB）。
    pub chunk_cache_mib: u64,
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
        volume_mode: VolumeMode::Threshold,
        chunk_cache_mib: DEFAULT_CHUNK_CACHE_MIB,
    };
    /// 表面触发阈值范围（面）。
    pub const SURFACE_TRIANGLES_RANGE: std::ops::RangeInclusive<u64> = 1_000_000..=2_000_000_000;
    /// LOD 预算范围（面），且须小于触发阈值。
    pub const SURFACE_BUDGET_RANGE: std::ops::RangeInclusive<u64> = 100_000..=20_000_000;
    /// 体网格流式激活阈值范围（四面体）。
    pub const VOLUME_CELLS_RANGE: std::ops::RangeInclusive<u64> = 1_000_000..=10_000_000_000;
    /// 块缓存预算范围（MiB）：64 MiB 到 1 TiB。
    pub const CHUNK_CACHE_MIB_RANGE: std::ops::RangeInclusive<u64> = 64..=1_048_576;

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
        if !Self::CHUNK_CACHE_MIB_RANGE.contains(&self.chunk_cache_mib) {
            return invalid("chunk_cache_mib");
        }
        Ok(())
    }

    /// 生效的体网格分块阈值（四面体）。`available_memory` 为宿主报告的可用物理内存（B），0 表示未知。
    pub fn volume_cell_threshold(&self, available_memory: u64) -> u64 {
        match self.volume_mode {
            VolumeMode::Threshold => self.volume_cells,
            VolumeMode::Resident => u64::MAX,
            VolumeMode::AutoMemory if available_memory == 0 => self.volume_cells,
            VolumeMode::AutoMemory => (available_memory / 2 / RESIDENT_BYTES_PER_TET).clamp(
                *Self::VOLUME_CELLS_RANGE.start(),
                *Self::VOLUME_CELLS_RANGE.end(),
            ),
        }
    }

    /// 块缓存预算（B）。
    pub fn chunk_cache_bytes(&self) -> usize {
        usize::try_from(self.chunk_cache_mib.saturating_mul(1 << 20)).unwrap_or(usize::MAX)
    }
    /// 完整读取（导出等只读命令）：不走 LOD 或流式激活。
    pub(crate) const RESIDENT: Self = Self {
        surface_triangles: u64::MAX,
        surface_budget: u64::MAX,
        volume_cells: u64::MAX,
        volume_mode: VolumeMode::Resident,
        chunk_cache_mib: DEFAULT_CHUNK_CACHE_MIB,
    };
}

/// 宿主环境（task 028）：可用内存与派生分块缓存目录，worker 开始读取时取值。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ActivationEnvironment {
    /// 可用物理内存（B），0 表示未知。
    pub(crate) available_memory: u64,
    /// 派生 `.pchunk` 缓存目录；`None` 时超过阈值的体网格退回逐次流式扫描（task 025）。
    pub(crate) derived_dir: Option<PathBuf>,
}

/// 一次读取的全部设置。
#[derive(Debug, Clone, Default)]
pub(crate) struct LoadOptions {
    pub(crate) limits: ActivationLimits,
    pub(crate) environment: ActivationEnvironment,
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
    /// 流式体网格的派生分块文件（task 028）；不可用时命令退回逐次流式扫描。
    pub(crate) chunked_volume: Option<Arc<ChunkedVolume>>,
}

impl LoadedMesh {
    fn surface(mesh: SurfaceMesh) -> Self {
        Self {
            mesh,
            lod_source: 0,
            volume: None,
            streamed_volume: None,
            chunked_volume: None,
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
    /// 宿主环境（task 028）。
    environment: Mutex<ActivationEnvironment>,
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

    pub(crate) fn set_available_memory(&self, bytes: u64) {
        self.environment
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .available_memory = bytes;
    }

    pub(crate) fn set_derived_dir(&self, directory: Option<PathBuf>) {
        self.environment
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .derived_dir = directory;
    }

    pub(crate) fn environment(&self) -> ActivationEnvironment {
        self.environment
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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
        chunked_volume: None,
    };
    match result {
        Ok(Some(loaded)) if !entry.cancel_requested.load(Ordering::SeqCst) => {
            outcome.kind = OutcomeKind::Succeeded;
            outcome.mesh = Some(loaded.mesh);
            outcome.lod_source_triangles = loaded.lod_source;
            outcome.volume = loaded.volume;
            outcome.streamed_volume = loaded.streamed_volume;
            outcome.chunked_volume = loaded.chunked_volume;
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
    let options = LoadOptions {
        limits: coordinator.limits(),
        environment: coordinator.environment(),
    };
    let result = load_asset_with(
        request,
        || cancel_requested.load(Ordering::SeqCst),
        &options,
    );
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
    let options = LoadOptions {
        limits,
        environment: ActivationEnvironment::default(),
    };
    load_asset_with(request, cancelled, &options)
}

fn load_asset_with(
    request: ActivationRequest,
    cancelled: impl Fn() -> bool,
    options: &LoadOptions,
) -> Result<Option<LoadedMesh>, ActivationFailure> {
    let limits = options.limits;
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
    // 结果场路径不合法时按无结果场处理：几何激活不应因可选 sidecar 失败。
    let results_path = request.results.as_ref().and_then(|results| {
        crate::project::resolve_asset_path(&request.project_root, &results.relative_path)
            .ok()
            .map(|path| (path, results.digest.as_str()))
    });
    if let Some(loaded) = load_streamed_volume(
        &asset_path,
        &request.expected_digest,
        results_path
            .as_ref()
            .map(|(path, digest)| crate::derived_chunks::ResultSource { path, digest }),
        &cancelled,
        options,
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
    results: Option<crate::derived_chunks::ResultSource<'_>>,
    cancelled: &impl Fn() -> bool,
    options: &LoadOptions,
) -> Result<Option<Option<LoadedMesh>>, ActivationFailure> {
    let cell_threshold = options
        .limits
        .volume_cell_threshold(options.environment.available_memory);
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
    drop(chunks);
    match result {
        Ok(Some(surface)) if !cancelled() => {
            let chunked_volume = crate::derived_chunks::open_or_build(
                path,
                expected_digest,
                results,
                counts,
                options,
                cancelled,
            )?;
            if cancelled() {
                return Ok(Some(None));
            }
            Ok(Some(Some(LoadedMesh {
                streamed_volume: Some(counts),
                chunked_volume,
                ..LoadedMesh::surface(surface)
            })))
        }
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
pub(crate) fn with_cancellation<T: Send>(
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
    // 完整驻留（用户决策 2026-10-24 12:31 UTC+8）：MiB 级大块顺序读进进程自有内存，不映射；
    // `read` 消费读取器，返回时文件句柄已关闭。
    let reader = MeshAssetReader::open(
        BufReader::with_capacity(RESIDENT_READ_BUFFER, file),
        file_len,
    )
    .map_err(|error| match error {
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
                    results: None,
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
                results: None,
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
            results: None,
        };
        let small = ActivationLimits {
            surface_triangles: 1_000,
            surface_budget: 500,
            volume_cells: 1_000,
            ..ActivationLimits::DEFAULT
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
            results: None,
        };
        let limits = ActivationLimits {
            surface_triangles: u64::MAX,
            surface_budget: 500,
            volume_cells: 1_000,
            ..ActivationLimits::DEFAULT
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
    fn derived_chunks_are_built_once_reused_and_gated_by_memory()
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
            results: None,
        };
        let cache = fixture.root.join("cache").join("volume-chunks");
        let mut options = LoadOptions {
            limits: ActivationLimits {
                volume_cells: 1_000,
                ..ActivationLimits::DEFAULT
            },
            environment: ActivationEnvironment {
                available_memory: 0,
                derived_dir: Some(cache.clone()),
            },
        };
        let target = cache.join(crate::derived_chunks::file_name(&digest, None).ok_or("digest")?);
        let listing = || -> Result<Vec<String>, std::io::Error> {
            let mut names = std::fs::read_dir(&cache)?
                .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
                .collect::<Result<Vec<_>, _>>()?;
            names.sort();
            Ok(names)
        };
        {
            let loaded = load_asset_with(request(), || false, &options)
                .map_err(|f| f.detail)?
                .ok_or("cancelled")?;
            let chunks = loaded.chunked_volume.ok_or("chunks missing")?;
            assert_eq!(chunks.tet_count(), block.tet_count() as u64);
            assert_eq!(chunks.cache().budget(), options.limits.chunk_cache_bytes());
            assert_eq!(loaded.streamed_volume, Some(block.counts()));
            let bytes = std::fs::read(&target)?;
            // 第二次激活复用同一文件，不重写。
            let again = load_asset_with(request(), || false, &options)
                .map_err(|f| f.detail)?
                .ok_or("cancelled")?;
            assert!(again.chunked_volume.is_some());
            assert_eq!(std::fs::read(&target)?, bytes);
            assert_eq!(listing()?.len(), 1);
        }
        // 无效文件（头部校验失败）重建；Windows 上须先释放映射才能覆盖。
        std::fs::write(&target, b"not a chunk file")?;
        {
            let rebuilt = load_asset_with(request(), || false, &options)
                .map_err(|f| f.detail)?
                .ok_or("cancelled")?;
            assert!(rebuilt.chunked_volume.is_some());
        }
        // 可用内存不足以构建时退回逐次流式扫描，不写文件。
        std::fs::remove_file(&target)?;
        options.environment.available_memory = 1;
        let gated = load_asset_with(request(), || false, &options)
            .map_err(|f| f.detail)?
            .ok_or("cancelled")?;
        assert!(gated.chunked_volume.is_none());
        assert_eq!(gated.streamed_volume, Some(block.counts()));
        assert!(listing()?.is_empty());
        // 没有缓存目录时同样退回（task 025 行为）。
        options.environment = ActivationEnvironment::default();
        let plain = load_asset_with(request(), || false, &options)
            .map_err(|f| f.detail)?
            .ok_or("cancelled")?;
        assert!(plain.chunked_volume.is_none());
        // 完整驻留模式不分块。
        options.limits.volume_mode = VolumeMode::Resident;
        let resident = load_asset_with(request(), || false, &options)
            .map_err(|f| f.detail)?
            .ok_or("cancelled")?;
        assert!(resident.streamed_volume.is_none() && resident.chunked_volume.is_none());
        assert_eq!(
            resident.volume.map(|v| v.tets.len()),
            Some(block.tet_count())
        );
        Ok(())
    }

    #[test]
    fn mixed_assets_build_derived_chunks_with_original_cell_ids()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::project::{
            SharedChunks, StreamedAsset, StreamedKind, section_plane_field,
            streamed_section_plane_field, streamed_volume_quality,
        };
        use panta_mesh::{FieldAssociation, FieldComponent, ResultField};
        let fixture = crate::project::temp_directory::Fixture::new()?;
        let block = panta_mesh::SyntheticMixedBlock::with_layers(4, 2, 2);
        let counts = block.counts();
        assert_ne!(counts.cells, counts.tets);
        let file = File::create(fixture.root.join("mixed.pmesh"))?;
        let (_, digest) = panta_mesh::write_volume_asset_streaming(file, counts, &block, None)?
            .ok_or("cancelled")?;
        let field = ResultField {
            name: "cell".to_owned(),
            association: FieldAssociation::Cell,
            components: 1,
            units: String::new(),
            values: (0..counts.cells).map(|cell| (cell % 97) as f64).collect(),
        };
        let (bytes, result_digest) = panta_mesh::encode_result_asset(&panta_mesh::ResultSet {
            mesh_digest: digest.clone(),
            node_count: counts.nodes as u64,
            cell_count: counts.cells as u64,
            fields: vec![field.clone()],
        })?;
        std::fs::write(fixture.root.join("mixed.presult"), bytes)?;
        let panta_mesh::MeshAsset::Volume(mesh) =
            panta_mesh::decode_mesh_asset(&std::fs::read(fixture.root.join("mixed.pmesh"))?)?
        else {
            return Err("not a volume".into());
        };
        let options = LoadOptions {
            limits: ActivationLimits {
                volume_cells: 100,
                ..ActivationLimits::DEFAULT
            },
            environment: ActivationEnvironment {
                available_memory: 0,
                derived_dir: Some(fixture.root.join("cache")),
            },
        };
        let request = ActivationRequest {
            project_root: fixture.root.clone(),
            relative_path: "mixed.pmesh".to_owned(),
            expected_digest: digest.clone(),
            results: Some(ResultRequest {
                relative_path: "mixed.presult".to_owned(),
                digest: result_digest,
            }),
        };
        let loaded = load_asset_with(request, || false, &options)
            .map_err(|f| f.detail)?
            .ok_or("cancelled")?;
        assert_eq!(loaded.streamed_volume, Some(counts));
        // 混合资产也构建派生分块（task 030 PR 3 起），单元数为原始单元数。
        let chunks = loaded.chunked_volume.ok_or("chunks missing")?;
        assert_eq!(chunks.cell_count(), counts.cells as u64);
        assert_eq!(chunks.tet_count(), counts.tets as u64);
        assert_eq!(chunks.fields().len(), 1);
        let chunked = StreamedAsset {
            path: fixture.root.join("mixed.pmesh"),
            digest,
            kind: StreamedKind::Volume(counts),
            chunks: Some(SharedChunks(chunks)),
        };
        let scanned = StreamedAsset {
            chunks: None,
            ..chunked.clone()
        };
        let progress = panta_mesh::Progress::new();
        let from_chunks = streamed_volume_quality(&chunked, &progress)?.ok_or("cancelled")?;
        let from_scan = streamed_volume_quality(&scanned, &progress)?.ok_or("cancelled")?;
        assert_eq!(
            from_chunks.by_type.map(|q| q.count as u64),
            block.type_counts()
        );
        assert_eq!(from_chunks.poor_count, from_scan.poor_count);
        for (a, b) in from_chunks.by_type.iter().zip(&from_scan.by_type) {
            assert_eq!(a.min_scaled_jacobian, b.min_scaled_jacobian);
            assert!((a.mean_scaled_jacobian - b.mean_scaled_jacobian).abs() < 1e-9);
        }
        // 单元场切面：分块（按需读块）与驻留网格的值集合一致。
        let (normal, fraction) = ([0.3, 1.0, 0.2], 0.45);
        let keys = |section: &panta_mesh::FieldSection| {
            let mut keys: Vec<_> = section
                .surface
                .triangles
                .iter()
                .zip(&section.values)
                .map(|(t, v)| (t.map(|p| p.map(f64::to_bits)), v.map(f64::to_bits)))
                .collect();
            keys.sort_unstable();
            keys
        };
        let a = streamed_section_plane_field(
            &chunked,
            "cell",
            FieldAssociation::Cell,
            FieldComponent::Magnitude,
            normal,
            fraction,
            &progress,
        )?
        .ok_or("cancelled")?;
        let b = section_plane_field(
            &mesh,
            &field,
            FieldComponent::Magnitude,
            normal,
            fraction,
            &progress,
        )?
        .ok_or("cancelled")?;
        assert!(!a.values.is_empty());
        assert_eq!(keys(&a), keys(&b));
        Ok(())
    }

    #[test]
    fn derived_chunks_carry_result_fields_and_serve_field_sections()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::project::{
            ProjectError, SharedChunks, StreamedAsset, StreamedKind, section_plane,
            section_plane_field, streamed_section_plane, streamed_section_plane_field,
        };
        use panta_mesh::{FieldAssociation, FieldComponent, ResultField};
        let fixture = crate::project::temp_directory::Fixture::new()?;
        let block = panta_mesh::SyntheticTetBlock::with_cells_per_axis(6);
        let file = File::create(fixture.root.join("volume.pmesh"))?;
        let (_, digest) =
            panta_mesh::write_volume_asset_streaming(file, block.counts(), &block, None)?
                .ok_or("cancelled")?;
        let mesh = panta_mesh::TetMesh {
            nodes: (0..block.node_count()).map(|i| block.node(i)).collect(),
            tets: (0..block.tet_count()).map(|i| block.tet(i)).collect(),
            boundary: (0..block.boundary_count())
                .map(|i| block.boundary_face(i))
                .collect(),
            region_count: 1,
            boundary_group_count: 6,
            cells: None,
        };
        let fields = vec![
            ResultField {
                name: "t".to_owned(),
                association: FieldAssociation::Node,
                components: 1,
                units: String::new(),
                values: mesh.nodes.iter().map(|p| p[0] - p[2]).collect(),
            },
            ResultField {
                name: "id".to_owned(),
                association: FieldAssociation::Cell,
                components: 1,
                units: String::new(),
                values: (0..mesh.tets.len()).map(|i| i as f64).collect(),
            },
        ];
        let (bytes, result_digest) = panta_mesh::encode_result_asset(&panta_mesh::ResultSet {
            mesh_digest: digest.clone(),
            node_count: mesh.nodes.len() as u64,
            cell_count: mesh.tets.len() as u64,
            fields: fields.clone(),
        })?;
        std::fs::write(fixture.root.join("volume.presult"), bytes)?;
        let cache = fixture.root.join("cache");
        let options = LoadOptions {
            limits: ActivationLimits {
                volume_cells: 1_000,
                ..ActivationLimits::DEFAULT
            },
            environment: ActivationEnvironment {
                available_memory: 0,
                derived_dir: Some(cache.clone()),
            },
        };
        let request = |results_digest: &str| ActivationRequest {
            project_root: fixture.root.clone(),
            relative_path: "volume.pmesh".to_owned(),
            expected_digest: digest.clone(),
            results: Some(ResultRequest {
                relative_path: "volume.presult".to_owned(),
                digest: results_digest.to_owned(),
            }),
        };
        let loaded = load_asset_with(request(&result_digest), || false, &options)
            .map_err(|f| f.detail)?
            .ok_or("cancelled")?;
        let chunks = loaded.chunked_volume.ok_or("chunks missing")?;
        assert_eq!(chunks.fields().len(), 2);
        let name =
            crate::derived_chunks::file_name(&digest, Some(&result_digest)).ok_or("digest")?;
        assert!(cache.join(&name).is_file());

        let asset = StreamedAsset {
            path: fixture.root.join("volume.pmesh"),
            digest: digest.clone(),
            kind: StreamedKind::Volume(block.counts()),
            chunks: Some(SharedChunks(chunks)),
        };
        let progress = panta_mesh::Progress::new();
        let sorted = |section: &panta_mesh::FieldSection| {
            let mut keys: Vec<_> = section
                .surface
                .triangles
                .iter()
                .zip(&section.values)
                .map(|(t, v)| (t.map(|p| p.map(f64::to_bits)), v.map(f64::to_bits)))
                .collect();
            keys.sort_unstable();
            keys
        };
        for (field, association) in [
            ("t", FieldAssociation::Node),
            ("id", FieldAssociation::Cell),
        ] {
            let normal = [0.2, 1.0, 0.4];
            let streamed = streamed_section_plane_field(
                &asset,
                field,
                association,
                FieldComponent::Magnitude,
                normal,
                0.4,
                &progress,
            )?
            .ok_or("cancelled")?;
            let plain =
                streamed_section_plane(&asset, normal, 0.4, &progress)?.ok_or("cancelled")?;
            assert_eq!(streamed.surface, plain);
            let index = usize::from(association == FieldAssociation::Cell);
            let resident = section_plane_field(
                &mesh,
                &fields[index],
                FieldComponent::Magnitude,
                normal,
                0.4,
                &progress,
            )?
            .ok_or("cancelled")?;
            assert_eq!(
                resident.surface,
                section_plane(&mesh, normal, 0.4, &progress)?.ok_or("cancelled")?
            );
            assert_eq!(sorted(&streamed), sorted(&resident));
        }
        assert!(matches!(
            streamed_section_plane_field(
                &asset,
                "t",
                FieldAssociation::Cell,
                FieldComponent::Magnitude,
                [0.0, 0.0, 1.0],
                0.5,
                &progress
            ),
            Err(ProjectError::CommandInvalid(_))
        ));
        assert!(matches!(
            streamed_section_plane_field(
                &asset,
                "t",
                FieldAssociation::Node,
                FieldComponent::Magnitude,
                [0.0, 0.0, 0.0],
                0.5,
                &progress
            ),
            Err(ProjectError::CommandInvalid(_))
        ));
        // 逐次流式回退（无派生文件）不支持场剖面。
        let fallback = StreamedAsset {
            chunks: None,
            ..asset.clone()
        };
        assert!(matches!(
            streamed_section_plane_field(
                &fallback,
                "t",
                FieldAssociation::Node,
                FieldComponent::Magnitude,
                [0.0, 0.0, 1.0],
                0.5,
                &progress
            ),
            Err(ProjectError::CommandInvalid(_))
        ));
        drop(asset);

        // 结果摘要不符：按无结果场构建，几何派生文件照常可用。
        let wrong = format!("sha256:{}", "0".repeat(64));
        let plain = load_asset_with(request(&wrong), || false, &options)
            .map_err(|f| f.detail)?
            .ok_or("cancelled")?;
        let chunks = plain.chunked_volume.ok_or("chunks missing")?;
        assert!(chunks.fields().is_empty());
        let plain_name = crate::derived_chunks::file_name(&digest, None).ok_or("digest")?;
        assert!(cache.join(plain_name).is_file());
        assert!(crate::derived_chunks::file_name(&digest, Some("md5:00")).is_none());
        Ok(())
    }

    #[test]
    fn derived_chunk_cache_keeps_the_newest_files() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = crate::project::temp_directory::Fixture::new()?;
        let base = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for index in 0..6u64 {
            let path = fixture.root.join(format!("{index:064x}.pchunk"));
            let file = File::create(&path)?;
            file.set_modified(base + std::time::Duration::from_secs(index * 60))?;
        }
        let stale = fixture.root.join("a.pchunk.tmp-1-2");
        File::create(&stale)?.set_modified(base - std::time::Duration::from_secs(86_400))?;
        let fresh = fixture.root.join("b.pchunk.tmp-1-2");
        File::create(&fresh)?;
        let other = fixture.root.join("notes.txt");
        File::create(&other)?;
        let keep = fixture.root.join(format!("{:064x}.pchunk", 0));
        crate::derived_chunks::prune(&fixture.root, &keep);
        let mut names: Vec<String> = std::fs::read_dir(&fixture.root)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let expected = [
            format!("{:064x}.pchunk", 0),
            format!("{:064x}.pchunk", 3),
            format!("{:064x}.pchunk", 4),
            format!("{:064x}.pchunk", 5),
            "b.pchunk.tmp-1-2".to_owned(),
            "notes.txt".to_owned(),
        ];
        assert_eq!(names, expected);
        assert_eq!(crate::derived_chunks::file_name("sha256:short", None), None);
        assert_eq!(crate::derived_chunks::file_name("md5:00", None), None);
        Ok(())
    }

    #[test]
    fn volume_modes_resolve_the_effective_threshold() {
        let manual = ActivationLimits {
            volume_cells: 20_000_000,
            ..ActivationLimits::DEFAULT
        };
        assert_eq!(manual.volume_cell_threshold(64 << 30), 20_000_000);
        let resident = ActivationLimits {
            volume_mode: VolumeMode::Resident,
            ..manual
        };
        assert_eq!(resident.volume_cell_threshold(0), u64::MAX);
        let auto = ActivationLimits {
            volume_mode: VolumeMode::AutoMemory,
            ..manual
        };
        assert_eq!(auto.volume_cell_threshold(0), 20_000_000);
        // 64 GiB 可用：一半 32 GiB / 190 B ≈ 1.8 亿四面体。
        assert_eq!(
            auto.volume_cell_threshold(64 << 30),
            (32u64 << 30) / RESIDENT_BYTES_PER_TET
        );
        assert_eq!(auto.volume_cell_threshold(1 << 20), 1_000_000);
        assert_eq!(auto.volume_cell_threshold(u64::MAX), 10_000_000_000);
        for index in 0..3 {
            assert_eq!(
                VolumeMode::from_index(index).map(VolumeMode::index),
                Some(index)
            );
        }
        assert_eq!(VolumeMode::from_index(3), None);
        assert_eq!(
            ActivationLimits::RESIDENT.volume_cell_threshold(0),
            u64::MAX
        );
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
            ..ActivationLimits::DEFAULT
        };
        assert_eq!(field(lowest), "");
        let highest = ActivationLimits {
            surface_triangles: 2_000_000_000,
            surface_budget: 20_000_000,
            volume_cells: 10_000_000_000,
            ..ActivationLimits::DEFAULT
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
            results: None,
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
                    results: None,
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
            results: None,
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
                    results: None,
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
