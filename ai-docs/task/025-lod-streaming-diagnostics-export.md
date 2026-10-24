# 025 — LOD 状态下流式导出，体网格流式激活、诊断、切面与导出

- 状态：done
- 阶段：网格
- 依赖：[019](019-large-mesh-lod.md)、[016](016-background-progress.md)、[021](021-volume-mesh.md)、[024](024-arbitrary-section-plane.md)
- 优先级：P1
- 负责人：Yuki-Nagori（代理实施）
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

019 在表面 LOD 状态下拒绝诊断和导出（`project.mesh_lod_only`）。用户确认“亿级 / 十亿级”指 CFD 体网格单元数，并要求本任务覆盖体网格资产。仓库定位是查看和后处理为主。

本任务要做到两点：

- 表面 LOD 状态下按块读取完整源资产导出。
- 大体网格激活时不驻留单元，质量统计、切面、`.vtu` 导出都按需从资产流式读取。

## 必读

- `crates/panta-mesh/src/asset/stream.rs`（`SurfaceAssetChunks`、`VolumeAssetChunks`、`write_volume_asset_streaming`）
- `crates/panta-mesh/src/volume.rs`（`*_streaming`）
- `crates/panta-import/src/export_stream.rs`
- `crates/panta-core/src/activation.rs`、`crates/panta-core/src/project/volume.rs`

## 范围与非目标

- `panta-mesh`：
  - `write_volume_asset_streaming`（按 `VolumeSource` 分块生成）和 `VolumeAssetChunks`（逐块读节点 / 单元 / 外表面，校验每块 SHA-256）。
  - 合成四面体块 `SyntheticTetBlock`（n³ 立方格，每格 6 个 Kuhn 四面体，6 个边界组）。
  - 流式算法：`boundary_surface_streaming`、`tet_quality_streaming`、`section_plane_streaming`。节点驻留（24 B/节点），单元逐块扫描。
- `panta-import`：
  - `export_surface_asset_with_progress`：STL 二进制与内存导出逐字节一致。OBJ / PLY / 旧版 VTK / VTP / VTU 未焊接，每面 3 个独立点，坐标原样。
  - `export_volume_asset_vtu`：与 `write_tet_mesh_vtu` 逐字节一致。
  - 公共的 `write_atomically`：写 `.partial`，同步后改名；取消或出错时删除临时文件。
- `panta-core`：
  - 激活：体网格四面体数超过 `VOLUME_STREAM_ACTIVATION_CELLS`（默认 1000 万）时走流式激活。先校验摘要，只流式提取精确外表面用于显示，单元不驻留。
  - `StreamedAsset` 记录路径、摘要和规模。每次操作前重新校验摘要，资产被替换时返回 `project.streamed_asset_failed` 或 `project.export_source_unavailable`。
  - 表面 LOD 状态下导出改走源资产。修复了一个潜在问题：之前 `export_import_mesh` 在 LOD 状态会导出缓存的 LOD。
- FFI / ViewModel / QML：
  - 导出、诊断、切面按“驻留体网格 → 流式体网格 → 流式表面 → 完整表面”分派。
  - 体网格信息标注“单元未载入内存”。
  - LOD 提示改为“导出流式读取完整源资产（索引格式不焊接）；表面质量诊断不可用”。
  - QML 的 `canExport` 去掉 LOD 条件，`canDiagnose` 保留。
- `scale_bench --volume N`：按体单元数报告生成、外表面（激活）、质量、切面、驻留读取的耗时和峰值 RSS。
- 非目标：
  - 表面 LOD 状态的流式拓扑诊断（已放弃，见下）。
  - 真正的 out-of-core 架构（转讨论，见下）。
  - 大体网格文件的流式导入（候选任务）。

## 与主流工具对照与取舍

| 工作项 | ParaView / EnSight / Tecplot / STAR-CCM+ / Fluent 后处理的做法 | 结论 |
|---|---|---|
| LOD 状态下导出完整数据 | ParaView、EnSight 的 LOD 只用于渲染，导出、计算始终基于完整数据 | **做**：成本低，语义与主流一致 |
| 表面 LOD 状态的流式拓扑诊断（外部排序，临时磁盘约 120 B/面） | 五款后处理工具都不做表面拓扑诊断。该功能在前处理工具里（ANSA、Fluent Meshing 的 check），且在内存中完成 | **放弃**：定位以后处理为主；表面规模已更正为百万～千万面，可以直接驻留；实现量大。替代方案：026 允许调高 LOD 阈值，在内存中诊断 |
| 大体网格不驻留单元，流式出外表面、质量、切面、VTU | ParaView、EnSight 用 client-server / MPI 分布式驻留；Tecplot SZL 按子区域按需加载；STAR-CCM+ 和 Fluent 分布式驻留 | **做**（单机保守版）：内存从约 180 B/单元降到约 24 B/节点 |
| 真正的 out-of-core：空间分块索引、视锥/切面裁剪只读相关块、多级缓存、client-server | Tecplot SZL 子区域、EnSight / ParaView 服务端分布式 | **讨论**：属于架构决策（资产格式要加空间分块，要不要 client-server）。当前方案每次拖动切面都要全量扫描（3 亿单元约 5.7 s，10 亿单元约 31 s） |

## 保守决策

- 流式导出的索引格式不做全局顶点去重。去重所需内存与规模成正比，这样做文件更大，但几何一致；LOD 提示里已注明。
- 表面 LOD 状态仍拒绝诊断（保留 `project.mesh_lod_only`，只用于诊断），文案提示导出不受影响。
- 体网格阈值实测后从 2000 万下调到 1000 万：驻留读取含完整拓扑校验，1010 万单元峰值 1.8 GiB、15.8 s；同规模流式质量、切面各约 0.3 s。026 会把它改为设置项。
- 流式切面、质量每次都完整扫描单元流，不做缓存（缓存需要与规模成正比的内存）。

## 验收标准

- [x] 流式写入的体网格资产可读回并通过完整校验；流式外表面、质量、切面与内存结果一致；可取消（Rust 单测）。
- [x] 流式表面导出各格式可被现有解析器读回，STL 逐字节一致；体网格 VTU 逐字节一致；取消不留目标文件或 `.partial`（Rust 单测）。
- [x] 大体网格流式激活得到精确外表面；服务层流式结果与驻留一致；资产摘要变化时报错（Rust 单测）。
- [x] FFI：流式体网格的诊断、切面、VTU 导出成功；表面 LOD 导出为完整源资产；摘要过期时报错；表面 LOD 诊断仍被拒绝。
- [x] 最大可行合成四面体网格基准（按体单元数）写入本文件。
- [x] 全量检查通过。
- [ ] 实机 GUI：大体网格激活、切面拖动、导出（需 GUI/GPU，未在 box 验证）。

## 本机基准（8 核、15 GiB 内存、可用约 11 GiB、无 GPU；release；`scale_bench --volume`）

| 体单元（四面体） | 节点 | 资产文件 | 生成 | 外表面（激活） | 质量统计 | 切面 | 驻留读取+校验 |
|---|---|---|---|---|---|---|---|
| 998,250 | 175,616 | 8.6 MiB | 0.03 s / 39 MiB | 0.01 s / 24 MiB | 0.04 s / 52 MiB | 0.03 s / 59 MiB | 1.08 s / 245 MiB |
| 10,110,954 | 1,728,000 | 85 MiB | 0.24 s / 114 MiB | 0.07 s / 118 MiB | 0.31 s / 289 MiB | 0.23 s / 345 MiB | 15.8 s / 1.80 GiB |
| 99,488,250 | 16,777,216 | 783 MiB | 2.9 s / 117 MiB | 0.65 s / 515 MiB | 2.5 s / 670 MiB | 1.76 s / 884 MiB | 跳过（估算 14 GiB） |
| 299,016,192 | 50,243,409 | 2.61 GiB | 6.8 s / 116 MiB | 2.3 s / 1.33 GiB | 7.9 s / 1.44 GiB | 5.7 s / 1.98 GiB | 跳过（估算 42 GiB） |
| 998,250,000 | 167,284,151 | 8.71 GiB | 26.7 s / 115 MiB | 9.3 s / 4.13 GiB | 36.5 s / 3.90 GiB | 31.2 s / 5.50 GiB | 跳过（估算 141 GiB） |

“耗时 / 峰值 RSS”的峰值在每个阶段前重置。说明：

- 合成块的结构规则，压缩率很高（约 9 B/单元），真实 CFD 网格的文件更大，I/O 占比更高。
- 10 亿单元是本机的最大可行规模：节点驻留约 3.7 GiB，加上切面输出，峰值 5.5 GiB。
- 10 亿单元的质量统计和切面耗时增长超过线性（36 s / 31 s，而 3 亿单元是 7.9 s / 5.7 s）。原因是 8.7 GiB 资产超出页缓存，每次扫描都要重新读盘。
- 大体网格的流式 VTU 导出为 ASCII，3 亿单元约 20 GiB，没有纳入基准；正确性由逐字节一致的单测保证。

复现：`cargo run --release -p panta-mesh --example scale_bench -- --volume 1B --dir /path/with/10GB/free --full never`

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | box：`cargo test -p panta-mesh volume_streaming` | 往返 + 校验；流式等于内存；取消；越界拒绝 | 通过 |
| 2026-10-24 | box：`cargo test -p panta-import streamed` | 格式读回；逐字节一致；取消无残留 | 通过 |
| 2026-10-24 | box：`cargo test -p panta-core`（activation、volume） | 流式激活精确外表面；流式等于驻留；摘要过期报错 | 通过 |
| 2026-10-24 | box：`cargo test -p panta-ffi streamed` | 流式体网格诊断、切面、导出；表面 LOD 导出完整源；摘要过期；诊断拒绝 | 通过 |
| 2026-10-24 | box：`scale_bench --volume 1M/10M/100M/300M/1B` | 见基准表 | 见基准表 |
| 2026-10-24 | box：全量检查 | 全部通过 | 见 PR |
| — | 实机 GUI/GPU | 大体网格交互 | 未执行 |

## 清理与兼容例外

- 原计划删除 `project.mesh_lod_only`，现在保留，仅用于表面诊断，因为流式诊断已放弃。
- 删除 QML 中 `canExport` 的 LOD 条件。
- 资产格式不变（流式写入产出标准 v3 体网格资产）。无兼容例外。

## 风险与回退

- 流式切面拖动在亿级规模下每次要数秒（已合并最新意图，不会堆积任务）。回退：调高体网格阈值（026）改为驻留。
- 资产被外部修改：每次操作前校验摘要，不一致时报错。

## 决策与工作记录

- 2026-10-24：经用户批准创建任务。
- 2026-10-24：用户更正规模口径：亿级指 CFD 体单元。范围扩展到体网格，基准按体单元报告。
- 2026-10-24：按“与主流工具对照、性价比”原则，放弃表面流式拓扑诊断，真正的 out-of-core 转讨论（理由见对照表）。
- 2026-10-24：实施完成；体网格阈值依据基准下调到 1000 万。

## 完成摘要

- 表面 LOD 状态可以导出完整源资产（STL 逐字节一致，索引格式未焊接）。
- 体网格超过 1000 万单元时流式激活，单元不驻留；质量、切面、VTU 导出按需流式读取。本机 10 亿单元峰值 5.5 GiB，每次切面约 31 s。
- 表面 LOD 诊断按评估放弃；out-of-core 架构待讨论。
