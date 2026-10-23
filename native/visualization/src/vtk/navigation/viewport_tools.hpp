/// 视口工具的纯 CPU 计算（task 017）：预设视角坐标系与射线拾取。
/// 不依赖 VTK 对象，供视口与无 GPU 测试共用。
#pragma once

#include "viewport_camera.hpp"
#include <QString>
#include <array>
#include <cstdint>
#include <optional>
#include <panta/visualization/mesh_source.hpp>

namespace panta::visualization {

enum class ViewPreset : std::uint8_t { Front, Back, Left, Right, Top, Bottom, Isometric };

/// QML 名称（front/back/left/right/top/bottom/iso）到预设；未知名称返回空。
[[nodiscard]] std::optional<ViewPreset> parse_view_preset(const QString& name);

/// 预设视角的相机坐标系（Z 向上，前视从 -Y 看向 +Y）。
[[nodiscard]] CameraFrame preset_frame(ViewPreset preset);

struct PickHit {
    std::array<double, 3> point{};
    /// 三角形序号（三角形汤中第 triangle 个三角形）。
    std::uint64_t triangle = 0;
    /// 沿射线的参数距离（direction 已归一化时即世界距离）。
    double distance = 0.0;
};

/// 射线与三角形汤求最近交点（Möller–Trumbore，双面）；线性扫描，O(n)。
[[nodiscard]] std::optional<PickHit> pick_triangle_soup(const SurfaceMeshSnapshot& mesh,
                                                        const std::array<double, 3>& origin,
                                                        const std::array<double, 3>& direction);

} // namespace panta::visualization
