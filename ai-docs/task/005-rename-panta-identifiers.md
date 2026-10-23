# 005 — panta 名称迁移评估

- 状态：draft
- 阶段：仓库命名
- 依赖：无
- 优先级：P2
- 负责人：待分配
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

本仓库为 vtk-test，但 crate（`panta-*`）、可执行 `panta`、工程扩展名 `.panta`、Cargo alias `cargo panta-package`、composite action `panta-*`、CMake 配置 `panta-native-config` 及 `target/panta-tools` 等路径仍沿用 panta；AGENTS.md 与 README 明确“暂沿用”。是否改名及新名称需维护者决策。

## 范围与非目标

- 本任务先盘点影响面并给出分步方案（Cargo package、CMake target、可执行与 bundle、工程文件扩展名与持久化兼容、CI 缓存键、文档）。
- 非目标：在决策前实施任何改名；SDK 来源链接不改。

## 前置条件与待决策

- 维护者决定是否改名、新名称及 `.panta` 工程文件是否需迁移（默认不写兼容代码，见代码生命周期）。

## 验收标准

- [ ] 影响清单与分步方案写入本任务并经维护者确认后再拆分实施任务。

## 风险与回退

改动面大且影响缓存键与工程文件格式，必须分步、逐步验证。

## 决策与工作记录

- 2026-10-24：创建任务，仅登记，不实施。

## 完成摘要

未完成。
