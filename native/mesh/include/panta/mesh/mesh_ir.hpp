/// Native 网格输出 DTO。Rust `panta-mesh` 才是权威 Mesh IR 与领域校验所有者。
///
/// 本头文件只含转换结果与标准库，任何 Netgen/VTK 类型不得进入。约定：
/// - 节点与元素索引一律 0 起；宽度固定 std::uint32_t（冒烟规模，
///   索引宽度与内存布局的扩展性记录在任务 010）；
/// - 坐标与生成参数一律毫米（与 geometry 契约一致）；
/// - 四面体局部节点顺序按有向体积为正定义。该头文件仅承载 native DTO；
///   领域校验由 panta-mesh 的 Rust 实现负责，适配器不得复制领域规则。
#pragma once

#include <array>
#include <cstdint>
#include <panta/geometry/step_import.hpp>
#include <string>
#include <vector>

namespace panta::mesh {

/// 节点/单元/分组共用的索引类型。
using MeshIndex = std::uint32_t;

/// native 到领域 Mesh IR 的四面体转换记录。
struct NativeTetrahedronDto {
    std::array<MeshIndex, 4> nodes{};
    MeshIndex region = 0;
};

/// native 到领域 Mesh IR 的边界分组转换记录。
struct NativeSurfaceTriangleDto {
    std::array<MeshIndex, 3> nodes{};
    MeshIndex group = 0;
};

/// Netgen adapter 生成的批量 DTO；不承载独立的校验 / 存储策略。
struct NativeTetMeshDto {
    std::vector<std::array<double, 3>> nodes;
    std::vector<NativeTetrahedronDto> tets;
    std::vector<NativeSurfaceTriangleDto> boundary;
    /// 按连续 mesh group 索引映射至来源 ShapeRef revision 的 FaceId。
    std::vector<panta::geometry::FaceId> boundary_group_face_ids;
    MeshIndex region_count = 0;
    MeshIndex boundary_group_count = 0;
};

/// Rust 校验适配结果；issues 为确定性英文诊断，体积由 Rust 汇总。
struct RustMeshValidationResult {
    bool ok = true;
    std::vector<std::string> issues;
    double volume_mm3 = 0.0;
};

/// 将 native DTO 编码并交给 Rust 领域校验；诊断顺序保持确定。
RustMeshValidationResult validate_native_mesh_with_rust(const NativeTetMeshDto& mesh);

/// 与导入体网格共用的 Rust 质量统计（task 021 子项 5）；`error_code` 为空表示成功。
struct RustVolumeQuality {
    std::string error_code;
    std::uint64_t tet_count = 0;
    double min_dihedral_degrees = 0.0;
    double max_aspect_ratio = 0.0;
    double mean_aspect_ratio = 0.0;
    std::uint64_t poor_count = 0;
};

RustVolumeQuality volume_quality_with_rust(const NativeTetMeshDto& mesh);

/// 与导入体网格共用的 VTU 体网格导出；返回稳定错误码，空串表示成功。
std::string export_native_mesh_vtu_with_rust(const NativeTetMeshDto& mesh, const std::string& path);

} // namespace panta::mesh
