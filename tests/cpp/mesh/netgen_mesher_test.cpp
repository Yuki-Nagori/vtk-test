/// 任务 010 的 Netgen 生成冒烟：简单闭合实体（复用 009 的 box_mm.step，
/// 10×20×30 mm）→ 体网格 → 自有 IR 的转换与失败边界。
#include <algorithm>
#include <cstddef>
#include <filesystem>
#include <gtest/gtest.h>
#include <panta/geometry/step_import.hpp>
#include <panta/mesh/mesh_ir.hpp>
#include <panta/mesh/netgen_mesher.hpp>
#include <vector>

#ifndef PANTA_STRINGIFY
#define PANTA_STRINGIFY_(value) (#value)
#define PANTA_STRINGIFY(value) (PANTA_STRINGIFY_(value))
#endif

namespace {

namespace fs = std::filesystem;

using panta::geometry::import_step;
using panta::geometry::release_shape;
using panta::geometry::ShapeRef;
using panta::geometry::ShapeRepositoryStatus;
using panta::geometry::StepImportStatus;
using panta::mesh::generate_tet_mesh;
using panta::mesh::TetMeshGenerationParameters;
using panta::mesh::TetMeshGenerationStatus;
using panta::mesh::validate_native_mesh_with_rust;

const fs::path& fixture_dir() {
    // 夹具根由构建配置注入，测试按本机路径读取固定 STEP 输入。
    static const fs::path dir = PANTA_STRINGIFY(PANTA_MESH_FIXTURE_DIR);
    return dir;
}

struct ShapeGuard {
    ShapeRef shape;

    ~ShapeGuard() {
        if (shape.id != 0)
            release_shape(shape);
    }
};

TEST(NetgenMesher, BoxYieldsValidTetMeshWithExpectedVolumeAndGroups) {
    TetMeshGenerationParameters parameters;
    parameters.max_length_mm = 5.0;
    const auto imported = import_step(fixture_dir() / "box_mm.step");
    ShapeGuard guard{imported.shape};
    ASSERT_EQ(imported.status, StepImportStatus::kNone) << imported.detail;
    const auto result = generate_tet_mesh(imported.shape, parameters);
    ASSERT_EQ(result.status, TetMeshGenerationStatus::kNone) << result.detail;

    EXPECT_FALSE(result.mesh.nodes.empty());
    EXPECT_FALSE(result.mesh.tets.empty());
    EXPECT_FALSE(result.mesh.boundary.empty());
    // 单一实体 → 单一区域；长方体 6 个面 → 6 个边界分组，且无丢失。
    EXPECT_EQ(result.mesh.region_count, 1u);
    EXPECT_EQ(result.mesh.boundary_group_count, 6u);
    ASSERT_EQ(result.mesh.boundary_group_face_ids.size(), 6u);
    auto face_ids = result.mesh.boundary_group_face_ids;
    std::sort(face_ids.begin(), face_ids.end());
    EXPECT_EQ(face_ids, (std::vector<panta::geometry::FaceId>{0, 1, 2, 3, 4, 5}));
    const auto source_faces = panta::geometry::query_shape_faces(imported.shape);
    ASSERT_EQ(source_faces.status, ShapeRepositoryStatus::kNone) << source_faces.detail;
    ASSERT_EQ(source_faces.face_summaries.size(), 6u);
    constexpr double mapping_tolerance = 1e-5;
    for (std::size_t group = 0; group < result.mesh.boundary_group_face_ids.size(); ++group) {
        const auto face =
            std::find_if(source_faces.face_summaries.begin(), source_faces.face_summaries.end(),
                         [face_id = result.mesh.boundary_group_face_ids[group]](
                             const auto& summary) { return summary.id == face_id; });
        ASSERT_NE(face, source_faces.face_summaries.end());
        ASSERT_TRUE(face->has_bounds);

        int planar_axis = -1;
        for (int axis = 0; axis < 3; ++axis) {
            if (face->bbox_max[axis] - face->bbox_min[axis] <= mapping_tolerance) {
                planar_axis = axis;
                break;
            }
        }
        ASSERT_GE(planar_axis, 0) << "box face should have one planar axis";

        std::size_t triangles_in_group = 0;
        for (const auto& triangle : result.mesh.boundary) {
            if (triangle.group != group)
                continue;
            ++triangles_in_group;
            for (const auto node : triangle.nodes) {
                const auto& point = result.mesh.nodes[node];
                for (int axis = 0; axis < 3; ++axis) {
                    EXPECT_GE(point[axis], face->bbox_min[axis] - mapping_tolerance);
                    EXPECT_LE(point[axis], face->bbox_max[axis] + mapping_tolerance);
                }
                EXPECT_NEAR(point[planar_axis], face->bbox_min[planar_axis], mapping_tolerance);
            }
        }
        EXPECT_GT(triangles_in_group, 0u);
    }
    for (const auto& triangle : result.mesh.boundary)
        EXPECT_LT(triangle.group, result.mesh.boundary_group_face_ids.size());
    EXPECT_EQ(validate_native_mesh_with_rust(result.mesh).ok, true);

    // Netgen 结果与导入体网格共用质量统计与 VTU 导出（task 021 子项 5）。
    const auto quality = panta::mesh::volume_quality_with_rust(result.mesh);
    EXPECT_TRUE(quality.error_code.empty());
    EXPECT_EQ(quality.tet_count, result.mesh.tets.size());
    EXPECT_GT(quality.min_dihedral_degrees, 0.0);
    EXPECT_GE(quality.max_aspect_ratio, 1.0 - 1e-9);
    const auto vtu = std::filesystem::temp_directory_path() / "panta-netgen-box.vtu";
    EXPECT_TRUE(panta::mesh::export_native_mesh_vtu_with_rust(result.mesh, vtu.string()).empty());
    EXPECT_TRUE(std::filesystem::is_regular_file(vtu));
    std::filesystem::remove(vtu);

    // 名义体积 10×20×30 = 6000 mm³；面网格贴合平面，断言容差 1%。
    EXPECT_NEAR(result.volume_mm3, 6000.0, 60.0);
}

TEST(NetgenMesher, MaxLengthControlsElementCount) {
    TetMeshGenerationParameters coarse;
    coarse.max_length_mm = 10.0;
    TetMeshGenerationParameters fine;
    fine.max_length_mm = 3.0;
    const auto imported = import_step(fixture_dir() / "box_mm.step");
    ShapeGuard guard{imported.shape};
    ASSERT_EQ(imported.status, StepImportStatus::kNone) << imported.detail;
    const auto coarse_mesh = generate_tet_mesh(imported.shape, coarse);
    ASSERT_EQ(coarse_mesh.status, TetMeshGenerationStatus::kNone) << coarse_mesh.detail;
    const auto fine_mesh = generate_tet_mesh(imported.shape, fine);
    ASSERT_EQ(fine_mesh.status, TetMeshGenerationStatus::kNone) << fine_mesh.detail;
    EXPECT_LT(coarse_mesh.mesh.tets.size(), fine_mesh.mesh.tets.size());
    EXPECT_EQ(coarse_mesh.mesh.boundary_group_face_ids, fine_mesh.mesh.boundary_group_face_ids);
}

TEST(NetgenMesher, UnknownShapeIsRejectedWithoutOutput) {
    const auto result = generate_tet_mesh({}, {});
    EXPECT_EQ(result.status, TetMeshGenerationStatus::kShapeNotFound);
    EXPECT_TRUE(result.mesh.tets.empty());
    EXPECT_NEAR(result.volume_mm3, 0.0, 1e-12);
}

TEST(NetgenMesher, StaleRevisionIsRejectedWithoutOutput) {
    const auto imported = import_step(fixture_dir() / "box_mm.step");
    ShapeGuard guard{imported.shape};
    ASSERT_EQ(imported.status, StepImportStatus::kNone) << imported.detail;
    ShapeRef stale = imported.shape;
    ++stale.revision;
    const auto result = generate_tet_mesh(stale, {});
    EXPECT_EQ(result.status, TetMeshGenerationStatus::kShapeRevisionMismatch);
    EXPECT_TRUE(result.mesh.tets.empty());
}

TEST(NetgenMesher, FailureDoesNotCorruptSubsequentGeneration) {
    const auto failed = generate_tet_mesh({}, {});
    ASSERT_EQ(failed.status, TetMeshGenerationStatus::kShapeNotFound);

    const auto imported = import_step(fixture_dir() / "box_mm.step");
    ShapeGuard guard{imported.shape};
    ASSERT_EQ(imported.status, StepImportStatus::kNone) << imported.detail;
    TetMeshGenerationParameters parameters;
    parameters.max_length_mm = 5.0;
    const auto recovered = generate_tet_mesh(imported.shape, parameters);
    ASSERT_EQ(recovered.status, TetMeshGenerationStatus::kNone) << recovered.detail;
    EXPECT_EQ(recovered.mesh.boundary_group_count, 6u);
    EXPECT_NEAR(recovered.volume_mm3, 6000.0, 60.0);
}

TEST(NetgenMesher, RepeatedGenerationsReleaseNativeMeshes) {
    const auto imported = import_step(fixture_dir() / "box_mm.step");
    ShapeGuard guard{imported.shape};
    ASSERT_EQ(imported.status, StepImportStatus::kNone) << imported.detail;
    TetMeshGenerationParameters parameters;
    parameters.max_length_mm = 5.0;

    for (int generation = 0; generation < 3; ++generation) {
        const auto result = generate_tet_mesh(imported.shape, parameters);
        ASSERT_EQ(result.status, TetMeshGenerationStatus::kNone) << result.detail;
        EXPECT_FALSE(result.mesh.tets.empty());
    }
}

} // namespace
