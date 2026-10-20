#include "surface_mesh.hpp"

#include "mesh_source.hpp"
#include <cmath>
#include <cstddef>
#include <vtkCellArray.h>
#include <vtkFloatArray.h>
#include <vtkMath.h>
#include <vtkPointData.h>
#include <vtkPoints.h>
#include <vtkPolyData.h>
#include <vtkSmartPointer.h>
#include <vtkType.h>

namespace panta::visualization {

vtkSmartPointer<vtkPolyData> make_surface_poly_data(const SurfaceMeshSnapshot& mesh) {
    const auto triangle_count = static_cast<vtkIdType>(mesh.vertices.size() / 3);
    const auto point_count = triangle_count * 3;
    auto points = vtkSmartPointer<vtkPoints>::New();
    points->SetDataTypeToDouble();
    points->SetNumberOfPoints(point_count);
    auto cells = vtkSmartPointer<vtkCellArray>::New();
    cells->AllocateExact(triangle_count, point_count);
    auto normals = vtkSmartPointer<vtkFloatArray>::New();
    normals->SetNumberOfComponents(3);
    normals->SetNumberOfTuples(point_count);
    for (vtkIdType triangle = 0; triangle < triangle_count; ++triangle) {
        const auto vertex = static_cast<std::size_t>(triangle) * 3;
        const auto& a = mesh.vertices[vertex];
        const auto& b = mesh.vertices[vertex + 1];
        const auto& c = mesh.vertices[vertex + 2];
        double ab[3]{b[0] - a[0], b[1] - a[1], b[2] - a[2]};
        double ac[3]{c[0] - a[0], c[1] - a[1], c[2] - a[2]};
        double normal[3]{};
        vtkMath::Cross(ab, ac, normal);
        const double length = std::hypot(normal[0], normal[1], normal[2]);
        if (length > 0.0 && std::isfinite(length)) {
            for (double& component : normal) {
                component /= length;
            }
        } else {
            for (double& component : normal) {
                component = 0.0;
            }
        }
        vtkIdType ids[3]{triangle * 3, triangle * 3 + 1, triangle * 3 + 2};
        for (std::size_t corner = 0; corner < 3; ++corner) {
            points->SetPoint(ids[corner], mesh.vertices[vertex + corner].data());
            normals->SetTuple3(ids[corner], normal[0], normal[1], normal[2]);
        }
        cells->InsertNextCell(3, ids);
    }
    auto data = vtkSmartPointer<vtkPolyData>::New();
    data->SetPoints(points);
    data->SetPolys(cells);
    // 当前三角面快照每面独占三个顶点，直接写面法线以避免重复三角化和拆点。
    data->GetPointData()->SetNormals(normals);
    return data;
}

} // namespace panta::visualization
