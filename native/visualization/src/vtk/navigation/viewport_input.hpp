/// VTK 输入事件到视口交互职责的映射。
#pragma once

#include <cstdint>

namespace panta::visualization {

enum class ViewportInputAction : std::uint8_t {
    Ignore,
    ZoomIn,
    ZoomOut,
    PressCube,
    BeginRotation,
    MoveRotation,
    ReleaseCube,
    EndRotation,
};

/// 将 VTK 事件映射到视口职责；只有右键按下后移动才产生旋转动作。
[[nodiscard]] ViewportInputAction classify_viewport_input(unsigned long event_id,
                                                          bool rotation_active);

} // namespace panta::visualization
