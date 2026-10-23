# 测试目录与运行入口

| 目录 | 职责 |
|---|---|
| `rust/` | Rust 公共 API 行为回归，由对应 crate 的 Cargo test target 登记 |
| `cpp/` | C++ 功能测试，按模块分组 |
| `qml/` | QML 组件与交互功能测试 |
| `integration/` | Cargo 的 native/QML 聚合入口 |
| `support/` | 被多种功能测试实际复用的辅助代码，按 Rust、Qt、QML 分组 |
| `fixtures/` | 固定回归输入；来源、单位和预期随夹具记录 |
| `src/`、`build.rs` | Cargo 质量命令的编排实现 |

本地按改动运行 `cargo test --locked -p <package>`；完整 Rust/native/QML 行为回归使用 `cargo test --locked --workspace`。格式检查覆盖 Rust、C++ 和 QML；CMake 由真实配置、构建和测试验证。native/QML 聚合入口执行完整 CTest 与 qmllint；PR / main 推送 CI 在 Linux 运行完整行为测试，macOS 与 Windows 由 nightly 运行。

新增文件须同步 Cargo / CMake 注册、源码质量清单与路径引用；单个测试内部的局部辅助函数不为了分层而单独拆文件。共享辅助代码不持有应用业务状态，不复制领域规则。
