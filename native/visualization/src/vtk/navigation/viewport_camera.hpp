/// VTK 视口相机导航的 CPU 状态计算。
#pragma once

#include <array>
#include <vtkQuaternion.h>

class vtkActor;
class vtkCamera;

namespace panta::visualization {

struct CameraFrame {
    /// 从焦点指向相机的世界坐标轴。
    std::array<double, 3> backward;
    /// 世界坐标中的屏幕向上轴。
    std::array<double, 3> view_up;
};

struct CameraPose {
    vtkQuaterniond orientation;
    std::array<double, 3> focal_point{};
    /// 相机位置到焦点的距离，单位与场景几何相同。
    double distance = 1.0;
};

/// 将滚轮缩放和方向切换的纯相机计算隔离出来，供视口和无窗口测试共同使用。
class ViewportCamera final {
  public:
    static void zoom(vtkCamera* camera, vtkActor* actor, bool zoom_in);
    [[nodiscard]] static CameraPose capture(vtkCamera* camera);
    [[nodiscard]] static CameraPose make_target(const CameraFrame& frame,
                                                const std::array<double, 3>& focal_point,
                                                double distance);
    [[nodiscard]] static CameraPose interpolate(const CameraPose& start, const CameraPose& target,
                                                double progress);
    static void apply(vtkCamera* camera, const CameraPose& pose);

  private:
    friend class ViewportCameraTransition;
    static void clamp_distance(CameraPose& pose, vtkActor* actor);
};

/// 记录一次可被新点击重定向的相机过渡；姿态采样由视口的 GUI 定时器驱动。
class ViewportCameraTransition final {
  public:
    /// 从相机当前姿态开始过渡；目标已相同时返回 false。
    [[nodiscard]] bool start(vtkCamera* camera, const CameraFrame& target_frame,
                             const std::array<double, 3>& target_focal_point, vtkActor* actor);
    /// 按毫秒采样缓动姿态；进度超出时长时返回目标姿态。
    [[nodiscard]] CameraPose pose_at(double elapsed_ms, double duration_ms) const;
    [[nodiscard]] bool active() const;
    void cancel();

  private:
    CameraPose start_;
    CameraPose target_;
    bool active_ = false;
};

} // namespace panta::visualization
