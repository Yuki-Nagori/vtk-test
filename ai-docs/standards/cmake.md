# CMake target 与依赖规范

更新日期：2026-10-20。Cargo / CMake 与 Qt 构建已存在；Windows ABI 和最终链接按本仓库实际平台验证，不沿用旧任务的通过状态。

适用于 `native/` 和其包含的 QML 模块构建。CMake/Ninja 的精确版本由依赖基线任务确定，在线文档的 latest 不等于项目锁定版本。

## 官方依据

CMake 用 targets 和 usage requirements 表达构建关系，`PRIVATE`、`PUBLIC`、`INTERFACE` 控制依赖属性传播。[Buildsystem](https://cmake.org/cmake/help/latest/manual/cmake-buildsystem.7.html)

`CMakePresets.json` 可共享，`CMakeUserPresets.json` 用于本机配置；preset schema 版本与 CMake 版本相关。[Presets](https://cmake.org/cmake/help/latest/manual/cmake-presets.7.html)

## 项目规则

- 使用源码树外构建；标准、include、defines、链接和告警都优先绑定 target。禁止为了一个 adapter 设置整个工程的 include/link 目录。
- 自有目标设 `CXX_STANDARD 20`、`CXX_STANDARD_REQUIRED ON`、`CXX_EXTENSIONS OFF` 或等价约束；PUBLIC 头需要 C++20 时向消费者传播要求。
- 依赖优先使用已发现的 imported targets；若上游包没有合适导出，在本地 adapter 内封装，不让平台库名散布在多个业务 CMake 文件。
- 明确列出源码、QML 和资源。生成代码使用带输入依赖与产物声明的构建规则，避免只在 configure 时执行且遗漏增量追踪。
- 共享 presets 不包含个人路径；本机前缀通过 user presets 或文档化参数传入。Cargo 调度与直接诊断构建使用同一套有效配置，避免两套默认值漂移。
- `cmake_minimum_required` 依据实际用到的 API 和验证结果确定。不要因为查到最新文档就直接要求最新 CMake。当前 native 树为 3.22（presets schema 3 所需）。
- install 规则定义运行产物和布局；测试通过 CTest 注册。native 层不得无条件调用发起构建的同一个 Cargo target。

## 配置与依赖创建顺序

`native/cmake/build-policy.cmake` 是构建默认值和平台 ABI 的单一来源，在任何 `find_package`、`FetchContent_MakeAvailable`、`qt_add_qml_module` 之前加载。顶层按基础库 → Bridge → Shell → App 排列；业务子目录只声明自身目标和依赖，不修改共享 GTest 或其它模块的目标。

单配置生成器在 `CMAKE_BUILD_TYPE` 为空时默认 Debug；Visual Studio/Ninja Multi-Config 由 `--config Debug|Release` 选择，CTest 必须带 `-C`。不要把 `CMAKE_BUILD_TYPE` 当成多配置生成器的选择器。安装前缀用 `CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT` 判断是否应设置为构建树内 `install`，保留用户显式指定值。

FFI 开关开启时，在下载依赖前检查 `BUILD_TESTING`、生成头和 staticlib 是否存在。Qt Test/GTest 仅在 `BUILD_TESTING=ON` 时发现和创建。Cargo 负责生成 Rust 产物，CMake 消费已经存在的路径，不回调 Cargo。

## Windows ABI 是整个链接图的契约

当前 Rust/CXX staticlib 使用动态 Release CRT。native 所有配置统一 `/MD`（`CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL`）和 `_ITERATOR_DEBUG_LEVEL=0`；Debug 仍保留调试符号与未优化代码，但不使用 Debug CRT/STL 迭代器检查。不能以 `/NODEFAULTLIB` 或忽略 LNK2038 掩盖 ABI 冲突。

CRT 默认值在 target 创建时初始化。只对自有 target 调用 helper 会遗漏 Qt 创建的 `*_plugin_init`、`*_resources_*` object libraries。本项目允许将这两项 ABI 设置作为目录级策略，使第三方源码和生成目标一起继承；告警、include 和业务宏仍绑定具体 target。见 [CMAKE_MSVC_RUNTIME_LIBRARY](https://cmake.org/cmake/help/latest/variable/CMAKE_MSVC_RUNTIME_LIBRARY.html)。

Windows Debug 消费预编译依赖的 Release/RelWithDebInfo 配置，使用 `CMAKE_MAP_IMPORTED_CONFIG_DEBUG` 在发现 Qt 之前设置；允许无配置 imported 工具，不回退到 Debug 库。这样 Qt DLL 与调用方使用同一 CRT；该选择不改变自有目标的 Debug 优化级别。见 [MAP_IMPORTED_CONFIG](https://cmake.org/cmake/help/latest/prop_tgt/MAP_IMPORTED_CONFIG_CONFIG.html)。升级 SDK 时必须重新核对所有配置的 CRT、STL ABI 和实际链接路径，不能假定“动态 CRT”意味着 `/MD` 与 `/MDd` 可混用。

`panta_verify_msvc_abi` 在配置末尾递归审计真实目标（含 Qt 生成目标、GTest 和测试），发现 ABI 策略被覆盖就终止 configure。该审计不替代 Windows 编译、链接和运行验证。

CTest `Build.MsvcAbiPolicy` 在独立配置工程中模拟 MSVC 策略，验证无需 helper 的 object target 自动继承、imported 配置映射，以及 CRT/iterator 覆盖的失败诊断。它在非 Windows 上只验证 CMake 元数据，不编译 Windows 程序。

## 回归范围与证据

构建策略改动至少覆盖：干净 Debug 构建与增量重建、Release、多配置生成器、Bridge ON/OFF、`BUILD_TESTING=OFF`、无效 FFI 参数。必须先 build 再运行 CTest，单跑 `all_qmllint` 不会重建全部测试程序。Windows 检查 `plugin_init` 等生成目标及实际 Qt 链接库，不能用 macOS 测试通过宣称 MSVC 修复已验证。

检查从干净目录 configure/build/install、构建配置切换和找不到依赖时的诊断；查看 usage requirements 是否将 adapter 的私有库泄露到领域 core。共享 presets 为 `debug` 与 `release`（单配置 Ninja），在 `native/` 下可复制命令：`cmake --preset debug` → `cmake --build --preset debug` → `ctest --preset debug` → `cmake --install ../target/native/debug`；`cmake --install` 不支持 preset，显式给构建目录。
