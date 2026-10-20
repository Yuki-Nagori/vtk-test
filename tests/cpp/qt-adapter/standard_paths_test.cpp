/// 验证 Qt 标准目录类别映射及未知枚举的安全回退。
#include "panta/qt_adapter/standard_paths.hpp"
#include <QStandardPaths>
#include <gtest/gtest.h>

TEST(StandardPathsTest, MapsEveryLocation) {
    using panta::qt_adapter::standard_location;
    using panta::qt_adapter::StandardLocation;

    EXPECT_EQ(standard_location(StandardLocation::UserConfig),
              QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation));
    EXPECT_EQ(standard_location(StandardLocation::AppData),
              QStandardPaths::writableLocation(QStandardPaths::AppDataLocation));
    EXPECT_EQ(standard_location(StandardLocation::Cache),
              QStandardPaths::writableLocation(QStandardPaths::CacheLocation));
    EXPECT_EQ(standard_location(StandardLocation::Session),
              QStandardPaths::writableLocation(QStandardPaths::TempLocation));
    EXPECT_EQ(standard_location(StandardLocation::Documents),
              QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation));
}
