/// Qt Quick 原生窗口与 VTK hardware window 的平台桥接。
///
/// 该头文件只暴露 VTK 基类和不透明 surface 状态；AppKit、Win32、Wayland
/// 类型及其生命周期全部留在平台实现中。
#pragma once

#include <memory>
#include <vtkHardwareWindow.h>

class QQuickItem;
class QQuickWindow;

namespace panta::visualization {

/// hardware window 所有权用 unique_ptr 承载；删除器按 VTK 引用计数规则
/// 经 Delete() 释放，三平台桥接共用这一定义。
struct HardwareWindowDeleter {
    void operator()(vtkHardwareWindow* hardware) const {
        if (hardware != nullptr) {
            hardware->Delete();
        }
    }
};

using NativeHardwareWindow = std::unique_ptr<vtkHardwareWindow, HardwareWindowDeleter>;

struct NativeSurface {
    void* host = nullptr;
    void* view = nullptr;
};

/// 为 Qt Quick 窗口创建使用其原生 surface 的 VTK hardware window。
NativeHardwareWindow create_native_hardware_window(QQuickWindow* window);

/// 将 hardware window 的 view/layer 接入 Qt Quick 原生窗口。
bool attach_native_surface(QQuickWindow* window, QQuickItem* item, vtkHardwareWindow* hardware,
                           NativeSurface& surface);

void sync_native_surface(QQuickWindow* window, QQuickItem* item, vtkHardwareWindow* hardware,
                         NativeSurface& surface);
void set_native_surface_visible(NativeSurface& surface, bool visible);
void detach_native_surface(NativeSurface& surface);

} // namespace panta::visualization
