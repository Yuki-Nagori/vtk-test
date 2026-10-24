#include "primitive_scene.hpp"

#include "surface_mesh.hpp"
#include <algorithm>
#include <array>
#include <cmath>
#include <memory>
#include <vtkActor.h>
#include <vtkCamera.h>
#include <vtkColorTransferFunction.h>
#include <vtkNew.h>
#include <vtkPointData.h>
#include <vtkPolyData.h>
#include <vtkPolyDataMapper.h>
#include <vtkProperty.h>
#include <vtkRenderWindow.h>
#include <vtkSmartPointer.h>
#include <vtkWebGPURenderer.h>

namespace panta::visualization {

// 按视口宽高比和调用者边距适配透视相机，窄窗口也能容纳完整几何。
void configure_default_camera(vtkWebGPURenderer* renderer, vtkActor* actor, double aspect,
                              double fit_margin) {
    if (actor->GetMapper() == nullptr) {
        return;
    }
    double bounds[6];
    actor->GetBounds(bounds);
    constexpr double view_angle = 30.0;
    constexpr double half_angle_radians = 0.2617993877991494;
    const double half_width = (bounds[1] - bounds[0]) / 2;
    const double half_height = (bounds[3] - bounds[2]) / 2;
    const double half_depth = (bounds[5] - bounds[4]) / 2;
    const double distance =
        fit_margin * std::max(half_height, half_width / aspect) / std::tan(half_angle_radians) +
        half_depth;
    vtkCamera* camera = renderer->GetActiveCamera();
    const double* center = actor->GetCenter();
    camera->SetPosition(center[0], center[1], center[2] + distance);
    camera->SetFocalPoint(center);
    camera->SetViewUp(0, 1, 0);
    camera->SetViewAngle(view_angle);
    renderer->ResetCameraClippingRange();
}

std::array<double, 2> field_display_range(double minimum, double maximum) {
    if (!std::isfinite(minimum) || !std::isfinite(maximum) || maximum < minimum)
        return {0.0, 1.0};
    if (maximum > minimum)
        return {minimum, maximum};
    // 常值场：扩成非零区间，颜色落在色标中段。
    const double half = std::max(std::abs(minimum) * 1e-6, 0.5);
    return {minimum - half, maximum + half};
}

void configure_field_colors(vtkPolyDataMapper* mapper, const SurfaceMeshSnapshot& mesh) {
    const auto [low, high] = field_display_range(mesh.scalar_min, mesh.scalar_max);
    vtkNew<vtkColorTransferFunction> colors;
    colors->SetColorSpaceToRGB();
    for (const auto& stop : field_color_stops)
        colors->AddRGBPoint(low + (high - low) * stop.position, stop.red, stop.green, stop.blue);
    colors->SetNanColor(field_missing_color[0], field_missing_color[1], field_missing_color[2]);
    colors->ClampingOn();
    mapper->SetLookupTable(colors);
    mapper->SetScalarModeToUsePointData();
    mapper->SetColorModeToMapScalars();
    mapper->SetScalarRange(low, high);
    mapper->ScalarVisibilityOn();
}

void replace_primitive_geometry(vtkActor* actor, vtkRenderWindow* window,
                                const std::shared_ptr<const SurfaceMeshSnapshot>& mesh) {
    auto* previous_mapper = actor->GetMapper();
    if (previous_mapper != nullptr && window != nullptr) {
        // 页签切换会替换整条 mapper 管线；先显式释放旧窗口的图形资源，
        // 不依赖 mapper 析构来回收 WebGPU buffers。
        previous_mapper->ReleaseGraphicsResources(window);
    }
    if (!mesh || mesh->vertices.size() < 3) {
        actor->SetMapper(nullptr);
        return;
    }
    vtkNew<vtkPolyDataMapper> mapper;
    vtkSmartPointer<vtkPolyData> data = make_surface_poly_data(*mesh);
    mapper->SetInputData(data.GetPointer());
    const bool colored = data->GetPointData()->GetScalars() != nullptr;
    if (colored) {
        configure_field_colors(mapper, *mesh);
    } else {
        mapper->ScalarVisibilityOff();
    }
    actor->SetMapper(mapper);
    actor->SetOrientation(0.0, 0.0, 0.0);
    auto* material = actor->GetProperty();
    material->SetInterpolationToPhong();
    material->SetAmbient(0.22);
    material->SetDiffuse(0.78);
    material->SetSpecular(0.28);
    material->SetSpecularPower(28.0);
    material->SetColor(0.72, 0.82, 0.94);
}

} // namespace panta::visualization
