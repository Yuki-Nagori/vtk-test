# 037 — UI 重制阶段 2：对象树与上下文属性面板

- 状态：implemented
- 阶段：UI 重制（[034](034-ui-redesign.md)）
- 依赖：[036](036-ui-stage1-layout-skeleton.md)
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

用 Rust 提供的对象树（工程 → 资产 → 切面 / 着色 / 时间序列 / 诊断）替换旧侧栏区块；选中与激活分离；属性面板按节点类型显示「属性 / 显示 / 信息」。

## 必读

[QML 规范](../standards/qml.md)、[Qt 规范](../standards/qt.md)、[图标规范](../standards/icons.md)、[测试规范](../standards/testing.md)、[注释规范](../standards/comments.md)、[提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)、[UI 与 Bridge 架构](../architecture/ui-and-bridge.md)；设计依据为 [034](034-ui-redesign.md) 对应阶段与「用户决定」。

## 范围与非目标

本次交付：Rust 树模型与 Bridge 暴露；可见性眼睛（只影响显示）；单击选中、双击 / Enter 激活；属性面板三页；播放中禁用项就地说明原因；删除旧侧栏「模型资产 / 时序动画 / 体网格」区块。

非目标：撤销 / 重做（Q10 不做）；色标进视口（038）。

## 涉及分组与跨组依赖

Rust 应用层 / Bridge ViewModel、Shell；不新增跨组依赖（树模型留在既有 ViewModel 分组）。

## 前置条件与待决策

依赖 036 合并。可见性若需持久化，必须先在本任务记录并验证 dirty 行为；否则只作显示状态。 设计取舍已在 034「用户决定」中确定；遇到文档外的重大取舍时停下汇报，不自行扩大范围。

## 实施步骤

1. Rust 树模型与单元测试。
2. Bridge 暴露 `QAbstractItemModel`（或现有列表模型扩展）与选中 / 激活接口。
3. QML 树视图与属性面板三页，迁移旧侧栏控件后删除旧区块。

## 预计改动

`crates/` 下 ViewModel 相关模块、`native/bridge/`、`qml/Panels/`、测试。

## 清理与兼容例外

被替换的旧控件与调用点在同一 PR 删除，不保留并行实现。无兼容例外。

## 验收标准

- [x] 树模型由 Rust 单元测试覆盖：资产增删、切面开关、字段切换、时间步出现 / 消失时，节点结构正确。
- [x] 单击节点只切换属性面板，不切换页签；双击或 Enter 激活页签；键盘上下 / 左右可以浏览和展开树。
- [x] 可见性开关只影响显示，不改工程数据；如果写入工程（例如记住隐藏状态），需在任务中明确，并验证 dirty 行为。
- [x] 旧侧栏的「模型资产 / 时序动画 / 体网格」区块删除，无残留调用点。
- [x] 改动 C++/QML 的 PR 在分支上跑 nightly 并通过；每个 commit 的代码、测试、文档和 task 自洽，索引状态一致。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | `cargo test -p panta-ffi object_tree` | 资产增删、切面开关、字段切换、时间步出现 / 消失、动画、质量节点结构正确 | 7 项通过 |
| 2026-10-24 | `panta_qml_shell_module_test`（offscreen） | 单击只选中不切页签；`activateNode` / Enter 激活；Down / Up / Left / Right 浏览与折叠展开（TreeView `rows` 3→1→3）；隐藏资产后视口 `meshVisible=false`、切换页签按资产恢复、dirty 不变；属性面板各节点 / 页签控件在侧栏内 | 16 项通过 |
| 2026-10-24 | 离屏截图 `QSG_RHI_BACKEND=software` | 对象树、切面属性页、资产信息页、隐藏资产弱化 | `/workspace/shots/037/sidebar-1280x720-dpr1.png`、`sidebar-1600x900-dpr1.png`、`tree-assets-info.png`、`sidebar-collapsed.png` |
| 2026-10-24 | `cargo format`、`cargo lint && cargo quality`、`cargo test --locked --workspace` | 全部通过 | 见 PR CI |
| — | 真实 GPU / Windows / macOS 窗口 | 原生视口与树、属性面板交互 | 未执行（Xvfb / offscreen 不能替代），保留 implemented |

## 风险与回退

主要风险是原生视口层与 QML 布局的交互，以及 Xvfb 无法替代真实 GPU 验收。每个 PR 独立可回退（revert squash commit），不改工程文件格式，不影响用户已有工程数据。

## 决策与工作记录

- 2026-10-24：按 034 用户决定登记任务。
- 2026-10-24：可见性只作会话显示状态（不持久化），不改 dirty；切面节点的眼睛等同属性页的切面开关（`setSectionEnabled`），不另设状态。
- 2026-10-24：资产节点暂不按网格类型区分图标（只用文字与粗体标记活动资产），避免为图标新增文档外决策。
- 2026-10-24：旧侧栏「时序动画」区块删除后，打开 / 关闭表面序列入口移到「文件」菜单与工程节点属性页；体网格生成保持在「网格」菜单，体网格信息进资产信息页。
- 2026-10-24：表示方式 / 颜色 / 透明度不在本任务范围，「显示」页就地说明。选中质量诊断节点时展开底部「诊断」页。

## 完成摘要

对象树（Rust 生成、Bridge `ObjectTreeModel` 投影）与上下文属性面板替换旧侧栏区块；选中 / 激活分离，键盘可浏览；可见性只影响显示。剩余真实 GPU / 跨平台窗口验收。
