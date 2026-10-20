# 构建与行为验证工具

[模块导航](README.md)

## 当前入口

仓库只保留基本格式、Rust Clippy、QML lint 及构建 / 行为测试。C++ 和 CMake 通过真实编译、链接和 CTest 验证；职责边界由人工评审，不提供自动依赖允许边检查。

| 检查面 | 当前入口 | 范围 |
|---|---|---|
| 格式 | `cargo format --check` | Rust、C++/Objective-C++ 与实际 QML 文件 |
| Rust | `cargo lint clippy --check` | workspace Clippy |
| QML | `cargo lint qmllint --check` | 构建后模块的类型、导入和绑定诊断 |
| 行为 | `cargo test --locked -p <package>`、`cargo test --locked --workspace` | 后者聚合完整 Rust/native/QML；窗口与 GPU 另验收 |

`cargo format` 默认修复格式，`--check` 仅检查。`cargo lint` 运行 Clippy 与 qmllint，检查模式使用 `--check`；`cargo quality` 顺序执行格式检查、两项 lint 和完整 workspace 行为测试。测试范围依据实际消费关系显式选择，不维护基于 Git 路径的自动分发。

## 构建工具与编辑器

LLVM 22.1.7 提供真实构建所需编译器、归档工具和 clang-format；Qt 6.11.2 提供 qmlformat/qmllint。Rust 工具随固定 Rust 工具链供给。CMake 不另设格式或 lint 工具，其语法与构建规则通过实际 configure/build/CTest 验证。

资产来源和摘要集中在 `crates/panta-build/src/assets.rs` 与 native 供给清单。构建工具按版本、平台和摘要隔离，下载校验、安装加锁、临时解包后发布；失败不污染有效缓存。Qt 和 native SDK 的来源及许可证随实际消费清单维护。

编辑器使用 CMake 原生生成的 `target/native/debug/compile_commands.json`，不再合并 CXX 编译命令或生成质量分析专用数据库。先运行 `cargo build --locked` 准备实际构建元数据；编辑器诊断以真实编译结果为准。

## CI 与本地验证

PR 与 main push 在 macOS、Linux 和 Windows 执行 `cargo test --locked --workspace`，由 Cargo 与聚合测试完成构建及完整行为回归。单一 Ubuntu quality job 运行格式、Clippy 和 qmllint。当前没有每日诊断、自动打包或 SDK 生产 workflow。

本地按变更选择明确 package 的 Rust 测试；跨语言或共享构建变更使用完整 workspace 聚合。native/QML 测试必须实际构建和执行，空套件、构建失败和测试失败返回非零。新增 target 或模块同步 Cargo/CMake 注册与实际格式输入，确认测试被发现。

真实窗口、DPR、多显示屏、GPU、线程及 ABI 检查单独留证；无头组件测试不能代替真实图形生命周期验证。VTK、OCCT 和 Netgen 继续按 manifest 消费既有 Release，`tools/sdk/releases/` 仅保留来源说明。

## 测试与评审

优先断言行为、不变量、错误上下文、取消和资源生命周期；不复制实现或重复调用来追求数字。未验证的系统故障列明具体缺口，按实际风险补受控输入或测试接缝。编译器告警和基本 lint 不能证明全部线程、所有权与模块边界正确，独立评审仍需依据完整改动判断。
