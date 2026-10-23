# 017 — 视口工具：预设视角、复位、拾取与测距

- 状态：implemented
- 阶段：可视化 / UI
- 依赖：[016](016-background-progress.md)
- 优先级：P1
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

视口只有方向立方体和鼠标导航，没有标准视角按钮、相机复位，也不能在模型上拾取点或测量距离。本任务补齐这些常用 CAE 视口工具。

## 必读

[UI 与桥接架构](../architecture/ui-and-bridge.md)、[分层规则](../standards/layering.md)、[提交规范](../standards/commits.md)。

## 范围与非目标

- 纯 CPU 模块 `navigation/viewport_tools.{hpp,cpp}`：`ViewPreset`、`parse_view_preset`（front/back/left/right/top/bottom/iso）、`preset_frame`（Z 向上；前视从 -Y 看向 +Y；顶/底视 up=+Y；轴测 backward=norm(1,-1,1)）；`pick_triangle_soup`（Möller–Trumbore，双面，取最近交点）。
- 后端接口 `ViewportBackend::set_view_preset(name)` / `reset_view()`；VtkViewport 复用方向立方体的平滑相机过渡，复位复用 `configure_default_camera`。
- 拾取：左键在模型区域（非方向立方体）松开时，把显示坐标反投影到近/远裁剪面构成世界射线，在当前网格快照上求交，发出 `pointPicked(hit, x, y, z, triangle)`（双精度）。
- CaeViewport：`Q_INVOKABLE setViewPreset(QString)`、`resetView()`、信号 `pointPicked`。
- QML：侧栏“视图与测量”区——7 个预设按钮 + “复位”，拾取读数（点 A/B 坐标、三角面序号、两点距离 mm）。
- 非目标：角度/面积测量、截面、标注持久化、拾取高亮标记、键盘快捷键。

## 保守决策

- 原生视口表面覆盖 QML，工具按钮与读数放在侧栏而不是视口浮层。
- 拾取为 O(n) 线性扫描，在 GUI 线程执行；百万级以下可接受，亿级网格需 BVH/分块空间索引，留给 task 019。
- 第三次拾取清空并重新开始 A 点；未命中不清除已有点，只提示“未命中”。
- 预设视角以模型包围盒中心为焦点，`reducedMotion` 语义沿用既有相机过渡实现。

## 验收标准

- [x] 预设名称解析、各视角相机帧单位正交（C++ QTest）。
- [x] 拾取命中两层三角形中最近的一层，未命中与反向射线返回空（C++ QTest）。
- [x] 全量测试、format、clippy、qmllint 通过。
- [ ] 真实窗口点击预设/复位的过渡效果、拾取坐标与屏幕位置一致（需 GPU/GUI，待实机验证）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | 本机 Linux `cargo test --locked --workspace`（CTest 62 项、qmllint）、clippy、format | 通过 | 通过 |
| 2026-10-24 | CI Linux test + quality | 通过 | 见 PR |
| — | 实机点击预设、复位、拾取测距 | 与屏幕一致 | 无 GPU/GUI 环境，未执行 |

## 清理与兼容例外

方向立方体点击改为调用共享的 `start_camera_frame`，行为不变。无兼容垫片。

## 风险与回退

拾取在超大网格上可能卡顿 GUI 线程（见保守决策）。回退即 revert。

## 决策与工作记录

- 2026-10-24：创建并实施。

## 完成摘要

新增预设视角、复位、拾取与两点测距；待实机验证交互。
