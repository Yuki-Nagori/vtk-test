# 023 — 工作台命令：STEP → Netgen 生成体网格

- 状态：planned
- 阶段：网格
- 依赖：[021](021-volume-mesh.md)、[022](022-volume-background-tasks.md)
- 优先级：P1
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

Netgen 适配（task 010）和体网格链路（task 021）都已具备，但工作台里没有入口：用户无法从 STEP 生成体网格并放进工程。本任务新增用户命令和对应的工程记录类型，生成结果走与导入体网格相同的显示、切面、诊断和导出路径。与 [008](008-step-project-workflow.md) 的关系：本任务只做“STEP → 体网格资产”，STEP 本身的三角化显示和几何资产仍属于 008。

## 必读

`native/mesh/include/panta/mesh/netgen_mesher.hpp`、`native/geometry/include/panta/geometry/step_import.hpp`、[Netgen 规范](../standards/netgen.md)、[分层规则](../standards/layering.md)、[UI 与桥接架构](../architecture/ui-and-bridge.md)。

## 范围与非目标

- 工程记录：`ImportRecord` 新增来源种类 `generated` 和生成信息，包括来源 STEP 的文件名与 SHA-256、生成器 `netgen`、`max_length_mm`；资产 kind 为 `volume`，单位为毫米。不复制来源 STEP（与表面导入一致：资产自包含，不依赖源文件）。
- Rust：`ProjectService::begin_generated_volume` / `finish_generated_volume`（与导入相同的提交闸门、`.partial` 资产与清单原子提交），接收 native 生成的 `TetMeshData`。
- native：`generate_tet_mesh` 增加进度回调和取消标志。阶段进度：STEP 读取、边、面、体、转换。取消通过 Netgen `multithread.terminate` 协作生效，生成结果被丢弃。
- 桥接：bridge 链接 `panta::mesh`；ViewModel `beginGenerateVolumeFromStep(url, maxLength)` 在后台线程执行，状态栏显示进度并提供取消。
- QML：菜单“文件 → 从 STEP 生成体网格…”，对话框选择 STEP 和最大单元尺寸（0 表示 Netgen 默认）。
- 非目标：STEP 表面显示、几何资产持久化、局部加密、二次单元、多实体装配逐体参数。

## 保守决策

- Netgen 全局锁不变，同一时刻只允许一个生成任务。
- 取消在 Netgen 检查点生效；越过清单提交闸门后不可取消。
- 生成失败不写清单，不留资产。

## 验收标准

- [ ] Rust：生成记录写入、重开工程后激活为体网格、取消不留资产（单测）。
- [ ] native：带进度回调的生成在 box 上报告单调进度；预先取消返回取消状态（CTest）。
- [ ] ViewModel：从 STEP 夹具生成后文档可显示、`volumeAvailable` 为真（C++ 测试）。
- [ ] format、clippy、qmllint、全量测试通过。
- [ ] 实机：菜单和对话框交互、生成过程中界面不卡（需 GUI）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| — | 待实施 | — | 未执行 |

## 清理与兼容例外

无废弃项；工程 manifest 新增可选字段，旧工程无该字段时按普通导入解析。

## 风险与回退

- bridge 链接 OCCT/Netgen 会增加 app 的运行期依赖。macOS/Windows 的部署由 nightly 验证，失败时回退为仅 Linux 启用并登记。
- Netgen 中途终止可能留下全局状态；沿用 010 的失败恢复测试。

## 决策与工作记录

- 2026-10-24：经用户批准创建任务。

## 完成摘要

未完成。
