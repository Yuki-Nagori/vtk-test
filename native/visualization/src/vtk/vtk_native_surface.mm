/// macOS Cocoa/Metal surface 桥接。VTK 持有 Cocoa hardware view 及其
/// CAMetalLayer；本文件只把该视图重新挂入 Qt Quick 的原生宿主视图。
#include "vtk_native_surface.hpp"

#import <Cocoa/Cocoa.h>
#include <QPointF>
#include <QQuickItem>
#include <QQuickWindow>
#include <QtGlobal>
#include <vtkCocoaHardwareView.h>
#include <vtkCocoaHardwareWindow.h>
#include <vtkHardwareWindow.h>

namespace panta::visualization {
namespace {

NSView* qt_native_view(QQuickWindow* window) {
    if (window == nullptr) {
        return nullptr;
    }
    // Cocoa 的 WId 编码 NSView*；这里只恢复 Qt 的句柄，不计算数值地址。
    return reinterpret_cast<NSView*>(window->winId());
}

} // namespace

NativeHardwareWindow create_native_hardware_window(QQuickWindow*) {
    return NativeHardwareWindow{vtkCocoaHardwareWindow::New()};
}

bool attach_native_surface(QQuickWindow* window, QQuickItem*, vtkHardwareWindow* hardware,
                           NativeSurface& surface) {
    auto* cocoa = vtkCocoaHardwareWindow::SafeDownCast(hardware);
    NSView* host = qt_native_view(window);
    NSView* view = cocoa != nullptr ? cocoa->GetViewId() : nullptr;
    if (cocoa == nullptr || host == nullptr || view == nullptr ||
        cocoa->GetMetalLayer() == nullptr) {
        return false;
    }

    [view removeFromSuperview];
    [view setHidden:NO];
    [view setWantsLayer:YES];
    [view setAutoresizingMask:NSViewNotSizable];
    [host addSubview:view positioned:NSWindowAbove relativeTo:nil];
    surface.host = host;
    surface.view = view;
    return true;
}

void sync_native_surface(QQuickWindow*, QQuickItem* item, vtkHardwareWindow* hardware,
                         NativeSurface& surface) {
    auto* host = static_cast<NSView*>(surface.host);
    auto* view = static_cast<NSView*>(surface.view);
    auto* cocoa = vtkCocoaHardwareWindow::SafeDownCast(hardware);
    if (host == nullptr || view == nullptr || cocoa == nullptr || item == nullptr) {
        return;
    }

    const QPointF scene_position = item->mapToScene(QPointF(0, 0));
    const CGFloat width = static_cast<CGFloat>(item->width());
    const CGFloat height = static_cast<CGFloat>(item->height());
    const int logical_width = qMax(1, qRound(item->width()));
    const int logical_height = qMax(1, qRound(item->height()));

    // vtkCocoaHardwareWindow::SetSize() 使用逻辑点而非 backing 像素，且会
    // 更新 VTK 持有的 NSView frame——先调用它，再恢复 QQuickItem 在宿主
    // 视图中的位置。
    cocoa->SetSize(logical_width, logical_height);
    // Qt 的 content view 是 flipped 坐标系（原点左上），子视图 frame 直接
    // 使用 QML scene 坐标；仅非 flipped 宿主需要换算底部原点。
    const NSRect host_bounds = [host bounds];
    const CGFloat y = [host isFlipped] ? static_cast<CGFloat>(scene_position.y())
                                       : host_bounds.size.height -
                                             static_cast<CGFloat>(scene_position.y()) - height;
    [view setFrame:NSMakeRect(static_cast<CGFloat>(scene_position.x()), y, width, height)];
}

void set_native_surface_visible(NativeSurface& surface, bool visible) {
    if (auto* view = static_cast<NSView*>(surface.view); view != nullptr) {
        [view setHidden:visible ? NO : YES];
    }
}

void detach_native_surface(NativeSurface& surface) {
    if (auto* view = static_cast<vtkCocoaHardwareView*>(surface.view); view != nullptr) {
        // AppKit 的已排队事件可能继续持有 view；先断开反向指针，再销毁 hardware window。
        [view setHardwareWindow:nullptr];
        [view setHidden:YES];
        [view removeFromSuperview];
    }
    surface = {};
}

} // namespace panta::visualization
