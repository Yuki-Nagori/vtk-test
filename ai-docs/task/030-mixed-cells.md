# 030 — 原生混合单元：类型 / offset 布局、拆分、资产 v4、导入导出与质量

- 状态：in-progress
- 阶段：PR 1（#37）、PR 2 已合并；PR 3（`.pchunk` v3、混合流式写入、混合基准）实施中
- 依赖：[029](029-result-fields.md)、[028](028-out-of-core-rollout.md)、[021](021-volume-mesh.md)
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

体网格 IR 目前只有线性四面体；来源 VTU / 旧版 VTK 中的 hex、wedge、pyramid 无法导入。用户 2026-10-24 15:00 UTC+8 确认方案 A（原生混合单元）：Mesh IR 增加 VTK 风格的单元类型数组与 offset 布局，支持 tet / hex / prism(wedge) / pyramid，为 polyhedron 预留；局部节点顺序写进格式说明；资产格式与 `.pchunk` 升版本、旧派生缓存自动重建；切面、体积、质量计算时临时拆成四面体但保持原单元 ID；质量统计按单元类型（参考 Verdict / ParaView）；基准用棱柱边界层 + 四面体或六面体核心的合成混合网格。按 3–4 个 PR 拆分。

## 必读

- [网格架构](../architecture/mesh.md)（「原生混合单元」一节为格式说明）
- [资产激活](../modules/asset-activation.md)、[性能](../modules/performance.md)
- [提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)
- VTK 9.7 头文件 `vtkCellType.h`、`vtkWedge.h`、`vtkPyramid.h`、`vtkHexahedron.h`（本地 SDK `target/panta-deps/sdk`，2026-10-24 查阅）
- Verdict 库手册（scaled Jacobian、aspect ratio 等指标定义；PR 2 引用）

## 范围与非目标

- PR 1：`panta-mesh` 的 `CellType` / `CellLayout`、共形拆分表、`volume_mesh_from_cells`、`cell_volumes`、校验、资产 v4 读写与分块流式读取（`visit_cell_tets`）；结果场单元数改按原始单元计。
- PR 2：VTU / 旧版 VTK 读取 10/12/13/14（42 明确报错），VTU 原生单元导出，单元场按原单元对应，按类型的诊断与质量统计，FFI / QML 展示类型计数。
- PR 3：`.pchunk` v3 带原单元 ID（旧 v2 派生缓存因版本不符自动重建），分块质量与切面，混合流式写入，合成混合网格基准（读次数、字节、打开文件数）。
- 非目标：polyhedron 实际支持（需面流布局，另立任务）、二次单元、非共形（悬挂节点）网格、Netgen 输出混合单元。

## 涉及分组与跨组依赖

`panta-mesh`（IR、资产）、`panta-io`（导入导出，PR 2）、`panta-core`（工程 / 激活）、`panta-ffi` 与 QML（PR 2）。不新增跨组依赖。

## 前置条件与待决策

- 已决定：拆分不落盘、加载时确定性重算；混合资产用固定 40 B 记录（便于分块与流式），polyhedron 将来用独立流；`.presult` 容器保持版本 3，已有结果 sidecar 不失效；v3 资产继续可读，新写一律 v4。
- 已决定（PR 2）：全部类型给 Verdict 风格缩放雅可比（劣质阈值 0.2，取 Verdict 各类型可接受下限中最宽松者），四面体保留最小二面角与长宽比且只统计原生四面体；体素导入转为六面体；PR 3 前混合资产不建 `.pchunk`（PR 3 已取消此限制）。
- 已决定（PR 3）：`.pchunk` v3 沿用 v2 的 `global_cells` 字段改记原始单元 ID（每拆分四面体一项，差分编码，纯四面体不变）；混合块追加块内原始单元布局（类型 + 局部连接），不存区域与 offset（由类型推出）；单元场按块内原始单元存值。k-d 划分按原始单元，不把一个单元拆到两块。

## 实施步骤

1. PR 1：IR + 拆分表 + 资产 v4 + 测试 + 格式文档。
2. PR 2：导入导出、单元场、诊断与质量、FFI / QML。
3. PR 3：`.pchunk` v3、流式路径、混合基准与 nightly。
4. PR 4（视需要）：收尾（GUI 文案、文档）。

## 预计改动

- PR 3：`panta-mesh` `chunked/{format,build,cache,volume,mod}.rs`、新增 `chunked/mixed_tests.rs`、`asset/stream.rs`（`VolumeSource::cells`、混合流式写出）、`synthetic.rs`（`SyntheticMixedBlock`）、新增 `examples/mixed_bench.rs`；`panta-core` `derived_chunks.rs`（取消混合跳过、单元数核对、内存估算）与激活测试；`nightly.yml` 加小规模混合基准。

- PR 2：新增 `crates/panta-mesh/src/cell_quality.rs`；修改 `panta-io` 导入（`RawGrid` 改为类型 + 连接数组）、VTU 导出（驻留 / 流式）、`panta-mesh` 质量与场剖面、校验消息、分块质量累加器、`panta-core` 派生分块跳过混合资产、FFI `VolumeInfoResult` / `MeshQualityReport` 新字段、`project_view_model.cpp`、`App.qml`。
- PR 1：新增 `crates/panta-mesh/src/cells.rs`、`mixed_tests.rs`；修改 `lib.rs`、`volume.rs`、`asset/{mod,stream,tests}.rs`、`synthetic.rs`；`panta-core` 结果场单元计数；所有 `TetMesh` 字面量增加 `cells: None`。
- 持久化：Mesh 资产头部版本 3 → 4；纯四面体资产目录与摘要不变。

## 清理与兼容例外

- COMPAT：v3 Mesh 资产只读（`is_readable_mesh_asset_version` 与 `open_layout`），新写入一律 v4；两者块布局一致，读取开销为零，暂不设删除任务（v3 为现存工程的主要格式）。
- 无废弃项。

## 验收标准

- [x] PR 1：hex 全部 40320 种节点标号、wedge / pyramid 全部标号拆分为正向四面体且体积正确；镜像输入自动修正；节点重复、polyhedron、offset 不符被拒绝。
- [x] PR 1：混合网格（hex + pyramid + wedge + tet）构建、校验、按单元体积、外表面正确；篡改区域或拆分顺序时校验失败。
- [x] PR 1：v4 混合资产完整读与流式读往返；混合流在 v3 头部下被拒绝；v3 纯四面体资产仍可读；`VolumeAssetChunks` 给出与驻留一致的拆分四面体与单元 ID。
- [x] PR 2：VTU / 旧版 VTK 混合导入导出往返（含体素、42 与类型 / 节点数不符拒绝），驻留与流式导出逐字节一致；单元场长度、场剖面单元值、校验消息按原单元；按类型缩放雅可比（理想单元 = 1，反转 < 0），驻留与流式质量一致；FFI / QML 显示原始单元数、类型构成与按类型质量。
- [x] PR 3：`.pchunk` v3 带原始单元 ID；从混合资产与驻留混合网格构建的文件逐字节一致；分块切面 / 单元场切面 / 逐类型质量与驻留算法一致；v2 文件被拒绝（派生缓存重建）；流式写出混合资产与 `encode_mesh_asset` 逐字节一致且拒绝非法记录；合成混合网格基准（棱柱边界层 + 金字塔 / 四面体过渡 + 六面体核心）。
- [ ] PR 3：Windows / macOS nightly 混合小基准通过（合并前确认）。
- [ ] GUI / GPU 实机验收（混合网格切面显示与类型统计）待人工。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | `/workspace/vtk-test`，`cargo test -p panta-mesh --locked` | 全部通过 | 75 passed（含 6 个混合单元新测试与拆分表穷举测试） |
| 2026-10-24 | `cargo quality`（fmt + clippy + qmllint） | 通过 | 通过 |
| 2026-10-24 | `cargo test --locked --workspace`（Ninja，含 CTest） | 全部通过 | 全部通过，CTest 69/69 |
| 2026-10-24 | `QT_QPA_PLATFORM=offscreen timeout 8 target/native/debug/app/panta` | 启动后被 timeout 结束（124） | 124 |
| 2026-10-24 | PR 2：`cargo quality`；`cargo test --locked --workspace` | 通过 | 通过；新增混合导入导出 2 项、按类型质量 / 单元对应 2 项、缩放雅可比 2 项；体素夹具由“拒绝”改为“按六面体导入”；CTest 69/69 |
| 2026-10-24 | PR 2：offscreen 启动 | 124 | 124（GUI 中类型统计与诊断面板显示待实机确认） |
| 2026-10-24 | PR 3：`cargo test -p panta-mesh chunked::mixed_tests`、`cargo test -p panta-core mixed_assets` | 通过 | 4 + 1 项新测试通过 |
| 2026-10-24 | PR 3：`mixed_bench --cells 2M --verify`（auto=mmap） | 分块与驻留一致 | `verified:true`，见下方基准 |
| 2026-10-24 | PR 3：`mixed_bench --cells 20M`，pread / mmap，预算 1024 / 2048 MiB | 记录数据 | 见下方基准 |
| 2026-10-24 | PR 3：`cargo quality`；`cargo test --locked --workspace`；offscreen 启动 | 通过 / 124 | 通过（panta-mesh 83 项、panta-core 124 项，CTest 69/69）；124 |

## 风险与回退

- 拆分表错误会导致非共形或负体积：穷举测试覆盖全部节点标号；校验在加载时重算拆分并比对。
- v4 头部旧版本程序无法读取：只影响降级使用；纯四面体资产可把头部版本改回 3 恢复（目录摘要不含版本）。

### PR 3 基准（盒子 8 核，15 GiB 内存与其他任务共享；文件可能在 OS 页缓存中，冷缓存指块 LRU 冷）

合成网格 `SyntheticMixedBlock`：底 / 顶各 3 层棱柱边界层（厚度 0.05h × 1.5ⁱ），各 1 层金字塔 + 四面体过渡层（每个立方体 5 金字塔 + 2 四面体，与相邻棱柱三角面共形），中间六面体核心；内部节点 xy 扰动 3% h。`--cells 20M`：原始单元 20,086,164（tet 425,104 / hex 17,322,988 / wedge 1,275,312 / pyramid 1,062,760），拆分四面体 110,314,488，节点 18,604,340。块上限 262144 原始单元。

| 阶段（20M，pread） | 耗时 | IO |
|---|---|---|
| 流式写出 v4 资产（484.1 MiB） | 2.41 s，峰值 RSS 118 MiB | 写调用 74，打开文件 1 |
| 从资产构建 `.pchunk` v3（128 块，787.9 MiB，带节点场 + 单元场） | 12.67 s，峰值 RSS 6.6 GiB | 资产读调用 896（484.1 MiB，打开 1 次），写调用 131 |
| 打开 | 78 ms | 读调用 3（8.8 MiB），打开文件 1 |
| 边界层 z 切面（命中 32 块，108 万三角形） | 426 ms | 读调用 5，176.6 MiB |
| 核心斜切面（命中 54 块，171 万三角形） | 723 ms | 读调用 14，335.0 MiB |
| x 轴切面（命中 18 块，80 万三角形） | 240 ms | 读调用 17，106.8 MiB |
| 单元场 cell_id 切面：边界层 / 核心斜 / x 轴 | 315 / 816 / 136 ms | 读调用 8 / 21 / 5（27.6 / 343.4 / 6.5 MiB） |
| 逐类型质量（首次 / 缓存后） | 1.57 s / 0.00 ms | 读调用 16，762.9 MiB |
| 回退：逐次扫描资产质量 / 核心斜切面 | 0.99 s / 2876 ms | 读调用 407 / 419（每次整读约 480 MiB） |

- mmap 实现数据相同（读调用 0，映射访问次数与 pread 读调用次数相同），核心斜切面 705 ms。
- 核心斜切面后的单元场切面在 1024 MiB 预算下重读 343 MiB：命中的 54 块解码后超出预算，被 LRU 淘汰；预算 2048 MiB 时同一步只读 7 次、21.6 MiB，耗时 458 ms。属已知的预算权衡，未改策略。
- 分块质量首次比逐次扫描压缩资产略慢（1.4–1.6 s 对 1.0 s，合成网格压缩率高、拆分开销在两条路径相同），但结果缓存后再次为 0；切面快 4 倍且只读命中块。
- 2M 规模（1,983,553 单元，9,938,214 拆分四面体）`--verify`：分块切面三角形集合、单元场值集合、逐类型质量与驻留算法逐位一致；构建 1.23 s、峰值 RSS 1.7 GiB。
- 构建峰值 RSS 6.6 GiB 与 `derived_chunks` 估算（每拆分四面体 48 + 4 B、每原始单元 41 B，约 6.5 GiB + 节点）吻合；超过可用内存一半时激活会跳过派生构建并回退逐次扫描，行为与纯四面体一致。

## 决策与工作记录

- 2026-10-24：创建任务（用户 15:00 UTC+8 确认方案 A）。PR 1 实现 IR、拆分表（脚本生成后内嵌常量，单元测试穷举验证）、资产 v4 与流式单元读取；`VolumeStreamCounts` 新增 `cells`，`tets` 为拆分四面体数。导入器尚未产生混合网格，导出 / 质量 / `.pchunk` 留给 PR 2 / 3。
- 2026-10-24：PR 2。导入接受 10–14（体素改写为六面体）；VTU 原生单元导出；单元场与诊断按原单元；`TetQuality` 增加 `by_type`（缩放雅可比），`tet_count` 语义改为原生四面体数（纯四面体网格不变）。为避免 `.pchunk` v2 把拆分四面体当单元（单元场错位），PR 3 前混合资产不建派生分块文件，回退逐次顺序扫描——保守选择，代价是大混合网格每次诊断 / 切面都完整读资产。流式质量对混合资产逐原始单元统计（40 B 记录），纯四面体资产仍走四面体流。

- 2026-10-24：PR 3。`.pchunk` 升 v3：按原始单元分块、`global_cells` 改记原始单元 ID、混合块带单元布局；单元场与逐类型质量按原始单元；v2 派生文件版本不符自动重建，复用时另核对原始单元数。取消 PR 2 的“混合资产不建 `.pchunk`”过渡限制。流式写出支持混合单元（`VolumeSource::cells`，默认实现为空，纯四面体来源不受影响）。新增 `SyntheticMixedBlock` 与 `mixed_bench`，nightly 在 macOS / Windows 跑 50 万单元 `--verify`。保守选择：不在 v3 中存区域（分块路径未用到），以后需要时再升版本。

## 完成摘要

未完成。
