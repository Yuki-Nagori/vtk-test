/// 自有表面网格快照到 VTK 显示数据的边界转换。
#pragma once

#include <panta/visualization/mesh_source.hpp>
#include <vtkSmartPointer.h>

class vtkPolyData;

namespace panta::visualization {

vtkSmartPointer<vtkPolyData> make_surface_poly_data(const SurfaceMeshSnapshot& mesh);

} // namespace panta::visualization
