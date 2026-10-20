/// VTK 事件与视口交互职责的映射，不持有相机或窗口状态。
#include "viewport_input.hpp"

#include <vtkCommand.h>

namespace panta::visualization {

ViewportInputAction classify_viewport_input(unsigned long event_id, bool rotation_active) {
    if (event_id == vtkCommand::MouseWheelForwardEvent) {
        return ViewportInputAction::ZoomIn;
    }
    if (event_id == vtkCommand::MouseWheelBackwardEvent) {
        return ViewportInputAction::ZoomOut;
    }
    if (event_id == vtkCommand::LeftButtonPressEvent) {
        return ViewportInputAction::PressCube;
    }
    if (event_id == vtkCommand::RightButtonPressEvent) {
        return ViewportInputAction::BeginRotation;
    }
    if (event_id == vtkCommand::MouseMoveEvent && rotation_active) {
        return ViewportInputAction::MoveRotation;
    }
    if (event_id == vtkCommand::LeftButtonReleaseEvent) {
        return ViewportInputAction::ReleaseCube;
    }
    if (event_id == vtkCommand::RightButtonReleaseEvent) {
        return ViewportInputAction::EndRotation;
    }
    return ViewportInputAction::Ignore;
}

} // namespace panta::visualization
