/// panta native 层基础契约。
///
/// 当前仅含构建链验证与跨语言版本一致性；几何、网格、渲染契约由
/// 后续模块任务追加，不在本头文件堆积。
#pragma once

namespace panta::foundation {

/// native 层语义化版本；与根 Cargo.toml 的 workspace.package.version
/// 及顶层 project(VERSION) 保持一致，由 004 的 launcher 诊断校验。
struct Version {
    int major;
    int minor;
    int patch;
};

/// 返回 native 层当前版本。取值来自 CMake 注入的编译期定义，无运行时开销。
Version native_version() noexcept;

} // namespace panta::foundation
