# 024 — 任意平面切面

- 状态：done
- 阶段：网格
- 依赖：[021](021-volume-mesh.md)、[022](022-volume-background-tasks.md)
- 优先级：P2
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

021 只支持 X/Y/Z 轴向切面。本任务支持由法向和偏移确定的任意平面，UI 可调，并保留 X/Y/Z 快捷预设。

## 必读

`crates/panta-mesh/src/volume.rs`（`section_surface`）、[022](022-volume-background-tasks.md)。

## 范围与非目标

- Rust：
  - 平面参数为单位化后的法向 n 和偏移 d。偏移按体网格包围盒在 n 上的投影区间做 0..1 归一化，滑块语义与轴向一致。
  - 法向为零或非有限时返回 `project.command_invalid`。
- FFI 和 ViewModel：
  - 切面请求改为（nx, ny, nz, fraction）。
  - 轴向预设只是把法向设为坐标轴。
- QML：X/Y/Z 预设按钮；法向三个分量可编辑（SpinBox，步长 0.05，范围 −1..1）；偏移滑块。
- 非目标：视口内拖拽平面控件（需要 VTK widget 和 GPU 验证，另立任务）、多切面。

## 保守决策

- 平面参数为（法向、投影区间位置）；法向在 Rust 侧单位化，长度不影响结果。偏移按节点在单位法向上的投影区间归一化，轴向法向时与 021 的包围盒语义一致（单测按位相等）。
- 保留 `dot(p − origin, n) ≤ 0` 的一侧（沿用 021 的 `section_surface`）；法向取反即翻转保留侧。
- 校验分两层：ViewModel 在启用时拒绝零法向和非有限值（`project.command_invalid`），已有平面保持不变；Rust 再做同样校验，作为 FFI 边界的兜底。关闭切面时不校验参数，只保留有效的平面供下次启用。
- QML：X/Y/Z 预设只是把法向设为坐标轴，当前法向与预设一致时高亮。SpinBox 内部用整数（×100）表示 −1..1、步长 0.05；用户编辑后重新绑定到模型值，零法向被拒时回显原值。
- 法向分量编辑后立即重算切面，沿用 022 的“合并最新意图”，连续编辑不会堆积后台任务。

## 验收标准

- [x] 斜切面与轴向切面在轴向法向下结果一致；零法向被拒绝（Rust 单测）。
- [x] ViewModel 法向属性往返（C++ 测试）。
- [x] 全量检查通过。
- [ ] 实机交互：预设按钮、法向 SpinBox、偏移滑块（需 GUI/GPU，未在 box 验证）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | box：`cargo test -p panta-core volume` | 轴向法向结果与直接构造平面按位相等，缩放法向结果不变；斜平面所有顶点在负侧、存在切口顶点；零法向和 NaN 被拒绝 | 通过 |
| 2026-10-24 | box：`cargo test -p panta-ffi section` | 合法法向成功、零法向报错、取消无错误 | 通过 |
| 2026-10-24 | box：`panta_bridge_project_view_model_test --gtest_filter=*VolumeQuality*` | 法向属性往返；零法向和 NaN 被拒绝且不改已有平面；斜平面 y+z=0.75 切口正确 | 通过 |
| 2026-10-24 | box：`cargo format`、`cargo lint clippy --check`、`cargo lint qmllint --check`、`cargo test --locked --workspace`、offscreen 启动 | 全部通过 | 通过；CTest 66/66；启动退出码 124（超时结束，符合预期） |
| — | 实机 GUI/GPU | 预设、SpinBox、滑块交互 | 未执行 |

## 清理与兼容例外

删除仅轴向的 `section_along_axis` 和 `sectionAxis` 属性，由法向接口替代。无兼容例外。

## 风险与回退

切面仅用于显示，回退无数据影响。

## 决策与工作记录

- 2026-10-24：经用户批准创建任务。
- 2026-10-24：实施完成。`section_along_axis` 改为 `section_plane`；FFI `section_mesh_display` 参数改为（nx, ny, nz, fraction）；`sectionAxis` 属性替换为 `sectionNormalX/Y/Z`；`setSection` 签名改为 `(enabled, nx, ny, nz, fraction)`。

## 完成摘要

体网格切面支持任意平面：法向可编辑，偏移按法向投影区间做 0..1 归一化，保留 X/Y/Z 预设。仅轴向的接口已删除。实机交互待 GUI 验证。
