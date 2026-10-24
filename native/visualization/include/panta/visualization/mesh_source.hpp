/// 工程服务向视口提交的后端中立网格快照；没有第三方渲染类型。
#pragma once

#include <QObject>
#include <array>
#include <cstdint>
#include <memory>
#include <vector>

namespace panta::visualization {

/// 结果场色标控制点（task 029）：位置 0–1，RGB 线性插值；视口查找表与 QML 色标共用同一组。
struct FieldColorStop {
    double position;
    double red;
    double green;
    double blue;
};

/// ParaView 默认 "Cool to Warm" 的三个端点（发散型），RGB 线性插值近似其 Msh 插值。
inline constexpr std::array<FieldColorStop, 3> field_color_stops{{
    {0.0, 0.231, 0.299, 0.754},
    {0.5, 0.865, 0.865, 0.865},
    {1.0, 0.706, 0.016, 0.150},
}};
/// 缺失值（NaN）顶点的颜色，与色标两端和中段都可区分。
inline constexpr std::array<double, 3> field_missing_color{1.0, 1.0, 0.0};

struct SurfaceMeshSnapshot {
    std::vector<std::array<double, 3>> vertices;
    std::uint64_t project_revision = 0;
    /// 场剖面着色（task 029）：与 `vertices` 一一对应时按查找表着色，否则用材质单色；
    /// NaN 为缺失值。`scalar_min <= scalar_max`，相等时由视口扩成非零区间。
    std::vector<float> scalars;
    double scalar_min = 0.0;
    double scalar_max = 1.0;
};

class MeshSource : public QObject {
    Q_OBJECT

  public:
    using QObject::QObject;
    ~MeshSource() override = default;
    /// 返回已准备好的本地显示 DTO。视口刷新会读取此值；实现不得在此调用
    /// Rust / FFI，网格跨界复制只发生在导入、激活等粗粒度状态变化中。
    [[nodiscard]] virtual std::shared_ptr<const SurfaceMeshSnapshot> mesh_snapshot() const = 0;

  signals:
    void meshChanged();
};

} // namespace panta::visualization
