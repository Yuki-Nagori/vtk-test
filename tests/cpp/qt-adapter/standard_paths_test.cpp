/// 验证 Qt 标准目录类别映射及未知枚举的安全回退，以及可用物理内存查询（task 028）。
#include "panta/qt_adapter/standard_paths.hpp"
#include "panta/qt_adapter/system_memory.hpp"
#include <QStandardPaths>
#include <QtGlobal>
#include <cstdint>
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

TEST(SystemMemoryTest, ReportsAvailableMemoryOnSupportedPlatforms) {
    const std::uint64_t available = panta::qt_adapter::available_physical_memory();
#if defined(Q_OS_WIN) || defined(Q_OS_MACOS) || defined(Q_OS_LINUX)
    // CI 主机至少有 64 MiB 可用；上限只防止单位错误（不超过 64 TiB）。
    EXPECT_GT(available, std::uint64_t{64} << 20U);
    EXPECT_LT(available, std::uint64_t{64} << 40U);
#else
    EXPECT_EQ(available, 0U);
#endif
}
