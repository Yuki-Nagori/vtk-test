//! Rust 工程、网格与进程设施的 CXX 边界；以值 DTO 和拥有式句柄传递结果。

mod project_response;

#[cfg(test)]
#[path = "../../../tests/support/rust/temp_directory.rs"]
mod temp_directory;

use project_response::{
    project_service_begin_stl_import_response, project_service_begin_stl_preview_response,
    project_service_cancel_stl_import_response, project_service_cancel_stl_preview,
    project_service_current_response, project_service_execute_response,
    project_service_finish_stl_import_response, project_service_finish_stl_preview_response,
    project_service_import_stl_response, project_service_imports_response,
};
mod support;
use support::{finish_background_writes, install_crash_handler};

mod project;
pub use project::ProjectService;
use project::{
    default_mesh_type, format_project_dimensions, mesh_type_catalog, project_error_message,
    project_service_begin_stl_import as begin_stl_import,
    project_service_cancel_stl_import as cancel_stl_import, project_service_current,
    project_service_drain_workspace_activations, project_service_execute,
    project_service_finish_stl_import, project_service_import_stl, project_service_imports,
    project_service_new, project_service_workspace_activate, project_service_workspace_close,
    project_service_workspace_move, project_service_workspace_open_import,
    project_service_workspace_publish_import, project_service_workspace_reset,
    project_service_workspace_snapshot, stl_import_preview,
};
mod project_operation;
pub use project_operation::{ProjectOperationCompletion, ProjectOperationRequest};
use project_operation::{
    project_operation_accepted, project_operation_wait, project_service_begin_operation,
    project_service_finish_operation,
};
mod display;
pub use display::MeshDisplayInput;
use display::{
    diagnose_mesh_display, export_mesh_display, prepare_mesh_display,
    project_service_mesh_display_input,
};
mod mesh;
use mesh::{mesh_coordinates, mesh_validate_tet};

// unsafe 仅由 CXX 宏生成；共享 DTO 与 opaque Rust 类型由生成胶水校验。
// 崩溃设施的手写 unsafe 留在 panta-foundation::crash。
#[allow(unsafe_code)]
#[cxx::bridge(namespace = "panta::ffi")]
pub mod bridge {
    /// Rust 工程服务返回的稳定状态快照；path/name 均为可往返 UTF-8。
    #[derive(Default, Clone, PartialEq, Eq)]
    pub struct ProjectSnapshot {
        pub path: String,
        pub name: String,
        pub revision: u64,
        pub dirty: bool,
    }

    /// 已导入网格的持久化元数据。尺寸拆成标量，避免 CXX shared struct
    /// 直接暴露 Rust 数组，便于 Qt 侧消费。
    #[derive(Default, Clone, PartialEq)]
    pub struct ProjectImport {
        pub record_version: u32,
        pub parser_version: u32,
        pub id: String,
        pub display_name: String,
        pub source_name: String,
        pub mesh_asset_path: String,
        pub format: String,
        pub mesh_type: String,
        pub units: String,
        pub show_import_log: bool,
        pub triangle_count: u64,
        pub size_x: f64,
        pub size_y: f64,
        pub size_z: f64,
    }

    /// Rust 工程文档工作区的批量显示快照；文档顺序与活动 ID 由 Rust 决定。
    #[derive(Default)]
    pub struct WorkspaceDocument {
        pub id: String,
        pub kind: String,
        pub state: String,
        pub title: String,
        pub message: String,
        pub diagnostic_code: String,
        pub diagnostic_category: String,
        pub diagnostic_detail: String,
        pub display_ordinal: u64,
        pub display_count: u64,
    }

    #[derive(Default)]
    pub struct WorkspaceSnapshot {
        pub documents: Vec<WorkspaceDocument>,
        pub active_document_id: String,
    }

    struct ChoiceDefinition {
        id: String,
        source_text: String,
    }

    #[derive(Default)]
    enum ProjectOperationKind {
        #[default]
        Create,
        Open,
        Save,
    }
    #[derive(Default)]
    struct ProjectOperationInput {
        kind: ProjectOperationKind,
        path: String,
        name: String,
    }

    /// 源文件尚未复制时供导入弹窗展示的 STL 元数据。
    #[derive(Default, Clone, PartialEq)]
    pub struct StlImportPreview {
        pub expanded_mesh_bytes: u64,
        pub large_mesh_warning: bool,
        pub source_name: String,
        pub triangle_count: u64,
        pub size_x: f64,
        pub size_y: f64,
        pub size_z: f64,
    }

    /// 连续三角面坐标：每三个 f64 为一个点，每三个点为一片面；
    /// CXX Vec 独占缓冲区，调用返回后 C++ 复制到自身场景快照。
    #[derive(Default)]
    pub struct SurfaceMeshSnapshot {
        pub coordinates: Vec<f64>,
        pub revision: u64,
    }

    /// native Netgen 转换结果的批量输入。坐标每三项一个节点；tet 每四项、
    /// boundary 每三项一个单元；区域 / 分组数组各与对应单元数一致。
    pub struct TetMeshData {
        pub nodes: Vec<f64>,
        pub tets: Vec<u32>,
        pub tet_regions: Vec<u32>,
        pub boundary: Vec<u32>,
        pub boundary_groups: Vec<u32>,
        pub region_count: u32,
        pub boundary_group_count: u32,
    }

    /// Rust 领域校验结果；诊断和有效网格总体积统一从 Rust 返回。
    pub struct TetMeshValidation {
        pub issues: Vec<String>,
        pub volume_mm3: f64,
    }

    /// 工程模型命令种类；新增值必须同步 Rust 映射与失败测试。
    pub enum ProjectCommandKind {
        Rename = 0,
    }

    /// 工程模型命令 DTO；value 的语义由 kind 决定。
    pub struct ProjectCommand {
        pub kind: ProjectCommandKind,
        pub value: String,
    }

    /// 所有工程结果遵循同一契约：code 为空表示成功，其余诊断字段为空；
    /// 失败时 value 为缺省占位，不可消费，调用方保留上一次有效状态。
    /// detail 保留原始上下文供诊断，界面仅按稳定 code 选择本地化摘要。
    #[derive(Default, Debug, Clone, PartialEq, Eq)]
    pub struct ProjectDiagnostic {
        pub code: String,
        pub category: String,
        pub detail: String,
    }

    /// 工程操作的快照结果。
    #[derive(Default)]
    pub struct ProjectSnapshotResult {
        pub value: ProjectSnapshot,
        pub error: ProjectDiagnostic,
    }

    /// 一次已提交导入的结果。
    #[derive(Default)]
    pub struct ProjectImportResult {
        pub value: ProjectImport,
        pub error: ProjectDiagnostic,
    }

    /// 当前工程导入记录列表；空列表也可为成功。
    #[derive(Default)]
    pub struct ProjectImportsResult {
        pub value: Vec<ProjectImport>,
        pub error: ProjectDiagnostic,
    }

    /// 只读预检的运行期请求编号；失败 value 不可消费。
    #[derive(Default)]
    pub struct StlPreviewRequestResult {
        pub value: u64,
        pub error: ProjectDiagnostic,
    }

    /// ready=false 且诊断为空表示尚未完成；只有 ready=true 才能消费 value。
    #[derive(Default)]
    pub struct StlPreviewPoll {
        pub ready: bool,
        pub value: StlImportPreview,
        pub error: ProjectDiagnostic,
    }

    /// 确认导入的终态轮询；ready=false 且诊断为空表示仍在写入。
    #[derive(Default)]
    pub struct StlImportPoll {
        pub ready: bool,
        pub value: ProjectImport,
        pub error: ProjectDiagnostic,
    }

    /// 表面网格质量诊断摘要（复用 panta_mesh::diagnose_surface，默认容差）。
    #[derive(Default, Clone, PartialEq)]
    pub struct MeshQualityReport {
        pub weld_tolerance: f64,
        pub triangle_count: u64,
        pub raw_vertex_count: u64,
        pub welded_vertex_count: u64,
        pub near_duplicate_vertex_count: u64,
        pub degenerate_triangle_count: u64,
        pub boundary_edge_count: u64,
        pub hole_count: u64,
        pub non_manifold_edge_count: u64,
        pub inconsistent_orientation_edge_count: u64,
        pub clean: bool,
    }

    /// 网格质量诊断结果；失败 value 不可消费。
    #[derive(Default)]
    pub struct MeshQualityReportResult {
        pub value: MeshQualityReport,
        pub error: ProjectDiagnostic,
    }

    /// 驻留 Mesh 的显示快照结果。
    #[derive(Default)]
    pub struct SurfaceMeshSnapshotResult {
        pub value: SurfaceMeshSnapshot,
        pub error: ProjectDiagnostic,
    }

    extern "Rust" {
        /// Rust 拥有的工程 application service：目录、清单和模型状态不由
        /// QML/C++ 持有；C++ 只负责 Qt 字符串/URL 适配和 ViewModel 通知。
        type ProjectService;

        type ProjectOperationRequest;
        type ProjectOperationCompletion;
        type MeshDisplayInput;
        fn project_service_begin_operation(
            service: &mut ProjectService,
            input: ProjectOperationInput,
        ) -> Box<ProjectOperationRequest>;
        fn project_operation_accepted(request: &ProjectOperationRequest) -> bool;
        fn project_operation_wait(
            request: Box<ProjectOperationRequest>,
        ) -> Box<ProjectOperationCompletion>;
        fn project_service_finish_operation(
            service: &mut ProjectService,
            completion: Box<ProjectOperationCompletion>,
        ) -> ProjectSnapshotResult;
        fn project_service_mesh_display_input(
            service: &ProjectService,
            import_id: &str,
        ) -> Box<MeshDisplayInput>;
        fn prepare_mesh_display(input: Box<MeshDisplayInput>) -> SurfaceMeshSnapshotResult;
        fn diagnose_mesh_display(input: Box<MeshDisplayInput>) -> MeshQualityReportResult;
        fn export_mesh_display(input: Box<MeshDisplayInput>, path: &str) -> ProjectDiagnostic;

        fn project_service_new() -> Box<ProjectService>;
        #[rust_name = "project_service_execute_response"]
        fn project_service_execute(
            service: &mut ProjectService,
            command: ProjectCommand,
        ) -> ProjectSnapshotResult;
        #[rust_name = "project_service_current_response"]
        fn project_service_current(service: &ProjectService) -> ProjectSnapshotResult;
        #[rust_name = "project_service_import_stl_response"]
        fn project_service_import_stl(
            service: &mut ProjectService,
            source: String,
            mesh_type: String,
            units: String,
            show_import_log: bool,
        ) -> ProjectImportResult;
        #[rust_name = "project_service_imports_response"]
        fn project_service_imports(service: &ProjectService) -> ProjectImportsResult;
        fn project_service_workspace_snapshot(service: &ProjectService) -> WorkspaceSnapshot;
        fn project_service_workspace_reset(service: &mut ProjectService);
        fn project_service_workspace_open_import(
            service: &mut ProjectService,
            import_id: &str,
            title: &str,
        );
        fn project_service_workspace_publish_import(
            service: &mut ProjectService,
            import_id: &str,
            title: &str,
        );
        fn project_service_workspace_activate(service: &mut ProjectService, document_id: &str);
        fn project_service_workspace_close(service: &mut ProjectService, document_id: &str);
        fn project_service_workspace_move(service: &mut ProjectService, from: u64, to: u64);
        fn project_error_message(code: &str) -> String;
        fn format_project_dimensions(size_x: f64, size_y: f64, size_z: f64, unit: &str) -> String;
        fn mesh_type_catalog() -> Vec<ChoiceDefinition>;
        fn default_mesh_type() -> String;
        #[rust_name = "project_service_begin_stl_preview_response"]
        fn project_service_begin_stl_preview(
            service: &mut ProjectService,
            source: String,
        ) -> StlPreviewRequestResult;
        #[rust_name = "project_service_finish_stl_preview_response"]
        fn project_service_finish_stl_preview(
            service: &mut ProjectService,
            request: u64,
        ) -> StlPreviewPoll;
        fn project_service_cancel_stl_preview(service: &mut ProjectService, request: u64) -> bool;
        #[rust_name = "project_service_begin_stl_import_response"]
        fn project_service_begin_stl_import(
            service: &mut ProjectService,
            source: String,
            mesh_type: String,
            units: String,
            show_import_log: bool,
        ) -> StlPreviewRequestResult;
        #[rust_name = "project_service_finish_stl_import_response"]
        fn project_service_finish_stl_import(
            service: &mut ProjectService,
            request: u64,
        ) -> StlImportPoll;
        #[rust_name = "project_service_cancel_stl_import_response"]
        fn project_service_cancel_stl_import(service: &mut ProjectService, request: u64) -> bool;

        fn project_service_drain_workspace_activations(service: &mut ProjectService);
        fn mesh_validate_tet(data: TetMeshData) -> TetMeshValidation;

        /// QML 服务销毁、事件循环退出后等待已接受写入；禁止在写入 worker 调用。
        fn finish_background_writes();

        /// 崩溃信号处理器安装（任务 047，panta_foundation::crash 的 FFI 面）：
        /// 返回日志路径；log_dir 为空时用系统临时目录。
        fn install_crash_handler(log_dir: &str) -> Result<String>;
    }
}
