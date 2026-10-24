# 033 — 播放控制与时序结果场

- 状态：done
- 阶段：PR 1（任务文档 + 时间步存储 + OpenFOAM / CGNS 多时间步导入）已合并（#47）；PR 2（VTU `.pvd` 时间集合）已合并（#48）；PR 3（统一播放工具栏 + 切面随时间更新）见本文完成摘要
- 依赖：[020](020-time-series-animation.md)、[029](029-result-fields.md)、[032](032-openfoam-cgns-import.md)
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

用户 2026-10-24 17:35 UTC+8 新批准：播放功能与时序结果场合并做，对照 ParaView 的动画工具栏（播放、暂停、逐帧、循环、调速、时间轴），结果场和切面随时间更新，并与已有时序动画（task 020，用户口中的「#19」）整合。顺序在 032 之后、UI 重制设计稿之前。

现状（已核实）：

- 020 的 `AnimationSequence` 只播放「同前缀编号的表面网格文件」：后台解析、预取 4 帧、512 MiB LRU；侧栏有打开序列、播放 / 暂停、帧率（1–60）与帧滑块，始终循环，无首帧 / 末帧 / 逐帧按钮、无循环开关；动画期间切面、导出、诊断禁用。
- 029 的结果场每个 Mesh 资产只有一份 `.presult`（`MeshAssetRef.results`），切面着色读其中一个字段；029 文档已预留「一个网格可对应多份结果文件，适合后续时间序列」。
- 032 的 OpenFOAM 只读最新时间目录；CGNS 多个 FlowSolution 以 `解名/数组名` 前缀区分，未识别 `BaseIterativeData_t`。

完成后：导入带多个时间步的算例（OpenFOAM 时间目录、CGNS `BaseIterativeData` / `ZoneIterativeData`、VTU `.pvd` 集合）得到一个体网格 + 多个时间步的结果场；视口下方统一的播放工具栏（首帧、上一帧、播放 / 暂停、下一帧、末帧、循环开关、帧率、时间轴滑块、时间值标签）驱动「结果场时间步」或 020 的「表面文件序列」；切面开启且选了字段时，切面颜色与色标随时间步更新。

## 必读

- [网格架构](../architecture/mesh.md)「结果场」「OpenFOAM 导入」「CGNS 导入」各节；[020](020-time-series-animation.md)、[029](029-result-fields.md)
- [Rust 规范](../standards/rust.md)、[C++ 规范](../standards/cpp.md)、[QML 规范](../standards/qml.md)、[测试规范](../standards/testing.md)
- [提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)
- 外部参考（2026-10-24 查阅）：ParaView 5.13 用户指南「Animation View / VCR Controls / Current Time Controls」（首帧、上一帧、播放、下一帧、末帧、循环；时间值 / 时间步下标两种输入；Snap To TimeSteps / Sequence / Real Time 三种模式）；`vtkOpenFOAMReader` 时间目录发现；CGNS SIDS `BaseIterativeData_t`（`NumberOfSteps`、`TimeValues`）与 `ZoneIterativeData_t`（`FlowSolutionPointers`）；ParaView `.pvd`（`<Collection><DataSet timestep= file=>`）。

## 对照评估

| 能力 | ParaView | EnSight / Tecplot / STAR-CCM+ / Fluent | 本任务 |
|---|---|---|---|
| VCR：首 / 上 / 播放 / 暂停 / 下 / 末 | 有 | 都有（EnSight Solution Time 面板、Tecplot Animate Time、STAR-CCM+ Solution History 播放器） | **有**：统一工具栏 |
| 循环开关 | 有 | 有 | **有**（默认开，与 020 现状一致） |
| 调速 | Sequence 模式帧数 / Real Time 时长 | 帧率或帧间延迟 | **帧率 1–60 fps**（沿用 020）；不做「按物理时间实时播放」 |
| 时间轴 | 时间值 + 下标，Animation View 轨道 | 时间滑块 | **滑块 + 「步 i / N，t = 值」标签**；不做关键帧轨道 |
| 时间步来源 | 文件序列、`.pvd`、reader 内时间 | 求解器原生瞬态文件 | OpenFOAM 时间目录、CGNS BaseIterativeData、`.pvd`、020 表面序列 |
| 色标范围随时间 | 默认固定，可「按当前时间」/「全时段」重设 | 多为固定，可重设 | **默认全部时间步的全局范围**（由 sidecar 头部摘要直接算出，不额外读场）；可切换为「当前步」 |
| 网格随时间变形 / 拓扑变化 | 支持 | 支持 | 非目标：所有时间步共享同一网格（`.pvd` 拓扑不一致整体拒绝） |
| 动画导出视频 / 图片序列 | 有 | 有 | 非目标（候选 task） |

## 范围与非目标

范围：

1. **时间步存储**：`MeshAssetRef` 新增可选 `result_steps: [{ time, label, results: ResultAssetRef }]`（≥ 2 项时写入）；每步一份不可变 `.presult`（`assets/results/<asset>-r1-s<NNNNN>.presult`）。`results` 继续指向最后一步（与 `result_steps` 末项引用同一文件），因此派生 `.pchunk`、out-of-core 路径、旧读者与无时间步工程的行为不变；清单格式版本不升级（字段可选，旧清单缺省为空）。
2. **导入**：`RawGrid` 新增 `time_steps`（每步 time / label / 字段）；
   - OpenFOAM：收入全部数值时间目录中的 volScalar / volVector 场；只在有场的时间目录成步；仅 1 步时与 032 行为一致；
   - CGNS：有 `BaseIterativeData_t/TimeValues` 且各 Zone 的 `ZoneIterativeData_t/FlowSolutionPointers` 指向的解存在时按步组织（不再加前缀）；否则保持 032 的前缀行为；
   - VTU `.pvd`（PR 2）：读集合中各 `DataSet`（`part` 只接受单一值），最新一步给出网格，其余各步解析后必须与之等价（点坐标按位、连接、类型），否则整体拒绝；
   - 内存预算：除最新步外的场值总量超过 `MAX_TIME_STEP_VALUES`（2^28 个 f64 = 2 GiB）时只导入最新步，并在 `skipped_result_fields` 记一条说明（保守，避免导入时爆内存）。
3. **服务 API**：`result_time_steps(import_id)`（time / label / 每步字段摘要）与按步的 `result_field_source_at(import_id, step)`；单字段缓存键已含结果摘要，跨步天然区分。
4. **播放（PR 3）**：VM 把 020 的 `animation*` 属性替换为统一的 `playback*`（来源：`sequence` 或 `timeSteps`），新增首帧 / 末帧 / 循环开关；QML 新建 `PlaybackBar`（`qml/Panels/PlaybackBar.qml`）放在视口下方，替换侧栏的 020 动画控件；结果场时间步播放时切面按当前步读取字段重算，目标帧未就绪时保持上一帧（与 020 一致，不阻塞 GUI）；播放期间 export / diagnose 禁用规则沿用 020；时间步播放允许切面（这是目的）。
5. **out-of-core / 流式体网格**：分块派生缓存只嵌入最新步，时间轴禁用并提示「分块体网格仅显示最新时间步」（保守）。

非目标（不实施，记为候选）：网格随时间变化、物理时间实时播放模式、关键帧动画轨道、动画导出视频 / 图片序列、时间插值（相邻步之间线性插值）、表面着色（029 只给切面上色，表面场着色另立 task）、OpenFOAM 分区算例时间步、CGNS 网格运动（`RigidGridMotion` / `ArbitraryGridMotion`）。

## 涉及分组与跨组依赖

panta-io（格式解析）、panta-core（工程模型 / 导入 / 结果服务）、panta-ffi（显示输入与切面请求）、native/bridge（VM）、qml。无新增跨组依赖，无新增第三方依赖。

## 前置条件与待决策

- 032 已合并（PR #46）后从 main 开分支。
- 待决策（本文档自行按保守方案定，记录在决策记录）：色标默认全局范围；内存预算；`.pvd` 拓扑一致性按解析值比较（见决策记录）。

## 实施步骤

1. PR 1：本文档 + `ResultStepRef` 模型 / 清单校验 / 导入提交多份 sidecar（失败回滚全部新文件）+ `RawGrid.time_steps` + OpenFOAM 多时间目录 + CGNS BaseIterativeData + 服务 API + 测试（重开工程、篡改某步 sidecar 被拒、单步算例行为不变）。
2. PR 2：`.pvd` 集合：格式检测、快照（集合文件 + 引用的 VTU 按固定顺序打包）、拓扑一致性校验、测试与夹具。
3. PR 3：FFI 时间步列表与按步切面请求；VM `playback*` 统一状态机（替换 020 属性）；`PlaybackBar.qml`；C++ / QML 测试；nightly。

## 预计改动

- `crates/panta-io/src/import/formats/{mod.rs,openfoam/*,cgns/*,vtu*}`、`crates/panta-io/src/import.rs`
- `crates/panta-core/src/project/{model.rs,import.rs,results.rs,service.rs}`、清单校验
- `crates/panta-ffi/src/{lib.rs,display.rs}`、`native/bridge/src/project_view_model.{hpp,cpp}`、`qml/App.qml`、`qml/Panels/PlaybackBar.qml`
- 文档：`ai-docs/architecture/mesh.md` 结果场一节、020 / 029 文档交叉引用
- 持久化：清单新增可选字段；旧工程可读、新工程被旧版本读取时忽略未知字段只看 `results`（最新步）。

## 清理与兼容例外

PR 3 删除 VM 的 `animation*` 属性与 invokable、侧栏 020 动画控件，调用点与测试一并改为 `playback*`；无兼容例外。

## 验收标准

- [x] OpenFOAM 多时间目录算例导入后 `result_time_steps` 按时间升序给出全部步，最新步与 032 单步导入结果逐值相等；单时间目录算例清单不出现 `result_steps`。
- [x] CGNS 带 BaseIterativeData 的文件按步导入，字段名不带解名前缀；无 BaseIterativeData 的多解文件行为与 032 相同。
- [x] 任一步 sidecar 摘要不符时读取该步被拒，其余步与网格不受影响；导入中途失败不残留新 sidecar。
- [x] `.pvd` 集合导入；拓扑不一致整体拒绝并给出明确错误。
- [x] 播放工具栏：首 / 上 / 播放 / 暂停 / 下 / 末 / 循环 / 帧率 / 滑块在两种来源下都可用；循环关闭时播放到末帧自动暂停；切面 + 字段时颜色与色标随步更新；分块体网格时间轴禁用并提示。
- [x] `cargo format` / `cargo quality` / `cargo test --locked --workspace` / CTest / offscreen 启动通过；PR 3 nightly 绿。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | PR 1：`cargo test -p panta-io`（OpenFOAM transient 4 时间目录、CGNS transient 3 步、单步 / 超预算 / 坏字段分步记录） | 通过 | 通过 |
| 2026-10-24 | PR 1：panta-core `results::tests`（多步 sidecar 路径、重开、篡改单步、清单 4 种非法改写被拒、单步不写 `result_steps`、CGNS NaN）+ `time_series_sanitizing_*` | 通过 | 通过 |
| 2026-10-24 | PR 1 合并（#47，CI 与 nightly 绿） | — | 合并于 18:37（UTC+8） |
| 2026-10-24 | PR 2：`cargo test -p panta-io pvd`（乱序 timestep 排序、逐步字段集合、拓扑变化拒绝、集合文件 8 种非法情形、单步 / 超预算 / 条目不符、缺失 VTU）+ core `pvd_collection_imports_time_steps_and_rejects_moving_meshes` | 通过 | 通过 |

| 2026-10-24 | PR 3：panta-ffi `time_step_sections_read_the_requested_step`（`.pvd` 3 步：标签、逐步切面值落在该步范围、步越界 / 缺字段拒绝、全局范围 T [0,108]、分量包络 p [-2,2]、非法关联拒绝） | 通过 | 通过 |
| 2026-10-24 | PR 3：C++ `PlaysResultTimeStepsAndUpdatesFieldSections`（来源 `timeSteps`、默认末步、无切面时只移时间轴、循环开 / 关回绕与夹紧、切面 T 随步重算且全局色标 0..108 不变、切到「当前步」后色标收窄、60 fps 循环关闭播到末帧自动暂停）；Shell 加载测试含 `playbackBar` 且无来源时隐藏 | 通过 | 通过（CTest 70/70） |
| 2026-10-24 | PR 3：`cargo format` / `cargo quality` / `cargo test --locked --workspace` / `QT_QPA_PLATFORM=offscreen timeout 8 target/native/debug/app/panta` | 通过 / 退出码 124 | 通过 / 124 |
| 待实机 | GPU 桌面：PlaybackBar 布局、逐帧切面刷新流畅度、百万单元算例每步切面延迟 | — | 未验证（box 无 GPU） |

## 风险与回退

- 多步 sidecar 体积：每步一份完整字段；大算例磁盘占用线性增长（与 ParaView 直接读原始文件不同，本项目快照入工程）。由内存预算兜底，超限只留最新步。
- 播放卡顿：每步切面需整字段读取 + 切面插值；无跨步预取。PR 3 记录实测，必要时另立预取 task。
- 回退：`result_steps` 可选，回退代码后旧逻辑只看 `results`（最新步），工程仍可打开。

## 决策与工作记录

- 2026-10-24：创建任务，按上文保守方案定范围与 PR 拆分。
- 2026-10-24（PR 1）：OpenFOAM 只把含体场文件的时间目录算作步（`0.02/uniform` 之类不算）；更早步字段文件预算 2 GiB、场值预算 2^28 个 f64，超出只留最新步并记录。解压失败从整体失败改为单字段跳过（同一步其他字段照常）。CGNS 迭代声明不完整（缺 TimeValues / 指针长度不符）时回退 032 的前缀多解行为；未被指针引用的解跳过并记录；Null 指针填 NaN（与 032 跨 Zone 缺失规则一致）。导入层再清洗：无场的更早步省略，标签空 / 过长 / 重复改为 `#N`，时间非有限或倒序回退最新步。清单不升版本，最后一步与 `results` 共用引用。
- 2026-10-24（PR 2）：`.pvd` 拓扑一致性按解析后的值比较（坐标按位、单元表逐项），而不是原始字节——同一网格可能以不同编码 / 压缩写出，按值比较更宽松但仍严格排除网格变化。多 part（多块）集合、非 VTU 数据集、重复 timestep 均整体拒绝；任一步 VTU 损坏整体拒绝（不做部分时间序列）。对话框对 `.pvd` 默认实体 3D（表面导入无时间步），不推断单位。

- 2026-10-24（PR 3）：统一状态机 `playback*` 替换 020 的 `animation*`（`animationActive` / `openAnimationUrl` / `closeAnimation` 保留为「打开 / 关闭表面序列」入口）。来源优先级：打开序列时为 `sequence`，否则当前文档有 ≥ 2 个时间步时为 `timeSteps`。时间步默认停在最新一步（与入库的 `results` 一致）。时间步播放「显示完成再前进」：只有上一步切面已显示才推进目标步，不取消在途工作；未开切面或未选字段时逐帧只移动时间轴并提示。色标默认「全部时间步」：由各步 sidecar 头部摘要并集得出（分量取 ±最大模长包络，保守但不额外读场），可切为「当前步」（该步切面实际范围）。字段列表取最新一步；某步缺该字段或读取失败时保持上一帧、在播放状态行提示，不弹全局错误。流式（分块）体网格且有多步时禁用时间轴并提示仅显示最新步。循环默认开（保留 020 行为），关闭时播到末帧自动暂停，再按播放从首帧开始。播放栏放在视口下方而非叠加在视口上（QML 无法覆盖原生视口表面）。

## 完成摘要

- 三个 PR：#47（时间步存储 + OpenFOAM / CGNS 多步）、#48（`.pvd`）、PR 3（统一播放栏 + 切面随时间步更新 + 全局 / 当前步色标）。
- 限制：网格不随时间变化；无跨步预取与时间插值；无物理时间实时模式、关键帧轨道与视频导出；表面不着色（沿用 029）；分块体网格只显示最新步；未在 GPU 实机验证播放流畅度。
- 候选后续 task（未实施）：跨步预取 / 后台缓存、动画导出（视频 / 图片序列）、表面场着色、分块体网格多时间步、网格运动。
