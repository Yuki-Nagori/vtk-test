/// VTK 视口方向标记：在同一原生 render window 内绘制坐标轴和定位六面体。
#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <vtkSmartPointer.h>

class vtkActor;
class vtkBillboardTextActor3D;
class vtkCamera;
class vtkCubeSource;
class vtkRenderWindow;
class vtkRenderer;
class vtkTransform;
class vtkTransformFilter;
class vtkVectorText;

namespace panta::visualization {

enum class CubeDirection : std::uint8_t {
    PositiveX,
    NegativeX,
    PositiveY,
    NegativeY,
    PositiveZ,
    NegativeZ,
};

/// 方向标记不依赖 QML；原生 VTK surface 覆盖在 Qt Quick 之上时仍可见。
class ViewportOrientation final {
  public:
    void attach(vtkRenderWindow* render_window);
    void detach();
    void update(vtkCamera* scene_camera);

    [[nodiscard]] bool contains_cube(int display_x, int display_y, int width, int height) const;
    [[nodiscard]] std::optional<CubeDirection> cube_direction(int display_x, int display_y,
                                                              int width, int height) const;

  private:
    vtkSmartPointer<vtkRenderWindow> render_window_;
    vtkSmartPointer<vtkRenderer> axes_renderer_;
    vtkSmartPointer<vtkRenderer> cube_renderer_;
    vtkSmartPointer<vtkActor> x_axis_;
    vtkSmartPointer<vtkActor> y_axis_;
    vtkSmartPointer<vtkActor> z_axis_;
    vtkSmartPointer<vtkActor> cube_actor_;
    vtkSmartPointer<vtkCubeSource> cube_source_;
    vtkSmartPointer<vtkBillboardTextActor3D> x_label_;
    vtkSmartPointer<vtkBillboardTextActor3D> y_label_;
    vtkSmartPointer<vtkBillboardTextActor3D> z_label_;
    std::array<vtkSmartPointer<vtkVectorText>, 6> cube_label_sources_;
    std::array<vtkSmartPointer<vtkTransform>, 6> cube_label_transforms_;
    std::array<vtkSmartPointer<vtkTransformFilter>, 6> cube_label_geometry_;
    std::array<vtkSmartPointer<vtkActor>, 6> cube_labels_;
};

} // namespace panta::visualization
