/// Qt 跨平台标准目录查询。
#pragma once

#include <QString>
#include <cstdint>

namespace panta::qt_adapter {

/// Qt 为应用发现的标准用户目录。
enum class StandardLocation : std::uint8_t { UserConfig, AppData, Cache, Session, Documents };

/// 返回平台为该类别提供的目录；Qt 未提供时返回空字符串。
[[nodiscard]] QString standard_location(StandardLocation location);

} // namespace panta::qt_adapter
