#include "panta/qt_adapter/system_motion_preference.hpp"
#import <AppKit/NSAccessibility.h>
#import <AppKit/NSWorkspace.h>
#import <Foundation/Foundation.h>
#include <QMetaObject>
#include <QPointer>

extern "C" bool panta_macos_read_reduced_motion() {
    return [[NSWorkspace sharedWorkspace] accessibilityDisplayShouldReduceMotion];
}

extern "C" void*
panta_macos_observe_reduced_motion(panta::qt_adapter::SystemMotionPreference* preferences) {
    QPointer<panta::qt_adapter::SystemMotionPreference> guardedPreferences(preferences);
    NSWorkspace* workspace = [NSWorkspace sharedWorkspace];
    id observer = [[workspace notificationCenter]
        addObserverForName:NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification
                    object:nil
                     queue:[NSOperationQueue mainQueue]
                usingBlock:^(NSNotification*) {
                  if (guardedPreferences != nullptr) {
                      QMetaObject::invokeMethod(guardedPreferences.data(), "refreshReducedMotion",
                                                Qt::QueuedConnection);
                  }
                }];
    return (__bridge_retained void*)observer;
}

extern "C" void panta_macos_remove_reduced_motion_observer(void* observer) {
    id token = (__bridge_transfer id)observer;
    [[[NSWorkspace sharedWorkspace] notificationCenter] removeObserver:token];
}
