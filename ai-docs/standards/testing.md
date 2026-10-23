# 测试目录与入口规范

更新日期：2026-10-20。本文定义测试代码的归属、发现方式和统一命令；具体领域断言仍由 [验证与评审](validation-and-review.md)、[GTest](gtest.md)、[Rust](rust.md) 和 [QML](qml.md) 约束。

## 目录职责

根 `tests/` 是跨语言测试域，按职责固定分层：

```text
tests/
├── Cargo.toml                 # panta-tests 聚合 package
├── build.rs                   # 解析当前 profile 的 CMake/工具路径
├── src/main.rs                # format/lint/quality 调度器
├── integration/native.rs      # cargo test 触发的 CTest + qmllint 聚合
├── rust/<domain>.rs            # Rust 公共 API 黑盒测试，由所属 crate 注册
├── cpp/<module>/*_test.cpp    # C++20、GTest/QtTest 源文件
├── qml/*_test.cpp             # 面向 QML 组件的 QtTest 源文件
├── support/{rust,qt,qml}/     # 跨功能测试共享辅助代码
└── fixtures/                 # 固定回归输入，QML 夹具仍在 fixtures/qml
```

- `tests/src/` 只放入口编排代码，不放领域测试断言。
- `tests/integration/` 只放 `panta-tests` package 的 Cargo 集成测试；manifest 用显式 `[[test]]` 注册，不能依赖 Cargo 对 `tests/tests` 的默认猜测。
- `tests/rust/` 放 Rust crate 公共 API 的黑盒测试；所属 crate 的 Cargo manifest 用 `[[test]]` 显式注册相对路径，故 `cargo test -p <crate>` 和 workspace 测试均会运行。
- C++ / QML 功能测试归档于 `tests/cpp/` 和 `tests/qml/`，CMake target 在被测模块注册；测试二进制不安装、不导出。
- 多种测试实际复用的辅助代码放 `tests/support/`。局部辅助函数保留在使用者中，不为目录分层制造单一调用的封装。
- `native/cmake/tests/` 只保留 CMake 脚本和小型 configure fixture，不放 C++/QML 行为测试。
- 生产 QML 仍在 `qml/`；`tests/qml/` 只存 QtTest 驱动的验证代码，不复制生产组件。
- 小型、可追溯的输入夹具放 `tests/fixtures/<module>/`；生成物、日志和测试报告放构建树或 `artifacts/`，不混入源码目录。

## Rust 测试边界

- 单元测试使用同一 `.rs` 文件中的 `#[cfg(test)] mod tests`，因为它们需要访问模块私有项；不要为了目录整齐把私有实现测试搬到根 `tests/`。
- crate 级公共行为、跨模块和黑盒测试优先放根 `tests/rust/`，按领域命名并由所属 crate 注册；只能使用公开 API。已有的 crate 内 `tests/` 可随相关任务逐步迁移，不能保留同一用例的双份实现。
- 根 `tests/integration/native.rs` 是跨语言聚合测试，不承载 Rust 领域断言；它只验证构建树、qmllint、CTest 的退出码和失败传播。
- 测试应断言行为、不变量和错误上下文；不要复制实现、依赖测试顺序或通过重复调用追求统计数字。

## C++、QML 与 CMake

- C++ 单元/行为测试遵循 [GTest](gtest.md)，Qt 对象和 QML 资源加载使用 QtTest；每个可执行测试 target 名为 `<被测目标>_test`。
- 功能测试在 `BUILD_TESTING=ON` 时注册；GTest 使用 `gtest_discover_tests`，QtTest 使用 `add_test`，测试名按 `模块.行为` 命名。功能测试禁止只构建测试二进制而不注册 CTest。
- 根集成测试（随 `cargo test --workspace` 执行）依次构建 `all_qmllint` 和 `all`，再运行 `ctest --output-on-failure --no-tests=error -C <profile>`；空套件、构建失败和任一测试失败都返回非零。
- QML lint/format 使用同一托管 Qt 工具版本；真实窗口、GPU、DPR 和平台生命周期验证另行记录，不能把无头 CTest 结果写成完整图形验收。
- CMake 测试源不得通过宽泛 `GLOB` 自动发现；新增测试必须在对应模块 `CMakeLists.txt` 显式注册并同步 task/验证记录。

## 命令与 CI

本地明确选择直接受影响的 package；涉及共享构建、FFI、native/QML 或无法安全缩小影响的变更时，执行完整 workspace。测试范围根据实际消费关系判断，不维护 Git 路径选择器。

```sh
cargo test --locked -p panta-core  # 示例：工程领域测试
cargo test --locked --workspace   # 完整 Rust/native/QML 聚合
cargo format --check             # Rust、C++/Objective-C++、QML 格式
cargo lint clippy --check        # Rust 静态检查
cargo lint qmllint --check       # 实际QML模块检查
cargo lint --check               # Clippy与QML lint
cargo quality                   # 手动格式、Clippy、QML lint及完整行为测试
```

PR 与 main 推送的 CI（`ci.yml`）只在 Linux 执行 `cargo test --locked --workspace`，其中包含 native/QML 构建、完整 CTest 和 qmllint；单一 Ubuntu quality job 顺序执行格式、Clippy 和 qmllint。macOS 与 Windows 的同一测试由 `nightly.yml` 每日定时（18:00 UTC）及手动触发运行。没有按变更路径分发的测试层级。

新增、迁移或删除测试时，同步 Cargo manifest、CMake source/target/CTest 注册、实际格式/lint 输入、README 和所属任务，并验证测试被发现、失败能正确传播。禁止保留同一测试的双份实现。
