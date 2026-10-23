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

panta::ffi::TetMeshData to_rust_dto(const NativeTetMeshDto& mesh) {
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
    return data;
}

} // namespace

RustVolumeQuality volume_quality_with_rust(const NativeTetMeshDto& mesh) {
    const auto info = panta::ffi::mesh_tet_quality(to_rust_dto(mesh));
    RustVolumeQuality quality;
    quality.error_code = std::string(info.error.code);
    quality.tet_count = info.tet_count;
    quality.min_dihedral_degrees = info.min_dihedral_degrees;
    quality.max_aspect_ratio = info.max_aspect_ratio;
    quality.mean_aspect_ratio = info.mean_aspect_ratio;
    quality.poor_count = info.poor_count;
    return quality;
}

std::string export_native_mesh_vtu_with_rust(const NativeTetMeshDto& mesh,
                                             const std::string& path) {
    return std::string(panta::ffi::mesh_tet_export_vtu(to_rust_dto(mesh), path).code);
}

RustMeshValidationResult validate_native_mesh_with_rust(const NativeTetMeshDto& mesh) {
    const panta::ffi::TetMeshValidation native_report =
        panta::ffi::mesh_validate_tet(to_rust_dto(mesh));
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
