/// VtkViewport：VTK WebGPU 后端适配器。
///
/// 该类型只作为 QQuickItem 宿主参与布局；VTK 不进入 Qt Quick scenegraph。
/// 原生 view/layer/surface 的创建和尺寸同步由 vtk_native_surface 平台桥接完成。
#pragma once

#include <QQuickItem>
#include <QSize>
#include <memory>
#include <panta/visualization/render_scene.hpp>
#include <panta/visualization/viewport_backend.hpp>

class vtkRenderWindowInteractor;

namespace panta::visualization {

class ViewportInteractionCommand;
struct CameraFrame;
class ViewportTestAccess;

class VtkViewport final : public QQuickItem, public ViewportBackend {
    Q_OBJECT

  public:
    explicit VtkViewport(QQuickItem* parent = nullptr);
    ~VtkViewport() override;

    QQuickItem* item() override;
    void apply_state(const RenderScene& state) override;
    QImage capture_image() override;
    bool set_view_preset(const QString& name) override;
    void reset_view() override;

  signals:
    /// 原生 surface 与最小 VTK 场景已建立。
    void sceneInitialized();
    /// 左键在模型区域（非方向立方体）单击后的拾取结果；未命中时 hit=false。
    void pointPicked(bool hit, double x, double y, double z, quint64 triangle);

  protected:
    void geometryChange(const QRectF& new_geometry, const QRectF& old_geometry) override;
    void itemChange(ItemChange change, const ItemChangeData& value) override;

  private:
    friend class ViewportInteractionCommand;
    friend class ViewportTestAccess;

    void bind_window(QQuickWindow* window);
    void ensure_render_window();
    void schedule_refresh();
    void watch_ancestors();
    void update_mesh_actor();
    void handle_interaction_event(unsigned long event_id, vtkRenderWindowInteractor* interactor);
    void advance_camera_transition();
    void stop_camera_transition();
    void start_camera_frame(const CameraFrame& frame);
    void pick_at(int display_x, int display_y);
    /// 排队刷新时同步原生区域，仅状态/像素尺寸改变或恢复显示时提交帧。
    void sync_native_surface();
    void destroy_render_window();
    /// 测试只读生命周期状态，不转移 VTK 对象所有权。
    const void* test_actor_identity() const;
    bool test_has_mapper() const;
    bool test_previous_mapper_released() const;
    bool test_interaction_observers_registered() const;
    bool test_previous_window_resources_released() const;
    bool test_camera_transition_active() const;
    bool test_camera_transition_timer_active() const;
    QSize test_applied_pixel_size() const;
    /// 当前没有 render window、hardware window、interactor 或已接入的原生 view。
    bool test_render_resources_absent() const;
    /// 下次初始化用 Dawn 不提供的 backend 预检 adapter，走 device 失败清理。
    /// 生产路径不调用。VTK 9.7.0 在 adapter 为空时会于 Initialize 内崩溃。
    void test_fail_webgpu_device_initialization();
    /// 在真实 VTK 窗口中启动短时相机过渡，验证资源回收会停止 GUI 定时器。
    void test_start_camera_transition();

    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace panta::visualization
