//! 工程窗口中的运行期文档页签状态；不参与工程文件持久化。

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Import,
}

impl DocumentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentState {
    Ready,
    Loading,
    Failed,
    Unloaded,
}

impl DocumentState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Loading => "loading",
            Self::Failed => "failed",
            Self::Unloaded => "unloaded",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentDiagnostic {
    pub code: String,
    pub category: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDocument {
    pub id: String,
    pub kind: DocumentKind,
    pub state: DocumentState,
    pub title: String,
    /// 直接面向用户显示的中文说明。
    pub message: String,
    pub diagnostic: DocumentDiagnostic,
    pub display_ordinal: u64,
    pub display_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    pub documents: Vec<WorkspaceDocument>,
    pub active_document_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationRequest {
    Ignore,
    ActivateImport(String),
    LoadImport(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseDocumentResult {
    pub cancel_attempt: Option<u64>,
    pub release_import: bool,
    pub active_changed: bool,
    pub activate_document: Option<String>,
    pub load_document: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationResultKind {
    Succeeded,
    Failed,
    Cancelled,
    Expired,
}

#[derive(Debug, Default)]
pub struct DocumentWorkspace {
    documents: Vec<WorkspaceDocument>,
    active_document_id: Option<String>,
    activation_attempts: HashMap<String, u64>,
}

impl DocumentWorkspace {
    /// 重置为空白工作区；返回需由调用方请求取消的后台 attempt。
    pub fn reset(&mut self) -> Vec<u64> {
        let attempts = self
            .activation_attempts
            .drain()
            .map(|(_, attempt)| attempt)
            .collect();
        self.documents.clear();
        self.active_document_id = None;
        attempts
    }

    pub fn snapshot(&self) -> WorkspaceSnapshot {
        let mut documents = self.documents.clone();
        let mut title_counts: HashMap<String, u64> = HashMap::new();
        for document in &documents {
            *title_counts.entry(document.title.clone()).or_default() += 1;
        }
        let mut title_ordinals: HashMap<String, u64> = HashMap::new();
        for document in &mut documents {
            let ordinal = title_ordinals.entry(document.title.clone()).or_default();
            *ordinal += 1;
            document.display_ordinal = *ordinal;
            document.display_count = title_counts[&document.title];
        }
        WorkspaceSnapshot {
            documents,
            active_document_id: self.active_document_id.clone().unwrap_or_default(),
        }
    }

    pub fn open_import(&mut self, id: &str, title: &str) -> bool {
        if self.document_index(id).is_some() {
            return false;
        }
        self.documents.push(WorkspaceDocument {
            id: id.to_owned(),
            kind: DocumentKind::Import,
            state: DocumentState::Unloaded,
            title: title.to_owned(),
            message: String::new(),
            diagnostic: DocumentDiagnostic::default(),
            display_ordinal: 1,
            display_count: 1,
        });
        true
    }

    pub fn open_ready_import(&mut self, id: &str, title: &str) -> bool {
        if !self.open_import(id, title) {
            return false;
        }
        if let Some(document) = self.document_mut(id) {
            document.state = DocumentState::Ready;
        }
        self.active_document_id = Some(id.to_owned());
        true
    }

    pub fn fail_document(
        &mut self,
        id: &str,
        message: &str,
        diagnostic: DocumentDiagnostic,
    ) -> bool {
        let Some(document) = self.document_mut(id) else {
            return false;
        };
        document.state = DocumentState::Failed;
        document.message = message.to_owned();
        document.diagnostic = diagnostic;
        true
    }

    pub fn activation_request(&mut self, id: &str) -> ActivationRequest {
        let Some(index) = self.document_index(id) else {
            return ActivationRequest::Ignore;
        };
        let document = &mut self.documents[index];
        match document.state {
            DocumentState::Loading => ActivationRequest::Ignore,
            DocumentState::Ready => ActivationRequest::ActivateImport(document.id.clone()),
            DocumentState::Failed | DocumentState::Unloaded => {
                document.state = DocumentState::Loading;
                document.message.clear();
                document.diagnostic = DocumentDiagnostic::default();
                ActivationRequest::LoadImport(document.id.clone())
            }
        }
    }

    pub fn mark_unloaded(&mut self, id: &str) {
        if let Some(document) = self.document_mut(id) {
            document.state = DocumentState::Unloaded;
            document.message.clear();
            document.diagnostic = DocumentDiagnostic::default();
        }
    }

    pub fn set_active(&mut self, id: &str) -> bool {
        if self.document_index(id).is_none() {
            return false;
        }
        self.active_document_id = Some(id.to_owned());
        true
    }

    pub fn active_document_id(&self) -> Option<&str> {
        self.active_document_id.as_deref()
    }

    pub fn set_activation_attempt(&mut self, id: &str, attempt: u64) -> bool {
        if self.document_index(id).is_none() {
            return false;
        }
        self.activation_attempts.insert(id.to_owned(), attempt);
        true
    }

    pub fn activation_attempt(&self, id: &str) -> Option<u64> {
        self.activation_attempts.get(id).copied()
    }

    pub fn resolve_activation(
        &mut self,
        id: &str,
        attempt: u64,
        result: ActivationResultKind,
        message: &str,
        diagnostic: DocumentDiagnostic,
    ) -> bool {
        if self.activation_attempt(id) != Some(attempt) {
            return false;
        }
        self.activation_attempts.remove(id);
        let Some(document) = self.document_mut(id) else {
            return false;
        };
        match result {
            ActivationResultKind::Succeeded => {
                document.state = DocumentState::Ready;
                document.message.clear();
                document.diagnostic = DocumentDiagnostic::default();
                self.active_document_id = Some(id.to_owned());
            }
            ActivationResultKind::Failed => {
                document.state = DocumentState::Failed;
                document.message = message.to_owned();
                document.diagnostic = diagnostic;
            }
            ActivationResultKind::Cancelled => {
                document.state = DocumentState::Unloaded;
                document.message.clear();
                document.diagnostic = DocumentDiagnostic::default();
            }
            ActivationResultKind::Expired => {
                document.state = DocumentState::Unloaded;
                document.message.clear();
                document.diagnostic = DocumentDiagnostic::default();
            }
        }
        true
    }

    pub fn sync_residency<'a>(&mut self, resident_ids: impl IntoIterator<Item = &'a str>) -> bool {
        let residents: std::collections::HashSet<&str> = resident_ids.into_iter().collect();
        let mut changed = false;
        for document in &mut self.documents {
            if matches!(
                document.state,
                DocumentState::Loading | DocumentState::Failed
            ) {
                continue;
            }
            let next = if residents.contains(document.id.as_str()) {
                DocumentState::Ready
            } else {
                DocumentState::Unloaded
            };
            if document.state != next {
                document.state = next;
                document.message.clear();
                document.diagnostic = DocumentDiagnostic::default();
                changed = true;
            }
        }
        changed
    }

    pub fn close(&mut self, id: &str) -> Option<CloseDocumentResult> {
        let index = self.document_index(id)?;
        self.documents.remove(index);
        let cancel_attempt = self.activation_attempts.remove(id);
        let active_changed = self.active_document_id.as_deref() == Some(id);
        let mut result = CloseDocumentResult {
            cancel_attempt,
            release_import: true,
            active_changed,
            activate_document: None,
            load_document: None,
        };
        if !active_changed {
            return Some(result);
        }

        self.active_document_id = None;
        if let Some(next) = self.neighbor(index, DocumentState::Ready) {
            self.active_document_id = Some(next.clone());
            result.activate_document = Some(next);
            return Some(result);
        }
        if let Some(next) = self.neighbor(index, DocumentState::Unloaded) {
            if let Some(next_document) = self.document_mut(&next) {
                next_document.state = DocumentState::Loading;
                next_document.message.clear();
                next_document.diagnostic = DocumentDiagnostic::default();
            }
            result.load_document = Some(next);
        }
        Some(result)
    }

    pub fn move_document(&mut self, from: usize, to: usize) -> bool {
        if from >= self.documents.len() || to >= self.documents.len() || from == to {
            return false;
        }
        let document = self.documents.remove(from);
        self.documents.insert(to, document);
        true
    }

    pub fn document_index(&self, id: &str) -> Option<usize> {
        self.documents.iter().position(|document| document.id == id)
    }

    fn document_mut(&mut self, id: &str) -> Option<&mut WorkspaceDocument> {
        self.documents.iter_mut().find(|document| document.id == id)
    }

    fn neighbor(&self, removed_index: usize, state: DocumentState) -> Option<String> {
        self.documents
            .iter()
            .skip(removed_index)
            .find(|document| document.state == state)
            .or_else(|| {
                self.documents
                    .iter()
                    .take(removed_index)
                    .rev()
                    .find(|document| document.state == state)
            })
            .map(|document| document.id.clone())
    }
}

/// 错误码映射为直接显示的中文提示，稳定错误码另行保留。
pub fn project_error_message(code: &str) -> &'static str {
    match code {
        "project.empty_name" | "project.invalid_name" => "请输入有效的工程名称。",
        "project.location_empty" | "project.location_not_absolute" => {
            "请选择绝对路径作为工程位置。"
        }
        "project.location_create_failed" => "无法创建工程位置。",
        "project.file_missing" => "所选工程文件不存在。",
        "project.invalid_file" => "请选择 .panta 工程文件。",
        "project.file_not_local" => "请选择此设备上的工程文件。",
        "project.already_exists" => "同名工程已存在。",
        "project.manifest_invalid" | "project.unsupported_schema" => "所选工程文件不受支持。",
        "project.no_project" => "请先打开或创建工程。",
        "project.command_invalid" => "工程更改无效。",
        "project.import_file_missing" => "所选网格文件不存在。",
        "project.import_invalid_file" => "请选择要导入的网格文件。",
        "project.import_unsupported_mesh_type" | "project.import_unsupported_units" => {
            "请选择有效的 STL 导入选项。"
        }
        "project.import_source_changed" => "预览后网格文件已更改，请重新选择。",
        "project.import_parse_failed" => "无法读取所选网格文件。",
        "project.import_asset_write_failed" => "无法将网格资产保存到工程中。",
        _ => "无法完成工程操作。",
    }
}

pub fn activation_error_message(code: &str) -> &'static str {
    match code {
        "project.asset_missing" => "工程包中缺少已保存的网格资产。",
        "project.asset_unsupported_format" => "不支持已保存网格资产的格式。",
        "project.import_unsupported_units" => "已保存网格资产使用了不支持的单位。",
        "project.asset_read_failed" | "project.asset_parse_failed" => "无法读取已保存的网格资产。",
        _ => "无法打开已保存的网格资产。",
    }
}

/// 工程 UI 中使用的三边尺寸字符串；数值单位为调用方约定的显示单位。
pub fn format_dimensions(size_x: f64, size_y: f64, size_z: f64, unit: &str) -> String {
    let values = format!("{size_x:.2} × {size_y:.2} × {size_z:.2}");
    if unit.is_empty() {
        values
    } else {
        format!("{values} {unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_import(workspace: &mut DocumentWorkspace, id: &str, title: &str, attempt: u64) {
        assert!(workspace.open_import(id, title));
        assert_eq!(
            workspace.activation_request(id),
            ActivationRequest::LoadImport(id.to_owned())
        );
        assert!(workspace.set_activation_attempt(id, attempt));
    }

    #[test]
    fn new_and_reset_workspaces_stay_empty_and_return_pending_attempts() {
        let mut workspace = DocumentWorkspace::default();
        assert!(workspace.snapshot().documents.is_empty());
        assert!(workspace.snapshot().active_document_id.is_empty());
        add_import(&mut workspace, "pending", "part", 42);
        assert_eq!(workspace.reset(), vec![42]);
        assert!(workspace.snapshot().documents.is_empty());
        assert!(workspace.snapshot().active_document_id.is_empty());
        assert!(workspace.reset().is_empty());
    }

    #[test]
    fn newly_opened_import_requests_its_first_activation() {
        let mut workspace = DocumentWorkspace::default();
        assert!(workspace.open_import("first", "part"));

        assert_eq!(
            workspace.activation_request("first"),
            ActivationRequest::LoadImport("first".into())
        );
        assert_eq!(
            workspace.snapshot().documents[0].state,
            DocumentState::Loading
        );
    }

    #[test]
    fn ready_documents_activate_and_reorder_preserves_active_identity() {
        let mut workspace = DocumentWorkspace::default();
        add_import(&mut workspace, "a", "part", 1);
        assert!(workspace.resolve_activation(
            "a",
            1,
            ActivationResultKind::Succeeded,
            "",
            DocumentDiagnostic::default()
        ));
        assert_eq!(
            workspace.activation_request("a"),
            ActivationRequest::ActivateImport("a".into())
        );
        assert_eq!(workspace.active_document_id(), Some("a"));
        assert!(workspace.open_ready_import("b", "other"));
        assert!(workspace.set_active("a"));
        assert!(workspace.move_document(0, 1));
        assert_eq!(workspace.snapshot().documents[1].id, "a");
        assert_eq!(workspace.active_document_id(), Some("a"));
    }

    #[test]
    fn closing_active_document_prefers_ready_right_then_ready_left() {
        let mut workspace = DocumentWorkspace::default();
        add_import(&mut workspace, "left", "left", 1);
        add_import(&mut workspace, "active", "active", 2);
        add_import(&mut workspace, "right", "right", 3);
        for (id, attempt) in [("left", 1), ("active", 2), ("right", 3)] {
            workspace.resolve_activation(
                id,
                attempt,
                ActivationResultKind::Succeeded,
                "",
                Default::default(),
            );
        }
        workspace.set_active("active");
        assert_eq!(
            workspace
                .close("active")
                .and_then(|result| result.activate_document),
            Some("right".into())
        );
        workspace.set_active("right");
        assert_eq!(
            workspace
                .close("right")
                .and_then(|result| result.activate_document),
            Some("left".into())
        );
    }

    #[test]
    fn close_without_ready_neighbor_loads_right_unloaded_before_left() {
        let mut workspace = DocumentWorkspace::default();
        add_import(&mut workspace, "left", "left", 1);
        add_import(&mut workspace, "active", "active", 2);
        add_import(&mut workspace, "right", "right", 3);
        for id in ["left", "active", "right"] {
            workspace.mark_unloaded(id);
        }
        workspace.set_active("active");
        assert_eq!(
            workspace
                .close("active")
                .and_then(|result| result.load_document),
            Some("right".into())
        );
        assert_eq!(
            workspace.activation_request("right"),
            ActivationRequest::Ignore
        );
    }

    #[test]
    fn residency_sync_preserves_loading_and_failed_states() {
        let mut workspace = DocumentWorkspace::default();
        add_import(&mut workspace, "loading", "loading", 1);
        add_import(&mut workspace, "failed", "failed", 2);
        workspace.open_import("resident", "resident");
        workspace.mark_unloaded("resident");
        workspace.resolve_activation(
            "failed",
            2,
            ActivationResultKind::Failed,
            "Could not load.",
            DocumentDiagnostic {
                code: "asset.failed".into(),
                category: "io".into(),
                detail: "disk".into(),
            },
        );
        assert!(workspace.sync_residency(["loading", "failed", "resident"].iter().copied()));
        let snapshot = workspace.snapshot();
        assert_eq!(snapshot.documents[0].state, DocumentState::Loading);
        assert_eq!(snapshot.documents[1].state, DocumentState::Failed);
        assert_eq!(snapshot.documents[2].state, DocumentState::Ready);
    }

    #[test]
    fn stale_and_replaced_activation_results_cannot_mutate_reopened_tabs() {
        let mut workspace = DocumentWorkspace::default();
        add_import(&mut workspace, "same", "part", 1);
        assert_eq!(
            workspace
                .close("same")
                .and_then(|result| result.cancel_attempt),
            Some(1)
        );
        add_import(&mut workspace, "same", "part", 2);
        assert!(!workspace.resolve_activation(
            "same",
            1,
            ActivationResultKind::Succeeded,
            "",
            Default::default()
        ));
        assert!(workspace.resolve_activation(
            "same",
            2,
            ActivationResultKind::Failed,
            "failed",
            Default::default()
        ));
        assert_eq!(
            workspace.snapshot().documents[0].state,
            DocumentState::Failed
        );
    }

    #[test]
    fn draining_multiple_results_updates_each_document_and_uses_last_success_as_active() {
        let mut workspace = DocumentWorkspace::default();
        add_import(&mut workspace, "first", "first", 10);
        add_import(&mut workspace, "second", "second", 20);
        for (id, attempt) in [("first", 10), ("second", 20)] {
            assert!(workspace.resolve_activation(
                id,
                attempt,
                ActivationResultKind::Succeeded,
                "",
                DocumentDiagnostic::default()
            ));
        }
        assert_eq!(workspace.active_document_id(), Some("second"));
        assert_eq!(
            workspace.snapshot().documents[0].state,
            DocumentState::Ready
        );
        assert_eq!(
            workspace.snapshot().documents[1].state,
            DocumentState::Ready
        );
    }

    #[test]
    fn current_expired_attempt_leaves_loading_state_and_releases_polling() {
        let mut workspace = DocumentWorkspace::default();
        add_import(&mut workspace, "expired", "part", 1);
        assert!(workspace.resolve_activation(
            "expired",
            1,
            ActivationResultKind::Expired,
            "",
            DocumentDiagnostic::default()
        ));
        assert_eq!(
            workspace.snapshot().documents[0].state,
            DocumentState::Unloaded
        );
        assert_eq!(
            workspace.activation_request("expired"),
            ActivationRequest::LoadImport("expired".into())
        );
    }

    #[test]
    fn duplicate_titles_receive_stable_ordinals_after_reordering() {
        let mut workspace = DocumentWorkspace::default();
        workspace.open_import("first", "part");
        workspace.open_import("second", "part");
        let initial = workspace.snapshot();
        assert_eq!(
            (
                initial.documents[0].display_ordinal,
                initial.documents[0].display_count
            ),
            (1, 2)
        );
        assert!(workspace.move_document(0, 1));
        let reordered = workspace.snapshot();
        assert_eq!(reordered.documents[1].display_ordinal, 2);
    }

    #[test]
    fn project_error_codes_map_to_stable_translation_sources() {
        let cases = [
            ("project.empty_name", "请输入有效的工程名称。"),
            ("project.invalid_name", "请输入有效的工程名称。"),
            ("project.location_empty", "请选择绝对路径作为工程位置。"),
            (
                "project.location_not_absolute",
                "请选择绝对路径作为工程位置。",
            ),
            ("project.location_create_failed", "无法创建工程位置。"),
            ("project.file_missing", "所选工程文件不存在。"),
            ("project.invalid_file", "请选择 .panta 工程文件。"),
            ("project.file_not_local", "请选择此设备上的工程文件。"),
            ("project.already_exists", "同名工程已存在。"),
            ("project.manifest_invalid", "所选工程文件不受支持。"),
            ("project.unsupported_schema", "所选工程文件不受支持。"),
            ("project.no_project", "请先打开或创建工程。"),
            ("project.command_invalid", "工程更改无效。"),
            ("project.import_file_missing", "所选网格文件不存在。"),
            ("project.import_invalid_file", "请选择要导入的网格文件。"),
            (
                "project.import_unsupported_mesh_type",
                "请选择有效的 STL 导入选项。",
            ),
            (
                "project.import_unsupported_units",
                "请选择有效的 STL 导入选项。",
            ),
            (
                "project.import_source_changed",
                "预览后网格文件已更改，请重新选择。",
            ),
            ("project.import_parse_failed", "无法读取所选网格文件。"),
            (
                "project.import_asset_write_failed",
                "无法将网格资产保存到工程中。",
            ),
            ("unknown", "无法完成工程操作。"),
        ];
        for (code, expected) in cases {
            assert_eq!(project_error_message(code), expected, "{code}");
        }
        let activation_cases = [
            ("project.asset_missing", "工程包中缺少已保存的网格资产。"),
            (
                "project.asset_unsupported_format",
                "不支持已保存网格资产的格式。",
            ),
            (
                "project.import_unsupported_units",
                "已保存网格资产使用了不支持的单位。",
            ),
            ("project.asset_read_failed", "无法读取已保存的网格资产。"),
            ("project.asset_parse_failed", "无法读取已保存的网格资产。"),
            ("project.asset_source_changed", "无法打开已保存的网格资产。"),
            (
                "project.activation_spawn_failed",
                "无法打开已保存的网格资产。",
            ),
            (
                "project.activation_worker_failed",
                "无法打开已保存的网格资产。",
            ),
            ("unknown", "无法打开已保存的网格资产。"),
        ];
        for (code, expected) in activation_cases {
            assert_eq!(activation_error_message(code), expected, "{code}");
        }
    }

    #[test]
    fn dimension_format_matches_existing_precision_and_optional_unit() {
        assert_eq!(
            format_dimensions(1.234, 0.0, 99.999, ""),
            "1.23 × 0.00 × 100.00"
        );
        assert_eq!(
            format_dimensions(1.0, 2.0, 3.0, "mm"),
            "1.00 × 2.00 × 3.00 mm"
        );
    }
}
