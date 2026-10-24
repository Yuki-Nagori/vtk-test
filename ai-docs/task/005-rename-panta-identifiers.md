# 005 — panta-import 改名为 panta-io，并拆分为 import / export 模块

- 状态：done
- 阶段：仓库命名
- 依赖：无
- 优先级：P2
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

本任务原为“panta 名称迁移评估”，评估 crate、可执行 `panta`、`.panta` 工程扩展名、CI action 等是否改名。

**维护者决定（2026-10-24 11:06 UTC+8）：panta 整体不改名。** crate 前缀 `panta-*`、可执行 `panta`、`.panta` 扩展名、Cargo alias、composite action、CMake 配置和 `target/panta-tools` 路径都保持现状，不需要迁移工程文件。原评估范围就此关闭，任务改写为下面这项实际改动。

`panta-import` 自 013 起也承担网格导出（`export` 模块），名称已不符合职责。本任务把它改名为 `panta-io`，并在内部拆成 `import` 和 `export` 两个模块。

## 必读

[分层规范](../standards/layering.md)、[Rust 规范](../standards/rust.md)、[仓库布局](../architecture/repository-layout.md)、[提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)。

## 范围与非目标

- crate 目录与包名：`crates/panta-import` → `crates/panta-io`，`panta-import` → `panta-io`。同步更新 workspace `members`、`[workspace.dependencies]`、`panta-core` 依赖、`Cargo.lock` 和 launcher `build.rs` 的源码监视路径。
- 模块：
  - `panta_io::import`：原 crate 根，包括格式识别、来源快照、预览和解析，`formats` 子模块随之移入。
  - `panta_io::export`：原 `export` 模块，`stream` 和 `tests` 改为常规子模块文件，不再使用 `#[path]`。
  - 两个模块互不依赖，只有 export 的测试读取 import 的解析器做往返校验。
- 调用方：`panta-core` 中的引用全部改为 `panta_io::import::…` / `panta_io::export::…`。crate 根不做 re-export，不保留旧名别名。
- 文档：architecture/、standards/ 和夹具 README 中描述现状的引用同步更新。历史任务记录（010、011、013 等）保留当时的名称，不改写。
- CI：workflow 和 composite action 中没有 `panta-import` 引用（已用 `rg` 核对），无需改动。
- 非目标：panta 整体改名；修改任何行为或公共函数签名。

## 涉及分组与跨组依赖

只涉及 Rust L2（panta-io）与 L3（panta-core）。依赖方向仍为 `panta-core → panta-io → panta-mesh`，不新增跨组依赖。

## 验收标准

- [x] 仓库内（历史任务记录除外）不再有 `panta-import` / `panta_import` 引用。
- [x] `cargo format`、`cargo lint clippy --check`、`cargo lint qmllint --check` 和 `cargo test --locked --workspace`（CTest 67/67）全部通过，测试数量不变。
- [x] CI 通过。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | box：`rg -n "panta[-_]import" -g '!ai-docs/task/**' -g '!target'` | 无结果 | 通过 |
| 2026-10-24 | box：全量检查（同上） | 全部通过，测试数不变 | 通过 |

## 清理与兼容例外

旧 crate 名直接删除，不保留 re-export 或别名，符合代码生命周期规范。`.panta` 工程文件和持久化格式不受影响。临时目录前缀 `panta-import-*` 改为 `panta-io-*`（仅测试使用）。

## 风险与回退

纯重命名，不改动行为。回退时 revert 本 PR 即可。

## 决策与工作记录

- 2026-10-24：创建任务，仅登记，不实施。
- 2026-10-24 11:06 UTC+8：维护者决定 panta 整体不改名。任务改写为 panta-import → panta-io 改名与模块拆分，在 026 合并后、下一个大改动之前插队完成，避免大范围冲突。
- 2026-10-24：保守决定：
  - crate 根不 re-export import 项，调用方显式写 `panta_io::import::…`，使模块边界在调用处可见。
  - 历史任务记录不改写。

## 完成摘要

`panta-import` 已改名为 `panta-io`，拆分为 `import` / `export` 两个模块，引用、文档和 lock 文件都已同步，行为不变。panta 整体不改名的决定已记录在本任务中。
