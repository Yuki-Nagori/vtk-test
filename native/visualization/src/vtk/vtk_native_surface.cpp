/// Windows/Wayland 原生 surface 桥接。Windows 使用子 HWND；Wayland 把
/// wl_subsurface 挂到 Qt Quick 的顶层 wl_surface。
#include "vtk_native_surface.hpp"

#include <QGuiApplication>
#include <QPointF>
#include <QQuickItem>
#include <QQuickWindow>
#include <QSize>
#include <QtGlobal>
#include <vtkHardwareWindow.h>

#if defined(Q_OS_WIN)
#include <vtkWin32HardwareWindow.h>
// windef/winuser 是 HWND、SetWindowPos 等符号的 granular provider；伞头
// windows.h 不能删除：它按 winnt → winbase → winuser 次序装配的宏与类型
// 前提无法被拆分头重提供，单独删除后 granular 头直接编译失败（PSLIST_
// HEADER 等）。保留伞头是登记例外，见 standards/comments.md。
#include <windef.h>
#include <windows.h>
#include <winuser.h>
#elif defined(Q_OS_LINUX)
#include <algorithm>
#include <cstdint>
#include <cstring>
#include <qguiapplication_platform.h>
#include <vtkObjectFactory.h>
#include <vtkSetGet.h>
#include <vtkWaylandHardwareWindow.h>
#include <vtkWindow.h>
#include <wayland-client-core.h>
#include <wayland-client-protocol.h>

namespace {

class PantaWaylandHardwareWindow final : public vtkWaylandHardwareWindow {
  public:
    static PantaWaylandHardwareWindow* New();
    // vtkTypeMacro 生成的 NewInstance 按设计逐层遮蔽基类同名非虚方法（VTK
    // 的虚分发走 NewInstanceInternal），是 VTK 子类化惯例而非缺陷。
    vtkTypeMacro(PantaWaylandHardwareWindow, vtkWaylandHardwareWindow)

        void set_qt_surface(wl_display* display, wl_compositor* compositor, wl_surface* parent) {
        DisplayId = display;
        Compositor = compositor;
        ParentSurface = parent;
        OwnDisplay = false;
    }

    void Create() override {
        if (DisplayId == nullptr || Compositor == nullptr || ParentSurface == nullptr) {
            return;
        }

        Registry = wl_display_get_registry(DisplayId);
        if (Registry == nullptr) {
            return;
        }
        wl_registry_add_listener(Registry, &registry_listener, this);
        if (wl_display_roundtrip(DisplayId) < 0 || Subcompositor == nullptr) {
            return;
        }

        Surface = wl_compositor_create_surface(Compositor);
        if (Surface == nullptr) {
            return;
        }
        Subsurface = wl_subcompositor_get_subsurface(Subcompositor, Surface, ParentSurface);
        if (Subsurface == nullptr) {
            wl_surface_destroy(Surface);
            Surface = nullptr;
            return;
        }
        wl_subsurface_set_desync(Subsurface);
        Mapped = true;
    }

    void Destroy() override {
        if (Subsurface != nullptr) {
            wl_subsurface_destroy(Subsurface);
            Subsurface = nullptr;
        }
        if (Surface != nullptr) {
            wl_surface_destroy(Surface);
            Surface = nullptr;
        }
        if (Subcompositor != nullptr) {
            wl_subcompositor_destroy(Subcompositor);
            Subcompositor = nullptr;
        }
        if (Registry != nullptr) {
            wl_registry_destroy(Registry);
            Registry = nullptr;
        }
        Mapped = false;
    }

    void set_geometry(const QPointF& position, const QSize& pixel_size, int scale) {
        // 刻意绕过 vtkWaylandHardwareWindow::SetSize——其按基类自有 surface
        // 语义实现；本类以自管 wl_subsurface 承载几何，仅需基类尺寸簿记。
        vtkWindow::SetSize(pixel_size.width(), pixel_size.height());
        if (Subsurface == nullptr || Surface == nullptr) {
            return;
        }
        wl_surface_set_buffer_scale(Surface, std::max(1, scale));
        wl_subsurface_set_position(Subsurface, qRound(position.x()), qRound(position.y()));
        wl_surface_commit(Surface);
    }

  protected:
    PantaWaylandHardwareWindow() = default;
    ~PantaWaylandHardwareWindow() override { Destroy(); }

  private:
    static void registry_global(void* data, wl_registry* registry, uint32_t name,
                                const char* interface, uint32_t version) {
        auto* self = static_cast<PantaWaylandHardwareWindow*>(data);
        if (self->Subcompositor == nullptr &&
            std::strcmp(interface, wl_subcompositor_interface.name) == 0) {
            self->Subcompositor = static_cast<wl_subcompositor*>(wl_registry_bind(
                registry, name, &wl_subcompositor_interface, std::min(version, 1u)));
        }
    }

    static void registry_global_remove(void*, wl_registry*, uint32_t) {}

    inline static const wl_registry_listener registry_listener = {
        &PantaWaylandHardwareWindow::registry_global,
        &PantaWaylandHardwareWindow::registry_global_remove,
    };

    wl_surface* ParentSurface = nullptr;
    wl_subcompositor* Subcompositor = nullptr;
    wl_subsurface* Subsurface = nullptr;
};

vtkStandardNewMacro(PantaWaylandHardwareWindow);

} // namespace
#endif

namespace panta::visualization {

#if defined(Q_OS_WIN)

namespace {

// Qt winId() 以 quintptr 承载原生 HWND，VTK SetParentId 以 void* 消费；
// 两侧 API 均无类型化通道，句柄回传转换集中在此豁免性能检查。
void* qt_native_handle_to_void(quintptr handle) { return reinterpret_cast<void*>(handle); }

} // namespace

NativeHardwareWindow create_native_hardware_window(QQuickWindow* window) {
    const auto parent_handle = window != nullptr ? window->winId() : quintptr{0};
    auto* hardware = vtkWin32HardwareWindow::New();
    hardware->SetParentId(qt_native_handle_to_void(parent_handle));
    return NativeHardwareWindow{hardware};
}

bool attach_native_surface(QQuickWindow*, QQuickItem*, vtkHardwareWindow* hardware,
                           NativeSurface& surface) {
    surface.view = hardware != nullptr ? hardware->GetGenericWindowId() : nullptr;
    return surface.view != nullptr;
}

void sync_native_surface(QQuickWindow*, QQuickItem* item, vtkHardwareWindow* hardware,
                         NativeSurface& surface) {
    auto* child = static_cast<HWND>(surface.view);
    if (child == nullptr || item == nullptr || hardware == nullptr || item->window() == nullptr) {
        return;
    }
    const QPointF position = item->mapToScene(QPointF(0, 0));
    const qreal scale = item->window()->devicePixelRatio();
    const int width = qMax(1, qRound(item->width() * scale));
    const int height = qMax(1, qRound(item->height() * scale));
    hardware->SetSize(width, height);
    SetWindowPos(child, HWND_TOP, qRound(position.x() * scale), qRound(position.y() * scale), width,
                 height, SWP_NOACTIVATE | SWP_NOOWNERZORDER);
}

void set_native_surface_visible(NativeSurface& surface, bool visible) {
    if (auto* child = static_cast<HWND>(surface.view); child != nullptr) {
        ShowWindow(child, visible ? SW_SHOW : SW_HIDE);
    }
}

void detach_native_surface(NativeSurface& surface) {
    if (auto* child = static_cast<HWND>(surface.view); child != nullptr) {
        ShowWindow(child, SW_HIDE);
    }
    surface = {};
}

#elif defined(Q_OS_LINUX)

NativeHardwareWindow create_native_hardware_window(QQuickWindow* window) {
    // nativeInterface<T>() 的兼容约束按接收者静态类型解析：QCoreApplication*
    // 不满足 QWaylandApplication 的要求，必须落在 QGuiApplication 上。
    auto* application = static_cast<QGuiApplication*>(QGuiApplication::instance());
    auto* wayland = application != nullptr
                        ? application->nativeInterface<QNativeInterface::QWaylandApplication>()
                        : nullptr;
    // winId() 在 Wayland 上承载 wl_surface 指针；Qt 未提供类型化入口，整型
    // 中转是平台句柄契约而非值语义转换。
    const auto parent_handle = window != nullptr ? window->winId() : quintptr{0};
    auto* parent_surface = reinterpret_cast<wl_surface*>(parent_handle);
    auto* hardware = PantaWaylandHardwareWindow::New();
    hardware->set_qt_surface(wayland != nullptr ? wayland->display() : nullptr,
                             wayland != nullptr ? wayland->compositor() : nullptr, parent_surface);
    return NativeHardwareWindow{hardware};
}

bool attach_native_surface(QQuickWindow*, QQuickItem*, vtkHardwareWindow* hardware,
                           NativeSurface& surface) {
    auto* wayland = vtkWaylandHardwareWindow::SafeDownCast(hardware);
    surface.view = wayland != nullptr ? wayland->GetWindowId() : nullptr;
    surface.host = wayland != nullptr ? wayland->GetDisplayId() : nullptr;
    return surface.view != nullptr && surface.host != nullptr;
}

void sync_native_surface(QQuickWindow* window, QQuickItem* item, vtkHardwareWindow* hardware,
                         NativeSurface&) {
    auto* wayland = PantaWaylandHardwareWindow::SafeDownCast(hardware);
    if (wayland == nullptr || window == nullptr || item == nullptr) {
        return;
    }
    const QPointF position = item->mapToScene(QPointF(0, 0));
    const qreal scale = window->devicePixelRatio();
    wayland->set_geometry(
        QPointF(qRound(position.x()), qRound(position.y())),
        QSize(qMax(1, qRound(item->width() * scale)), qMax(1, qRound(item->height() * scale))),
        qMax(1, qRound(scale)));
}

void set_native_surface_visible(NativeSurface& surface, bool visible) {
    // 隐藏经 attach(null)+commit 取消映射；重新显示不在此实现——由
    // VtkViewport 在可见分支强制 Render() 重新提交 swapchain buffer 完成
    // 重映射（该耦合需在真实 Wayland 环境持续验证，见任务 007 验证表）。
    auto* view = static_cast<wl_surface*>(surface.view);
    if (view == nullptr || visible) {
        return;
    }
    wl_surface_attach(view, nullptr, 0, 0);
    wl_surface_commit(view);
}
void detach_native_surface(NativeSurface& surface) { surface = {}; }

#else

NativeHardwareWindow create_native_hardware_window(QQuickWindow*) { return NativeHardwareWindow{}; }

bool attach_native_surface(QQuickWindow*, QQuickItem*, vtkHardwareWindow*, NativeSurface&) {
    return false;
}
void sync_native_surface(QQuickWindow*, QQuickItem*, vtkHardwareWindow*, NativeSurface&) {}
void set_native_surface_visible(NativeSurface&, bool) {}
void detach_native_surface(NativeSurface& surface) { surface = {}; }

#endif

} // namespace panta::visualization
