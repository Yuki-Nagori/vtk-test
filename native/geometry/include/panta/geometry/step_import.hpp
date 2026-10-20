/// STEP 导入冒烟的公共几何契约（任务 009，architecture/geometry.md 导入流程
/// 的最小可验证边界）。公共头只含自有类型与标准库；OCCT 类型限定在适配器
/// 内部（standards/occt.md）。坐标与包围盒一律毫米：源文件单位经读取器
/// 换算，摘要同时记录源单位与其到毫米的系数。
///
/// 冒烟采用 STEPControl 基础路径：装配结构、名称、颜色等 XDE 附属元数据
/// 不保留；完整导入产品流程另建业务任务。
#pragma once

#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace panta::geometry {

/// 进程内 shape 句柄；不作为工程资产身份或持久化拓扑 ID。
using ShapeId = std::uint64_t;
using ShapeRevision = std::uint64_t;
using FaceId = std::uint32_t;

struct ShapeRef {
    ShapeId id = 0;
    ShapeRevision revision = 0;

    friend bool operator==(const ShapeRef&, const ShapeRef&) = default;
};

enum class ShapeRepositoryStatus : std::uint8_t {
    kNone,
    kNotFound,
    kRevisionMismatch,
    kInternalFailure,
};

/// STEP 文件声明的长度单位；kUnknown 表示文件未声明或名称未被适配器识别。
/// 坐标无论如何都由读取器转换为毫米，kUnknown 只影响换算元数据。
/// 显式基础类型固定取值域宽度，避免公共枚举的 ABI 尺寸随实现漂移。
enum class LengthUnit : std::uint8_t {
    kUnknown,
    kMillimetre,
    kCentimetre,
    kMetre,
    kMicron,
    kInch,
    kFoot,
    kMile,
    kMil,
    kMicroinch,
};

/// 导入结果分类；kNone 表示成功。失败时 summary 仅用于诊断，不得当作
/// 有效导入结果提交工程。
enum class StepImportStatus : std::uint8_t {
    kNone,
    /// 文件不可读或不是可识别的 STEP 输入。
    kFileUnreadable,
    /// 文件已读取但无法解析出可用的模型内容。
    kParseFailed,
    /// 模型为空或没有可转移的根实体。
    kEmptyModel,
    /// 存在未转移成功的候选根；summary 记录已转移部分。
    kTransferIncomplete,
    /// 转移完成但结果不含任何几何实体。
    kEmptyResult,
    /// OCCT 异常或其他内部失败；detail 携带上下文。
    kInternalFailure,
};

/// 一次成功导入的自有摘要：单位、包围盒与去重拓扑计数。
struct StepImportSummary {
    /// 源文件长度单位。
    LengthUnit source_unit = LengthUnit::kUnknown;
    /// 源单位到毫米的换算系数（如 Inch=25.4）；kUnknown 时为 0，坐标仍为
    /// 读取器转换后的毫米值。
    double source_unit_to_mm = 0.0;
    /// 是否得到非空几何；包围盒仅在该值为 true 时有效。
    bool has_geometry = false;
    /// 包围盒角点（毫米）。这是含形状公差的保守包络（Bnd_Box gap，典型
    /// Precision::Confusion ≈ 1e-7 mm），不等于名义几何角点。
    double bbox_min[3] = {};
    double bbox_max[3] = {};
    /// 去重拓扑计数：共享拓扑只计一次。
    std::uint32_t solids = 0;
    /// 去重拓扑面数，同时是本 revision 可分配的零起面 ID 数量。
    std::uint32_t faces = 0;
    std::uint32_t edges = 0;
    /// 候选根总数与实际转移成功数；两者相等表示完整转换。
    std::uint32_t roots_total = 0;
    std::uint32_t roots_transferred = 0;
};

struct StepImportResult {
    StepImportStatus status = StepImportStatus::kNone;
    /// 面向日志的英文诊断上下文（OCCT 消息原文或失败阶段），不直接展示给
    /// 最终用户；用户可读文案由后续业务任务与 022 的语言字典承接。
    std::string detail;
    StepImportSummary summary;
    /// 成功导入后可用于查询或网格生成的进程内句柄。
    ShapeRef shape;
};

struct ShapeFaceSummary {
    /// 对应 ShapeRef revision 内的零起 FaceId。
    FaceId id = 0;
    /// 非空边界盒；退化/无效面保留 ID 并标记为无边界盒。
    bool has_bounds = false;
    double bbox_min[3] = {};
    double bbox_max[3] = {};
};

struct ShapeSummaryResult {
    ShapeRepositoryStatus status = ShapeRepositoryStatus::kNone;
    std::string detail;
    StepImportSummary summary;
};

struct ShapeFacesResult {
    ShapeRepositoryStatus status = ShapeRepositoryStatus::kNone;
    std::string detail;
    std::vector<ShapeFaceSummary> face_summaries;
};

/// 同步读取 STEP 文件、生成摘要并将同一个 shape 存入进程内仓库。
///
/// 适配器内部对全部 OCCT 访问串行化（Interface_Static 与 translator 参数
/// 为进程级状态，standards/occt.md），调用方无须额外加锁；当前不提供中途
/// 取消，超小样例外耗时亚秒级，异步任务与取消边界归后续业务任务。
/// 失败返回结构化 status 并携带诊断，不抛异常、不终止进程。成功结果的
/// ShapeRef 必须由调用方显式释放；shape 和其 FaceId 仅在该句柄 revision
/// 内有效。释放与内部借用由同一把锁串行，释放返回时已完成先前借用且句柄已移除。
/// 所有 OCCT 访问由适配器串行化。
StepImportResult import_step(const std::filesystem::path& file);

/// 按完整句柄查询已缓存的形状摘要，不重新遍历几何。已释放或未知 ID 返回
/// kNotFound，旧 revision 返回 kRevisionMismatch；查询不转移仓库所有权。
ShapeSummaryResult query_shape_summary(ShapeRef shape);

/// 显式查询各面 ID 与边界盒。该操作遍历所有面并调用 OCCT 包围盒算法，可能
/// 比常规摘要查询更耗时；只在确实需要面级几何信息时调用。
ShapeFacesResult query_shape_faces(ShapeRef shape);

/// 显式释放 shape。释放与 OCCT 操作串行，返回时已删除仓库持有的对象；成功后
/// 该 ID 不再可查询或用于网格生成；重复释放返回 kNotFound。当前没有修改 shape
/// 的操作，因此成功导入的 revision 固定。
ShapeRepositoryStatus release_shape(ShapeRef shape);

} // namespace panta::geometry
