# 039 — UI 重制阶段 4：视觉规范落地

- 状态：planned
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

本次交付：Theme token（colorWindow #f3f4f6、colorPanelAlt #f7f8fa、colorPanelLine #d5d8de、colorText #1f2328、colorTextMuted #59636e、colorTextPlaceholder #8c959f、accent #2563eb、colorSelectionBg #dbe7fd、warning #b26a00、success #1a7f37）；字号 11/12/13/14/16；间距 2/4/8/12/16/24；控件高 26/22/28；圆角 4/6；约 30 个 Lucide + 自绘 CAE 图标并登记许可；对话框统一尺寸与按钮区；颜色图预设。

非目标：运行时主题切换（Q1 不做）。

## 涉及分组与跨组依赖

Shell、app 图标 provider、可视化（颜色图预设）；不新增跨组依赖。

## 前置条件与待决策

依赖 038 合并。 设计取舍已在 034「用户决定」中确定；遇到文档外的重大取舍时停下汇报，不自行扩大范围。

## 实施步骤

1. Theme token 与组件替换，`rg` 检查字面量。
2. 图标批量引入与登记。
3. 颜色图预设与规范文档同步。

## 预计改动

`qml/Themes/Theme.qml`、`qml/Components/`、`qml/icons/`、`native/app/icon_provider.cpp`、`native/visualization/`、`ai-docs/standards/qml.md`、`ai-docs/standards/icons.md`、`ai-docs/modules/qml-components-and-theme.md`。

## 清理与兼容例外

被替换的旧控件与调用点在同一 PR 删除，不保留并行实现。无兼容例外。

## 验收标准

- [ ] `App.qml` 与各组件中没有写死的颜色、字号、间距（`rg` 检查颜色和数值字面量，例外须注明理由）。
- [ ] 每个新图标都有来源、许可、版本和使用位置记录，provider 白名单与打包清单一致，无效 ID 测试通过。
- [ ] 正文和次要文字对比度 ≥ 4.5:1（列出检查值）；焦点环在所有可聚焦控件上可见。
- [ ] 规范文档与实现一致，无失效描述。
- [ ] 改动 C++/QML 的 PR 在分支上跑 nightly 并通过；每个 commit 的代码、测试、文档和 task 自洽，索引状态一致。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| — | 待实施 | 见验收标准 | 未执行 |

## 风险与回退

主要风险是原生视口层与 QML 布局的交互，以及 Xvfb 无法替代真实 GPU 验收。每个 PR 独立可回退（revert squash commit），不改工程文件格式，不影响用户已有工程数据。

## 决策与工作记录

- 2026-10-24：按 034 用户决定登记任务。

## 完成摘要

未完成。
