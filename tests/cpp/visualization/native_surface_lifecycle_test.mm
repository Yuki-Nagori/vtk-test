// Cocoa 延迟事件回归：AppKit 保留的 view 必须在 hardware window 销毁后仍安全。
#include "vtk_native_surface.hpp"
#include <QGuiApplication>
#include <QObject>
#include <QQuickWindow>
#include <QString>
#include <QtCore/qtenvironmentvariables.h>
#include <QtCore/qtmetamacros.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <vtkCocoaHardwareView.h>
#include <vtkCocoaHardwareWindow.h>
#include <vtkHardwareWindow.h>

class NativeSurfaceLifecycleTest final : public QObject {
    Q_OBJECT
  private slots:
    void detached_view_ignores_late_mouse_events() {
        if (!qEnvironmentVariableIsSet("PANTA_TEST_NATIVE_VIEWPORT")) {
            QSKIP("Native Cocoa lifecycle requires PANTA_TEST_NATIVE_VIEWPORT=1");
        }
        QVERIFY(QGuiApplication::platformName() == QStringLiteral("cocoa"));
        QQuickWindow host;
        auto hardware = panta::visualization::create_native_hardware_window(&host);
        hardware->SetShowWindow(false);
        hardware->Create();
        auto* cocoa = vtkCocoaHardwareWindow::SafeDownCast(hardware.get());
        QVERIFY(cocoa != nullptr);
        auto* view = static_cast<vtkCocoaHardwareView*>(cocoa->GetViewId());
        QVERIFY(view != nullptr);
        // 强引用模拟排队的 AppKit 事件；不要求 surface 显示或提交 GPU 帧。
        [view retain];
        panta::visualization::NativeSurface surface{nullptr, view};
        panta::visualization::detach_native_surface(surface);
        const bool detached = [view getHardwareWindow] == nullptr;
        hardware->Destroy();
        hardware.reset();
        const bool no_interactor = [view getInteractor] == nullptr;
        for (const auto type : {NSEventTypeMouseEntered, NSEventTypeMouseExited}) {
            NSEvent* event = [NSEvent enterExitEventWithType:type
                                                    location:NSZeroPoint
                                               modifierFlags:0
                                                   timestamp:0
                                                windowNumber:0
                                                     context:nil
                                                 eventNumber:0
                                              trackingNumber:0
                                                    userData:nullptr];
            QVERIFY(event != nil);
            if (type == NSEventTypeMouseEntered) {
                [view mouseEntered:event];
            } else {
                [view mouseExited:event];
            }
        }
        [view release];
        QVERIFY(detached);
        QVERIFY(no_interactor);
        QVERIFY(surface.view == nullptr);
    }
};
QTEST_MAIN(NativeSurfaceLifecycleTest)
#include "native_surface_lifecycle_test.moc"
