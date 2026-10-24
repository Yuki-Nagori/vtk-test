# 023 — 工作台命令：STEP → Netgen 生成体网格

- 状态：done
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

实施中的取舍（2026-10-24）：

- 记录形态：未新增来源种类枚举，而是在 `ImportRecord` 上加可选的 `generation`（`generator`、`source_step_name`、`source_step_digest`、`max_length_mm`），`format` 记为 `step`，`mesh_type` 为 `solid-3d`，`parser_version` 为 0。`IMPORT_RECORD_VERSION` 不变（仍为 4）：字段带 serde 默认值，旧工程无该字段时按普通导入解析，旧版本读新工程时忽略该字段。
- 不复制来源 STEP，只记录文件名和流式计算的 SHA-256；删除源 STEP 后工程仍可重开（单测覆盖）。
- 取消：STEP 读取（OCCT）本身不可中断，取消标志在进入 Netgen 时立即生效；Netgen 各阶段通过 `multithread.terminate` 协作取消，粒度取决于 Netgen 内部检查点；监视线程每约 20 ms 轮询一次。取消优先于 Netgen 返回码。提交阶段沿用导入的取消语义，越过清单提交闸门后不可取消。
- 生成期间工程被切换或关闭时丢弃结果，不写入新工程。
- 生成复用导入的提交槽位和轮询（`m_importRequest` / `m_stlImportPoll`），因此生成与导入、导入预览互斥。
- STEP 转换不完整（部分实体失败）时拒绝生成，不生成部分网格。
- 进度：native 阶段占 0–0.9（边 0–0.2、面 0.2–0.5、体 0.5–0.85、转换 0.85–0.95），Rust 提交（哈希与写资产）占 0.9–1.0。
- `max_length_mm` 为 0 表示 Netgen 默认；上限 1e6 mm，负值、非有限值在提交后台作业前拒绝。

- Netgen 全局锁不变，同一时刻只允许一个生成任务。
- 取消在 Netgen 检查点生效；越过清单提交闸门后不可取消。
- 生成失败不写清单，不留资产。

## 验收标准

- [x] Rust：生成记录写入、重开工程后激活为体网格、取消不留资产（单测）。
- [x] native：带进度回调的生成在 box 上报告单调进度；预先取消返回取消状态（CTest）。
- [x] ViewModel：从 STEP 夹具生成后文档可显示、`volumeAvailable` 为真（C++ 测试）。
- [x] format、clippy、qmllint、全量测试通过。
- [ ] 实机：菜单和对话框交互、生成过程中界面不卡、大模型上取消的响应时间（需 GUI，未在 box 验证）。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | box：`cargo test -p panta-core generated` | 记录写入与重开、参数/空网格/缺 STEP 拒绝、取消不留资产 | 3 项通过 |
| 2026-10-24 | box：`panta_mesh_netgen_test` | 进度单调且以 1.0 结束；立即取消与中途取消（maxh 1.0，进度 ≥0.5）返回 `kCancelled`；取消后再次生成恢复（体积≈6000） | 7 项通过，1.1 s |
| 2026-10-24 | box：`panta_bridge_project_view_model_test --gtest_filter=*GeneratesVolume*` | 无工程/非法参数/缺文件拒绝；立即取消得 `project.volume_generation_cancelled` 且不发布；正常生成发布 `box_mm` 为 `solid-3d`，`volumeAvailable` 为真 | 通过 |
| 2026-10-24 | box：`panta_qml_theme_component_test` | 生成对话框可加载；缺文件提交被拒并保持打开 | 14 项通过 |
| 2026-10-24 | box：`cargo format`、`cargo lint clippy --check`、`cargo lint qmllint --check`、`cargo test --locked --workspace` | 全部通过 | 通过；CTest 66/66 |
| 2026-10-24 | box：`QT_QPA_PLATFORM=offscreen timeout 8 target/native/debug/app/panta` | 启动无报错 | 退出码 124（超时结束，符合预期） |
| — | 实机 GUI | 菜单、对话框、状态栏进度与取消、大模型响应 | 未执行（需 GUI） |

## 清理与兼容例外

无废弃项；工程 manifest 新增可选字段，旧工程无该字段时按普通导入解析。

## 风险与回退

- bridge 链接 OCCT/Netgen 会增加 app 的运行期依赖。macOS/Windows 的部署由 nightly 验证，失败时回退为仅 Linux 启用并登记。
- Netgen 中途终止可能留下全局状态；沿用 010 的失败恢复测试。

## 决策与工作记录

- 2026-10-24：经用户批准创建任务。
- 2026-10-24：实施完成，取舍见“保守决策”。附带修正：`panta-cli` 的 `CommandOutput::StlImport` 改为装箱（`ImportRecord` 增大后触发 clippy `large_enum_variant`）；`sha2` 从 dev-dependency 移到 `panta-core` 依赖（Cargo.lock 不变）；`NativeTetMeshDto → TetMeshData` 转换从 `mesh_ir.cpp` 匿名命名空间提升为 `panta/mesh/rust_dto.hpp` 供桥接复用。

## 完成摘要

新增“文件 → 从 STEP 生成体网格…”：后台读取 STEP，用 Netgen 生成四面体网格（带进度和协作取消），再经 Rust 提交为 `solid-3d` 体网格资产，记录来源 STEP 的文件名、SHA-256 和生成参数。生成结果复用 021/022 的显示、切面、诊断和 VTU 导出路径。未完成项：实机 GUI 交互和大模型取消响应（需 GUI）；app 链接 OCCT/Netgen 后在 macOS/Windows 上的运行期打包，待 nightly 确认。
