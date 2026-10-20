/// 任务 009 的有效/无效 STEP 样例导入冒烟。夹具来源、单位与预期记录在
/// tests/fixtures/geometry/README.md；断言值与该文件保持同步。
#include <filesystem>
#include <gtest/gtest.h>
#include <panta/geometry/step_import.hpp>
#include <string>

namespace {

namespace fs = std::filesystem;

#ifndef PANTA_STRINGIFY
#define PANTA_STRINGIFY_(value) (#value)
#define PANTA_STRINGIFY(value) (PANTA_STRINGIFY_(value))
#endif

using panta::geometry::import_step;
using panta::geometry::LengthUnit;
using panta::geometry::query_shape_faces;
using panta::geometry::query_shape_summary;
using panta::geometry::release_shape;
using panta::geometry::ShapeRef;
using panta::geometry::ShapeRepositoryStatus;
using panta::geometry::StepImportStatus;

const fs::path& fixture_dir() {
    // 夹具根经编译定义注入裸路径，这里统一字符串化并缓存。
    static const fs::path dir = PANTA_STRINGIFY(PANTA_GEOMETRY_FIXTURE_DIR);
    return dir;
}

struct ShapeGuard {
    ShapeRef shape;

    ~ShapeGuard() {
        if (shape.id != 0)
            release_shape(shape);
    }
};

/// 名义包围盒预期（毫米）；摘要包络含形状公差，断言容差见 expect_summary。
struct BoxExpectation {
    double min[3];
    double max[3];
};

/// 通用断言：单位、系数、包围盒角点与拓扑计数逐一核对（毫米坐标）。
/// 包围盒是含形状公差（≈1e-7 mm，见公共头契约）的保守包络，断言容差取
/// 1e-6 mm；名义值与容差策略记录在 tests/fixtures/geometry/README.md。
void expect_summary(const panta::geometry::StepImportSummary& summary, LengthUnit unit,
                    double unit_to_mm, const BoxExpectation& expected) {
    EXPECT_EQ(summary.source_unit, unit);
    EXPECT_DOUBLE_EQ(summary.source_unit_to_mm, unit_to_mm);
    EXPECT_TRUE(summary.has_geometry);
    constexpr double bbox_tolerance = 1e-6;
    for (int axis = 0; axis < 3; ++axis) {
        EXPECT_NEAR(summary.bbox_min[axis], expected.min[axis], bbox_tolerance) << "axis " << axis;
        EXPECT_NEAR(summary.bbox_max[axis], expected.max[axis], bbox_tolerance) << "axis " << axis;
    }
    EXPECT_EQ(summary.solids, 1u);
    EXPECT_EQ(summary.faces, 6u);
    EXPECT_EQ(summary.edges, 12u);
    EXPECT_GE(summary.roots_total, 1u);
    EXPECT_EQ(summary.roots_transferred, summary.roots_total);
}

TEST(StepImport, MillimetreBoxMatchesFixtureExpectations) {
    const auto result = import_step(fixture_dir() / "box_mm.step");
    ShapeGuard guard{result.shape};
    ASSERT_EQ(result.status, StepImportStatus::kNone) << result.detail;
    ASSERT_NE(result.shape.id, 0u);
    expect_summary(result.summary, LengthUnit::kMillimetre, 1.0,
                   {{0.0, 0.0, 0.0}, {10.0, 20.0, 30.0}});
    const auto queried = query_shape_summary(result.shape);
    ASSERT_EQ(queried.status, ShapeRepositoryStatus::kNone) << queried.detail;
    EXPECT_EQ(queried.summary.faces, 6u);
    const auto faces = query_shape_faces(result.shape);
    ASSERT_EQ(faces.status, ShapeRepositoryStatus::kNone) << faces.detail;
    ASSERT_EQ(faces.face_summaries.size(), 6u);
}

TEST(StepImport, MetreSourceConvertsToMillimetreCoordinates) {
    const auto result = import_step(fixture_dir() / "box_metre.step");
    ShapeGuard guard{result.shape};
    ASSERT_EQ(result.status, StepImportStatus::kNone) << result.detail;
    expect_summary(result.summary, LengthUnit::kMetre, 1000.0,
                   {{0.0, 0.0, 0.0}, {1000.0, 1000.0, 1000.0}});
}

TEST(StepImport, MissingFileReturnsStructuredError) {
    const auto result = import_step(fixture_dir() / "does_not_exist.step");
    EXPECT_EQ(result.status, StepImportStatus::kFileUnreadable);
    EXPECT_FALSE(result.detail.empty());
    EXPECT_FALSE(result.summary.has_geometry);
}

TEST(StepImport, TextualGarbageDoesNotCrashAndIsRejected) {
    // 实测 OCCT 对可读但非法的输入返回 RetFail（解析运行后失败），
    // 同时向 stderr 输出 StepFile 解析错误（已知行为，任务 009 记录）。
    const auto result = import_step(fixture_dir() / "invalid_text.step");
    EXPECT_EQ(result.status, StepImportStatus::kParseFailed);
    EXPECT_FALSE(result.summary.has_geometry);
}

TEST(StepImport, EmptyFileIsRejected) {
    const auto result = import_step(fixture_dir() / "empty.step");
    EXPECT_EQ(result.status, StepImportStatus::kParseFailed);
    EXPECT_FALSE(result.summary.has_geometry);
}

TEST(StepImport, FailedImportThenValidImportIsUncorrupted) {
    const auto failed = import_step(fixture_dir() / "invalid_text.step");
    ASSERT_NE(failed.status, StepImportStatus::kNone);
    const auto recovered = import_step(fixture_dir() / "box_mm.step");
    ShapeGuard guard{recovered.shape};
    ASSERT_EQ(recovered.status, StepImportStatus::kNone) << recovered.detail;
    expect_summary(recovered.summary, LengthUnit::kMillimetre, 1.0,
                   {{0.0, 0.0, 0.0}, {10.0, 20.0, 30.0}});
}

TEST(StepImport, RepeatedValidImportsAreStable) {
    for (int round = 0; round < 2; ++round) {
        const auto result = import_step(fixture_dir() / "box_mm.step");
        ShapeGuard guard{result.shape};
        ASSERT_EQ(result.status, StepImportStatus::kNone) << "round " << round;
        EXPECT_EQ(result.summary.faces, 6u);
    }
}

TEST(StepImport, ShapeIdsAreUniqueAndStaleRevisionsAreRejected) {
    const auto first = import_step(fixture_dir() / "box_mm.step");
    ShapeGuard first_guard{first.shape};
    const auto second = import_step(fixture_dir() / "box_mm.step");
    ShapeGuard second_guard{second.shape};
    ASSERT_EQ(first.status, StepImportStatus::kNone) << first.detail;
    ASSERT_EQ(second.status, StepImportStatus::kNone) << second.detail;
    EXPECT_NE(first.shape.id, second.shape.id);

    ShapeRef stale = first.shape;
    ++stale.revision;
    EXPECT_EQ(query_shape_summary(stale).status, ShapeRepositoryStatus::kRevisionMismatch);
    EXPECT_EQ(query_shape_faces(stale).status, ShapeRepositoryStatus::kRevisionMismatch);
    EXPECT_EQ(release_shape(stale), ShapeRepositoryStatus::kRevisionMismatch);
}

TEST(StepImport, ReleasedShapeCannotBeQueriedOrReleasedAgain) {
    const auto result = import_step(fixture_dir() / "box_mm.step");
    ASSERT_EQ(result.status, StepImportStatus::kNone) << result.detail;
    ASSERT_EQ(release_shape(result.shape), ShapeRepositoryStatus::kNone);
    EXPECT_EQ(query_shape_summary(result.shape).status, ShapeRepositoryStatus::kNotFound);
    EXPECT_EQ(query_shape_faces(result.shape).status, ShapeRepositoryStatus::kNotFound);
    EXPECT_EQ(release_shape(result.shape), ShapeRepositoryStatus::kNotFound);
}

} // namespace
