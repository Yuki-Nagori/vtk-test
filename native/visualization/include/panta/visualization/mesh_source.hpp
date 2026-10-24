/// 工程服务向视口提交的后端中立网格快照；没有第三方渲染类型。
#pragma once

#include <QObject>
#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <span>
#include <string>
#include <string_view>
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
inline constexpr std::array<FieldColorStop, 3> cool_to_warm_stops{{
    {0.0, 0.231, 0.299, 0.754},
    {0.5, 0.865, 0.865, 0.865},
    {1.0, 0.706, 0.016, 0.150},
}};
/// matplotlib "viridis" 等距 5 点采样（顺序型、感知均匀、色盲友好）。
inline constexpr std::array<FieldColorStop, 5> viridis_stops{{
    {0.00, 0.267, 0.005, 0.329},
    {0.25, 0.231, 0.322, 0.545},
    {0.50, 0.129, 0.569, 0.549},
    {0.75, 0.369, 0.788, 0.384},
    {1.00, 0.992, 0.906, 0.145},
}};
/// ParaView "Blue to Red Rainbow"：蓝-青-绿-黄-红，CAE 后处理的传统配色。
inline constexpr std::array<FieldColorStop, 5> rainbow_stops{{
    {0.00, 0.0, 0.0, 1.0},
    {0.25, 0.0, 1.0, 1.0},
    {0.50, 0.0, 1.0, 0.0},
    {0.75, 1.0, 1.0, 0.0},
    {1.00, 1.0, 0.0, 0.0},
}};
/// 灰度：黑到白。
inline constexpr std::array<FieldColorStop, 2> grayscale_stops{{
    {0.0, 0.0, 0.0, 0.0},
    {1.0, 1.0, 1.0, 1.0},
}};

/// 结果场颜色图预设（task 039）：视口查找表、视口色标与属性面板图例共用。
/// 缺失色（NaN）按预设选取，与该预设的两端和中段都可区分。
struct FieldColorMap {
    std::string_view label;
    std::span<const FieldColorStop> stops;
    std::array<double, 3> missing_color;
};

inline constexpr std::array<FieldColorMap, 4> field_color_maps{{
    {"冷暖（Cool to Warm）", cool_to_warm_stops, {1.0, 1.0, 0.0}},
    {"Viridis", viridis_stops, {1.0, 0.0, 0.0}},
    {"彩虹（蓝-红）", rainbow_stops, {0.5, 0.5, 0.5}},
    {"灰度", grayscale_stops, {1.0, 0.0, 0.0}},
}};

/// 越界索引回退到默认预设（0，Cool to Warm）。
[[nodiscard]] constexpr const FieldColorMap& field_color_map(std::size_t index) noexcept {
    return index < field_color_maps.size() ? field_color_maps[index] : field_color_maps[0];
}

struct SurfaceMeshSnapshot {
    std::vector<std::array<double, 3>> vertices;
    std::uint64_t project_revision = 0;
    /// 场剖面着色（task 029）：与 `vertices` 一一对应时按查找表着色，否则用材质单色；
    /// NaN 为缺失值。`scalar_min <= scalar_max`，相等时由视口扩成非零区间。
    std::vector<float> scalars;
    double scalar_min = 0.0;
    double scalar_max = 1.0;
    /// 视口色标（task 038）：UTF-8 场名、单位与区间口径（如“全部时间步”）；
    /// 场名为空时不显示色标，单位与口径为空时省略对应部分。
    std::string scalar_title;
    std::string scalar_units;
    std::string scalar_range_label;
    /// 颜色图预设索引（task 039，`field_color_maps`）；越界时按默认预设。
    std::size_t color_map = 0;
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
