# 022 — 体网格质量统计、切面与 VTU 导出后台化

- 状态：done
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

- [x] 体网格质量统计、切面、VTU 导出均可取消，取消后返回稳定错误码且不留下半成品（Rust 单测）。
- [x] ViewModel：体网格诊断结果包含质量段；VTU 导出在后台完成并发出 `exportFinished`（C++ 测试）。
- [x] format、clippy、qmllint、全量测试通过；offscreen 启动无 QML 警告。
- [ ] 实机：大体网格切面拖动流畅度（需 GPU，未在本机验证）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | `cargo test -p panta-mesh/-import/-core/-ffi` | 取消/进度单测通过 | 通过（含 `window_maps_stage_reports_into_overall_range`、tet VTU 取消无残留、`volume_diagnostics_section_and_vtu_export_run_on_snapshots_and_cancel`） |
| 2026-10-24 | `cargo test --locked --workspace`（64 项 CTest） | 全部通过 | 通过，新增 `RunsVolumeQualitySectionAndVtuExportInTheBackground` |
| 2026-10-24 | `cargo format`、`cargo lint clippy --check`、`cargo lint qmllint --check` | 无告警 | 通过 |
| 2026-10-24 | `QT_QPA_PLATFORM=offscreen timeout 8 target/native/debug/app/panta` | 无 QML 警告 | 超时退出（124），日志无 error/warning |

## 清理与兼容例外

删除 021 的同步 FFI `project_service_volume_info` / `project_service_volume_section` / `project_service_export_volume_vtu`，以及 ViewModel 中对应的同步调用。无兼容例外。

## 风险与回退

后台快照持有 `Arc<TetMesh>`，内存不翻倍；回退为同步路径即可。

## 决策与工作记录

- 2026-10-24：经用户批准创建任务。
- 2026-10-24：实施偏差（保守简化）：未新增独立 `VolumeInput`，而是在已有拥有式 `MeshDisplayInput` 快照中加 `volume: Option<Arc<TetMesh>>`，诊断/导出复用 016 的后台入口，新增 `section_mesh_display`；FFI 同步入口只保留 `project_service_volume_summary`（计数）。切面与质量统计取消时返回 `None`（不报错，结果丢弃），VTU 导出取消返回 `project.export_cancelled`。诊断进度窗口：外表面 0–80%，体网格质量 80–100%（`Progress::set_window`）。`.vtu` 扩展名在有体网格时导出四面体单元，否则仍导出外表面。
- 2026-10-24：无可用子代理，未做独立评审。

## 完成摘要

体网格质量统计、切面与 VTU 导出均已移到后台线程，接入 016 进度/取消协议；诊断面板显示体网格质量段（节点、四面体、最小二面角、长宽比、劣质单元），侧栏只显示计数摘要。切面按最新意图合并，切换文档或关闭切面时取消在途任务。待实机验证：大体网格切面拖动流畅度（需 GPU）。
