# C++ 测试框架（GoogleTest）

更新日期：2026-10-20。适用于仓库现有 native 测试；版本固定见 [依赖获取](dependency-acquisition.md)。

适用于 `tests/cpp/` 下自有代码的单元与行为测试。CMake 注册仍位于被测模块的 `native/**/CMakeLists.txt`；Rust 侧测试遵循 [Rust](rust.md)，目录归属和根入口见 [测试规范](testing.md)。

## 官方依据

GoogleTest 的断言在失败时输出表达式与两侧值；`EXPECT_*` 失败后继续执行，`ASSERT_*` 失败立即终止当前用例。[Primer](https://google.github.io/googletest/primer.html)

`gtest_discover_tests` 在构建后枚举用例，使每个用例成为独立 CTest 测试，比 `add_test` 整体注册提供更细粒度的结果与过滤。[gtest_discover_tests](https://google.github.io/googletest/advanced.html)

## 项目规则

- 只使用 GTest 断言（`EXPECT_*`/`ASSERT_*`），不手写 `if` + 返回码断言；失败上下文靠断言自动输出，需要补充语义时用 `<<` 追加说明。
- 可继续失败、一次暴露多个问题的检查用 `EXPECT_*`；后续步骤依赖其成立的前置状态用 `ASSERT_*`。
- 命名：`TEST(套件名, 用例名)` 中套件为被测模块或类型（PascalCase），用例为行为短语（PascalCase）；名字进入 CTest 与过滤表达式，不写 `Test1` 这类无义名。
- 共享环境用 fixture（`TEST_F`），fixture 成员即测试前置/后置；用例之间不得依赖执行顺序，不用静态/全局可变状态传值。
- 浮点比较用 `EXPECT_FLOAT_EQ`/`EXPECT_DOUBLE_EQ` 或 `ASSERT_NEAR` + 显式容差，容差注明单位与依据（[验证与评审](validation-and-review.md) CAE 容差要求）；不用 `==` 或放宽阈值掩盖失败。
- 字符串比较用 `EXPECT_STREQ`/`std::string` 的 `EXPECT_EQ`，不手写 `strcmp` 包装。
- 参数化用例（`TEST_P`）仅在多实现/多输入矩阵场景使用，不为结构整齐引入；death test（`EXPECT_DEATH`）仅用于进程级不可恢复路径，使用前确认该路径确为 abort/exit 而非可恢复错误。
- mock（gmock，随 googletest 提供）只用于模块边界的外部依赖隔离，不为内部实现细节打桩；引入时在对应 task 说明被隔离的边界。
- 测试源按 `tests/cpp/<module>/` 分类；二进制命名 `<被测目标>_test`（如 `panta_foundation_version_test`），构建产物只在构建树中生成，不把测试产物加入安装与导出。
- 获取与注册：GoogleTest v1.18.0 通过 `sdk-provision` 按 [依赖获取](dependency-acquisition.md) 固定 URL/SHA256 获取平台静态 SDK，仅在 `BUILD_TESTING=ON` 时启用；`BUILD_TESTING=OFF` 不请求 SDK。注册用 `gtest_discover_tests`，ctest 过滤用 `ctest --test-dir <dir> -R <用例名>`。
- 第三方隔离：本项目告警选项只作用于自有 target，不为 gtest 关闭或放宽自有告警；gtest 自身的编译选项不回灌本树。

## 验证

干净目录下 `BUILD_TESTING=ON` 时供给固定 GoogleTest SDK 并注册独立测试；验收检查 CTest 的成功、失败诊断和空套件错误。`BUILD_TESTING=OFF` 时不获取、不构建 GoogleTest，安装树不含测试产物。断言遵循本页规则，跨语言聚合使用 `cargo test --locked --workspace`；实际通过的平台与限制写入当前任务。

编辑器诊断以编译与 ctest 为准：`EXPECT_*` 宏展开后形如 `if (const AssertionResult gtest_ar = …)`，依赖 C++11 起的上下文 bool 转换；若 IntelliSense 未使用真实编译配置会误报"表达式必须包含 bool 类型"。共享 [.vscode/settings.json](../../.vscode/settings.json) 已指向 Cargo 共享构建树中的 `target/native/debug/compile_commands.json`，使用 CMake 原生生成的编译命令与 `c++20` 设置；遇此类误报先执行一次 `cargo build` 或 `cmake --preset debug`，再重载编辑器窗口。
