# 014 — 工作台 UI/UX 优化（保守版）

- 状态：implemented
- 阶段：UI
- 依赖：[013](013-mesh-export.md)
- 优先级：P1
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

用户批准的 task 5：优化导入/导出入口、进度反馈、错误提示、网格质量诊断展示、快捷键和视口交互。界面保持中文，沿用 `Theme` 风格。本次只做纯 QML 的保守改进，不改 bridge / FFI 接口。

## 必读

[UI 与桥接架构](../architecture/ui-and-bridge.md)、[提交规范](../standards/commits.md)。

## 范围与非目标

本次交付（仅 `qml/App.qml`）：
- 快捷键：Ctrl+N 新建、Ctrl+O 打开、Ctrl+S 保存（StandardKey）、Ctrl+I 导入、Ctrl+E 导出、Ctrl+Shift+P 截图；启用条件与按钮共用 `can*` 属性。
- 工具栏按钮 ToolTip 标注快捷键。
- 窗口标题对未保存工程加 `* ` 前缀。
- 状态栏左侧 BusyIndicator（任意长任务进行中时转动）。
- 侧栏显示活动文档状态消息与激活诊断（稳定码 + 详情，错误色）。

非目标（整理为建议，交父代理决策）：
- GUI 网格质量诊断面板（需新增 FFI/ViewModel 接口暴露 `diagnose_surface`）。
- 可取消、带百分比的进度（需后台任务进度协议）。
- 视口交互：视角预设、重置视图、测量、拾取（需 VtkViewport 新 API）。
- 拖放导入、最近工程列表、菜单栏、关闭前未保存确认。

## 保守决策

- Ctrl+Shift+P 用于截图，避免与 StandardKey.Print 平台差异冲突。
- 不新增 C++ 属性；诊断展示仅用现有 `openDocuments[].diagnostic`。
- 不改按钮布局与主题色值。

## 验收标准

- [x] 快捷键在忙碌或无工程/无活动网格时不触发。
- [x] 激活失败的文档在侧栏显示中文消息与诊断码。
- [x] `cargo format --check` 通过；qmllint 与 C++ 测试由 CI 验证。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | 本机 `cargo format --check` | 通过 | 通过 |
| 2026-10-24 | CI Linux（qmllint、shell_module_load_test） | 通过 | 见 PR |
| — | 实机快捷键 / 诊断目测 | 符合预期 | 无 GUI 环境，未执行 |

## 风险与回退

纯 QML 改动，回退即 revert 该 PR，不影响工程数据。

## 决策与工作记录

- 2026-10-24：创建并实施保守版；大改动列入非目标建议。

## 完成摘要

实现上述 QML 改进；GUI 实机检查待用户完成。
