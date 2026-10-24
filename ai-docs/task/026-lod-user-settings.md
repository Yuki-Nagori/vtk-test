# 026 — LOD 触发阈值与输出预算设为用户设置

- 状态：planned
- 阶段：网格
- 依赖：[019](019-large-mesh-lod.md)
- 优先级：P2
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

019 中 LOD 触发阈值（2000 万面）和输出预算（200 万面）是常量。本任务把两者做成用户可设置项，默认值不变，持久化保存，并做范围校验。

## 必读

`crates/panta-core/src/activation.rs`、`native/bridge/src/settings.*`、[019](019-large-mesh-lod.md)。

## 范围与非目标

- Rust：
  - `LodPolicy { activation_triangles, budget_triangles }`，默认值为 2000 万和 200 万。
  - 校验在 Rust：阈值 100 万–20 亿；预算 10 万–2000 万，且预算 < 阈值。不合法时返回 `project.lod_settings_invalid`。
  - `ProjectService::set_lod_policy`：只影响之后的激活，已驻留文档不重算。
- 桥接：Settings 用 QSettings 持久化两个值，启动时读取并下发 Rust；读取到非法值时回落默认值。
- QML：设置对话框新增“大网格 LOD”分组：两个数值输入（单位：万面）、“恢复默认”按钮和范围提示。
- 非目标：按文档单独设置，以及修改后自动重新激活。

## 验收标准

- [ ] 校验边界（Rust 单测）；激活使用设置值（Rust 单测）。
- [ ] Settings 持久化往返、非法值回落（C++ 测试）。
- [ ] 全量检查通过；实机设置界面（需 GUI）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| — | 待实施 | — | 未执行 |

## 清理与兼容例外

删除常量 `LOD_ACTIVATION_TRIANGLES`；`DEFAULT_LOD_TRIANGLE_BUDGET` 保留为默认值来源。无兼容例外。

## 风险与回退

阈值过低会让中等网格也走 LOD；有下限校验，并提供“恢复默认”。

## 决策与工作记录

- 2026-10-24：经用户批准创建任务。

## 完成摘要

未完成。
