#include "panta/qt_adapter/system_memory.hpp"

#include <QtGlobal>
#include <cstdint>

#if defined(Q_OS_WIN)
// Windows SDK 需先经此伞头建立目标架构和基础类型上下文，例外见 comments.md。
#include <windows.h>
#endif

#if defined(Q_OS_WIN)
#include <sysinfoapi.h>
#elif defined(Q_OS_MACOS)
#include <mach/host_info.h>
#include <mach/kern_return.h>
#include <mach/mach_host.h>
#include <mach/message.h>
#include <mach/vm_page_size.h>
#include <mach/vm_statistics.h>
#elif defined(Q_OS_LINUX)
#include <fstream>
#include <ios>
#include <string>
#endif

namespace panta::qt_adapter {

std::uint64_t available_physical_memory() {
#if defined(Q_OS_WIN)
    MEMORYSTATUSEX status{};
    status.dwLength = sizeof(status);
    if (GlobalMemoryStatusEx(&status) == 0)
        return 0;
    return static_cast<std::uint64_t>(status.ullAvailPhys);
#elif defined(Q_OS_MACOS)
    vm_statistics64_data_t stats{};
    mach_msg_type_number_t count = HOST_VM_INFO64_COUNT;
    if (host_statistics64(mach_host_self(), HOST_VM_INFO64, reinterpret_cast<host_info64_t>(&stats),
                          &count) != KERN_SUCCESS)
        return 0;
    const auto pages = static_cast<std::uint64_t>(stats.free_count) +
                       static_cast<std::uint64_t>(stats.inactive_count) +
                       static_cast<std::uint64_t>(stats.purgeable_count);
    return pages * static_cast<std::uint64_t>(vm_page_size);
#elif defined(Q_OS_LINUX)
    // /proc 文件大小为 0，按行流式读取；单位固定为 kB。
    std::ifstream meminfo("/proc/meminfo");
    std::string key;
    std::uint64_t value = 0;
    std::string unit;
    while (meminfo >> key >> value >> unit) {
        if (key == "MemAvailable:")
            return value * 1024U;
    }
    return 0;
#else
    return 0;
#endif
}

} // namespace panta::qt_adapter
