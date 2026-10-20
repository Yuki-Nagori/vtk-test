/// Rust Mesh IR 规则经 native DTO 适配器的跨语言回归。
#include <cmath>
#include <gtest/gtest.h>
#include <panta/mesh/mesh_ir.hpp>

namespace {

using panta::mesh::MeshIndex;
using panta::mesh::NativeSurfaceTriangleDto;
using panta::mesh::NativeTetMeshDto;
using panta::mesh::NativeTetrahedronDto;
using panta::mesh::validate_native_mesh_with_rust;

/// 参考四面体：单位角四面体，有向体积为正（p1-p0 × p2-p0 指向 +z，p3 在 +z）。
NativeTetMeshDto valid_single_tet() {
    NativeTetMeshDto mesh;
    mesh.nodes = {{0.0, 0.0, 0.0}, {1.0, 0.0, 0.0}, {0.0, 1.0, 0.0}, {0.0, 0.0, 1.0}};
    mesh.tets.push_back({{0, 1, 2, 3}, 0});
    mesh.boundary.push_back({{0, 2, 1}, 0});
    mesh.region_count = 1;
    mesh.boundary_group_count = 1;
    return mesh;
}

TEST(MeshIr, ValidSingleTetPasses) {
    const auto report = validate_native_mesh_with_rust(valid_single_tet());
    EXPECT_TRUE(report.ok);
    EXPECT_TRUE(report.issues.empty());
    EXPECT_NEAR(report.volume_mm3, 1.0 / 6.0, 1e-15);
}

TEST(MeshIr, InvertedTetIsRejected) {
    auto mesh = valid_single_tet();
    mesh.tets.front().nodes = {{0, 2, 1, 3}}; // 交换两点 → 有向体积为负
    const auto report = validate_native_mesh_with_rust(mesh);
    EXPECT_FALSE(report.ok);
    ASSERT_FALSE(report.issues.empty());
    EXPECT_NE(report.issues.front().find("non-positive signed volume"), std::string::npos);
}

TEST(MeshIr, DegenerateTetIsRejected) {
    auto mesh = valid_single_tet();
    mesh.tets.front().nodes = {{0, 0, 2, 3}}; // 重复节点 → 体积为零
    const auto report = validate_native_mesh_with_rust(mesh);
    EXPECT_FALSE(report.ok);
    bool has_repeated = false;
    for (const auto& issue : report.issues) {
        has_repeated = has_repeated || issue.find("repeats a node") != std::string::npos;
    }
    EXPECT_TRUE(has_repeated);
}

TEST(MeshIr, OutOfRangeNodeIndexIsRejectedWithoutReadingCoordinates) {
    auto mesh = valid_single_tet();
    mesh.tets.front().nodes = {{0, 1, 2, 99}};
    const auto report = validate_native_mesh_with_rust(mesh); // 不因越界读取而崩溃
    EXPECT_FALSE(report.ok);
    bool has_range_issue = false;
    for (const auto& issue : report.issues) {
        has_range_issue =
            has_range_issue || issue.find("outside the node range") != std::string::npos;
    }
    EXPECT_TRUE(has_range_issue);
}

TEST(MeshIr, NonFiniteCoordinateIsRejected) {
    auto mesh = valid_single_tet();
    mesh.nodes[2][1] = std::nan("");
    const auto report = validate_native_mesh_with_rust(mesh);
    EXPECT_FALSE(report.ok);
    bool has_finite_issue = false;
    for (const auto& issue : report.issues) {
        has_finite_issue =
            has_finite_issue || issue.find("non-finite coordinate") != std::string::npos;
    }
    EXPECT_TRUE(has_finite_issue);
}

TEST(MeshIr, UnreferencedRegionAndGroupAreRejected) {
    auto mesh = valid_single_tet();
    mesh.region_count = 2;         // 区域 1 无引用
    mesh.boundary_group_count = 2; // 分组 1 无引用
    const auto report = validate_native_mesh_with_rust(mesh);
    EXPECT_FALSE(report.ok);
    bool has_region_issue = false;
    bool has_group_issue = false;
    for (const auto& issue : report.issues) {
        has_region_issue =
            has_region_issue || issue.find("region 1 is not referenced") != std::string::npos;
        has_group_issue = has_group_issue ||
                          issue.find("boundary group 1 is not referenced") != std::string::npos;
    }
    EXPECT_TRUE(has_region_issue);
    EXPECT_TRUE(has_group_issue);
}

TEST(MeshIr, OutOfRangeRegionReferenceIsRejected) {
    auto mesh = valid_single_tet();
    mesh.tets.front().region = 5;
    mesh.region_count = 1;
    const auto report = validate_native_mesh_with_rust(mesh);
    EXPECT_FALSE(report.ok);
    bool has_range_issue = false;
    for (const auto& issue : report.issues) {
        has_range_issue =
            has_range_issue || issue.find("outside the declared count") != std::string::npos;
    }
    EXPECT_TRUE(has_range_issue);
}

TEST(MeshIr, EmptyMeshIsRejected) {
    const auto report = validate_native_mesh_with_rust(NativeTetMeshDto{});
    EXPECT_FALSE(report.ok);
    EXPECT_GE(report.issues.size(), 2u);
}

} // namespace
