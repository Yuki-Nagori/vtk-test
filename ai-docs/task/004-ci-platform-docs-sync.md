# 004 — 同步文档中的 CI 平台描述

- 状态：implemented
- 阶段：文档一致性
- 依赖：无
- 优先级：P1
- 负责人：待分配
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

PR #2 已把 PR / main push CI（`ci.yml`）收敛为仅 Linux，macOS / Windows 完整行为回归移至 `nightly.yml`，README 已同步，但以下位置仍描述“CI 在三平台运行”：

- `AGENTS.md`“构建、测试与窗口验收”一节。
- `tests/README.md` 第 13 行。
- `ai-docs/standards/dependency-acquisition.md` CI 小节（仍写 `ci.yml` 三平台矩阵）。
- `.github/actions/panta-cache-save/action.yml` 头注释。
- `ai-docs/architecture/build-and-development.md`“三平台构建由 CI 覆盖”。

完成后上述描述与 `ci.yml` / `nightly.yml` 实际行为一致。

## 必读

[文档规范](../standards/documentation.md)、[提交规范](../standards/commits.md)。

## 范围与非目标

- 只改描述文字与注释。
- 非目标：不改 workflow 触发、矩阵或步骤；不把 macOS / Windows 改回 PR CI；历史任务 001 的工作记录属于历史事实，不改写。

## 涉及分组与跨组依赖

文档与 CI 注释，不适用。

## 前置条件与待决策

无。

## 实施步骤

1. 逐处改为“PR/main 推送 Linux；macOS/Windows 由 nightly 定时与手动触发”。
2. `rg` 复查无残留“CI 三平台”表述（历史记录除外）。

## 预计改动

上列 5 个现存文件。

## 清理与兼容例外

无废弃项，无兼容例外。

## 验收标准

- [x] 上列位置描述与 workflow 一致。
- [ ] Linux CI 通过（注释改动不影响行为）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | `rg -n "三平台" AGENTS.md tests ai-docs .github` 人工复核 | 仅剩历史记录与非 CI 语义 | 已复核：剩余为 task 001 历史记录、SDK 三平台制品与依赖说明，非 CI 触发描述 |

## 风险与回退

纯文档；回退即还原。

## 决策与工作记录

- 2026-10-24：创建任务。
- 2026-10-24：同步上列 5 处描述；nightly 的 save 步骤因条件限定 main push 而不保存缓存，注释仅描述 ci.yml 行为。待 Linux CI。

## 完成摘要

文档与注释已与 workflow 一致；Linux CI 结果见对应 PR。
