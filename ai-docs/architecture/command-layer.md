# Rust 应用命令层

[架构总览](README.md) · [分层规则](../standards/layering.md)

## 当前状态

Rust 应用服务位于 `panta-core`，例如 `ProjectService` 编排工程状态和网格激活。GUI 通过 `panta-ffi`/CXX 将请求转到 Rust，再由 C++ ViewModel 提供 Qt 属性、信号及异步调度。工程打开由 Qt 无关的 `ProjectCommand::Open` 表达，GUI FFI 与 CLI 都经 `ProjectService::execute` 调用同一校验和读取路径。

`panta-launcher` 负责启动 native app 与打包。`panta-cli` 提供窄应用操作：`project inspect <path>` 读取工程摘要和已导入资产来源，`project create <parent> <name>` 创建工程，`stl inspect <path> [--weld-tolerance <ratio>]` 只读预检 STL 并输出表面网格质量诊断，`stl import <project> <path>` 将 STL 导入现有工程；它不负责启动 GUI。工程 inspect 与 GUI 打开操作共用 `ProjectCommand::Open`；create 与 GUI 共用 `ProjectService::create` 及名称、路径和覆盖保护规则；STL 检查与 GUI 导入预检共用 `ProjectService::inspect_stl` 和 Rust 解析诊断；CLI 导入调用同步 `ProjectService::import_stl`，与 GUI 后台入口共用 Rust 导入提交实现、选项校验和 sidecar 事务。

## 目标职责

```text
GUI 参数 → Qt ViewModel / Bridge ─┐
                                 ├→ Rust 应用命令/查询 → 领域服务 → 结果 / 错误
CLI 参数 → Rust CLI adapter ─────┘
```

- 应用命令定义可执行的用户意图；查询定义只读结果。输入、结果与错误使用 Qt 无关的 Rust 类型。
- Rust 应用入口负责业务校验与编排，调用既有领域服务。领域不依赖 UI、命令行、Qt 或 CXX。
- FFI 只转换 DTO、错误与句柄并转发；Bridge/ViewModel 只做 Qt 生命周期、属性/信号和线程适配。
- CLI 解析参数、调用同一 Rust 入口并呈现结果；不得另行复制业务校验和流程。
- 不预先搭建命令总线或插件系统。优先让应用命令表达用户意图，由真实消费者决定后续接口扩展。

## 命令选择

打开工程命令由 GUI 用于加载活动工程状态，CLI 则使用隔离的 `ProjectService` 实例检查 `.panta` 文件并显示名称、路径、修订号及已保存的导入记录摘要。打开命令会复用文件存在性、扩展名、清单解析和 schema 校验，不复制业务规则。工程创建也复用 `ProjectService::create`，由 CLI 解析父目录和名称后调用，不复制目录创建、名称验证及目标冲突规则。STL 导入先通过 `inspect_stl` 取得预览和面数估算，超 2 GiB 时在提交前给出非阻断提醒，再由同步 `import_stl` 复核源字节并复用缓存的已解析 Mesh 提交；GUI 使用后台 begin/finish 入口。两条路径共享 `panta-core` 的导入事务实现。导入只把 Mesh sidecar 和来源快照写入工程，来源 STL 之后可以删除。参数/格式错误返回非零退出码，领域错误保留稳定的 `ProjectError` code 供诊断。

开发时可运行 `cargo run -p panta-cli -- project inspect <工程文件.panta>` 检查工程及资产、`cargo run -p panta-cli -- project create <父目录> <工程名>` 创建工程、`cargo run -p panta-cli -- stl inspect <文件.stl>` 查看只读预检摘要，或 `cargo run -p panta-cli -- stl import <工程文件.panta> <文件.stl> [--mesh-type <id>] [--units <unit>]` 导入 STL。默认网格类型来自 Rust 领域常量，单位为毫米；STL 预检不推断单位；质量诊断由 `panta_mesh::diagnose_surface` 在同一解析结果上只读执行（`ProjectService::inspect_stl_with_diagnostics`），焊接容差默认取包围盒对角线 × 1e-6。新增跨 GUI/CLI 的用户操作时，先定义 Qt 无关输入/输出和错误，再由 FFI 与 CLI 各自做薄适配。
