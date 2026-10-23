# 003 — Cargo 元数据 repository 指向本仓库

- 状态：implemented
- 阶段：仓库维护
- 依赖：无
- 优先级：P2
- 负责人：待分配
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

根 `Cargo.toml` 的 `[workspace.package] repository` 仍为 `https://github.com/Yuki-Nagori/panta`，所有成员经 `repository.workspace = true` 继承，`cargo metadata` 会把本仓库包指向来源仓库。完成后元数据指向 `https://github.com/Yuki-Nagori/vtk-test`。

## 必读

[Cargo 规范](../standards/cargo.md)、[提交规范](../standards/commits.md)。

## 范围与非目标

- 只修改 workspace `repository` 字段。
- 非目标：crate / 应用 `panta` 名称（见 [005](005-rename-panta-identifiers.md)）；`tools/sdk/releases/` 与依赖规范中指向原 Panta Release / workflow 的来源链接是真实来源，保持不变。

## 涉及分组与跨组依赖

仅 Cargo 元数据，不新增跨组依赖。

## 前置条件与待决策

无。

## 实施步骤

1. 修改根 `Cargo.toml`。
2. 用 `cargo metadata --no-deps` 确认全部成员继承新值，CI 通过。

## 预计改动

`Cargo.toml`（现存）；`Cargo.lock` 不受影响。

## 清理与兼容例外

无废弃项，无兼容例外。

## 验收标准

- [x] `cargo metadata --no-deps --format-version 1` 中所有 workspace 包 repository 为 vtk-test。
- [ ] Linux CI 通过。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | /workspace/vtk-test，`cargo metadata --no-deps --format-version 1 --offline` | 全部为 vtk-test | 9 个 workspace 包均为 `https://github.com/Yuki-Nagori/vtk-test` |

## 风险与回退

无运行期影响；回退即还原字段。

## 决策与工作记录

- 2026-10-24：创建任务。
- 2026-10-24：修改根 `Cargo.toml` repository；本地 metadata 核对通过，Cargo.lock 无变化。待 Linux CI。

## 完成摘要

repository 已指向 vtk-test；Linux CI 结果见对应 PR。
