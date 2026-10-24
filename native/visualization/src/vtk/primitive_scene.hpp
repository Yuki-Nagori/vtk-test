/// VTK 私有场景构建：窗口视口与硬件离屏基准共用相同几何、材质和相机配置。
#pragma once

#include <array>
#include <memory>
#include <string>

class vtkActor;
class vtkPolyDataMapper;
class vtkRenderWindow;
class vtkScalarBarActor;
class vtkWebGPURenderer;

namespace panta::visualization {
struct SurfaceMeshSnapshot;

/// GUI 线程调用；actor 非空；window 可空（仅 CPU 场景）。有窗口时先释放旧 mapper 的图形资源；空
/// mesh 清除 mapper，保留空白视口。
void replace_primitive_geometry(vtkActor* actor, vtkRenderWindow* window,
                                const std::shared_ptr<const SurfaceMeshSnapshot>& mesh);
/// 场剖面显示区间（task 029）：非有限或倒置区间退回 [0, 1]，常值区间扩成非零宽度。
std::array<double, 2> field_display_range(double minimum, double maximum);
/// 为带标量的切面配置查找表（`field_color_map(mesh.color_map)` 的控制点与缺失色）与显示区间。
void configure_field_colors(vtkPolyDataMapper* mapper, const SurfaceMeshSnapshot& mesh);
/// 视口色标标题（task 038）：`场名 (单位)`，区间口径另起一行；场名为空返回空串。
std::string scalar_bar_title(const SurfaceMeshSnapshot& mesh);
/// 视口色标样式（task 038）：右侧竖排、浅色文字、中文字体回退；只需在创建时调用一次。
void style_scalar_bar(vtkScalarBarActor* bar);
/// 按 actor 当前 mapper 同步色标：mapper 有标量着色且场名非空时共用其查找表并显示，
/// 否则隐藏。`mesh` 可空。调用者在 `replace_primitive_geometry` 之后调用。
void sync_scalar_bar(vtkScalarBarActor* bar, vtkActor* actor, const SurfaceMeshSnapshot* mesh);
/// 色标中文字体：按平台候选列表返回第一个存在的 CJK 字体文件，找不到返回空串
/// （回退 VTK 内置字体，中文可能缺字）。
std::string scalar_bar_font_file();
/// 使用 actor 的有效包围盒配置透视相机；aspect 和 fit_margin 必须为正。
void configure_default_camera(vtkWebGPURenderer* renderer, vtkActor* actor, double aspect,
                              double fit_margin);
} // namespace panta::visualization
