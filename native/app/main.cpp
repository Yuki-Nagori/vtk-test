/// Qt 桌面入口（任务 005）：加载 Panta.Shell 模块并运行主窗口。
///
/// 启动序列：先处理 --version（不进事件循环），再把剩余参数交给
/// QGuiApplication（支持 --platform 等标准参数）；QML 加载失败时引擎经
/// qWarning 输出全部错误，本入口以 69（EX_UNAVAILABLE）退出，不静默降级。
/// 窗口生命周期归 QML（ApplicationWindow visible: true），退出走关闭事件。

#include "panta_ffi.h"
#include <QCoreApplication>
#include <QGuiApplication>
#include <QIcon>
#include <QObject>
#include <QQmlApplicationEngine>
#include <QtCore/qnamespace.h>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickControls2/qquickstyle.h>
#include <cstdio>
#include <icon_provider.hpp>
#include <panta/foundation/version.hpp>
#include <rust/cxx.h>
#include <string>
#include <string_view>
#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <limits>
#include <windows.h>
#endif
#if defined(__GLIBC__) || defined(__linux__)
#include <cstdlib>
#ifdef __GLIBC__
#include <malloc.h>
#endif
#endif
#ifdef PANTA_ENABLE_BRIDGE_MODULE

Q_IMPORT_QML_PLUGIN(Panta_BridgePlugin)
Q_IMPORT_QML_PLUGIN(Panta_VisualizationPlugin)
#endif

Q_IMPORT_QML_PLUGIN(Panta_ShellPlugin)

namespace {

/// 用法错误退出码，与既有约定一致（BSD sysexits.h 的 EX_USAGE）。
constexpr int kExitUsage = 64;
/// UI 资源加载失败退出码（EX_UNAVAILABLE）。
constexpr int kExitUnavailable = 69;

/// 比 QML 引擎先构造、后销毁；等待 Rust 写入时事件循环与工程服务均已退出。
struct BackgroundWriteDrain {
    ~BackgroundWriteDrain() { panta::ffi::finish_background_writes(); }
};

void print_version() {
    const panta::foundation::Version version = panta::foundation::native_version();
    std::printf("panta %d.%d.%d\n", version.major, version.minor, version.patch);
}

/// 将 UTF-8 诊断写到 stderr；Windows 控制台需要 UTF-16 控制台 API，重定向时
/// 保留 UTF-8 字节流，避免依赖当前活动代码页。
void write_stderr_utf8(std::string_view message) {
#ifdef _WIN32
    const HANDLE output = GetStdHandle(STD_ERROR_HANDLE);
    DWORD console_mode = 0;
    if (output != nullptr && output != INVALID_HANDLE_VALUE &&
        GetConsoleMode(output, &console_mode) != 0 &&
        message.size() <= static_cast<std::size_t>((std::numeric_limits<int>::max)())) {
        const int byte_count = static_cast<int>(message.size());
        const int wide_count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, message.data(),
                                                   byte_count, nullptr, 0);
        if (wide_count > 0) {
            std::wstring wide_message(static_cast<std::size_t>(wide_count), L'\0');
            const int converted = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, message.data(),
                                                      byte_count, wide_message.data(), wide_count);
            if (converted == wide_count) {
                DWORD written = 0;
                if (WriteConsoleW(output, wide_message.data(), static_cast<DWORD>(wide_count),
                                  &written, nullptr) != 0 &&
                    written == static_cast<DWORD>(wide_count)) {
                    return;
                }
            }
        }
    }
#endif
    (void)std::fwrite(message.data(), sizeof(char), message.size(), stderr);
}

/// 限制 glibc malloc arena 数（task 028）。
///
/// 分块读取与 Rayon 工作线程各自分配 MiB 级缓冲，glibc 默认每核最多 8 个 arena，
/// 释放后的块留在各 arena 里，RSS 长期远高于实际占用。上限 2 是低风险折中：
/// 分配器竞争对本应用的单次大块分配影响很小。用户显式设置了 MALLOC_ARENA_MAX 时尊重环境变量。
/// Windows（系统堆）与 macOS（libmalloc）不受影响，这里不做任何事。
void limit_malloc_arenas() {
#ifdef __GLIBC__
    if (std::getenv("MALLOC_ARENA_MAX") == nullptr) {
        // 失败只意味着保留 glibc 默认行为，无需报告。
        static_cast<void>(mallopt(M_ARENA_MAX, 2));
    }
#endif
}

} // namespace

int main(int argc, char* argv[]) {
    // 必须早于任何工作线程创建（task 028）。
    limit_malloc_arenas();
    // 原生崩溃此前控制台零输出（任务 047，Rust 实现 panta_foundation::crash）：
    // 先于一切逻辑安装；失败直接写 stderr，此时 Qt 应用尚未创建。
    try {
        panta::ffi::install_crash_handler(rust::String(""));
    } catch (const rust::Error& error) {
        std::string diagnostic = "panta: 崩溃日志初始化失败：";
        diagnostic.append(error.what());
        diagnostic.push_back('\n');
        write_stderr_utf8(diagnostic);
    }
    for (int index = 1; index < argc; ++index) {
        const std::string_view argument = argv[index];
        if (argument == "--version") {
            print_version();
            return 0;
        }
        if (argument == "--") {
            break;
        }
        // 其余 Qt 标准参数交给 QGuiApplication；明确的未知参数在此拒绝，
        // 避免吞掉拼写错误。
        std::fprintf(stderr, "panta: 未知参数 '%.*s'（用法：panta [--version] [-- <Qt 参数>]）\n",
                     static_cast<int>(argument.size()), argument.data());
        return kExitUsage;
    }

    const QGuiApplication app(argc, argv);
#ifdef Q_OS_LINUX
    // QPA 会将空窗口标题回退到 applicationName；标题栏文字由 Shell 自绘，
    // Linux 原生标题栏只保留系统按钮，因此不向窗口管理器追加应用显示名。
    QGuiApplication::setApplicationDisplayName(QString{});
#endif
    QGuiApplication::setWindowIcon(
        QIcon(QStringLiteral(":/qt/qml/Panta/Shell/branding/panta-placeholder.svg")));

    // Shell 自绘控件（ThemedToolButton 背景等）依赖非原生样式（qml.md：
    // 定制需 Basic/Fusion 等）；固定 Basic 保证三平台观感一致，测试同源。
    QQuickStyle::setStyle(QStringLiteral("Basic"));

    const BackgroundWriteDrain write_drain;
    QQmlApplicationEngine engine;
    panta::install_icon_provider(engine);
    QObject::connect(
        &engine, &QQmlApplicationEngine::objectCreationFailed, &app,
        []() { QCoreApplication::exit(kExitUnavailable); }, Qt::QueuedConnection);
#ifdef PANTA_ENABLE_BRIDGE_MODULE
    engine.loadFromModule("Panta.App", "AppEntry");
#else
    engine.loadFromModule("Panta.App", "AppNoBridgeEntry");
#endif
    if (engine.rootObjects().isEmpty()) {
        return kExitUnavailable;
    }
    return QGuiApplication::exec();
}
