# CXX：Rust ↔ C++ 首选桥接方案

更新日期：2026-10-20。仓库使用锁定的 CXX 1.0.202，桥接声明与胶水位于 `crates/panta-ffi`；完整跨平台 ABI 和应用行为以本仓库实际验收为准。

## 适配本项目的判断

CXX 用一个 `#[cxx::bridge]` 模块声明双向边界，并生成两侧桥接代码。它适合项目服务这种数量有限、可明确所有权的接口；不是读取任意 C++ 头文件就自动生成完整 SDK 的工具。其安全设计仍依赖 C++ 实现满足声明的约束。[CXX upstream](https://github.com/dtolnay/cxx)、[Core concepts](https://cxx.rs/concepts.html)

本项目优先采用下列位置，Qt 元对象与 CAE 库适配继续由 C++ 负责：

```text
QML → QObject ViewModel → CXX → Rust 应用服务 / 领域模型
                                  ↓ 自有后端接口
                              CXX → C++ adapter → OCCT / Netgen
Rust 资产 / 显示快照 → CXX → C++ RenderScene / VTK 后端
```

这是目标职责，当前接入情况与 crate 依赖见 [边界审计](../architecture/native-domain-boundaries.md)。`#[cxx::bridge]` 内仅声明映射和签名；模块外的胶水仅做转换、组装与转发，领域实现留在 Rust 业务模块。先写小而稳定的服务桥接，不尝试暴露整个 OCCT、VTK 或 QObject 继承体系，也不为每帧渲染往返 Rust。

## 类型与所有权

CXX 提供共享结构体、opaque 类型，以及字符串、Vec、slice、Box、UniquePtr 等内建映射，但每种映射有限制，例如 `CxxString` 不支持按值传递，`Box<T>` 与 `UniquePtr<T>` 对 opaque 类型归属要求不同。[Shared types](https://cxx.rs/shared.html)、[Built-in bindings](https://cxx.rs/bindings.html)

项目规则：

- 小 DTO 用共享结构体；Rust 服务由 opaque Rust 类型与 `Box` 持有；需要 C++ 所有权时用受支持的 opaque C++ 包装和 `UniquePtr`。
- Qt 字符串、容器及第三方句柄在 adapter 转成受支持类型，不把它们的二进制布局直接暴露给 Rust。
- 借用 slice 仅在明确有效期内使用；后台任务不得保存调用结束后可能失效的缓冲区。大型数据的复制次数与生命周期单独验证。
- 不假定所有泛型容器、继承、回调方向或 async 函数可直接映射。新接口逐项检查绑定支持，必要时用请求/响应、句柄或事件轮询简化。
- `unsafe extern "C++"` 的声明是需要人工保证的安全契约，不意味着生成代码替开发者证明了 C++ 对象线程安全。

## 错误与 panic

CXX 的 `Result<T>` 在边界映射为 C++ 异常机制。未声明 Result 的 C++ 函数若抛出异常会 terminate；Rust 导出函数 panic 会 abort，即使桥接签名返回 Result 也一样。[Result binding](https://cxx.rs/binding/result.html)

项目规则：可恢复错误使用显式结果；要求保留机器可读错误码时定义错误 DTO，不能仅依赖异常消息。使用 CXX Result 时，C++ 服务适配器捕获并转成应用错误，不能让异常越过 Qt 事件回调。panic 是缺陷路径；不要声称 CXX 会把 panic 自动转换为 Err。

## Cargo / CMake 集成

本仓库在 `crates/panta-ffi/build.rs` 调用锁定版本的 `cxx_build::bridge("src/lib.rs")`，由 Cargo 生成并编译 CXX 胶水，汇入供 native 链接的 Rust `staticlib`。生成器与 Rust `cxx` 使用相同 release，不使用开发机任意版本的全局 cxxbridge。[Cargo 集成](https://cxx.rs/build/cargo.html)

1. Cargo 先构建 Rust 服务与 FFI staticlib；FFI build script 使用托管 C++ 编译器和平台 ABI 选项编译生成胶水，并导出实际编译命令供质量检查。
2. launcher 向 CMake 提供 staticlib 和生成头文件路径；CMake 编译自有 native adapter、Qt Bridge 与窗口程序，完成最终链接。
3. CMake 不再编译同一份 CXX 生成胶水，也不回调 Cargo；launcher 启动最终桌面程序。产物路径、重建输入与依赖顺序以两个 build script 和 CMake 导入配置为准。

当前真实服务方向为 C++ 调用 Rust 的工程、导入、路径和网格接口。无实际消费者的文本变换、计数句柄和 panic 冒烟接口已经删除，不为证明双向能力保留演示实现。后续只有实际后端契约需要时才新增 Rust 调 C++ 接口，并验证该方向的符号、所有权与异常边界。

## 桥接的最低验证

- 通过真实 `ProjectService` 与网格消费者验证 C++ 调 Rust，覆盖共享 DTO、opaque 所有权和显式错误；新增调用方向时验证对应实际消费者，不要求无用途的双向示例。
- 非 ASCII 文本、空 slice、重复创建/销毁、非法输入的行为有证据。
- CMake 最终链接，干净/增量构建及配置切换通过，生成器版本完全匹配。
- 明确线程/回调支持范围；涉及异步工作时补充关闭与取消验证。

CXX 当前主分支 README 的编译器要求不应等同于未来锁定 release 的要求；应以实际选用版本确认 Rust 最低版本，不能只依赖 edition 2024 的最低线。

## 重库边界与所有权

2026-09-23 复核在线官方文档；仓库依赖仍为锁定的 CXX 1.0.202，新接口落地须以该版本编译验证。

- 自有 C++ opaque wrapper 可通过 `UniquePtr` 管理，禁止直接绑定 OCCT / Netgen / VTK 原始类型。需要自定义库释放操作时，由 wrapper 析构执行；CXX 当前仅支持使用默认 deleter 的 `std::unique_ptr`。[UniquePtr](https://cxx.rs/binding/uniqueptr.html)
- CXX 的签名静态校验不证明线程和借用安全；opaque C++ 类型不会自动具有 Send/Sync，业务层不得为转移线程盲目补 unsafe 实现。[extern C++](https://cxx.rs/extern-c%2B%2B.html)
- CXX 默认异常转换捕获 `std::exception`；adapter 必须显式归一化 OCCT 等库的异常，不能假定任意异常都自动变成 Err。需要错误码时用结构化状态，不从日志文字反推业务类别。[Result](https://cxx.rs/binding/result.html)
