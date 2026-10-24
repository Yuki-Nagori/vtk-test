//! Rust 工程、网格与进程设施的 CXX 边界；以值 DTO 和拥有式句柄传递结果。

mod project_response;

#[cfg(test)]
#[path = "../../../tests/support/rust/temp_directory.rs"]
mod temp_directory;

use project_response::{
    project_service_begin_generated_volume, project_service_begin_stl_import_response,
    project_service_begin_stl_preview_response, project_service_cancel_stl_import_response,
    project_service_cancel_stl_preview, project_service_current_response,
    project_service_execute_response, project_service_finish_stl_import_response,
    project_service_finish_stl_preview_response, project_service_import_stl_response,
    project_service_imports_response,
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
    project_service_mesh_display_input, section_field_mesh_display, section_mesh_display,
};
mod progress;
pub use progress::TaskProgress;
use progress::{
    project_service_stl_import_progress, project_service_stl_preview_progress,
    task_progress_cancel, task_progress_fraction, task_progress_is_cancelled, task_progress_new,
    task_progress_set_fraction, task_progress_share,
};
mod volume;
use volume::{project_service_result_fields, project_service_volume_summary};
mod limits;
use limits::{
    default_large_mesh_limits, project_service_large_mesh_limits,
    project_service_set_available_memory, project_service_set_derived_chunk_dir,
    project_service_set_large_mesh_limits,
};
mod animation;
use animation::{
    AnimationSequence, animation_cached_frames, animation_frame, animation_frame_count,
    animation_frame_name, animation_open, animation_request,
};
mod mesh;
use mesh::{mesh_coordinates, mesh_tet_export_vtu, mesh_tet_quality, mesh_validate_tet};

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
        /// 非 0：坐标是显示 LOD，值为源资产面数。
        pub lod_source_triangles: u64,
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
        /// 体网格文档（task 022）：以下质量字段有效。
        pub volume_available: bool,
        pub node_count: u64,
        pub tet_count: u64,
        pub min_dihedral_degrees: f64,
        pub max_aspect_ratio: f64,
        pub mean_aspect_ratio: f64,
        pub poor_count: u64,
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

    /// 时序动画单帧（task 020）：ready 时 coordinates 为扁平三角面坐标。
    /// 体网格摘要与质量统计；`available` 为假时 error 说明原因（表面网格或未驻留）。
    #[derive(Default)]
    pub struct VolumeInfoResult {
        pub available: bool,
        /// 单元未驻留、命令按需从资产流式读取（task 025）。
        pub streamed: bool,
        pub node_count: u64,
        pub tet_count: u64,
        pub min_dihedral_degrees: f64,
        pub max_aspect_ratio: f64,
        pub mean_aspect_ratio: f64,
        pub poor_count: u64,
        pub error: ProjectDiagnostic,
    }

    /// 大网格激活设置（task 026）：表面 LOD 阈值 / 预算（面）、体网格流式阈值（四面体）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LargeMeshLimits {
        pub surface_triangles: u64,
        pub surface_budget: u64,
        pub volume_cells: u64,
        /// 体网格模式（task 028）：0 按阈值分块、1 按可用内存自动、2 禁用分块完整驻留。
        pub volume_mode: u32,
        /// 分块块缓存预算（MiB）。
        pub chunk_cache_mib: u64,
    }

    /// 结果场摘要（task 029）：`association` 为 "node" / "cell"；`sectionable` 表示当前
    /// 驻留方式下可做场剖面（驻留体网格，或派生分块文件带该字段）。
    #[derive(Default, Debug, Clone)]
    pub struct ResultFieldInfo {
        pub name: String,
        pub association: String,
        pub components: u8,
        pub units: String,
        pub missing: u64,
        pub has_range: bool,
        pub min: f64,
        pub max: f64,
        pub sectionable: bool,
    }

    #[derive(Default)]
    pub struct ResultFieldListResult {
        pub fields: Vec<ResultFieldInfo>,
        pub error: ProjectDiagnostic,
    }

    /// 场剖面请求（task 029）：`association` 为 "node" / "cell"，`component` 为 -1 取模、
    /// 0..=2 取分量；平面参数与 `section_mesh_display` 相同。
    #[derive(Default, Debug, Clone)]
    pub struct FieldSectionRequest {
        pub field_name: String,
        pub association: String,
        pub component: i32,
        pub normal_x: f64,
        pub normal_y: f64,
        pub normal_z: f64,
        pub fraction: f64,
    }

    /// 场剖面（task 029）：`scalars` 与三角面顶点一一对应，缺失值为 NaN；
    /// 范围只统计有限值，`has_range` 为假表示全部缺失。
    #[derive(Default)]
    pub struct FieldSectionResult {
        pub ready: bool,
        pub coordinates: Vec<f64>,
        pub scalars: Vec<f32>,
        pub has_range: bool,
        pub min: f64,
        pub max: f64,
        pub missing_vertices: u64,
        pub error: ProjectDiagnostic,
    }

    #[derive(Default)]
    pub struct AnimationFrameResult {
        pub ready: bool,
        pub coordinates: Vec<f64>,
        pub error: ProjectDiagnostic,
    }

    extern "Rust" {
        /// 只读时序序列：后台解析、预取与 LRU 帧缓存，GUI 线程轮询。
        type AnimationSequence;
        fn animation_open(path: &str, error: &mut ProjectDiagnostic) -> Box<AnimationSequence>;
        fn animation_frame_count(sequence: &AnimationSequence) -> u64;
        fn animation_cached_frames(sequence: &AnimationSequence) -> u64;
        fn animation_frame_name(sequence: &AnimationSequence, index: u64) -> String;
        fn animation_request(sequence: &mut AnimationSequence, index: u64);
        fn animation_frame(sequence: &mut AnimationSequence, index: u64) -> AnimationFrameResult;
        /// 体网格节点/单元计数摘要；质量字段留空，由后台诊断填充。
        fn project_service_volume_summary(
            service: &ProjectService,
            import_id: &str,
        ) -> VolumeInfoResult;
        /// 结果场摘要（只读 sidecar 头部与字段表）；无结果场返回空表。
        fn project_service_result_fields(
            service: &ProjectService,
            import_id: &str,
        ) -> ResultFieldListResult;
    }

    extern "Rust" {
        /// Rust 拥有的工程 application service：目录、清单和模型状态不由
        /// QML/C++ 持有；C++ 只负责 Qt 字符串/URL 适配和 ViewModel 通知。
        type ProjectService;

        type ProjectOperationRequest;
        type ProjectOperationCompletion;
        type MeshDisplayInput;
        /// 后台任务进度与协作式取消句柄（共享 Arc，可跨线程）。
        type TaskProgress;
        fn task_progress_new() -> Box<TaskProgress>;
        fn task_progress_share(progress: &TaskProgress) -> Box<TaskProgress>;
        fn task_progress_fraction(progress: &TaskProgress) -> f64;
        fn task_progress_cancel(progress: &TaskProgress);
        fn task_progress_set_fraction(progress: &TaskProgress, value: f64);
        fn task_progress_is_cancelled(progress: &TaskProgress) -> bool;
        fn project_service_stl_import_progress(service: &ProjectService, request: u64) -> f64;
        fn project_service_stl_preview_progress(service: &ProjectService, request: u64) -> f64;
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
        fn diagnose_mesh_display(
            input: Box<MeshDisplayInput>,
            progress: &TaskProgress,
        ) -> MeshQualityReportResult;
        fn export_mesh_display(
            input: Box<MeshDisplayInput>,
            path: &str,
            progress: &TaskProgress,
        ) -> ProjectDiagnostic;
        /// 体网格任意平面切面显示坐标；法向在 Rust 侧单位化，`fraction` 为法向投影区间位置。
        /// 后台线程调用，取消时 ready 为假且无错误。
        fn section_mesh_display(
            input: Box<MeshDisplayInput>,
            normal_x: f64,
            normal_y: f64,
            normal_z: f64,
            fraction: f64,
            progress: &TaskProgress,
        ) -> AnimationFrameResult;
        /// 带结果场着色的切面（task 029）。后台线程调用，取消时 ready 为假且无错误。
        fn section_field_mesh_display(
            input: Box<MeshDisplayInput>,
            request: &FieldSectionRequest,
            progress: &TaskProgress,
        ) -> FieldSectionResult;

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
        fn default_large_mesh_limits() -> LargeMeshLimits;
        fn project_service_large_mesh_limits(service: &ProjectService) -> LargeMeshLimits;
        /// 校验并替换设置；只影响之后的激活。非法时返回 `project.lod_settings_invalid`。
        fn project_service_set_large_mesh_limits(
            service: &mut ProjectService,
            limits: LargeMeshLimits,
        ) -> ProjectDiagnostic;
        /// 宿主报告的可用物理内存（B，0 表示未知），激活前调用（task 028）。
        fn project_service_set_available_memory(service: &mut ProjectService, bytes: u64);
        /// 派生分块缓存目录（task 028）；空字符串表示不缓存，大体网格逐次流式扫描。
        fn project_service_set_derived_chunk_dir(service: &mut ProjectService, directory: &str);
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
        /// STEP → Netgen 生成结果的工程提交（task 023）；轮询与取消复用
        /// `project_service_finish_stl_import` / `project_service_cancel_stl_import`。
        fn project_service_begin_generated_volume(
            service: &mut ProjectService,
            source_step: String,
            max_length_mm: f64,
            data: TetMeshData,
        ) -> StlPreviewRequestResult;

        fn project_service_drain_workspace_activations(service: &mut ProjectService);
        fn mesh_validate_tet(data: TetMeshData) -> TetMeshValidation;
        /// Netgen 体网格与导入体网格共用质量统计 / VTU 导出（task 021 子项 5）。
        fn mesh_tet_quality(data: TetMeshData) -> VolumeInfoResult;
        fn mesh_tet_export_vtu(data: TetMeshData, path: &str) -> ProjectDiagnostic;

        /// QML 服务销毁、事件循环退出后等待已接受写入；禁止在写入 worker 调用。
        fn finish_background_writes();

        /// 崩溃信号处理器安装（任务 047，panta_foundation::crash 的 FFI 面）：
        /// 返回日志路径；log_dir 为空时用系统临时目录。
        fn install_crash_handler(log_dir: &str) -> Result<String>;
    }
}
