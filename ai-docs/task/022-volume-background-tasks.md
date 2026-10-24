# 022 — 体网格质量统计、切面与 VTU 导出后台化

- 状态：planned
- 阶段：网格
- 依赖：[015](015-mesh-quality-panel.md)、[016](016-background-progress.md)、[021](021-volume-mesh.md)
- 优先级：P1
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

021 首期的体网格质量统计、切面计算和 `.vtu` 体网格导出都在 GUI 线程同步执行，大体网格会卡住界面，也不能取消。本任务把这三项移到后台线程：进度和取消走 016 的 `Progress` / `TaskProgress` 协议，质量结果显示在 015 的诊断面板。

## 必读

[016](016-background-progress.md)、[015](015-mesh-quality-panel.md)、[UI 与桥接架构](../architecture/ui-and-bridge.md)、[注释规范](../standards/comments.md)、[提交规范](../standards/commits.md)。

## 范围与非目标

- `panta-mesh`：`tet_quality` 和 `section_surface` 增加检查点，支持协作取消（`VolumeError::Cancelled`）。
- `panta-import`：`export_tet_mesh_vtu_with_progress`，按估算字节报告进度；取消时删除 `.partial`，目标文件不变。
- FFI：拥有式 `VolumeInput` 快照（共享 `Arc<TetMesh>`），提供 `diagnose_volume`、`section_volume`、`export_volume` 三个后台入口，均接收 `TaskProgress`。
- ViewModel：
  - 体网格的“诊断”沿用 `beginDiagnoseActiveMesh`，结果交给诊断面板：外表面诊断加体网格质量段。
  - `.vtu` 体网格导出走 `beginExportActiveMeshUrl` 的后台通道。
  - 切面在后台计算，连续拖动时合并为最新请求。
- QML：诊断面板显示体网格质量（单元数、最小二面角、长宽比、劣质单元）；侧栏“体网格”区只保留摘要，不再同步计算质量。
- 非目标：并行化质量统计算法；切面结果缓存。

## 保守决策

- 切面计算使用独立的“最新意图”合并：同一时刻只跑一个切面任务，完成后若参数已变化再跑一次；切面不进入状态栏进度（计算量小、频率高），但可被新请求或切换文档取消。
- 质量阈值仍是常量，只报告。

## 验收标准

- [ ] 体网格质量统计、切面、VTU 导出均可取消，取消后返回稳定错误码且不留下半成品（Rust 单测）。
- [ ] ViewModel：体网格诊断结果包含质量段；VTU 导出在后台完成并发出 `exportFinished`（C++ 测试）。
- [ ] format、clippy、qmllint、全量测试通过；offscreen 启动无 QML 警告。
- [ ] 实机：大体网格切面拖动流畅度（需 GPU）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| — | 待实施 | — | 未执行 |

## 清理与兼容例外

删除 021 的同步 FFI `project_service_volume_info` / `project_service_volume_section` / `project_service_export_volume_vtu`，以及 ViewModel 中对应的同步调用。无兼容例外。

## 风险与回退

后台快照持有 `Arc<TetMesh>`，内存不翻倍；回退为同步路径即可。

## 决策与工作记录

- 2026-10-24：经用户批准创建任务。

## 完成摘要

未完成。
