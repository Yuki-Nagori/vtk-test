/// VTK 视口方向标记：使用独立 renderer viewport 避免覆盖主场景清屏。
#include "viewport_orientation.hpp"

#include <array>
#include <cmath>
#include <cstddef>
#include <limits>
#include <optional>
#include <utility>
#include <vtkActor.h>
#include <vtkBillboardTextActor3D.h>
#include <vtkCamera.h>
#include <vtkCubeSource.h>
#include <vtkLineSource.h>
#include <vtkMath.h>
#include <vtkMatrix4x4.h>
#include <vtkNew.h>
#include <vtkPolyDataMapper.h>
#include <vtkProperty.h>
#include <vtkRenderWindow.h>
#include <vtkRenderer.h>
#include <vtkSmartPointer.h>
#include <vtkTextProperty.h>
#include <vtkTransform.h>
#include <vtkTransformFilter.h>
#include <vtkVectorText.h>

namespace panta::visualization {
namespace {

struct AxisColor {
    double red;
    double green;
    double blue;
};

struct FaceFrame {
    /// 六面体局部坐标中的面中心、屏幕右、屏幕上和面外法向。
    std::array<double, 3> position;
    std::array<double, 3> right;
    std::array<double, 3> up;
    std::array<double, 3> normal;
};

constexpr AxisColor kXAxisColor{0.50, 0.04, 0.16}; // 勃艮第红
constexpr AxisColor kYAxisColor{0.10, 0.34, 0.18}; // 勃艮第绿
constexpr AxisColor kZAxisColor{0.12, 0.27, 0.52}; // 勃艮第蓝
constexpr std::array<const char*, 6> kDirectionLabels = {"RIGHT",  "LEFT",  "TOP",
                                                         "BOTTOM", "FRONT", "BACK"};
constexpr double kOverlayLeft = 0.90;
constexpr double kOverlayRight = 0.98;
constexpr double kAxesBottom = 0.0;
constexpr double kAxesTop = 0.24;
constexpr double kCubeBottom = 0.76;
constexpr double kCubeHalfExtent = 0.8;

vtkSmartPointer<vtkActor> make_axis(const double endpoint[3], const AxisColor& color) {
    vtkNew<vtkLineSource> line;
    line->SetPoint1(0.0, 0.0, 0.0);
    line->SetPoint2(endpoint);

    vtkNew<vtkPolyDataMapper> mapper;
    mapper->SetInputConnection(line->GetOutputPort());

    auto actor = vtkSmartPointer<vtkActor>::New();
    actor->SetMapper(mapper);
    actor->GetProperty()->SetColor(color.red, color.green, color.blue);
    actor->GetProperty()->SetLineWidth(3.0);
    return actor;
}

vtkSmartPointer<vtkBillboardTextActor3D> make_label(const char* text, const double position[3],
                                                    const AxisColor& color, int font_size) {
    auto label = vtkSmartPointer<vtkBillboardTextActor3D>::New();
    label->SetInput(text);
    label->SetPosition(position[0], position[1], position[2]);
    vtkNew<vtkTextProperty> text_property;
    text_property->SetFontSize(font_size);
    text_property->SetBold(true);
    text_property->SetColor(color.red, color.green, color.blue);
    text_property->SetJustificationToCentered();
    text_property->SetVerticalJustificationToCentered();
    label->SetTextProperty(text_property);
    label->ForceOpaqueOn();
    return label;
}

vtkSmartPointer<vtkBillboardTextActor3D> make_axis_label(const char* text, const double position[3],
                                                         const AxisColor& color) {
    return make_label(text, position, color, 20);
}

void recenter_face_label(vtkVectorText* source, vtkTransform* centering_transform,
                         vtkTransformFilter* geometry);

vtkSmartPointer<vtkActor> make_face_label(const char* text, const FaceFrame& frame,
                                          const AxisColor& color,
                                          vtkSmartPointer<vtkVectorText>& source,
                                          vtkSmartPointer<vtkTransform>& centering_transform,
                                          vtkSmartPointer<vtkTransformFilter>& geometry) {
    source = vtkSmartPointer<vtkVectorText>::New();
    source->SetText(text);
    source->Update();

    centering_transform = vtkSmartPointer<vtkTransform>::New();
    geometry = vtkSmartPointer<vtkTransformFilter>::New();
    geometry->SetInputConnection(source->GetOutputPort());
    geometry->SetTransform(centering_transform);

    vtkNew<vtkPolyDataMapper> mapper;
    mapper->SetInputConnection(geometry->GetOutputPort());
    auto actor = vtkSmartPointer<vtkActor>::New();
    actor->SetMapper(mapper);
    vtkNew<vtkMatrix4x4> face_matrix;
    for (int row = 0; row < 3; ++row) {
        face_matrix->SetElement(row, 0, frame.right[static_cast<std::size_t>(row)]);
        face_matrix->SetElement(row, 1, frame.up[static_cast<std::size_t>(row)]);
        face_matrix->SetElement(row, 2, frame.normal[static_cast<std::size_t>(row)]);
        face_matrix->SetElement(row, 3, frame.position[static_cast<std::size_t>(row)]);
    }
    actor->SetUserMatrix(face_matrix);
    actor->GetProperty()->SetColor(color.red, color.green, color.blue);
    actor->GetProperty()->SetAmbient(0.35);
    actor->GetProperty()->SetDiffuse(0.65);

    recenter_face_label(source, centering_transform, geometry);
    return actor;
}

void recenter_face_label(vtkVectorText* source, vtkTransform* centering_transform,
                         vtkTransformFilter* geometry) {
    if (source == nullptr || centering_transform == nullptr || geometry == nullptr) {
        return;
    }
    source->Update();
    double bounds[6];
    source->GetOutput()->GetBounds(bounds);
    const double center_x = (bounds[0] + bounds[1]) * 0.5;
    const double center_y = (bounds[2] + bounds[3]) * 0.5;
    const double center_z = (bounds[4] + bounds[5]) * 0.5;
    constexpr double label_scale = 0.20;
    vtkNew<vtkMatrix4x4> matrix;
    matrix->Identity();
    matrix->SetElement(0, 0, label_scale);
    matrix->SetElement(1, 1, label_scale);
    matrix->SetElement(2, 2, label_scale);
    matrix->SetElement(0, 3, -label_scale * center_x);
    matrix->SetElement(1, 3, -label_scale * center_y);
    matrix->SetElement(2, 3, -label_scale * center_z);
    centering_transform->SetMatrix(matrix);
    geometry->Update();
}

void copy_orientation(vtkCamera* source, vtkCamera& destination, double parallel_scale) {
    double position[3];
    double focal_point[3];
    double view_up[3];
    source->GetPosition(position);
    source->GetFocalPoint(focal_point);
    source->GetViewUp(view_up);

    double direction[3] = {
        position[0] - focal_point[0],
        position[1] - focal_point[1],
        position[2] - focal_point[2],
    };
    if (vtkMath::Normalize(direction) == 0.0) {
        direction[0] = 0.0;
        direction[1] = 0.0;
        direction[2] = 1.0;
    }

    constexpr double marker_distance = 5.0;
    destination.SetPosition(direction[0] * marker_distance, direction[1] * marker_distance,
                            direction[2] * marker_distance);
    destination.SetFocalPoint(0.0, 0.0, 0.0);
    destination.SetViewUp(view_up);
    destination.SetParallelProjection(true);
    destination.SetParallelScale(parallel_scale);
}

std::optional<CubeDirection> pick_cube_face(vtkRenderer* renderer, int display_x, int display_y) {
    if (renderer == nullptr) {
        return std::nullopt;
    }

    const auto world_point = [&](double display_depth) -> std::optional<std::array<double, 3>> {
        renderer->SetDisplayPoint(display_x, display_y, display_depth);
        renderer->DisplayToWorld();
        const double* point = renderer->GetWorldPoint();
        if (point == nullptr || !std::isfinite(point[3]) || std::abs(point[3]) < 1e-12) {
            return std::nullopt;
        }
        std::array<double, 3> result = {point[0], point[1], point[2]};
        for (double& coordinate : result) {
            coordinate /= point[3];
            if (!std::isfinite(coordinate)) {
                return std::nullopt;
            }
        }
        return result;
    };

    const auto ray_start = world_point(0.0);
    const auto ray_end = world_point(1.0);
    if (!ray_start.has_value() || !ray_end.has_value()) {
        return std::nullopt;
    }
    const std::array<double, 3> ray_direction = {ray_end->at(0) - ray_start->at(0),
                                                 ray_end->at(1) - ray_start->at(1),
                                                 ray_end->at(2) - ray_start->at(2)};
    double near_t = -std::numeric_limits<double>::infinity();
    double far_t = std::numeric_limits<double>::infinity();
    int near_axis = -1;
    int far_axis = -1;
    for (std::size_t axis = 0; axis < ray_direction.size(); ++axis) {
        const double origin = ray_start->at(axis);
        const double direction = ray_direction[axis];
        if (std::abs(direction) < 1e-12) {
            if (std::abs(origin) > kCubeHalfExtent) {
                return std::nullopt;
            }
            continue;
        }

        double axis_near = (-kCubeHalfExtent - origin) / direction;
        double axis_far = (kCubeHalfExtent - origin) / direction;
        if (axis_near > axis_far) {
            std::swap(axis_near, axis_far);
        }
        if (axis_near > near_t) {
            near_t = axis_near;
            near_axis = static_cast<int>(axis);
        }
        if (axis_far < far_t) {
            far_t = axis_far;
            far_axis = static_cast<int>(axis);
        }
        if (near_t > far_t) {
            return std::nullopt;
        }
    }

    const bool enters_cube = near_t >= 0.0;
    const double hit_t = enters_cube ? near_t : far_t;
    const int face_axis = enters_cube ? near_axis : far_axis;
    if (face_axis < 0 || hit_t < 0.0) {
        return std::nullopt;
    }
    const std::array<double, 3> hit = {
        ray_start->at(0) + hit_t * ray_direction[0],
        ray_start->at(1) + hit_t * ray_direction[1],
        ray_start->at(2) + hit_t * ray_direction[2],
    };
    const bool positive = hit[static_cast<std::size_t>(face_axis)] >= 0.0;
    if (face_axis == 0) {
        return positive ? CubeDirection::PositiveX : CubeDirection::NegativeX;
    }
    if (face_axis == 1) {
        return positive ? CubeDirection::PositiveY : CubeDirection::NegativeY;
    }
    return positive ? CubeDirection::PositiveZ : CubeDirection::NegativeZ;
}

} // namespace

void ViewportOrientation::attach(vtkRenderWindow* render_window) {
    if (render_window == nullptr || axes_renderer_ != nullptr) {
        return;
    }

    render_window_ = render_window;
    render_window->SetNumberOfLayers(2);

    axes_renderer_ = vtkSmartPointer<vtkRenderer>::New();
    axes_renderer_->SetLayer(1);
    axes_renderer_->SetErase(false);
    axes_renderer_->InteractiveOff();
    axes_renderer_->SetViewport(kOverlayLeft, kAxesBottom, kOverlayRight, kAxesTop);

    cube_renderer_ = vtkSmartPointer<vtkRenderer>::New();
    cube_renderer_->SetLayer(1);
    cube_renderer_->SetErase(false);
    cube_renderer_->InteractiveOff();
    cube_renderer_->SetViewport(kOverlayLeft, kCubeBottom, kOverlayRight, 1.0);

    constexpr double x_endpoint[3] = {1.0, 0.0, 0.0};
    constexpr double y_endpoint[3] = {0.0, 1.0, 0.0};
    constexpr double z_endpoint[3] = {0.0, 0.0, 1.0};
    x_axis_ = make_axis(x_endpoint, kXAxisColor);
    y_axis_ = make_axis(y_endpoint, kYAxisColor);
    z_axis_ = make_axis(z_endpoint, kZAxisColor);
    axes_renderer_->AddActor(x_axis_);
    axes_renderer_->AddActor(y_axis_);
    axes_renderer_->AddActor(z_axis_);

    constexpr double axis_label_offset = 1.26;
    constexpr double x_label_position[3] = {axis_label_offset, 0.0, 0.0};
    constexpr double y_label_position[3] = {0.0, axis_label_offset, 0.0};
    constexpr double z_label_position[3] = {0.0, 0.0, axis_label_offset};
    x_label_ = make_axis_label("X", x_label_position, kXAxisColor);
    y_label_ = make_axis_label("Y", y_label_position, kYAxisColor);
    z_label_ = make_axis_label("Z", z_label_position, kZAxisColor);
    axes_renderer_->AddActor(x_label_);
    axes_renderer_->AddActor(y_label_);
    axes_renderer_->AddActor(z_label_);

    cube_source_ = vtkSmartPointer<vtkCubeSource>::New();
    cube_source_->SetXLength(kCubeHalfExtent * 2.0);
    cube_source_->SetYLength(kCubeHalfExtent * 2.0);
    cube_source_->SetZLength(kCubeHalfExtent * 2.0);
    vtkNew<vtkPolyDataMapper> cube_mapper;
    cube_mapper->SetInputConnection(cube_source_->GetOutputPort());
    cube_actor_ = vtkSmartPointer<vtkActor>::New();
    cube_actor_->SetMapper(cube_mapper);
    cube_actor_->GetProperty()->SetColor(0.72, 0.78, 0.88);
    cube_actor_->GetProperty()->SetOpacity(1.0);
    cube_actor_->GetProperty()->SetEdgeVisibility(true);
    cube_actor_->GetProperty()->SetEdgeColor(0.18, 0.24, 0.34);
    cube_actor_->GetProperty()->SetLineWidth(2.0);
    cube_renderer_->AddActor(cube_actor_);

    // 锚点位于六个面的几何中心，只留极小面外距离避免和实体面共面闪烁。
    constexpr double face_center = kCubeHalfExtent;
    constexpr double face_offset = 0.03;
    constexpr double label_plane = face_center + face_offset;
    // 每个面的局部 X/Y/Z 轴分别对应屏幕右、屏幕上和面外法向。
    constexpr FaceFrame x_positive_frame{
        {label_plane, 0.0, 0.0}, {0.0, 1.0, 0.0}, {0.0, 0.0, 1.0}, {1.0, 0.0, 0.0}};
    constexpr FaceFrame x_negative_frame{
        {-label_plane, 0.0, 0.0}, {0.0, -1.0, 0.0}, {0.0, 0.0, 1.0}, {-1.0, 0.0, 0.0}};
    constexpr FaceFrame y_positive_frame{
        {0.0, label_plane, 0.0}, {-1.0, 0.0, 0.0}, {0.0, 0.0, 1.0}, {0.0, 1.0, 0.0}};
    constexpr FaceFrame y_negative_frame{
        {0.0, -label_plane, 0.0}, {1.0, 0.0, 0.0}, {0.0, 0.0, 1.0}, {0.0, -1.0, 0.0}};
    constexpr FaceFrame z_positive_frame{
        {0.0, 0.0, label_plane}, {1.0, 0.0, 0.0}, {0.0, 1.0, 0.0}, {0.0, 0.0, 1.0}};
    constexpr FaceFrame z_negative_frame{
        {0.0, 0.0, -label_plane}, {-1.0, 0.0, 0.0}, {0.0, 1.0, 0.0}, {0.0, 0.0, -1.0}};
    cube_labels_[0] =
        make_face_label(kDirectionLabels[0], x_positive_frame, kXAxisColor, cube_label_sources_[0],
                        cube_label_transforms_[0], cube_label_geometry_[0]);
    cube_labels_[1] =
        make_face_label(kDirectionLabels[1], x_negative_frame, kXAxisColor, cube_label_sources_[1],
                        cube_label_transforms_[1], cube_label_geometry_[1]);
    cube_labels_[2] =
        make_face_label(kDirectionLabels[2], y_positive_frame, kYAxisColor, cube_label_sources_[2],
                        cube_label_transforms_[2], cube_label_geometry_[2]);
    cube_labels_[3] =
        make_face_label(kDirectionLabels[3], y_negative_frame, kYAxisColor, cube_label_sources_[3],
                        cube_label_transforms_[3], cube_label_geometry_[3]);
    cube_labels_[4] =
        make_face_label(kDirectionLabels[4], z_positive_frame, kZAxisColor, cube_label_sources_[4],
                        cube_label_transforms_[4], cube_label_geometry_[4]);
    cube_labels_[5] =
        make_face_label(kDirectionLabels[5], z_negative_frame, kZAxisColor, cube_label_sources_[5],
                        cube_label_transforms_[5], cube_label_geometry_[5]);
    for (const auto& label : cube_labels_) {
        cube_renderer_->AddActor(label);
    }

    render_window->AddRenderer(axes_renderer_);
    render_window->AddRenderer(cube_renderer_);
}

void ViewportOrientation::detach() {
    if (render_window_ != nullptr) {
        if (axes_renderer_ != nullptr) {
            render_window_->RemoveRenderer(axes_renderer_);
        }
        if (cube_renderer_ != nullptr) {
            render_window_->RemoveRenderer(cube_renderer_);
        }
    }
    render_window_ = nullptr;
    axes_renderer_ = nullptr;
    cube_renderer_ = nullptr;
    x_axis_ = nullptr;
    y_axis_ = nullptr;
    z_axis_ = nullptr;
    cube_actor_ = nullptr;
    cube_source_ = nullptr;
    x_label_ = nullptr;
    y_label_ = nullptr;
    z_label_ = nullptr;
    cube_label_sources_.fill(nullptr);
    cube_label_transforms_.fill(nullptr);
    cube_label_geometry_.fill(nullptr);
    cube_labels_.fill(nullptr);
}

void ViewportOrientation::update(vtkCamera* scene_camera) {
    if (scene_camera == nullptr || axes_renderer_ == nullptr || cube_renderer_ == nullptr) {
        return;
    }
    copy_orientation(scene_camera, *axes_renderer_->GetActiveCamera(), 2.8);
    copy_orientation(scene_camera, *cube_renderer_->GetActiveCamera(), 2.4);

    const double* camera_position = cube_renderer_->GetActiveCamera()->GetPosition();
    const double* camera_focal_point = cube_renderer_->GetActiveCamera()->GetFocalPoint();
    double view_direction[3] = {
        camera_position[0] - camera_focal_point[0],
        camera_position[1] - camera_focal_point[1],
        camera_position[2] - camera_focal_point[2],
    };
    if (vtkMath::Normalize(view_direction) == 0.0) {
        view_direction[2] = 1.0;
    }

    constexpr std::array face_normals = {
        std::array{1.0, 0.0, 0.0},  std::array{-1.0, 0.0, 0.0}, std::array{0.0, 1.0, 0.0},
        std::array{0.0, -1.0, 0.0}, std::array{0.0, 0.0, 1.0},  std::array{0.0, 0.0, -1.0},
    };
    for (std::size_t index = 0; index < cube_labels_.size(); ++index) {
        const auto& normal = face_normals[index];
        const double facing = normal[0] * view_direction[0] + normal[1] * view_direction[1] +
                              normal[2] * view_direction[2];
        const bool visible = facing > 0.08;
        if (cube_labels_[index]->GetVisibility() != visible) {
            cube_labels_[index]->SetVisibility(visible);
        }
    }
}

bool ViewportOrientation::contains_cube(int display_x, int display_y, int width, int height) const {
    if (width <= 0 || height <= 0) {
        return false;
    }
    return display_x >= static_cast<int>(width * kOverlayLeft) &&
           display_x <= static_cast<int>(width * kOverlayRight) &&
           display_y >= static_cast<int>(height * kCubeBottom) && display_y <= height;
}

std::optional<CubeDirection> ViewportOrientation::cube_direction(int display_x, int display_y,
                                                                 int width, int height) const {
    if (!contains_cube(display_x, display_y, width, height)) {
        return std::nullopt;
    }

    if (cube_renderer_ != nullptr) {
        return pick_cube_face(cube_renderer_, display_x, display_y);
    }

    return std::nullopt;
}

} // namespace panta::visualization
