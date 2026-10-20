/// 相机缩放限幅、姿态变换和方向过渡采样。
#include "viewport_camera.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <vtkActor.h>
#include <vtkCamera.h>
#include <vtkMath.h>
#include <vtkQuaternion.h>

namespace panta::visualization {
namespace {

constexpr double kZoomFactor = 1.15;
constexpr double kMinimumDistanceFactor = 0.02;
constexpr double kMaximumDistanceFactor = 100.0;

using Vector3 = std::array<double, 3>;

vtkQuaterniond camera_orientation(const CameraFrame& frame) {
    Vector3 backward = frame.backward;
    Vector3 up = frame.view_up;
    if (vtkMath::Normalize(backward.data()) == 0.0) {
        backward = {0.0, 0.0, 1.0};
    }

    const double up_projection = vtkMath::Dot(up.data(), backward.data());
    for (std::size_t axis = 0; axis < up.size(); ++axis) {
        up[axis] -= up_projection * backward[axis];
    }
    if (vtkMath::Normalize(up.data()) == 0.0) {
        up = std::abs(backward[2]) < 0.9 ? Vector3{0.0, 0.0, 1.0} : Vector3{0.0, 1.0, 0.0};
        const double fallback_projection = vtkMath::Dot(up.data(), backward.data());
        for (std::size_t axis = 0; axis < up.size(); ++axis) {
            up[axis] -= fallback_projection * backward[axis];
        }
        vtkMath::Normalize(up.data());
    }

    Vector3 right{};
    vtkMath::Cross(up.data(), backward.data(), right.data());
    vtkMath::Normalize(right.data());
    vtkMath::Cross(backward.data(), right.data(), up.data());
    vtkMath::Normalize(up.data());
    double basis[3][3] = {{right[0], up[0], backward[0]},
                          {right[1], up[1], backward[1]},
                          {right[2], up[2], backward[2]}};
    vtkQuaterniond orientation;
    orientation.FromMatrix3x3(basis);
    orientation.Normalize();
    return orientation;
}

vtkQuaterniond camera_orientation(vtkCamera* camera) {
    double position[3];
    double focal_point[3];
    double view_up[3];
    camera->GetPosition(position);
    camera->GetFocalPoint(focal_point);
    camera->GetViewUp(view_up);
    const CameraFrame frame{
        {position[0] - focal_point[0], position[1] - focal_point[1], position[2] - focal_point[2]},
        {view_up[0], view_up[1], view_up[2]}};
    return camera_orientation(frame);
}

CameraFrame camera_frame(const vtkQuaterniond& orientation) {
    double basis[3][3];
    orientation.ToMatrix3x3(basis);
    CameraFrame frame;
    for (std::size_t axis = 0; axis < 3; ++axis) {
        frame.view_up[axis] = basis[axis][1];
        frame.backward[axis] = basis[axis][2];
    }
    return frame;
}

void camera_distance_limits(vtkActor* actor, double& minimum, double& maximum) {
    double bounds[6];
    actor->GetBounds(bounds);
    const double x = bounds[1] - bounds[0];
    const double y = bounds[3] - bounds[2];
    const double z = bounds[5] - bounds[4];
    double diagonal = std::sqrt(x * x + y * y + z * z);
    if (!std::isfinite(diagonal) || diagonal < 1e-6) {
        diagonal = 1.0;
    }
    minimum = std::max(diagonal * kMinimumDistanceFactor, 1e-4);
    maximum = std::max(diagonal * kMaximumDistanceFactor, minimum * 10.0);
}

double smoothstep(double progress) {
    const double bounded = std::clamp(progress, 0.0, 1.0);
    return bounded * bounded * (3.0 - 2.0 * bounded);
}

} // namespace

void ViewportCamera::zoom(vtkCamera* camera, vtkActor* actor, bool zoom_in) {
    camera->Dolly(zoom_in ? kZoomFactor : 1.0 / kZoomFactor);
    double minimum = 0.0;
    double maximum = 0.0;
    camera_distance_limits(actor, minimum, maximum);
    const double distance = std::clamp(camera->GetDistance(), minimum, maximum);
    if (std::abs(distance - camera->GetDistance()) < 1e-12) {
        return;
    }

    double position[3];
    double focal_point[3];
    camera->GetPosition(position);
    camera->GetFocalPoint(focal_point);
    Vector3 direction = {position[0] - focal_point[0], position[1] - focal_point[1],
                         position[2] - focal_point[2]};
    if (vtkMath::Normalize(direction.data()) == 0.0) {
        direction = {0.0, 0.0, 1.0};
    }
    camera->SetPosition(focal_point[0] + direction[0] * distance,
                        focal_point[1] + direction[1] * distance,
                        focal_point[2] + direction[2] * distance);
}

CameraPose ViewportCamera::capture(vtkCamera* camera) {
    CameraPose pose;
    pose.orientation = camera_orientation(camera);
    camera->GetFocalPoint(pose.focal_point.data());
    pose.distance = camera->GetDistance();
    return pose;
}

CameraPose ViewportCamera::make_target(const CameraFrame& frame,
                                       const std::array<double, 3>& focal_point, double distance) {
    CameraPose pose;
    pose.orientation = camera_orientation(frame);
    pose.focal_point = focal_point;
    pose.distance = distance;
    return pose;
}

CameraPose ViewportCamera::interpolate(const CameraPose& start, const CameraPose& target,
                                       double progress) {
    const double eased = smoothstep(progress);
    CameraPose pose;
    pose.orientation = start.orientation.Slerp(eased, target.orientation);
    for (std::size_t axis = 0; axis < pose.focal_point.size(); ++axis) {
        pose.focal_point[axis] =
            start.focal_point[axis] + (target.focal_point[axis] - start.focal_point[axis]) * eased;
    }
    pose.distance = start.distance + (target.distance - start.distance) * eased;
    return pose;
}

void ViewportCamera::apply(vtkCamera* camera, const CameraPose& pose) {
    const CameraFrame frame = camera_frame(pose.orientation);
    camera->SetFocalPoint(pose.focal_point.data());
    camera->SetPosition(pose.focal_point[0] + frame.backward[0] * pose.distance,
                        pose.focal_point[1] + frame.backward[1] * pose.distance,
                        pose.focal_point[2] + frame.backward[2] * pose.distance);
    camera->SetViewUp(frame.view_up.data());
    camera->OrthogonalizeViewUp();
}

void ViewportCamera::clamp_distance(CameraPose& pose, vtkActor* actor) {
    double minimum = 0.0;
    double maximum = 0.0;
    camera_distance_limits(actor, minimum, maximum);
    pose.distance = std::clamp(pose.distance, minimum, maximum);
}

bool ViewportCameraTransition::start(vtkCamera* camera, const CameraFrame& target_frame,
                                     const std::array<double, 3>& target_focal_point,
                                     vtkActor* actor) {
    start_ = ViewportCamera::capture(camera);
    ViewportCamera::clamp_distance(start_, actor);
    target_ = ViewportCamera::make_target(target_frame, target_focal_point, start_.distance);

    double quaternion_dot = 0.0;
    for (int component = 0; component < 4; ++component) {
        quaternion_dot += start_.orientation[component] * target_.orientation[component];
    }
    double focal_delta_squared = 0.0;
    for (std::size_t axis = 0; axis < start_.focal_point.size(); ++axis) {
        const double delta = start_.focal_point[axis] - target_.focal_point[axis];
        focal_delta_squared += delta * delta;
    }
    active_ = std::abs(quaternion_dot) <= 0.999999 || focal_delta_squared >= 1e-12;
    return active_;
}

CameraPose ViewportCameraTransition::pose_at(double elapsed_ms, double duration_ms) const {
    const double progress = duration_ms <= 0.0 ? 1.0 : elapsed_ms / duration_ms;
    return ViewportCamera::interpolate(start_, target_, progress);
}

bool ViewportCameraTransition::active() const { return active_; }

void ViewportCameraTransition::cancel() { active_ = false; }

} // namespace panta::visualization
