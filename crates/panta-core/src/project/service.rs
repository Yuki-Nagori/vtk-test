//! 工程会话与应用服务编排；格式解析、存储及缓存各有专用模块。
use super::model::{MeshAssetRef, next_revision, validate_name};
use super::repository::write_manifest;
use super::{
    ActivationAttempt, ActivationResultKind, DocumentDiagnostic, DocumentWorkspace,
    IMPORT_RECORD_VERSION, ImportRecord, ProjectCommand, ProjectError, ProjectSnapshot,
    ProjectState, StlImportSession, SurfaceMeshCache, WorkspaceActivationRequest,
    WorkspaceSnapshot, activation_error_message, preview,
};
use panta_mesh::SurfaceMesh;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 当前工程 service。其所有状态都由 Rust 拥有，C++ 仅持有 opaque Box。
/// `path` 是稳定的 `.panta` 主文件身份；重命名命令只更新清单名称，不移动
/// 工程目录或文件，避免把资产路径变更混入最小模型命令。
///
/// `generation` 是会话级运行期计数（create/open 各递增一次，不持久化），
/// 与 `ImportRecord.id` / attempt 一起构成只读资产激活的相关性键。
#[derive(Debug, Default)]
pub struct ProjectService {
    pub(super) current: Option<ProjectState>,
    pub(super) import_session: StlImportSession,
    pub(super) latest_mesh_id: Option<String>,
    pub(super) mesh_cache: SurfaceMeshCache,
    pub(super) activation_attempts: HashMap<String, u64>,
    /// 驻留网格为显示 LOD 时记录源面数（task 019）。
    pub(super) mesh_lod_sources: HashMap<String, u64>,
    /// 驻留网格来自体网格资产时的四面体数据（task 021）；显示网格为其外表面。
    pub(super) volume_meshes: HashMap<String, Arc<panta_mesh::TetMesh>>,
    /// 流式激活的体网格（task 025）：单元不驻留，按需从资产读取；值为资产计数。
    pub(super) streamed_volumes: HashMap<String, panta_mesh::VolumeStreamCounts>,
    /// 流式体网格的派生分块文件（task 028）：切面与质量按需读块。
    pub(super) chunked_volumes: HashMap<String, std::sync::Arc<panta_mesh::chunked::ChunkedVolume>>,
    pub(super) activation: Arc<crate::activation::ActivationCoordinator>,
    // 候选提交期间锁住工程元数据；Mesh 驻留和只读显示操作仍可继续。
    pub(super) pending_project: Option<super::command_job::PendingProjectOperation>,
    pub(super) pending_preview: Option<preview::PendingPreview>,
    pub(super) preview_request: u64,
    pub(super) pending_import: Option<super::import::PendingImport>,
    pub(super) import_request: u64,
    pub(super) execution: crate::execution::ProjectExecution,
    pub(super) workspace: DocumentWorkspace,
}

impl ProjectService {
    pub fn new() -> Self {
        Self::default()
    }

    // 后台候选已冻结；消费结果前，任何工程写入都会与候选清单竞争。
    pub(super) fn ensure_project_writable(&self) -> Result<(), ProjectError> {
        if self.pending_project.is_some() {
            return Err(ProjectError::CommandInvalid(
                "project operation pending".to_owned(),
            ));
        }
        if self.pending_import.is_some() {
            return Err(ProjectError::CommandInvalid(
                "STL import pending".to_owned(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn with_mesh_cache_budget(budget_bytes: usize) -> Self {
        let mut service = Self::default();
        service.mesh_cache = SurfaceMeshCache::with_budget(budget_bytes);
        service
    }

    /// 同步工程入口供 CLI 使用；GUI 通过后台工程操作执行同一准备流程。
    pub fn create(&mut self, location: &Path, name: &str) -> Result<ProjectSnapshot, ProjectError> {
        self.ensure_project_writable()?;
        let prepared = super::command_job::prepare_create(location, name)?;
        self.publish_project_replacement(prepared)
    }

    /// 打开清单，不加载 Mesh sidecar；GUI 使用后台工程操作。
    pub fn open(&mut self, path: &Path) -> Result<ProjectSnapshot, ProjectError> {
        self.ensure_project_writable()?;
        let prepared = super::command_job::prepare_open(path)?;
        self.publish_project_replacement(prepared)
    }

    pub(super) fn publish_project_replacement(
        &mut self,
        prepared: super::command_job::PreparedReplacement,
    ) -> Result<ProjectSnapshot, ProjectError> {
        self.current = Some(prepared.state);
        self.clear_stl_preview();
        self.latest_mesh_id = None;
        self.mesh_cache.clear();
        self.mesh_lod_sources.clear();
        self.volume_meshes.clear();
        self.streamed_volumes.clear();
        self.chunked_volumes.clear();
        self.activation_attempts.clear();
        self.activation.advance_generation();
        self.reset_document_workspace();
        self.snapshot()
    }

    /// 将当前模型以同目录临时文件写入后替换 `.panta` 主文件。
    pub fn save(&mut self) -> Result<ProjectSnapshot, ProjectError> {
        self.ensure_project_writable()?;
        let state = self.current.as_mut().ok_or(ProjectError::NoProject)?;
        write_manifest(state)?;
        state.dirty = false;
        self.snapshot()
    }

    /// 执行工程命令；打开命令复用工程读取流程，模型修改命令更新内存修订。
    pub fn execute(&mut self, command: ProjectCommand) -> Result<ProjectSnapshot, ProjectError> {
        match command {
            ProjectCommand::Open { path } => self.open(&path),
            ProjectCommand::Rename { name } => {
                self.ensure_project_writable()?;
                let state = self.current.as_mut().ok_or(ProjectError::NoProject)?;
                validate_name(&name)?;
                if name == state.name {
                    return Err(ProjectError::CommandInvalid("name is unchanged".to_owned()));
                }
                let revision = next_revision(state.revision)?;
                state.name = name;
                state.revision = revision;
                state.dirty = true;
                self.snapshot()
            }
        }
    }

    pub fn current(&self) -> Result<ProjectSnapshot, ProjectError> {
        self.snapshot()
    }

    /// 返回当前工程中已持久化导入记录的副本。
    pub fn imports(&self) -> Result<Vec<ImportRecord>, ProjectError> {
        self.current
            .as_ref()
            .map(|state| state.imports.clone())
            .ok_or(ProjectError::NoProject)
    }

    /// 按 `ImportRecord.id` 发起只读资产激活。
    ///
    /// 记录不存在或无工程时同步失败；同会话同记录的在飞 attempt 去重复用。
    /// 路径包含关系、符号链接和文件读取由 worker 校验，失败经终态返回。
    /// 结果经 [`Self::drain_asset_activations`] 拉取；加载不修改
    /// revision / dirty，也不写任何存储。
    pub fn begin_asset_activation(
        &mut self,
        import_id: &str,
    ) -> Result<ActivationAttempt, ProjectError> {
        let request = self.activation_request(import_id)?;
        let import_id = import_id.to_owned();
        let attempt = self
            .activation
            .begin(&import_id, request, Arc::clone(&self.execution.reads))
            .map_err(|detail| ProjectError::CommandInvalid(detail.to_owned()))?;
        self.activation_attempts.insert(import_id, attempt.attempt);
        Ok(attempt)
    }

    /// 已提交 Surface 资产的读取请求；校验清单索引但不访问文件系统。
    pub(super) fn activation_request(
        &self,
        import_id: &str,
    ) -> Result<crate::activation::ActivationRequest, ProjectError> {
        let state = self.current.as_ref().ok_or(ProjectError::NoProject)?;
        let record = state
            .imports
            .iter()
            .find(|record| record.id == import_id)
            .ok_or_else(|| ProjectError::ImportRecordMissing(import_id.to_owned()))?;
        let root = state
            .path
            .parent()
            .ok_or_else(|| ProjectError::Io("project has no package directory".to_owned()))?;
        let asset_ref = state
            .mesh_assets
            .get(&record.mesh_asset_id)
            .ok_or_else(|| {
                ProjectError::ManifestInvalid("mesh asset index entry is missing".to_owned())
            })?;
        if asset_ref.revision == 0
            || !panta_mesh::is_readable_mesh_asset_version(asset_ref.format_version)
            || !matches!(asset_ref.kind.as_str(), "surface" | "volume")
            || asset_ref.source_revision != record.source_digest
        {
            return Err(ProjectError::ManifestInvalid(
                "mesh asset index entry is unsupported".to_owned(),
            ));
        }
        let project_root = root.to_owned();
        let relative_path = asset_ref.relative_path.clone();
        let expected_digest = asset_ref.digest.clone();
        Ok(crate::activation::ActivationRequest {
            project_root,
            relative_path,
            expected_digest,
        })
    }

    /// 把导入记录的网格导出到 `target`（格式由扩展名决定，坐标为毫米）。
    /// 已驻留时直接使用缓存，否则同步读取并校验 sidecar；不改变工程状态。
    pub fn export_import_mesh(
        &self,
        import_id: &str,
        target: &Path,
    ) -> Result<panta_io::export::ExportFormat, ProjectError> {
        panta_io::export::detect_export_format(target).map_err(export_error)?;
        // 驻留网格为显示 LOD 时缓存不是完整网格，改为流式读取资产（task 025）。
        if let Some(
            asset @ super::StreamedAsset {
                kind: super::StreamedKind::Surface { .. },
                ..
            },
        ) = self.streamed_asset_for_import(import_id)
        {
            return super::export_streamed_surface(&asset, target, &panta_mesh::Progress::new());
        }
        let loaded;
        let mesh = match self.mesh_for_import(import_id) {
            Some(mesh) => mesh,
            None => {
                let request = self.activation_request(import_id)?;
                loaded = crate::activation::load_committed_surface(request).map_err(|failure| {
                    ProjectError::ExportSourceUnavailable(format!(
                        "{}: {}",
                        failure.code, failure.detail
                    ))
                })?;
                &loaded
            }
        };
        export_mesh_file(mesh, target)
    }

    /// 仅投影已提交 sidecar 的相对路径，不访问文件系统或构成资产身份。
    /// 包目录包含关系与符号链接校验由激活 worker 在实际读取前执行。
    pub fn mesh_asset_path_for_import(&self, import_id: &str) -> Result<String, ProjectError> {
        let state = self.current.as_ref().ok_or(ProjectError::NoProject)?;
        let record = state
            .imports
            .iter()
            .find(|record| record.id == import_id)
            .ok_or_else(|| ProjectError::ImportRecordMissing(import_id.to_owned()))?;
        let asset = state
            .mesh_assets
            .get(&record.mesh_asset_id)
            .ok_or_else(|| {
                ProjectError::ManifestInvalid("mesh asset index entry is missing".to_owned())
            })?;
        Ok(asset.relative_path.clone())
    }

    /// 校验并替换大网格设置（task 026）；只影响之后的激活，已驻留文档不重算。
    /// 块缓存预算（task 028）立即作用于已打开的分块体网格（超出部分按 LRU 淘汰）。
    pub fn set_activation_limits(
        &mut self,
        limits: crate::activation::ActivationLimits,
    ) -> Result<(), ProjectError> {
        limits.validate()?;
        self.activation.set_limits(limits);
        for volume in self.chunked_volumes.values() {
            volume.cache().set_budget(limits.chunk_cache_bytes());
        }
        Ok(())
    }

    /// 宿主报告的可用物理内存（B，0 表示未知），用于“按可用内存自动”与派生分块构建（task 028）。
    pub fn set_available_memory(&mut self, bytes: u64) {
        self.activation.set_available_memory(bytes);
    }

    /// 派生分块缓存目录（task 028）；`None` 时超过阈值的体网格逐次流式扫描。
    pub fn set_derived_chunk_dir(&mut self, directory: Option<std::path::PathBuf>) {
        self.activation.set_derived_dir(directory);
    }

    pub fn activation_limits(&self) -> crate::activation::ActivationLimits {
        self.activation.limits()
    }

    /// 请求取消一次在飞激活；`Cancelled` 终态由 worker 在安全检查点确认。
    pub fn cancel_asset_activation(&mut self, attempt: u64) -> bool {
        self.activation.cancel(attempt)
    }

    /// 消费终态并完成过期校验、缓存发布和工作区更新；消费者只读取最终结果。
    pub fn drain_asset_activations(&mut self) -> Vec<super::CompletedActivation> {
        self.activation
            .drain()
            .into_iter()
            .map(|outcome| self.complete_asset_activation(outcome))
            .collect()
    }

    /// 抛弃已关闭页签的 attempt，避免其迟到成功结果进入 Mesh 缓存。
    pub fn forget_asset_activation(&mut self, import_id: &str) {
        self.activation_attempts.remove(import_id);
    }

    /// 仅接受该记录最近一次仍有效的激活结果，防止同 ID 页签重开后的旧结果覆盖新数据。
    pub fn is_current_activation_attempt(&self, import_id: &str, attempt: u64) -> bool {
        self.activation_attempts.get(import_id) == Some(&attempt)
    }

    /// 激活终态已消费；旧 attempt 不得清理同 ID 的较新 attempt。
    pub fn finish_asset_activation(&mut self, import_id: &str, attempt: u64) {
        if self.is_current_activation_attempt(import_id, attempt) {
            self.activation_attempts.remove(import_id);
        }
    }

    /// 将仍有效的成功激活产物移交给工程运行期缓存。
    pub fn cache_activation_result(
        &mut self,
        import_id: &str,
        attempt: u64,
        mesh: SurfaceMesh,
    ) -> bool {
        if !self.is_current_activation_attempt(import_id, attempt) {
            return false;
        }
        self.finish_asset_activation(import_id, attempt);
        self.latest_mesh_id = Some(import_id.to_owned());
        self.mesh_cache.insert_active(import_id.to_owned(), mesh);
        true
    }

    /// 将新导入成功提交的 Mesh 移交给工程运行期缓存，并标记为活动项。
    pub fn cache_activated_mesh(&mut self, import_id: String, mesh: SurfaceMesh) {
        self.latest_mesh_id = Some(import_id.clone());
        self.mesh_cache.insert_active(import_id, mesh);
    }

    /// 选择 Rust 缓存中的活动网格；未驻留时由调用方启动异步资产激活。
    pub fn activate_mesh_document(&mut self, import_id: &str) -> bool {
        if !self.mesh_cache.activate(import_id) {
            return false;
        }
        self.latest_mesh_id = Some(import_id.to_owned());
        true
    }

    /// 返回一个导入记录的只读网格，不复制领域数据。
    pub fn mesh_for_import(&self, import_id: &str) -> Option<&SurfaceMesh> {
        self.mesh_cache.get(import_id)
    }

    /// 当前 Rust 运行期 Mesh 驻留集合，供 Qt ViewModel 投影页签状态。
    pub fn resident_mesh_ids(&self) -> Vec<String> {
        self.mesh_cache.resident_ids()
    }

    /// 文档关闭后释放对应 Mesh；工程资产与 ImportRecord 保留。
    pub fn release_mesh_document(&mut self, import_id: &str) {
        self.forget_asset_activation(import_id);
        self.mesh_cache.remove(import_id);
        self.mesh_cache.trim();
        if self.latest_mesh_id.as_deref() == Some(import_id) {
            self.latest_mesh_id = None;
        }
    }

    /// 活动文档切到空白时取消 Mesh pin，再按预算回收缓存。
    pub fn deactivate_mesh_document(&mut self) {
        self.mesh_cache.deactivate();
    }

    /// 重置运行期页签，并取消其未完成的激活任务。
    pub fn reset_document_workspace(&mut self) {
        let old_documents = self.workspace.snapshot().documents;
        let attempts = self.workspace.reset();
        for attempt in attempts {
            self.cancel_asset_activation(attempt);
        }
        for document in old_documents {
            self.release_mesh_document(&document.id);
        }
        self.deactivate_mesh_document();
    }

    pub fn document_workspace(&self) -> WorkspaceSnapshot {
        let mut snapshot = self.workspace.snapshot();
        if let Some(state) = &self.current {
            for document in &mut snapshot.documents {
                let Some(import_index) = state
                    .imports
                    .iter()
                    .position(|record| record.id == document.id)
                else {
                    continue;
                };
                let display_name = &state.imports[import_index].display_name;
                document.display_count = state
                    .imports
                    .iter()
                    .filter(|record| record.display_name == *display_name)
                    .count() as u64;
                document.display_ordinal = state.imports[..=import_index]
                    .iter()
                    .filter(|record| record.display_name == *display_name)
                    .count() as u64;
            }
        }
        snapshot
    }

    /// 打开或重试一个 STL 文档；异步读取仍由现有 ProjectService 执行。
    pub fn open_import_document(&mut self, import_id: &str, title: &str) {
        self.workspace.open_import(import_id, title);
        let request = self.workspace.activation_request(import_id);
        self.apply_workspace_activation_request(request);
    }

    /// 将刚导入且已在缓存中的网格登记为活动文档。
    pub fn publish_import_document(&mut self, import_id: &str, title: &str) {
        if self.workspace.open_ready_import(import_id, title) {
            self.activate_mesh_document(import_id);
            self.sync_workspace_residency();
        }
    }

    pub fn activate_workspace_document(&mut self, document_id: &str) {
        let request = self.workspace.activation_request(document_id);
        self.apply_workspace_activation_request(request);
    }

    pub fn close_workspace_document(&mut self, document_id: &str) {
        let Some(result) = self.workspace.close(document_id) else {
            return;
        };
        if let Some(attempt) = result.cancel_attempt {
            self.cancel_asset_activation(attempt);
        }
        if result.release_import {
            self.release_mesh_document(document_id);
        }
        if !result.active_changed {
            return;
        }
        if let Some(next) = result.activate_document {
            self.activate_workspace_document(&next);
        } else if let Some(next) = result.load_document {
            self.begin_workspace_activation(&next);
        } else {
            self.deactivate_mesh_document();
        }
        self.sync_workspace_residency();
    }

    pub fn move_workspace_document(&mut self, from: usize, to: usize) {
        self.workspace.move_document(from, to);
    }

    pub fn sync_workspace_residency(&mut self) -> bool {
        let resident_ids = self.mesh_cache.resident_ids();
        self.workspace
            .sync_residency(resident_ids.iter().map(String::as_str))
    }

    /// 激活完成由服务校验 attempt 并更新工作区，不让桥接层合并业务状态。
    pub fn resolve_workspace_activation(
        &mut self,
        import_id: &str,
        attempt: u64,
        result: ActivationResultKind,
        diagnostic: DocumentDiagnostic,
    ) -> bool {
        let message = if result == ActivationResultKind::Failed {
            activation_error_message(&diagnostic.code)
        } else {
            ""
        };
        let changed = self
            .workspace
            .resolve_activation(import_id, attempt, result, message, diagnostic);
        if changed && result == ActivationResultKind::Succeeded {
            self.activate_mesh_document(import_id);
        }
        if changed {
            self.sync_workspace_residency();
        }
        changed
    }

    fn apply_workspace_activation_request(&mut self, request: WorkspaceActivationRequest) {
        match request {
            WorkspaceActivationRequest::Ignore => {}
            WorkspaceActivationRequest::ActivateImport(import_id) => {
                if self.activate_mesh_document(&import_id) {
                    self.workspace.set_active(&import_id);
                    self.sync_workspace_residency();
                } else {
                    self.workspace.mark_unloaded(&import_id);
                    let request = self.workspace.activation_request(&import_id);
                    self.apply_workspace_activation_request(request);
                }
            }
            WorkspaceActivationRequest::LoadImport(import_id) => {
                self.begin_workspace_activation(&import_id);
            }
        }
    }

    fn begin_workspace_activation(&mut self, import_id: &str) {
        match self.begin_asset_activation(import_id) {
            Ok(attempt) => {
                self.workspace
                    .set_activation_attempt(import_id, attempt.attempt);
            }
            Err(error) => {
                self.workspace.fail_document(
                    import_id,
                    activation_error_message(error.code()),
                    DocumentDiagnostic {
                        code: error.code().to_owned(),
                        category: error.category().to_owned(),
                        detail: error.detail(),
                    },
                );
            }
        }
    }

    /// 最近一次成功导入或激活的网格；供 Rust 工程服务测试读取。
    pub fn current_mesh(&self) -> Option<&SurfaceMesh> {
        self.latest_mesh_id
            .as_deref()
            .and_then(|id| self.mesh_cache.get(id))
    }

    pub(super) fn snapshot(&self) -> Result<ProjectSnapshot, ProjectError> {
        self.current
            .as_ref()
            .map(|state| ProjectSnapshot {
                path: state.path.clone(),
                name: state.name.clone(),
                revision: state.revision,
                dirty: state.dirty,
            })
            .ok_or(ProjectError::NoProject)
    }
}

pub(super) fn validate_schema_four_mesh_assets(
    imports: &[ImportRecord],
    mesh_assets: &std::collections::BTreeMap<String, MeshAssetRef>,
) -> Result<(), ProjectError> {
    let mut import_ids = std::collections::BTreeSet::new();
    let mut asset_ids = std::collections::BTreeSet::new();
    if imports.iter().any(|import| {
        import.id.is_empty()
            || import.record_version != IMPORT_RECORD_VERSION
            || import.display_name.is_empty()
            || import.mesh_asset_id.is_empty()
            || !import_ids.insert(import.id.as_str())
            || !asset_ids.insert(import.mesh_asset_id.as_str())
            || import
                .source_digest
                .as_deref()
                .is_some_and(|digest| !is_sha256_digest(digest))
            || !mesh_assets.contains_key(&import.mesh_asset_id)
    }) {
        return Err(ProjectError::ManifestInvalid(
            "schema 4 imports require a mesh asset index entry and display name".to_owned(),
        ));
    }
    for (id, asset) in mesh_assets {
        if id != &asset.id
            || asset.revision == 0
            || !panta_mesh::is_readable_mesh_asset_version(asset.format_version)
            || !is_sha256_digest(&asset.digest)
            || asset.units != "millimeters"
            || !matches!(asset.kind.as_str(), "surface" | "volume")
            || asset.results.as_ref().is_some_and(|results| {
                asset.kind != "volume"
                    || results.format_version != panta_mesh::RESULT_ASSET_FORMAT_VERSION
                    || results.field_count == 0
                    || !is_sha256_digest(&results.digest)
                    || !results.relative_path.starts_with("assets/results/")
                    || !results.relative_path.ends_with(".presult")
            })
        {
            return Err(ProjectError::ManifestInvalid(
                "schema 4 mesh asset index contains an invalid entry".to_owned(),
            ));
        }
    }
    Ok(())
}

fn is_sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// 校验清单内的工程相对资产引用并解析为绝对路径；拒绝绝对路径与
/// `..` 等非普通分量，防止清单注入越出工程包目录。
pub(crate) fn resolve_asset_path(root: &Path, relative: &str) -> Result<PathBuf, ProjectError> {
    let mut paths = crate::path::PathService::new();
    paths
        .set_root(crate::path::RootCategory::Project, root)
        .map_err(|error| ProjectError::ManifestInvalid(error.to_string()))?;
    paths
        .resolve_write_target(&crate::path::ResourceRef {
            category: crate::path::RootCategory::Project,
            relative: relative.to_owned(),
        })
        .map_err(|error| ProjectError::ManifestInvalid(error.to_string()))
}

impl Drop for ProjectService {
    fn drop(&mut self) {
        // 读取取消及过期结果释放不等待解析；已接受的写入由执行通道持有候选。
        self.clear_stl_preview();
        self.activation.advance_generation();
        self.activation.drain();
    }
}

/// 把任意表面网格写到导出文件；GUI 后台线程对共享快照调用，不访问服务状态。
pub fn export_mesh_file(
    mesh: &SurfaceMesh,
    target: &Path,
) -> Result<panta_io::export::ExportFormat, ProjectError> {
    export_mesh_file_with_progress(mesh, target, &panta_mesh::Progress::new())
}

/// 同 [`export_mesh_file`]，向 `progress` 报告进度并响应其取消请求。
pub fn export_mesh_file_with_progress(
    mesh: &SurfaceMesh,
    target: &Path,
    progress: &panta_mesh::Progress,
) -> Result<panta_io::export::ExportFormat, ProjectError> {
    panta_io::export::export_surface_mesh_with_progress(mesh, target, progress)
        .map_err(export_error)
}

pub(super) fn export_error(error: panta_io::export::ExportError) -> ProjectError {
    use panta_io::export::ExportError;
    match error {
        ExportError::UnsupportedFormat(path) => ProjectError::ExportUnsupportedFormat(path),
        ExportError::TooLarge(limit) => ProjectError::ExportTooLarge(limit.to_owned()),
        ExportError::Cancelled => ProjectError::ExportCancelled,
        ExportError::Io(error) => ProjectError::ExportWriteFailed(error.to_string()),
        ExportError::Source(error) => ProjectError::ExportSourceUnavailable(error.to_string()),
    }
}
