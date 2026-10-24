# 039 — UI 重制阶段 4：视觉规范落地

- 状态：in-progress
- 阶段：UI 重制（[034](034-ui-redesign.md)）
- 依赖：[038](038-ui-stage3-viewport-legend.md)
- 优先级：P2
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

把 034「视觉规范」落到 Theme 与组件：浅色 UI token、字体阶梯、间距与控件高度、首批约 30 个图标、颜色图预设，并同步修改 Theme / 图标 / 工作台布局三份现行规范。

## 必读

[QML 规范](../standards/qml.md)、[Qt 规范](../standards/qt.md)、[图标规范](../standards/icons.md)、[测试规范](../standards/testing.md)、[注释规范](../standards/comments.md)、[提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)、[UI 与 Bridge 架构](../architecture/ui-and-bridge.md)；设计依据为 [034](034-ui-redesign.md) 对应阶段与「用户决定」。

## 范围与非目标

本次交付：Theme token（colorWindow #f3f4f6、colorPanelAlt #f7f8fa、colorPanelLine #d5d8de、colorText #1f2328、colorTextMuted #59636e、colorTextPlaceholder #8c959f、accent #2563eb、colorSelectionBg #dbe7fd、warning #9a5b00（034 原值 #b26a00 对白底仅 4.2:1，见决策记录）、success #1a7f37）；字号 11/12/13/14/16；间距 2/4/8/12/16/24；控件高 26/22/28；圆角 4/6；约 30 个 Lucide + 自绘 CAE 图标并登记许可；对话框统一尺寸与按钮区；颜色图预设。

非目标：运行时主题切换（Q1 不做）；六向 / 等轴视图图标与体网格图标（无消费者，见决策记录）；颜色图写入工程文件。

## 涉及分组与跨组依赖

Shell、app 图标 provider、可视化（颜色图预设）；不新增跨组依赖。

## 前置条件与待决策

依赖 038 合并。 设计取舍已在 034「用户决定」中确定；遇到文档外的重大取舍时停下汇报，不自行扩大范围。

## 实施步骤

1. Theme token 与组件替换，`rg` 检查字面量。
2. 图标批量引入与登记。
3. 颜色图预设与规范文档同步。

## 预计改动

`qml/Themes/Theme.qml`、`qml/Components/`、`qml/icons/`、`native/app/icon_provider.cpp`、`native/visualization/`（`mesh_source.hpp` 颜色图预设、`primitive_scene.cpp`）、`native/bridge/`（`fieldColorMap` 属性）、`qml/Dialogs/`（统一内边距与高度）、`ai-docs/standards/qml.md`、`ai-docs/standards/icons.md`、`ai-docs/modules/qml-components-and-theme.md`。

## 清理与兼容例外

被替换的旧控件与调用点在同一 PR 删除，不保留并行实现。无兼容例外。

## 验收标准

- [x] `App.qml` 与各组件中没有写死的颜色、字号、间距（`rg` 检查颜色和数值字面量，例外须注明理由）。
- [x] 每个新图标都有来源、许可、版本和使用位置记录，provider 白名单与打包清单一致，无效 ID 测试通过。
- [x] 正文和次要文字对比度 ≥ 4.5:1（列出检查值）；焦点环在所有可聚焦控件上可见。
- [ ] 规范文档与实现一致，无失效描述。
- [ ] 改动 C++/QML 的 PR 在分支上跑 nightly 并通过；每个 commit 的代码、测试、文档和 task 自洽，索引状态一致。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | Linux x86_64，`cargo format / lint（含 qmllint）/ quality / test --locked --workspace` | 全部通过 | 通过；ctest 73/73（含 `ThemeComponentTest`、`ProjectViewModelTest` 颜色图切换、`Visualization.PrimitiveScene` 预设 LUT 与越界回退） |
| 2026-10-24 | 字面量检查：`rg` 数值字面量（见 QML 规范）与 `"#rrggbb"` 颜色字面量，排除 `Themes/` | 仅 Theme 本身与业务数值 `stepSize: 1` | 通过；颜色字面量只出现在 SVG 资源内 |
| 2026-10-24 | 对比度计算（WCAG 相对亮度） | 正文 / 次要 ≥ 4.5:1 | 通过：正文 15.80 / 14.35、次要 6.11 / 5.56、accent 5.17、warning 5.43 / 4.93、success 5.08、error 5.62（白底 / `colorWindow`）；占位 3.04 为记录在案的例外 |
| 2026-10-24 | 离屏截图（offscreen + software RHI） | 字体、间距、图标、对话框统一 | `/workspace/shots/039/`：tree-assets-info.png、new-project-error.png、import-dialog.png、sidebar-1280x720-dpr1.png、sidebar-1600x900-dpr1.png、sidebar-collapsed.png、viewport-empty-state.png |
| 2026-10-24 | 目标平台：CJK 字体与 `tnum` 等宽数字、DPR 1.25–2.0 无裁切、真实 GPU 色标颜色图 | 三端人工确认 | 待验证 |

## 风险与回退

主要风险是原生视口层与 QML 布局的交互，以及 Xvfb 无法替代真实 GPU 验收。每个 PR 独立可回退（revert squash commit），不改工程文件格式，不影响用户已有工程数据。

## 决策与工作记录

- 2026-10-24：按 034 用户决定登记任务。
- 2026-10-24：`colorWarning` 由 034 表中的 #b26a00 调为 #9a5b00。原值对白底 4.24:1、对 `colorWindow` 3.85:1，达不到本任务「≥ 4.5:1」的验收线；新值 5.43 / 4.93，色相不变。属于实现层面的取值修正，不改变设计取舍。对比度全表见[组件与固定界面样式](../modules/qml-components-and-theme.md#视觉规范task-039)。
- 2026-10-24：`colorTextPlaceholder` #8c959f 对白底 3.04:1，按 035 / 034 Q 节保留为记录在案的例外：占位只是示例，字段标签以 `colorText` 常显。
- 2026-10-24：QML `font` 值类型在锁定 Qt 6.11 中没有 `families`，`Theme.fontFamily` 按平台给单个字族，缺失时由 Qt 回退系统字体。数值显示用 `font.features: {"tnum": 1}`；Noto Sans CJK / 微软雅黑 / 苹方的数字是否真正等宽需在目标平台截图确认，若不支持再按 034 改用等宽字族（待验证）。
- 2026-10-24：状态消息由布尔 `isError` 改为等级字符串（info / success / warning / error），状态栏与底部消息列表同时显示 `status-*` 图标与着色文字，满足「状态不只靠颜色」；`colorWarning` / `colorSuccess` 由此获得消费者。
- 2026-10-24：图标许可复核发现 Lucide 上游 `LICENSE` 列出了 Feather 派生（MIT）图标，原有 `tree-expand.svg`（`chevron-right`）属于其中，而图标规范写作「目前未使用」。已改为随源码分发上游许可全文（含 MIT 段与名单），派生图标的 SVG 注释补 MIT 版权行，图标规范表格标注「ISC + MIT（Feather）」。新引入的 `info`、`triangle-alert`、`circle-x`、`clock` 同属派生。
- 2026-10-24：CAE 专用图标自绘：`section.svg` 由临时的 `scissors` 换成斜置切平面 + 法向箭头，新增 `tree-field.svg`（分段色标条）。六向 / 等轴视图图标暂缓：视图预设目前是带文字的下拉菜单，没有图标消费者，等 040 或后续把视图方向放上工具栏时再自绘；体网格图标同理（树上尚不区分体 / 表面资产）。本批新增 10 个、替换 1 个，`qml/icons/` 现有 35 个 SVG（另有品牌占位 1 个）。
- 2026-10-24：颜色图预设 4 个：冷暖（ParaView Cool to Warm，默认，沿用 029）、Viridis（matplotlib，CC0，等距 5 点采样）、彩虹（ParaView Blue to Red Rainbow，CAE 常用）、灰度。034 列举的是「如 Cool to Warm、Viridis、灰度」，加入彩虹是因为 CAE 用户最常用、ParaView 内置；它不是感知均匀色表，默认仍为冷暖。缺失色按预设取（冷暖黄、Viridis 红、彩虹灰、灰度红），保证与色表本身可区分。预设属会话内视图状态（与字段选择一致，不写工程）；切换时复制当前快照只改预设索引，不重算切面。
- 2026-10-24：对话框统一：四个对话框内容区四边 `dialogContentPadding` 16 px，高度一律随内容收紧（新建工程 / 生成体网格 / 大网格设置原为固定 360 px，删除 `newProjectDialogHeight`）；`DialogFrame` 圆角 6 px + 1 px 边框，标题栏同步圆上角；按钮间距 5 → 8 px 回到栅格。

## 完成摘要

未完成。
