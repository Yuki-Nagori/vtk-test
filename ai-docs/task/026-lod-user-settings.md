# 026 — 大网格阈值（表面 LOD 阈值、LOD 预算、体网格流式阈值）设为用户设置

- 状态：done
- 阶段：网格
- 依赖：[019](019-large-mesh-lod.md)
- 优先级：P2
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

019 中 LOD 触发阈值（2000 万面）和输出预算（200 万面）是常量，025 新增的体网格流式激活阈值（1000 万四面体）也是常量。本任务把三者做成用户可设置项，默认值不变，持久化保存，并做范围校验。体网格阈值按体单元数计（用户 2026-10-24 10:12 UTC+8 澄清：“亿级”指 CFD 体单元）。

## 必读

`crates/panta-core/src/activation.rs`、`native/bridge/src/settings.*`、[019](019-large-mesh-lod.md)。

## 范围与非目标

- Rust：
  - `LodPolicy { activation_triangles, budget_triangles }`，默认值为 2000 万和 200 万。
  - 校验在 Rust：阈值 100 万–20 亿；预算 10 万–2000 万，且预算 < 阈值。不合法时返回 `project.lod_settings_invalid`。
  - `ProjectService::set_lod_policy`：只影响之后的激活，已驻留文档不重算。
- 桥接：Settings 用 QSettings 持久化两个值，启动时读取并下发 Rust；读取到非法值时回落默认值。
- QML：设置对话框新增“大网格 LOD”分组：两个数值输入（单位：万面）、“恢复默认”按钮和范围提示。
- 体网格：流式激活阈值 100 万–100 亿体单元，默认 1000 万（025 已按基准下调）。
- 非目标：按文档单独设置，以及修改后自动重新激活。

## 验收标准

- [x] 校验边界（Rust 单测）；激活使用设置值（Rust 单测）。
- [x] 设置持久化往返、非法值回落（C++ 测试）。
- [x] 全量检查通过；[ ] 实机设置界面（需 GUI，待用户在桌面环境确认）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | `cargo test -p panta-core`：`activation_limits_validate_ranges_and_budget_below_threshold`、`activation_limits_are_validated_before_replacing_the_service_setting`，以及表面 LOD / 体网格流式测试改用协调器设置 | 边界与“预算 < 阈值”被拒；worker 使用设置值 | 通过 |
| 2026-10-24 | `cargo test -p panta-ffi limits` | 往返一致；非法值返回 `project.lod_settings_invalid`、detail 为字段名，原设置不变 | 通过 |
| 2026-10-24 | `panta_bridge_project_view_model_test --gtest_filter=*LargeMesh*` | 持久化往返；手工改坏（非数字 / 超范围）回落默认；恢复默认清除持久化 | 通过 |
| 2026-10-24 | `theme_component_test large_mesh_settings_dialog_shows_wan_and_rejects_invalid_budget`（offscreen） | 对话框以“万”显示；预算 ≥ 阈值时保持打开、设置不变 | 通过 |
| 2026-10-24 | `cargo format`、`cargo lint clippy --check`、`cargo lint qmllint --check`、`cargo test --locked --workspace`（CTest 67/67）、offscreen 启动 8 s | 全部通过 | 通过 |

## 清理与兼容例外

原计划删除常量 `LOD_ACTIVATION_TRIANGLES`；实施中改为保留它和 `VOLUME_STREAM_ACTIVATION_CELLS`、`DEFAULT_LOD_TRIANGLE_BUDGET`，作为 `ActivationLimits::DEFAULT` 的唯一来源（benchmark 和文档也引用它们），运行时只读协调器中的设置。无兼容例外。

## 风险与回退

阈值过低会让中等网格也走 LOD；有下限校验，并提供“恢复默认”。

## 决策与工作记录

- 2026-10-24：经用户批准创建任务。
- 2026-10-24：与对照工具比较后决定**做**。
  - ParaView 在 Render View 设置里提供 LOD Threshold（按几何体大小触发）和 LOD Resolution，并另有远程渲染阈值，也就是说“阈值可调”是主流做法。
  - EnSight 的显示设置可调整 LOD 及其预算。
  - Tecplot 由 SZL 按需加载，没有单一阈值。STAR-CCM+ 和 Fluent 依靠分布式内存，不是同一类机制。
  - 实现量小，且能作为 025 放弃“流式表面拓扑诊断”后的出口：用户调高表面阈值后，就能完整驻留并做拓扑诊断。
- 2026-10-24：保守实施决定：
  - 设置存放在 `ActivationCoordinator`（`Mutex<ActivationLimits>`），每次激活开始时读取一次，修改只影响之后的激活。
  - 校验只在 Rust（`ActivationLimits::validate`）；C++ 只拒绝非整数 / 负数 / 非有限值。
  - 实际 LOD 预算取 `min(预算, 阈值/2)`，保证 LOD 确实比源网格小。
  - 持久化放在 `ProjectViewModel`，写入独立的 `large-mesh.ini`（UserConfig 目录，沿用 recent-projects.ini 的模式），没有放进文档设想的 `Settings` 单例：该单例只承载界面偏好，拿不到 Rust 服务。
  - 入口是“网格”菜单 → “大网格设置…”。
  - 体网格阈值一并纳入，因为它在 025 才成为常量。
- 需 GUI 的实机验证：打开对话框，看数值显示与错误提示；修改后重新打开大网格，确认 LOD / 流式状态切换；重启后设置保留。

## 完成摘要

三项大网格阈值都可在“网格 → 大网格设置…”中修改，以“万”为单位。默认值是 2000 万面 / 200 万面 / 1000 万体单元。校验在 Rust，非法时返回 `project.lod_settings_invalid` 并指出字段。设置持久化到 UserConfig 下的 `large-mesh.ini`，读到非法值时回落默认值；“恢复默认”会清除持久化。修改只影响之后的激活。
