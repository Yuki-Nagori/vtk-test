#include "primitive_scene.hpp"

#include "surface_mesh.hpp"
#include <QByteArray>
#include <QtGlobal>
#include <algorithm>
#include <array>
#include <cmath>
#include <filesystem>
#include <memory>
#include <string>
#include <system_error>
#include <vector>
#include <vtkActor.h>
#include <vtkCamera.h>
#include <vtkColorTransferFunction.h>
#include <vtkNew.h>
#include <vtkPointData.h>
#include <vtkPolyData.h>
#include <vtkPolyDataMapper.h>
#include <vtkProperty.h>
#include <vtkRenderWindow.h>
#include <vtkScalarBarActor.h>
#include <vtkScalarsToColors.h>
#include <vtkSmartPointer.h>
#include <vtkTextProperty.h>
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
    const auto& preset = field_color_map(mesh.color_map);
    for (const auto& stop : preset.stops)
        colors->AddRGBPoint(low + (high - low) * stop.position, stop.red, stop.green, stop.blue);
    const auto& missing = preset.missing_color;
    colors->SetNanColor(missing[0], missing[1], missing[2]);
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

std::string scalar_bar_title(const SurfaceMeshSnapshot& mesh) {
    if (mesh.scalar_title.empty())
        return {};
    std::string title = mesh.scalar_title;
    if (!mesh.scalar_units.empty())
        title += " (" + mesh.scalar_units + ")";
    if (!mesh.scalar_range_label.empty())
        title += "\n" + mesh.scalar_range_label;
    return title;
}

std::string scalar_bar_font_file() {
    std::vector<std::filesystem::path> candidates;
#if defined(Q_OS_MACOS)
    for (const char* name :
         {"PingFang.ttc", "Hiragino Sans GB.ttc", "STHeiti Medium.ttc", "STHeiti Light.ttc"})
        candidates.emplace_back(std::filesystem::path("/System/Library/Fonts") / name);
#elif defined(Q_OS_WIN)
    const QByteArray windir = qgetenv("WINDIR");
    const std::filesystem::path fonts =
        std::filesystem::path(windir.isEmpty() ? "C:\\Windows" : windir.toStdString()) / "Fonts";
    for (const char* name : {"msyh.ttc", "msyh.ttf", "simhei.ttf", "simsun.ttc"})
        candidates.emplace_back(fonts / name);
#else
    for (const char* path : {"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                             "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                             "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
                             "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
                             "/usr/share/fonts/wqy-microhei/wqy-microhei.ttc",
                             "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf"})
        candidates.emplace_back(path);
#endif
    for (const auto& candidate : candidates) {
        std::error_code error;
        if (std::filesystem::is_regular_file(candidate, error))
            return candidate.string();
    }
    return {};
}

void style_scalar_bar(vtkScalarBarActor* bar) {
    bar->SetOrientationToVertical();
    bar->SetTextPositionToPrecedeScalarBar();
    // 右侧留出边距，避开右上角方向立方体（task 019）。
    bar->GetPositionCoordinate()->SetCoordinateSystemToNormalizedViewport();
    bar->GetPositionCoordinate()->SetValue(0.90, 0.10);
    bar->SetWidth(0.08);
    bar->SetHeight(0.55);
    bar->SetMaximumWidthInPixels(96);
    bar->SetMaximumHeightInPixels(360);
    bar->SetNumberOfLabels(5);
    bar->SetLabelFormat("%-#.4g");
    bar->SetBarRatio(0.3);
    bar->SetNanAnnotation("缺失");
    bar->UnconstrainedFontSizeOn();
    const std::string font_file = scalar_bar_font_file();
    for (vtkTextProperty* text : {bar->GetTitleTextProperty(), bar->GetLabelTextProperty(),
                                  bar->GetAnnotationTextProperty()}) {
        text->SetColor(0.90, 0.92, 0.95);
        text->ShadowOff();
        text->BoldOff();
        text->ItalicOff();
        text->SetFontSize(text == bar->GetTitleTextProperty() ? 14 : 12);
        if (!font_file.empty()) {
            text->SetFontFamily(VTK_FONT_FILE);
            text->SetFontFile(font_file.c_str());
        }
    }
    bar->VisibilityOff();
}

void sync_scalar_bar(vtkScalarBarActor* bar, vtkActor* actor, const SurfaceMeshSnapshot* mesh) {
    auto* mapper = actor != nullptr ? actor->GetMapper() : nullptr;
    const std::string title = mesh != nullptr ? scalar_bar_title(*mesh) : std::string{};
    if (mapper == nullptr || !mapper->GetScalarVisibility() ||
        mapper->GetLookupTable() == nullptr || title.empty()) {
        bar->SetLookupTable(nullptr);
        bar->VisibilityOff();
        return;
    }
    bar->SetLookupTable(mapper->GetLookupTable());
    bar->SetTitle(title.c_str());
    // 只有切面含缺失值时才画“缺失”色块。
    bar->SetDrawNanAnnotation(std::any_of(mesh->scalars.begin(), mesh->scalars.end(),
                                          [](float value) { return std::isnan(value); })
                                  ? 1
                                  : 0);
    bar->VisibilityOn();
}

} // namespace panta::visualization
