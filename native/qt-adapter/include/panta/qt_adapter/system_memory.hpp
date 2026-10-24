/// 系统可用物理内存查询（task 028）：供“按可用内存自动”分块阈值与派生分块构建的内存门槛。
#pragma once

#include <cstdint>

namespace panta::qt_adapter {

/// 当前可用物理内存（字节）；平台不提供或查询失败时返回 0（调用方按“未知”处理）。
/// Windows 为 `GlobalMemoryStatusEx` 的 `ullAvailPhys`，Linux 为 `/proc/meminfo` 的
/// `MemAvailable`， macOS 为 `host_statistics64` 的空闲 + 非活跃 + 可清除页。
[[nodiscard]] std::uint64_t available_physical_memory();

} // namespace panta::qt_adapter
