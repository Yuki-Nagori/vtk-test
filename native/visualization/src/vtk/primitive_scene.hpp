/// VTK 私有场景构建：窗口视口与硬件离屏基准共用相同几何、材质和相机配置。
#pragma once

#include <array>
#include <memory>

class vtkActor;
class vtkPolyDataMapper;
class vtkRenderWindow;
class vtkWebGPURenderer;

namespace panta::visualization {
struct SurfaceMeshSnapshot;

/// GUI 线程调用；actor 非空；window 可空（仅 CPU 场景）。有窗口时先释放旧 mapper 的图形资源；空
/// mesh 清除 mapper，保留空白视口。
void replace_primitive_geometry(vtkActor* actor, vtkRenderWindow* window,
                                const std::shared_ptr<const SurfaceMeshSnapshot>& mesh);
/// 场剖面显示区间（task 029）：非有限或倒置区间退回 [0, 1]，常值区间扩成非零宽度。
std::array<double, 2> field_display_range(double minimum, double maximum);
/// 为带标量的切面配置查找表（`field_color_stops`、缺失色）与显示区间。
void configure_field_colors(vtkPolyDataMapper* mapper, const SurfaceMeshSnapshot& mesh);
/// 使用 actor 的有效包围盒配置透视相机；aspect 和 fit_margin 必须为正。
void configure_default_camera(vtkWebGPURenderer* renderer, vtkActor* actor, double aspect,
                              double fit_margin);
} // namespace panta::visualization
