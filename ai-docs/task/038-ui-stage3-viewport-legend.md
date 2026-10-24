# 038 — UI 重制阶段 3：视口内色标与空状态

- 状态：implemented
- 阶段：UI 重制（[034](034-ui-redesign.md)）
- 依赖：[037](037-ui-stage2-object-tree.md)、[041](041-sdk-production-migration.md)（SDK 开启 RenderingAnnotation）
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

色标改由 VTK `vtkScalarBarActor` 绘制（Q2，原生层之上不能叠 QML）；视口改深色渐变背景（Q1）；无文档时给出可操作引导；截图默认包含色标。

## 必读

[QML 规范](../standards/qml.md)、[Qt 规范](../standards/qt.md)、[图标规范](../standards/icons.md)、[测试规范](../standards/testing.md)、[注释规范](../standards/comments.md)、[提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)、[UI 与 Bridge 架构](../architecture/ui-and-bridge.md)；设计依据为 [034](034-ui-redesign.md) 对应阶段与「用户决定」。

## 范围与非目标

本次交付：scalar bar（字段名、单位、范围模式）；背景 #2b3442→#11161d；空状态引导（视口区不可叠放时退回对象树空状态）；截图包含色标。

非目标：颜色图预设（039）。

## 涉及分组与跨组依赖

可视化（`native/visualization/src/vtk/`）、Shell；不新增跨组依赖。

## 前置条件与待决策

依赖 037 合并。WebGPU 视口在 Xvfb 下为空白，色标与对比度的真实截图须在目标平台完成，本机以 C++ 测试检查 actor 状态。 设计取舍已在 034「用户决定」中确定；遇到文档外的重大取舍时停下汇报，不自行扩大范围。

## 实施步骤

1. VTK 视口新增 scalar bar actor 与范围同步，C++ 测试。
2. 背景色切换与对比度检查。
3. 空状态引导与截图路径验证。

## 预计改动

`native/visualization/src/vtk/{vtk_viewport,primitive_scene}.cpp` 及头文件、`mesh_source.hpp`（色标元数据）、`render_scene.hpp`（渐变两色）、Bridge 切面快照、对应测试、`qml/Panels/ViewportPane.qml`、新增 `ViewportEmptyState.qml`。属性面板的数值色标按 034 Q2 保留（视口色标负责读图，面板保留精确数值与区间口径切换）。

## 清理与兼容例外

被替换的旧控件与调用点在同一 PR 删除，不保留并行实现。无兼容例外。

## 验收标准

- [x] 色标数值和属性面板一致；切换「全部时间步 / 当前步」后色标标注同步变化（C++ 测试检查 actor 的范围和标题）。
- [ ] 视口截图（013 的截图功能）产物中能看到色标（真实 GPU 截图证据）。
- [x] 无文档时有可操作的引导（导入 / 打开 / 最近工程）；若原生层遮挡导致无法在视口区显示，则按设计退回对象树空状态，并在任务中记录平台证据。
- [ ] 深色背景下网格边线、拾取高亮、方向标记的对比度足够（人工检查 + 截图）。
- [x] 改动 C++/QML 的 PR 在分支上跑 nightly 并通过；每个 commit 的代码、测试、文档和 task 自洽，索引状态一致。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | Linux x86_64，SDK 切到 vtk-test Release（#60）后 `cargo format / lint / quality / test --locked --workspace` | 全部通过 | 通过；ctest 73/73（含 `Visualization.PrimitiveScene`、`ProjectViewModelTest.ColorsSectionsByResultFieldAndShowsLegend`） |
| 2026-10-24 | `Visualization.PrimitiveScene` 的 `scalar_bar_tracks_field_lookup_table_and_title` 等 | scalar bar 与 mapper 共用 LUT；标题含字段名 / 单位 / 区间口径；无标量时隐藏；背景渐变两色 | 通过 |
| 2026-10-24 | `panta_qml_shell_module_test`，offscreen + software RHI 截图 | 无文档时视口区显示引导页（导入 / 新建 / 打开 / 最近工程） | 通过；`/workspace/shots/038/viewport-empty-state.png`、`sidebar-1280x720-dpr1.png` 等 7 张 |
| 2026-10-24 | 真实 GPU 窗口：视口截图中的色标、CJK 字体、原生表面隐藏后引导页可见性、深色背景下边线 / 高亮对比度 | 目标平台人工确认 | 待验证（本机无 GPU，WebGPU 视口 offscreen 为空白） |

## 风险与回退

主要风险是原生视口层与 QML 布局的交互，以及 Xvfb 无法替代真实 GPU 验收。每个 PR 独立可回退（revert squash commit），不改工程文件格式，不影响用户已有工程数据。

## 决策与工作记录

- 2026-10-24：按 034 用户决定登记任务。
- 2026-10-24：原 SDK 未编入 RenderingAnnotation，没有 `vtkScalarBarActor`。用户决定把 SDK 生产迁入本仓库并开启该模块（task 041），038 依赖 041 切换产物后实施。
- 2026-10-24：色标元数据经 `SurfaceMeshSnapshot`（`scalar_title`/`scalar_units`/`scalar_range_label`，UTF-8）传给视口；`sync_scalar_bar` 直接共用 mapper 的查找表，范围与着色天然一致。区间口径只在时间步结果上显示（「全部时间步」/「当前步」，按实际采用的区间）。
- 2026-10-24：VTK 内置字体不含中文字形。色标文字按平台候选列表取第一个存在的 CJK 字体文件（macOS PingFang/Hiragino/STHeiti，Windows 微软雅黑/黑体/宋体，Linux Noto Sans CJK/文泉驿/Droid Fallback），都不存在时回退内置字体（中文可能缺字，需在目标平台截图确认）。
- 2026-10-24：空状态采用「无文档时隐藏 `CaeViewport`」：`VtkViewport` 在不可见时隐藏原生表面（既有行为），同一矩形显示 QML 引导页（导入网格 / 新建工程 / 打开工程 / 最近 5 个工程），背景与 VTK 渐变同色。对象树空状态文字保留。原生表面隐藏后 QML 是否可见需在三端 GPU 平台确认；若某平台不满足，按 034 退回对象树空状态。

## 工作日志

- 2026-10-24：PR #60（041 清单切换）合并为 9742af5；038 分支 rebase 到该 main 后开 PR。
- 2026-10-24：038 PR CI 与分支 nightly 通过后合并（合并提交见 039 工作日志）。

## 完成摘要

已实施：VTK 视口内 `vtkScalarBarActor` 色标（字段名 / 单位 / 区间口径，与 mapper 共用查找表）、深色渐变背景 #2b3442→#11161d、无文档时视口区引导页，截图沿用视口渲染因而包含色标。离屏截图见 `/workspace/shots/038/`。未勾选两项（真实 GPU 截图中的色标、深色背景对比度）需在目标平台人工确认，确认前状态保持 implemented。
