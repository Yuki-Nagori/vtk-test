/// Native Mesh IR 到 Rust 领域校验的批量转换。
#include "panta_ffi.h"
#include <cstddef>
#include <limits>
#include <panta/mesh/mesh_ir.hpp>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace panta::mesh {
namespace {

std::size_t checked_size(std::size_t count, std::size_t width) {
    if (count > std::numeric_limits<std::size_t>::max() / width) {
        throw std::length_error("mesh DTO size overflow");
    }
    return count * width;
}

} // namespace

RustMeshValidationResult validate_native_mesh_with_rust(const NativeTetMeshDto& mesh) {
    panta::ffi::TetMeshData data;
    data.nodes.reserve(checked_size(mesh.nodes.size(), 3));
    for (const auto& node : mesh.nodes) {
        for (const double coordinate : node) {
            data.nodes.push_back(coordinate);
        }
    }
    data.tets.reserve(checked_size(mesh.tets.size(), 4));
    data.tet_regions.reserve(mesh.tets.size());
    for (const NativeTetrahedronDto& tet : mesh.tets) {
        for (const MeshIndex node : tet.nodes) {
            data.tets.push_back(node);
        }
        data.tet_regions.push_back(tet.region);
    }
    data.boundary.reserve(checked_size(mesh.boundary.size(), 3));
    data.boundary_groups.reserve(mesh.boundary.size());
    for (const NativeSurfaceTriangleDto& face : mesh.boundary) {
        for (const MeshIndex node : face.nodes) {
            data.boundary.push_back(node);
        }
        data.boundary_groups.push_back(face.group);
    }
    data.region_count = mesh.region_count;
    data.boundary_group_count = mesh.boundary_group_count;

    const panta::ffi::TetMeshValidation native_report =
        panta::ffi::mesh_validate_tet(std::move(data));
    RustMeshValidationResult report;
    report.issues.reserve(native_report.issues.size());
    for (const auto& issue : native_report.issues) {
        report.issues.emplace_back(std::string(issue));
    }
    report.ok = report.issues.empty();
    report.volume_mm3 = native_report.volume_mm3;
    return report;
}

} // namespace panta::mesh
