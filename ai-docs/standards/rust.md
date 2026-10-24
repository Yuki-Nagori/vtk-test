# Rust 编码与工具链

更新日期：2026-10-20。workspace Clippy 启用 warning-as-error；Rust、FFI 与 native 消费按当前影响范围分别验证。

适用于 `crates/`、launcher 和 Rust 构建辅助代码。stable 表示工具链渠道；可复现构建仍需固定实际版本。

## 官方依据

Rust 2024 edition 随 Rust 1.85.0 发布；这只是 edition 的起点，不代表本项目所有依赖的最低 Rust 版本。[Rust Edition Guide](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)

跨语言接口需要明确 ABI、所有权及异常展开规则；不能假定 Rust 类型天然可供 C/C++ 使用。[Rustonomicon FFI](https://doc.rust-lang.org/nomicon/ffi.html)

## 项目规则

- 采用 edition 2024；工具链由 `rust-toolchain.toml` 固定在 stable 1.99.0。workspace `rust-version = "1.99"` 是本仓库承诺下限；源码采用该版本的原子 `try_update` API，不能沿用 CXX 依赖自身较低的 MSRV 或 edition 的最低要求作为仓库支持范围。调整下限时同步核对源码 API、依赖与验证结果。升级编译器与依赖分别评估，不能以浮动 stable 代替验证记录。
- crate 名使用 `panta-` 前缀，模块/函数 `snake_case`、类型 `PascalCase`。默认使用 rustfmt；workspace 内 Clippy 以 `cargo clippy --locked --workspace --all-targets -- -D warnings` 运行，warning 直接失败。第三方源码不纳入本项目 lint。
- 领域库用结构化 `Result`，在应用边界补充上下文；不在正常输入、文件读写和协议处理路径使用 `unwrap`/`expect` 代替错误处理。
- ID 使用 newtype；持久化 DTO 和内存领域对象分离。文件路径采用 `Path`/`PathBuf`，不假定操作系统路径一定能无损转成 UTF-8 字符串。
- `unsafe` 集中在边界模块，每处注明安全前提、所有者和线程条件。workspace 默认 `unsafe_code = "deny"`；当前登记的 allow 仅包括 CXX 桥接生成胶水和 `panta-foundation::crash` 专用模块，均不得向业务代码外扩。新增手写 unsafe 需专用模块并逐块写 `// SAFETY:`。不得为了通过类型检查无依据实现 `Send`/`Sync`。
- 大型数据使用明确所有者和借用/共享策略；`Arc` 解决共享所有权，不自动保证内部修改安全。异步运行时是否引入由具体任务决定，不先堆叠多套 runtime。
- panic 不作为业务错误；FFI 捕获策略依赖 panic 配置，`panic=abort` 不可由 `catch_unwind` 恢复。引擎隔离依靠外部进程，不能靠捕获 panic 承诺所有故障可恢复。
- 单元测试留在被测 `.rs` 的 `#[cfg(test)]` 模块中以访问私有项；公共 API 的黑盒/跨模块测试放根 `tests/rust/` 并由所属 crate 的 `[[test]]` 注册。根跨语言聚合测试放 `tests/integration/`，具体见[测试规范](testing.md)。

## 导入与领域 crate

按数据和生命周期划分 crate：`panta-io` 管理网格文件读写，其中 `import` 模块负责格式识别、来源快照、预览与导入选项，`export` 模块负责各格式写出，两个模块互不依赖；`panta-mesh` 拥有 STL 表面网格、体网格数据与校验；`panta-core` 拥有工程资产、清单、修订和提交。STEP / IGES 的底层几何读取通过 OCCT C++ adapter 完成，尚未接入的 Rust 领域接口不能写成已交付能力。当前边界见[职责审计](../architecture/native-domain-boundaries.md)。

拆 crate 时依次检查：是否需要重库、格式 / 规则是否频繁变化、消费者是否只占少数模块、逻辑与状态是否足够大。依赖重库的能力必须经独立 C++ adapter，不得放入 `panta-core`；但独立 Rust crate 本身也不直接链接重库。格式变化快、消费者少或生命周期独立时倾向独立 crate。`panta-core` 过渡期只拥有应用服务与工程存储；ID、单位和错误先留在各自的领域 crate，第二个真实消费者需要相同语义时再考虑低层契约 crate。不得为潜在复用提前创建 `panta-common` 或空领域 crate。此判断是项目模块边界准则，不是 Rust 语言的硬规则。

依赖只从应用服务流向导入与领域库；领域 crate 不依赖 `panta-core` / `panta-ffi`。跨格式统一的是来源快照、单位策略、修订与失败保留旧资产的导入事务，不把 B-rep、STL 表面网格和 Netgen 体网格强制塞进一个通用解析结构。STL 解码器位于 `panta-mesh`；STEP / IGES 的领域结果由未来的 `panta-geom` 拥有，native shape 由 C++ adapter 持有并释放，Rust 仅传稳定 ID 和修订号。导入器持有待提交快照，预览后来源变化必须重新确认；工程事务只在提交成功后发布新修订和显示资产。业务模块不可调用 `unsafe` 绕开 native adapter。

## 验证

`cargo fmt --all -- --check` 与 `cargo clippy --locked --workspace --all-targets -- -D warnings` 是 Rust-only 质量入口；跨语言项目验证使用根 `cargo format`、`cargo lint` 和 `cargo test`。领域测试覆盖输入修订、取消、错误转换，随首个业务 crate 建立。涉及 FFI 时同时运行 native 侧测试。实际工具入口与边界见[质量工具链](../modules/quality-tooling.md)，未运行的动态诊断不能写成已验证。
