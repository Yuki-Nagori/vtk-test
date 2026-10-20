#include "panta_ffi.h"
#include <cmath>
#include <gtest/gtest.h>
#include <utility>

TEST(FfiBoundary, OpaqueProjectServicesOwnIndependentWorkspaceState) {
    rust::Box<panta::ffi::ProjectService> first = panta::ffi::project_service_new();
    rust::Box<panta::ffi::ProjectService> second = panta::ffi::project_service_new();
    const auto missing = panta::ffi::project_service_current(*first);
    EXPECT_EQ(missing.error.code, "project.no_project");
    EXPECT_EQ(missing.error.category, "state");
    EXPECT_TRUE(missing.value.path.empty());

    // Rust Box 离开作用域后释放一个服务，不影响另一个真实工程工作区。
    {
        auto independent = std::move(first);
        EXPECT_TRUE(panta::ffi::project_service_workspace_snapshot(*independent).documents.empty());
    }
    EXPECT_TRUE(panta::ffi::project_service_workspace_snapshot(*second).documents.empty());
    EXPECT_TRUE(panta::ffi::project_service_workspace_snapshot(*second).active_document_id.empty());
}

TEST(FfiBoundary, MeshDtoRejectsIncompleteCoordinateTriples) {
    panta::ffi::TetMeshData data;
    data.nodes.push_back(0.0);
    data.nodes.push_back(1.0);
    const auto report = panta::ffi::mesh_validate_tet(std::move(data));
    ASSERT_EQ(report.issues.size(), 1U);
    EXPECT_EQ(report.issues[0], "invalid mesh DTO layout");
    EXPECT_DOUBLE_EQ(report.volume_mm3, 0.0);
}

TEST(FfiBoundary, MeshValidationReturnsRustComputedVolume) {
    panta::ffi::TetMeshData data;
    data.nodes = {0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0};
    data.tets = {0, 1, 2, 3};
    data.tet_regions = {0};
    data.boundary = {0, 1, 2};
    data.boundary_groups = {0};
    data.region_count = 1;
    data.boundary_group_count = 1;

    const auto report = panta::ffi::mesh_validate_tet(std::move(data));
    EXPECT_TRUE(report.issues.empty());
    EXPECT_TRUE(std::abs(report.volume_mm3 - 1.0 / 6.0) < 1e-15);
}
