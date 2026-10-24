/// Netgen 体网格生成适配器（任务 010）的公共契约。
///
/// 输入 STEP 的坐标与生成参数一律毫米；输出为自有 native DTO
/// （panta/mesh/mesh_ir.hpp），再由桥接交给 Rust Mesh IR 校验。Netgen/OCCT
/// 类型不出现在本头文件。
/// 冒烟范围：单一闭合实体 → 线性四面体 + 三角形边界；二次单元、
/// 混合单元与局部加密不在本任务内。
#pragma once

#include <cstdint>
#include <functional>
#include <panta/geometry/step_import.hpp>
#include <panta/mesh/mesh_ir.hpp>
#include <string>

namespace panta::mesh {

/// 生成结果分类；kNone 表示成功。失败时 mesh 保持空——候选网格
/// 不发布（architecture/mesh.md：失败不覆盖、不发布半成品）。
enum class TetMeshGenerationStatus : std::uint8_t {
    kNone,
    /// Netgen 边/面/体生成阶段失败；detail 携带阶段与返回码。
    kMeshingFailed,
    /// 生成完成但没有得到可用单元。
    kEmptyMesh,
    /// 结果包含冒烟契约外的单元类型（非四面体/三角形）。
    kConversionFailed,
    /// 候选网格未通过 Mesh IR 有效性检查，已丢弃。
    kInvalidMesh,
    /// Netgen/OCCT 抛出的未预期异常；detail 携带异常原文。
    kInternalFailure,
    /// shape ID 已释放或不存在。
    kShapeNotFound,
    /// shape ID 存在但 revision 已失效。
    kShapeRevisionMismatch,
    /// Netgen face map 中的拓扑面无法映射回来源 shape。
    kFaceMappingFailed,
    /// 调用方经 `is_cancelled` 取消（任务 023）；不发布半成品。
    kCancelled,
};

struct TetMeshGenerationParameters {
    /// 全局最大网格尺寸（毫米）；<= 0 表示使用 Netgen 默认尺寸。
    double max_length_mm = 0.0;
    /// 可选进度回调，参数为 [0,1] 的单调整体进度（任务 023）。在适配器的
    /// 监视线程上调用，实现须线程安全且不得阻塞。
    std::function<void(double)> on_progress;
    /// 可选取消查询（任务 023）。监视线程约每 20 ms 轮询；返回 true 后
    /// 经 Netgen 全局 terminate 标志在下一个检查点中止，结果为 kCancelled。
    /// 也会在调用线程的阶段边界被调用，实现须线程安全。
    std::function<bool()> is_cancelled;
};

struct TetMeshGenerationResult {
    TetMeshGenerationStatus status = TetMeshGenerationStatus::kNone;
    /// 面向日志的英文诊断上下文（失败阶段、Netgen 返回码或校验问题），
    /// 不直接展示给最终用户。
    std::string detail;
    NativeTetMeshDto mesh;
    /// 四面体有向体积之和（毫米³）；成功时为正，失败时为 0。
    double volume_mm3 = 0.0;
};

/// 同步从已导入的 shape 生成四面体网格并转换为 native DTO。
///
/// 适配器内部对全部 Netgen/OCCT 调用串行化（两者均为进程级全局状态，
/// standards/netgen.md 首期保守串行），调用方无须额外加锁。取消为协作式
/// （任务 023）：适配器置 Netgen 的 terminate 标志，Netgen 在其内部检查点
/// 返回；单个检查点之间的耗时由 Netgen 决定，取消不是即时的。调用阻塞，
/// 异步化由调用方（桥接后台任务）负责。
/// 失败返回结构化 status，不抛异常、不终止进程；边界三角形使用连续 mesh
/// group 索引，`mesh.boundary_group_face_ids[group]` 返回来源 shape revision
/// 内对应的稳定 FaceId。调用期间 shape 仓库保持对象有效并串行化 OCCT 访问。
TetMeshGenerationResult generate_tet_mesh(panta::geometry::ShapeRef shape,
                                          const TetMeshGenerationParameters& parameters);

} // namespace panta::mesh
