# 015 — GUI 网格质量诊断面板

- 状态：implemented
- 阶段：UI
- 依赖：[006](006-stl-mesh-diagnostics.md)、[014](014-ui-ux.md)
- 优先级：P1
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

task 006 的 `panta_mesh::diagnose_surface` 只能经 CLI `stl inspect` 使用。本任务把它接到 GUI：侧栏对活动网格一键诊断，展示近重复顶点、退化面、开放边界/孔洞、非流形边、法向不一致与焊接容差。

## 必读

[UI 与桥接架构](../architecture/ui-and-bridge.md)、[分层规则](../standards/layering.md)、[注释规范](../standards/comments.md)、[提交规范](../standards/commits.md)。

## 范围与非目标

- FFI：`MeshQualityReport` / `MeshQualityReportResult` 与 `diagnose_mesh_display(Box<MeshDisplayInput>)`，复用导出的共享快照输入，在宿主后台线程执行。
- ViewModel：`beginDiagnoseActiveMesh()`、`diagnosticsPending`、`meshQuality`（带 `documentId`）、`diagnosticsFinished(bool)`。
- QML：侧栏“网格质量”区、诊断按钮、Ctrl+D 快捷键；有问题的计数用错误色。
- 非目标：修复（task 007）、可调容差、问题位置高亮（需拾取，见 task 017）、诊断结果持久化。

## 保守决策

- 只用默认相对容差 1e-6，不暴露设置。
- 报告不持久化，切换页签后面板隐藏，需重新诊断（避免显示过期结果）。
- 诊断期间计入全局 busy，与导入/导出互斥。

## 验收标准

- [x] 无活动网格时拒绝诊断并给出 `project.mesh_not_resident`。
- [x] 单个开放三角形诊断得到 1 面、3 边界边、1 孔洞、非 clean（Rust 单测 + C++ ViewModel 测试）。
- [x] 诊断在后台线程执行，完成后发出信号。
- [ ] 真实窗口目测面板布局（无 GUI 环境，待用户实机验证）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | 本机 Linux `cargo test --locked --workspace`（含 CTest 61 项、qmllint） | 通过 | 通过 |
| 2026-10-24 | CI Linux test + quality | 通过 | 见 PR |
| — | 真实窗口目测 | 布局正常 | 无 GUI 环境，未执行 |

## 清理与兼容例外

无废弃项，无兼容例外。

## 风险与回退

诊断内存与焊接后顶点数、边数线性相关；超大网格诊断耗时和内存较高，进度与取消由 task 016 处理。回退即 revert。

## 决策与工作记录

- 2026-10-24：创建并实施。

## 完成摘要

GUI 侧栏可后台诊断活动网格并展示 task 006 全部指标；待实机目测。
