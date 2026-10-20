#include <gtest/gtest.h>
#include <panta/foundation/version.hpp>

namespace {

// 期望值由 CMake project(VERSION) 注入，与版本单一来源一致（gtest.md：常量前置）。
constexpr panta::foundation::Version kExpected{PANTA_TEST_EXPECT_MAJOR, PANTA_TEST_EXPECT_MINOR,
                                               PANTA_TEST_EXPECT_PATCH};

} // namespace

// 套件 = 被测模块，用例 = 行为短语（gtest.md 命名规则）。
TEST(Foundation, NativeVersionMatchesProjectConfiguration) {
    const panta::foundation::Version actual = panta::foundation::native_version();
    EXPECT_EQ(actual.major, kExpected.major);
    EXPECT_EQ(actual.minor, kExpected.minor);
    EXPECT_EQ(actual.patch, kExpected.patch);
}
