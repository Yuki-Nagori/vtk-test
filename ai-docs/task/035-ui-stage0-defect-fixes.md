# 035 — UI 重制阶段 0：现状缺陷修复（不改布局）

- 状态：in-progress
- 阶段：UI 重制（[034](034-ui-redesign.md)）
- 依赖：[034](034-ui-redesign.md)
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

修复 034 现状评估中不涉及布局调整的缺陷：播放栏 Unicode 字形在缺字体环境下不可辨认（Q9）、侧栏超宽内容被裁切、新建工程占位文字对比度与错误重复、导入对话框松散且单位缺少依据、文件选择器策略（Q5）。完成后布局不变，但上述问题消失。

## 必读

[QML 规范](../standards/qml.md)、[Qt 规范](../standards/qt.md)、[图标规范](../standards/icons.md)、[测试规范](../standards/testing.md)、[注释规范](../standards/comments.md)、[提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)、[UI 与 Bridge 架构](../architecture/ui-and-bridge.md)；设计依据为 [034](034-ui-redesign.md) 对应阶段与「用户决定」。

## 范围与非目标

本次交付：

- 播放栏 6 个按钮换成 SVG 图标（Lucide，ISC），循环按钮用描边 / 底色和 tooltip 文字表示开关状态，不只靠颜色；同步在 [图标规范](../standards/icons.md) 补充第三方图标的来源与许可登记规则（Q3 允许阶段内顺带修改）。
- 侧栏超宽行（切面法向、视图预设、色标范围、质量面板等）改为换行或纵向排布，并加 QML 测试断言关键控件不越过父项右边界。
- 新建工程占位文字使用 `colorTextPlaceholder`；名称非法时错误只显示一次。
- 导入对话框收紧行距与空白，单位旁注明来源（默认 / CFD 结果按 SI 米制 / 用户选择）。
- 文件选择器：核实并固定「平台原生优先、无原生实现时回退 Qt Quick 实现」（Q5），在测试中断言未设置 `DontUseNativeDialog`。

非目标：布局骨架（036）、主题 token 更换（039）、按包围盒推测单位（040）、拖放改进（040）。

## 涉及分组与跨组依赖

仅涉及 Shell（`qml/`）与 app 图标 provider（`native/app/icon_provider.cpp` 白名单）；不新增跨组依赖。

## 前置条件与待决策

034 设计已确认（2026-10-24 19:17 UTC+8）。真实 GPU 与 DPR 1.25 / 1.5 / 2.0 窗口验收在本机 Xvfb 无法完成，以 offscreen QML 测试（`QT_SCALE_FACTOR`）和 Xvfb 截图代替，目标平台复核记录为待办。 设计取舍已在 034「用户决定」中确定；遇到文档外的重大取舍时停下汇报，不自行扩大范围。

## 实施步骤

1. PR a：播放栏 SVG 图标 + 图标规范第三方登记规则 + provider 白名单。
2. PR b：侧栏裁切修复 + 边界测试。
3. PR c：新建工程 / 导入对话框修复，文件选择器策略测试。
4. 每个改 C++/QML 的 PR 在分支上跑 nightly，CI 绿后 squash 合并；全部合并后本任务标 implemented（目标平台复核待办）或 done。

## 预计改动

`qml/Panels/PlaybackBar.qml`、`qml/icons/*.svg`（新增）、`native/app/icon_provider.cpp`、`qml/App.qml`、`qml/Dialogs/NewProjectDialog.qml`、`qml/Dialogs/ImportDialog.qml`、`tests/cpp/app/shell_module_load_test.cpp`、`ai-docs/standards/icons.md`。无持久化与公共接口变化。

## 清理与兼容例外

被替换的旧控件与调用点在同一 PR 删除，不保留并行实现。无兼容例外。

## 验收标准

- [ ] 1280×720 和 1600×900，DPR 1.0 / 1.25 / 1.5 / 2.0 下，侧栏、对话框、播放栏没有任何控件被裁切（QML 测试断言关键控件 `x + width ≤ parent.width`，并附截图记录）。
- [ ] 播放栏 6 个按钮都是 SVG 图标，在没有 `⏮⏭⏸` 字形的环境（当前 Xvfb）中可辨认；循环状态可在不依赖颜色的情况下识别。
- [ ] 新建工程名称非法时，错误只出现一次。
- [ ] `cargo format`、`cargo quality`、`cargo lint`、`cargo test --locked --workspace` 全绿；现有 objectName 不变或同步更新测试。
- [ ] 改动 C++/QML 的 PR 在分支上跑 nightly 并通过；每个 commit 的代码、测试、文档和 task 自洽，索引状态一致。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | PR a：本机 `cargo test --locked --workspace`（含 `Qml.ShellModuleLoads` 新用例 `playback_bar_uses_svg_icons_and_shape_coded_loop_state`、`theme_component_test` 白名单图标逐尺寸检查）、`cargo quality`、`cargo lint` | 播放栏 6 个按钮均为白名单 SVG、无文字字形；循环开关有描边 + 无障碍名称 | 通过（70/70） |
| 2026-10-24 | PR b：offscreen 用例 `sidebar_controls_stay_within_sidebar_bounds`（导入 `tests/fixtures/mesh/pvd/flow.pvd`、开切面 + 字段 T + 质量诊断，1280×720 / 1600×900），CTest 另以 `QT_SCALE_FACTOR=1.25 / 1.5 / 2` 重跑（`Qml.ShellSidebarBounds.Dpr*`）；`PANTA_SHELL_SCREENSHOT_DIR` 保存离屏截图 | 侧栏全部可见控件右缘 ≤ 侧栏右缘；侧栏直接子项在窗口内，详情区可滚到最后一项 | 修复前侧栏内部被撑到 385px（右缘 393 > 248.5）；修复后 4 个 DPR 全部通过。截图：`/workspace/shots/035b/sidebar-{1280x720,1600x900}-dpr{1,1.25,1.5,2}.png`（盒子本地，视口在 offscreen 下为空白） |

## 风险与回退

主要风险是原生视口层与 QML 布局的交互，以及 Xvfb 无法替代真实 GPU 验收。每个 PR 独立可回退（revert squash commit），不改工程文件格式，不影响用户已有工程数据。

## 决策与工作记录

- 2026-10-24：按 034 用户决定登记任务。
- 2026-10-24：PR a 采用 Lucide 1.61.0 的 skip-back / step-back / play / pause / step-forward / skip-forward / repeat，几何未修改，按用途重命名为 `playback-*`；许可全文放 `qml/icons/LICENSE-lucide.txt`，每个 SVG 带来源注释。循环开启时按钮描边与图标改为 `colorFocus`，并通过 `checked` 和无障碍名称（「循环播放：开 / 关」）表达状态，不只靠颜色。objectName 不变。
- 2026-10-24：PR b 定位侧栏裁切根因：Basic 样式 Button 背景自带 100px 隐式宽度，切面行「切面 + X/Y/Z」三个按钮的最小宽度把整列 `ColumnLayout` 撑到 385px，所有 `fillWidth` 子项随之越界。修复：切面预设与视图预设按钮按文字计算隐式宽度；法向三个 SpinBox 改为每分量一行；质量面板标签换行；色标范围行填满宽度。另发现内容全部展开时总高约 1250px，超出 720 / 900 窗口、视图预设无法触达，因此把资产列表以下的详情放进可滚动区（滚动条在内容超出时常显）。这属于验收标准「没有任何控件被裁切」的直接修复，不改布局骨架（036 会重排）。

## 完成摘要

未完成。
