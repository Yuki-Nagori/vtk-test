/// VtkViewport 实现：在 GUI 线程管理 VTK WebGPU 与原生 surface。
///
/// Qt Quick 只负责 QQuickItem 宿主的几何和可见性；VTK 的 render window
/// 直接绘制到平台 surface，避免把 OpenGL/WebGPU 资源混入 scenegraph。
#include "vtk_viewport.hpp"

#include "mesh_source.hpp"
#include "navigation/viewport_camera.hpp"
#include "navigation/viewport_input.hpp"
#include "navigation/viewport_orientation.hpp"
#include "primitive_scene.hpp"
#include "vtk_native_surface.hpp"
#include <QElapsedTimer>
#include <QGuiApplication>
#include <QImage>
#include <QList>
#include <QLoggingCategory>
#include <QMetaObject>
#include <QQuickItem>
#include <QQuickWindow>
#include <QRectF>
#include <QSize>
#include <QString>
#include <QTimer>
#include <QWindow>
#include <QtCore/qnamespace.h>
#include <QtCore/qtmetamacros.h>
#include <QtGlobal>
#include <QtLogging>
#include <array>
#include <cstdint>
#include <memory>
#include <panta/visualization/render_scene.hpp>
#include <panta/visualization/viewport_backend.hpp>
#include <vtkActor.h>
#include <vtkCamera.h>
#include <vtkCommand.h>
#include <vtkHardwareWindow.h>
#include <vtkMapper.h>
#include <vtkNew.h>
#include <vtkObject.h>
#include <vtkProperty.h>
#include <vtkWebGPUConfiguration.h>
#include <webgpu/webgpu_cpp.h>
#if defined(Q_OS_MACOS)
#include <vtkCocoaRenderWindowInteractor.h>
#elif defined(Q_OS_WIN)
#include <vtkWin32RenderWindowInteractor.h>
#else
#include <vtkGenericRenderWindowInteractor.h>
#endif
#include <vtkRenderWindowInteractor.h>
#include <vtkSmartPointer.h>
#include <vtkUnsignedCharArray.h>
#include <vtkWeakPointer.h>
#include <vtkWebGPURenderWindow.h>
#include <vtkWebGPURenderer.h>

namespace panta::visualization {

namespace {

Q_LOGGING_CATEGORY(viewport_log, "panta.viewport", QtWarningMsg)

constexpr int kCameraTransitionDurationMs = 260;
constexpr double kModelCameraFitMargin = 1.25;
constexpr std::uint64_t kAdapterProbeTimeoutNs = 2'000'000'000ULL;
// Windows 桌面视口固定使用 D3D12，避免 Dawn 探测未选用的 Vulkan backend。
#if defined(Q_OS_WIN)
constexpr wgpu::BackendType kDefaultAdapterProbeBackend = wgpu::BackendType::D3D12;
#else
constexpr wgpu::BackendType kDefaultAdapterProbeBackend = wgpu::BackendType::Undefined;
#endif

// VTK 9.7.0 的 vtkWebGPUConfiguration::Initialize 在 RequestAdapter 失败后
// 仍对空 adapter 调用 PopulateRequiredLimits，Dawn 会空指针崩溃。预检使用
// 独立 instance，结束即释放，避免碰 VTK 稍后创建的共享 instance。
bool dawn_adapter_available(wgpu::BackendType backend) {
    wgpu::InstanceDescriptor descriptor;
    const wgpu::InstanceFeatureName feature = wgpu::InstanceFeatureName::TimedWaitAny;
    descriptor.requiredFeatureCount = 1;
    descriptor.requiredFeatures = &feature;
    const wgpu::Instance instance = wgpu::CreateInstance(&descriptor);
    if (!instance) {
        return false;
    }

    wgpu::RequestAdapterOptions options;
    options.backendType = backend;
    bool available = false;
    const wgpu::Future future = instance.RequestAdapter(
        &options, wgpu::CallbackMode::WaitAnyOnly,
        [&available](wgpu::RequestAdapterStatus status, const wgpu::Adapter& adapter, const char*) {
            available = status == wgpu::RequestAdapterStatus::Success && adapter != nullptr;
        });
    const wgpu::WaitStatus wait = instance.WaitAny(future, kAdapterProbeTimeoutNs);
    return wait == wgpu::WaitStatus::Success && available;
}

using Vector3 = std::array<double, 3>;

} // namespace

class ViewportInteractionCommand final : public vtkCommand {
  public:
    static ViewportInteractionCommand* New() { return new ViewportInteractionCommand; }

    void set_viewport(VtkViewport* viewport) { viewport_ = viewport; }

    void Execute(vtkObject* caller, unsigned long event_id, void*) override {
        auto* interactor = vtkRenderWindowInteractor::SafeDownCast(caller);
        if (viewport_ != nullptr && interactor != nullptr) {
            viewport_->handle_interaction_event(event_id, interactor);
        }
    }

  private:
    VtkViewport* viewport_ = nullptr;
};

struct VtkViewport::Impl {
    RenderScene pending;
    NativeSurface native_surface;
    NativeHardwareWindow hardware_window;
    vtkSmartPointer<vtkWebGPURenderWindow> render_window;
    vtkSmartPointer<vtkWebGPURenderer> renderer;
    vtkSmartPointer<vtkRenderWindowInteractor> interactor;
    vtkSmartPointer<vtkActor> primitive_actor;
    vtkWeakPointer<vtkObject> previous_mapper;
    vtkWeakPointer<vtkObject> webgpu_configuration;
    vtkWeakPointer<vtkObject> retired_actor;
    vtkWeakPointer<vtkObject> retired_interactor;
    vtkWeakPointer<vtkObject> retired_render_window;
    vtkWeakPointer<vtkObject> retired_hardware_window;
    vtkWeakPointer<vtkObject> retired_webgpu_configuration;
    ViewportOrientation orientation;
    QMetaObject::Connection window_visibility_connection;
    QMetaObject::Connection scene_graph_initialized_connection;
    QList<QMetaObject::Connection> ancestor_connections;
    QTimer camera_transition_timer;
    QElapsedTimer camera_transition_clock;
    ViewportCameraTransition camera_transition;
    /// 最近一次提交给 render window 的像素尺寸；空值表示尚无有效提交。
    QSize applied_pixel_size;
    bool refresh_scheduled = false;
    bool scene_dirty = true;
    bool creation_warning_emitted = false;
    bool fail_webgpu_device = false;
    std::shared_ptr<const SurfaceMeshSnapshot> applied_mesh;
    bool pointer_dragging = false;
    bool cube_pressed = false;
    int last_pointer_x = 0;
    int last_pointer_y = 0;
};

VtkViewport::VtkViewport(QQuickItem* parent) : QQuickItem(parent), impl_(std::make_unique<Impl>()) {
    setFlag(ItemHasContents, false);
    impl_->camera_transition_timer.setInterval(16);
    impl_->camera_transition_timer.setTimerType(Qt::PreciseTimer);
    connect(&impl_->camera_transition_timer, &QTimer::timeout, this,
            &VtkViewport::advance_camera_transition);
    connect(this, &QQuickItem::windowChanged, this, &VtkViewport::bind_window);
    bind_window(window());
    watch_ancestors();
}

void VtkViewport::bind_window(QQuickWindow* new_window) {
    QObject::disconnect(impl_->window_visibility_connection);
    QObject::disconnect(impl_->scene_graph_initialized_connection);
    // 原生 surface 绑定具体窗口，换宿主或脱离窗口时必须释放旧资源。
    destroy_render_window();
    impl_->creation_warning_emitted = false;
    if (new_window == nullptr) {
        return;
    }
    impl_->window_visibility_connection = QObject::connect(
        new_window, &QWindow::visibilityChanged, this, [this](QWindow::Visibility visibility) {
            if (visibility == QWindow::Hidden || visibility == QWindow::Minimized) {
                stop_camera_transition();
            }
            impl_->scene_dirty |= visibility != QWindow::Hidden && visibility != QWindow::Minimized;
            schedule_refresh();
        });
    impl_->scene_graph_initialized_connection = QObject::connect(
        new_window, &QQuickWindow::sceneGraphInitialized, this, [this]() { schedule_refresh(); });
    schedule_refresh();
}

VtkViewport::~VtkViewport() {
    // QQuickItem 基类析构会发 windowChanged，此时 impl_ 已被销毁；先断开自回调。
    QObject::disconnect(this, nullptr, this, nullptr);
    destroy_render_window();
}

QQuickItem* VtkViewport::item() { return this; }

QImage VtkViewport::capture_image() {
    if (!impl_->render_window || !impl_->applied_pixel_size.isValid() ||
        impl_->applied_pixel_size.isEmpty())
        return {};
    const int width = impl_->applied_pixel_size.width();
    const int height = impl_->applied_pixel_size.height();
    impl_->render_window->Render();
    vtkNew<vtkUnsignedCharArray> pixels;
    // front=0 读离屏颜色附件；VTK 行序自下而上，转换后垂直翻转。
    // WebGPU 实现成功时返回值个数，数据已按组件映射转换为 RGBA。
    const int values =
        impl_->render_window->GetRGBACharPixelData(0, 0, width - 1, height - 1, 0, pixels);
    if (values != width * height * 4 || pixels->GetPointer(0) == nullptr ||
        pixels->GetNumberOfTuples() != static_cast<vtkIdType>(width) * height ||
        pixels->GetNumberOfComponents() != 4) {
        qWarning() << "VtkViewport: failed to read back viewport pixels";
        return {};
    }
    const QImage view(pixels->GetPointer(0), width, height, width * 4, QImage::Format_RGBA8888);
    return view.mirrored(false, true);
}

const void* VtkViewport::test_actor_identity() const { return impl_->primitive_actor.GetPointer(); }

bool VtkViewport::test_has_mapper() const {
    return impl_->primitive_actor != nullptr && impl_->primitive_actor->GetMapper() != nullptr;
}

bool VtkViewport::test_previous_mapper_released() const {
    return impl_->previous_mapper.GetPointer() == nullptr;
}

bool VtkViewport::test_interaction_observers_registered() const {
    if (impl_->interactor == nullptr) {
        return false;
    }
    return impl_->interactor->HasObserver(vtkCommand::LeftButtonPressEvent) &&
           impl_->interactor->HasObserver(vtkCommand::RightButtonPressEvent) &&
           impl_->interactor->HasObserver(vtkCommand::MouseMoveEvent) &&
           impl_->interactor->HasObserver(vtkCommand::LeftButtonReleaseEvent) &&
           impl_->interactor->HasObserver(vtkCommand::RightButtonReleaseEvent) &&
           impl_->interactor->HasObserver(vtkCommand::MouseWheelForwardEvent) &&
           impl_->interactor->HasObserver(vtkCommand::MouseWheelBackwardEvent);
}

bool VtkViewport::test_previous_window_resources_released() const {
    return impl_->retired_actor.GetPointer() == nullptr &&
           impl_->retired_interactor.GetPointer() == nullptr &&
           impl_->retired_render_window.GetPointer() == nullptr &&
           impl_->retired_hardware_window.GetPointer() == nullptr &&
           impl_->retired_webgpu_configuration.GetPointer() == nullptr;
}

bool VtkViewport::test_camera_transition_active() const {
    return impl_->camera_transition.active();
}

bool VtkViewport::test_camera_transition_timer_active() const {
    return impl_->camera_transition_timer.isActive();
}

QSize VtkViewport::test_applied_pixel_size() const { return impl_->applied_pixel_size; }

bool VtkViewport::test_render_resources_absent() const {
    return impl_->render_window == nullptr && impl_->hardware_window == nullptr &&
           impl_->interactor == nullptr && impl_->renderer == nullptr &&
           impl_->native_surface.view == nullptr;
}

void VtkViewport::test_fail_webgpu_device_initialization() { impl_->fail_webgpu_device = true; }

void VtkViewport::test_start_camera_transition() {
    if (impl_->renderer == nullptr || impl_->primitive_actor == nullptr) {
        return;
    }
    const double* center = impl_->primitive_actor->GetCenter();
    if (impl_->camera_transition.start(impl_->renderer->GetActiveCamera(),
                                       CameraFrame{{1.0, 0.0, 0.0}, {0.0, 0.0, 1.0}},
                                       {center[0], center[1], center[2]}, impl_->primitive_actor)) {
        impl_->camera_transition_clock.restart();
        impl_->camera_transition_timer.start();
    }
}

void VtkViewport::apply_state(const RenderScene& state) {
    if (state.revision < impl_->pending.revision) {
        return;
    }
    impl_->scene_dirty |= state.background != impl_->pending.background ||
                          state.primitive_visible != impl_->pending.primitive_visible ||
                          state.mesh != impl_->pending.mesh;
    impl_->pending = state;
    schedule_refresh();
}

void VtkViewport::geometryChange(const QRectF& new_geometry, const QRectF& old_geometry) {
    QQuickItem::geometryChange(new_geometry, old_geometry);
    // 零尺寸时 surface 被隐藏；恢复到相同像素尺寸也需要重新提交 buffer。
    impl_->scene_dirty |= old_geometry.isEmpty();
    schedule_refresh();
}

void VtkViewport::itemChange(ItemChange change, const ItemChangeData& value) {
    QQuickItem::itemChange(change, value);
    if (change == ItemParentHasChanged) {
        watch_ancestors();
    } else if (change == ItemVisibleHasChanged) {
        // 隐藏立即撤下 surface；恢复显示时补帧，Wayland 据此重新映射 buffer。
        if (!isVisible()) {
            stop_camera_transition();
            set_native_surface_visible(impl_->native_surface, false);
        }
        impl_->scene_dirty = true;
        schedule_refresh();
    } else if (change == ItemDevicePixelRatioHasChanged) {
        schedule_refresh();
    }
}

void VtkViewport::watch_ancestors() {
    for (const auto& connection : impl_->ancestor_connections) {
        QObject::disconnect(connection);
    }
    impl_->ancestor_connections.clear();
    // 原生 surface 使用 scene 坐标；祖先平移不会触发本条目的 geometryChange。
    for (auto* ancestor = parentItem(); ancestor != nullptr; ancestor = ancestor->parentItem()) {
        impl_->ancestor_connections.append(
            connect(ancestor, &QQuickItem::xChanged, this, &VtkViewport::schedule_refresh));
        impl_->ancestor_connections.append(
            connect(ancestor, &QQuickItem::yChanged, this, &VtkViewport::schedule_refresh));
        impl_->ancestor_connections.append(
            connect(ancestor, &QQuickItem::parentChanged, this, &VtkViewport::watch_ancestors));
    }
    schedule_refresh();
}

void VtkViewport::ensure_render_window() {
    if (impl_->render_window != nullptr) {
        return;
    }
    if (window() == nullptr) {
        return;
    }
    // QML 几何在原生窗口显示前就可用；在该阶段创建平台 surface 会让它
    // 铺满宿主视图，或对接到不完整的 Cocoa/Wayland 窗口层级。
    if (!window()->isVisible() || !isVisible()) {
        return;
    }
    if (width() <= 0 || height() <= 0) {
        return;
    }
    // 创建会在后续几何/可见性事件里重试；告警每实例一次，避免不支持平台
    // 在每次重试时重复输出同一诊断。
    const auto warn_once = [this](const char* message) {
        if (!impl_->creation_warning_emitted) {
            impl_->creation_warning_emitted = true;
            qWarning("%s", message);
        }
    };
    if (QGuiApplication::platformName() == QStringLiteral("offscreen")) {
        warn_once("VtkViewport: offscreen 平台没有原生 surface，跳过 WebGPU 视口初始化");
        return;
    }

    impl_->hardware_window = create_native_hardware_window(window());
    if (impl_->hardware_window == nullptr) {
        warn_once("VtkViewport: 当前 Qt 平台没有可用的 VTK hardware window");
        return;
    }

    const qreal scale = window()->devicePixelRatio();
    const int pixel_width = qMax(1, qRound(width() * scale));
    const int pixel_height = qMax(1, qRound(height() * scale));
    impl_->hardware_window->SetShowWindow(false);
    impl_->hardware_window->SetSize(pixel_width, pixel_height);
    impl_->hardware_window->Create();

    if (!attach_native_surface(window(), this, impl_->hardware_window.get(),
                               impl_->native_surface)) {
        warn_once("VtkViewport: 无法把 VTK 原生 surface 接入 Qt Quick 窗口");
        impl_->native_surface = {};
        impl_->hardware_window->Destroy();
        impl_->hardware_window.reset();
        return;
    }

    // 失败时不能调用 render window Initialize：锁定的 VTK 会在空 adapter 上崩溃。
    const wgpu::BackendType probe_backend =
        impl_->fail_webgpu_device ? wgpu::BackendType::OpenGL : kDefaultAdapterProbeBackend;
    if (!dawn_adapter_available(probe_backend)) {
        warn_once("VtkViewport: VTK WebGPU device 初始化失败");
        destroy_render_window();
        return;
    }

    impl_->render_window = vtkSmartPointer<vtkWebGPURenderWindow>::New();
    impl_->render_window->SetShowWindow(false);
    impl_->render_window->SetHardwareWindow(impl_->hardware_window.get());
    impl_->render_window->SetSize(pixel_width, pixel_height);
    impl_->renderer = vtkSmartPointer<vtkWebGPURenderer>::New();
    impl_->renderer->SetBackground2(0.98, 0.98, 0.98);
    impl_->renderer->GradientBackgroundOn();
    impl_->primitive_actor = vtkSmartPointer<vtkActor>::New();

    update_mesh_actor();
    impl_->applied_mesh = impl_->pending.mesh;
    impl_->renderer->AddActor(impl_->primitive_actor);
    impl_->render_window->AddRenderer(impl_->renderer);

#if defined(Q_OS_WIN)
    // 让预检与 VTK 实际初始化使用同一 backend；只约束预检仍会让 VTK 再次探测 Vulkan。
    auto configuration = vtkSmartPointer<vtkWebGPUConfiguration>::New();
    configuration->SetBackend(vtkWebGPUConfiguration::BackendType::D3D12);
    impl_->render_window->SetWGPUConfiguration(configuration);
#endif

#if defined(Q_OS_MACOS)
    impl_->interactor = vtkSmartPointer<vtkCocoaRenderWindowInteractor>::New();
#elif defined(Q_OS_WIN)
    // 子 HWND 是原生窗口，鼠标消息由 Win32 消息泵送达；Win32 interactor
    // 自装 WndProc 把 WM_* 翻译成交互事件。Generic 变体不接入任何平台
    // 消息流，仅适合测试进程内手动 InvokeEvent 的场景。
    impl_->interactor = vtkSmartPointer<vtkWin32RenderWindowInteractor>::New();
#else
    impl_->interactor = vtkSmartPointer<vtkGenericRenderWindowInteractor>::New();
#endif
    impl_->interactor->SetRenderWindow(impl_->render_window);
    impl_->interactor->SetHardwareWindow(impl_->hardware_window.get());
    impl_->interactor->SetEnableRender(false);
    impl_->hardware_window->SetInteractor(impl_->interactor);
    auto interaction_command = vtkSmartPointer<ViewportInteractionCommand>::New();
    interaction_command->set_viewport(this);
    impl_->interactor->AddObserver(vtkCommand::LeftButtonPressEvent, interaction_command);
    impl_->interactor->AddObserver(vtkCommand::RightButtonPressEvent, interaction_command);
    impl_->interactor->AddObserver(vtkCommand::MouseMoveEvent, interaction_command);
    impl_->interactor->AddObserver(vtkCommand::LeftButtonReleaseEvent, interaction_command);
    impl_->interactor->AddObserver(vtkCommand::RightButtonReleaseEvent, interaction_command);
    impl_->interactor->AddObserver(vtkCommand::MouseWheelForwardEvent, interaction_command);
    impl_->interactor->AddObserver(vtkCommand::MouseWheelBackwardEvent, interaction_command);
    impl_->render_window->Initialize();
    impl_->orientation.attach(impl_->render_window);
    impl_->interactor->Initialize();
    impl_->webgpu_configuration = impl_->render_window->GetWGPUConfiguration();

    if (impl_->render_window->GetGenericContext() == nullptr) {
        warn_once("VtkViewport: VTK WebGPU device 初始化失败");
        destroy_render_window();
        return;
    }

    emit sceneInitialized();
}

void VtkViewport::schedule_refresh() {
    // QWindow 状态信号在 Qt 尚未完成原生窗口切换时发出，窗口级
    // 信号在同一次切换里也可能多次触发；排队一次延迟尝试，既等切换回到
    // 事件循环，又把重复信号折叠成一次。
    if (impl_->refresh_scheduled) {
        return;
    }
    impl_->refresh_scheduled = true;
    QTimer::singleShot(0, this, [this]() {
        impl_->refresh_scheduled = false;
        if (window() != nullptr && window()->isVisible()) {
            ensure_render_window();
            sync_native_surface();
        }
    });
}

void VtkViewport::update_mesh_actor() {
    if (impl_->primitive_actor == nullptr) {
        return;
    }

    vtkMapper* previous_mapper = impl_->primitive_actor->GetMapper();
    impl_->previous_mapper = previous_mapper;
    replace_primitive_geometry(impl_->primitive_actor, impl_->render_window, impl_->pending.mesh);
}

void VtkViewport::handle_interaction_event(unsigned long event_id,
                                           vtkRenderWindowInteractor* interactor) {
    if (impl_->renderer == nullptr || impl_->primitive_actor == nullptr ||
        impl_->render_window == nullptr || interactor == nullptr) {
        return;
    }

    const int* event_position = interactor->GetEventPosition();
    const int* render_size = impl_->render_window->GetSize();
    if (event_position == nullptr || render_size == nullptr || render_size[0] <= 0 ||
        render_size[1] <= 0) {
        return;
    }

    const int x = event_position[0];
    const int y = event_position[1];
    const int width = render_size[0];
    const int height = render_size[1];
    const auto action = classify_viewport_input(event_id, impl_->pointer_dragging);
    if (action == ViewportInputAction::ZoomIn || action == ViewportInputAction::ZoomOut) {
        stop_camera_transition();
        auto* camera = impl_->renderer->GetActiveCamera();
        ViewportCamera::zoom(camera, impl_->primitive_actor, action == ViewportInputAction::ZoomIn);
        impl_->renderer->ResetCameraClippingRange();
        impl_->orientation.update(camera);
        impl_->scene_dirty = true;
        schedule_refresh();
        return;
    }

    if (action == ViewportInputAction::PressCube) {
        impl_->cube_pressed = impl_->orientation.contains_cube(x, y, width, height);
        impl_->pointer_dragging = false;
        return;
    }

    if (action == ViewportInputAction::BeginRotation) {
        impl_->last_pointer_x = x;
        impl_->last_pointer_y = y;
        impl_->cube_pressed = false;
        impl_->pointer_dragging = true;
        stop_camera_transition();
        return;
    }

    if (action == ViewportInputAction::MoveRotation) {
        const int delta_x = x - impl_->last_pointer_x;
        const int delta_y = y - impl_->last_pointer_y;
        if (delta_x != 0 || delta_y != 0) {
            auto* camera = impl_->renderer->GetActiveCamera();
            camera->Azimuth(-static_cast<double>(delta_x) * 0.5);
            camera->Elevation(static_cast<double>(delta_y) * 0.5);
            camera->OrthogonalizeViewUp();
            impl_->renderer->ResetCameraClippingRange();
            impl_->orientation.update(camera);
            impl_->scene_dirty = true;
            impl_->last_pointer_x = x;
            impl_->last_pointer_y = y;
            schedule_refresh();
        }
        return;
    }

    if (action == ViewportInputAction::EndRotation) {
        impl_->pointer_dragging = false;
        impl_->cube_pressed = false;
        return;
    }
    if (action != ViewportInputAction::ReleaseCube) {
        return;
    }

    if (impl_->cube_pressed) {
        const auto direction = impl_->orientation.cube_direction(x, y, width, height);
        if (direction.has_value()) {
            const double* center = impl_->primitive_actor->GetCenter();
            const Vector3 focal_point = {center[0], center[1], center[2]};
            double direction_vector[3] = {0.0, 0.0, 1.0};
            double view_up[3] = {0.0, 0.0, 1.0};
            switch (*direction) {
            case CubeDirection::PositiveX:
                direction_vector[0] = 1.0;
                direction_vector[2] = 0.0;
                break;
            case CubeDirection::NegativeX:
                direction_vector[0] = -1.0;
                direction_vector[2] = 0.0;
                break;
            case CubeDirection::PositiveY:
                direction_vector[1] = 1.0;
                direction_vector[2] = 0.0;
                break;
            case CubeDirection::NegativeY:
                direction_vector[1] = -1.0;
                direction_vector[2] = 0.0;
                break;
            case CubeDirection::PositiveZ:
                view_up[1] = 1.0;
                view_up[2] = 0.0;
                break;
            case CubeDirection::NegativeZ:
                direction_vector[2] = -1.0;
                view_up[1] = 1.0;
                view_up[2] = 0.0;
                break;
            }
            auto* camera = impl_->renderer->GetActiveCamera();
            const CameraFrame target_frame{
                {direction_vector[0], direction_vector[1], direction_vector[2]},
                {view_up[0], view_up[1], view_up[2]}};
            if (impl_->camera_transition.start(camera, target_frame, focal_point,
                                               impl_->primitive_actor)) {
                // 从相机当前实姿态开始采样，连续点击会平滑重定向而不跳帧。
                impl_->camera_transition_clock.restart();
                impl_->camera_transition_timer.start();
            } else {
                stop_camera_transition();
            }
        }
    }
    impl_->pointer_dragging = false;
    impl_->cube_pressed = false;
}

void VtkViewport::advance_camera_transition() {
    if (impl_->renderer == nullptr || impl_->primitive_actor == nullptr ||
        impl_->render_window == nullptr || !impl_->camera_transition_clock.isValid() ||
        !impl_->camera_transition.active()) {
        stop_camera_transition();
        return;
    }

    const qint64 elapsed = impl_->camera_transition_clock.elapsed();
    auto* camera = impl_->renderer->GetActiveCamera();
    ViewportCamera::apply(camera, impl_->camera_transition.pose_at(static_cast<double>(elapsed),
                                                                   kCameraTransitionDurationMs));
    impl_->renderer->ResetCameraClippingRange();
    impl_->orientation.update(camera);
    impl_->scene_dirty = true;
    schedule_refresh();
    if (elapsed >= kCameraTransitionDurationMs) {
        stop_camera_transition();
    }
}

void VtkViewport::stop_camera_transition() {
    impl_->camera_transition_timer.stop();
    impl_->camera_transition_clock.invalidate();
    impl_->camera_transition.cancel();
}

void VtkViewport::sync_native_surface() {
    if (impl_->hardware_window == nullptr || window() == nullptr ||
        impl_->native_surface.view == nullptr) {
        return;
    }
    const bool visible = isVisible() && width() > 0 && height() > 0;
    set_native_surface_visible(impl_->native_surface, visible);
    if (!visible || impl_->render_window == nullptr) {
        stop_camera_transition();
        if (impl_->render_window != nullptr && impl_->primitive_actor != nullptr &&
            impl_->pending.mesh != impl_->applied_mesh) {
            // 文档可能在视口隐藏期间关闭；替换 actor 管线并释放旧快照，不等到
            // surface 再次显示才清理其 CPU 与 VTK 数据。
            update_mesh_actor();
            impl_->applied_mesh = impl_->pending.mesh;
            impl_->applied_pixel_size = {};
        }
        return;
    }
    ::panta::visualization::sync_native_surface(window(), this, impl_->hardware_window.get(),
                                                impl_->native_surface);
    qCDebug(viewport_log) << "surface synchronized";
    const qreal scale = window()->devicePixelRatio();
    const QSize pixel_size(qMax(1, qRound(width() * scale)), qMax(1, qRound(height() * scale)));
    bool resized = pixel_size != impl_->applied_pixel_size;
    if (impl_->primitive_actor != nullptr && impl_->pending.mesh != impl_->applied_mesh) {
        update_mesh_actor();
        impl_->applied_mesh = impl_->pending.mesh;
        resized = true;
    }
    if (!impl_->scene_dirty && !resized) {
        return;
    }
    // ResetCameraClippingRange 只计算可见 prop；恢复网格时先更新 actor 可见性。
    impl_->primitive_actor->SetVisibility(impl_->pending.primitive_visible &&
                                          impl_->primitive_actor->GetMapper() != nullptr);
    if (resized) {
        stop_camera_transition();
        impl_->render_window->SetSize(pixel_size.width(), pixel_size.height());
        if (impl_->interactor != nullptr) {
            impl_->interactor->UpdateSize(pixel_size.width(), pixel_size.height());
        }
        configure_default_camera(impl_->renderer, impl_->primitive_actor,
                                 static_cast<double>(pixel_size.width()) / pixel_size.height(),
                                 kModelCameraFitMargin);
    }
    impl_->orientation.update(impl_->renderer->GetActiveCamera());
    impl_->renderer->SetBackground(impl_->pending.background.redF(),
                                   impl_->pending.background.greenF(),
                                   impl_->pending.background.blueF());
    impl_->render_window->Render();
    qCDebug(viewport_log) << "frame submitted" << pixel_size;
    impl_->applied_pixel_size = pixel_size;
    impl_->scene_dirty = false;
}

void VtkViewport::destroy_render_window() {
    stop_camera_transition();
    impl_->applied_pixel_size = {};
    impl_->scene_dirty = true;
    impl_->pointer_dragging = false;
    impl_->cube_pressed = false;
    impl_->retired_actor = impl_->primitive_actor;
    impl_->retired_interactor = impl_->interactor;
    impl_->retired_render_window = impl_->render_window;
    impl_->retired_hardware_window = impl_->hardware_window.get();
    impl_->retired_webgpu_configuration = impl_->webgpu_configuration.GetPointer();
    impl_->webgpu_configuration = nullptr;
    if (impl_->interactor != nullptr) {
        impl_->interactor->Disable();
        impl_->interactor->SetRenderWindow(nullptr);
    }
    if (impl_->hardware_window != nullptr) {
        impl_->hardware_window->SetInteractor(nullptr);
    }
    impl_->orientation.detach();
    if (impl_->render_window != nullptr) {
        impl_->render_window->Finalize();
        impl_->render_window = nullptr;
    }
    if (impl_->native_surface.view != nullptr) {
        detach_native_surface(impl_->native_surface);
    }
    impl_->renderer = nullptr;
    impl_->interactor = nullptr;
    impl_->primitive_actor = nullptr;
    impl_->applied_mesh.reset();
    if (impl_->hardware_window != nullptr) {
        impl_->hardware_window->Destroy();
        impl_->hardware_window.reset();
    }
}

std::unique_ptr<ViewportBackend> create_vtk_viewport_backend() {
    return std::make_unique<VtkViewport>();
}

} // namespace panta::visualization
