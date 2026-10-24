# 029 — 结果场：导入、存储、分块携带、切面插值与视口上色

- 状态：in-progress
- 阶段：PR 1 已合并（#33）；PR 2a/4（`.pchunk` v2 携带结果场、驻留与分块场剖面）实施中
- 依赖：[028](028-out-of-core-rollout.md)、[021](021-volume-mesh.md)
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

体网格目前只有几何。来源 VTU / 旧版 VTK 常带节点 / 单元数组（温度、速度、材料号），导入时被丢弃。本任务让「实体 3D」导入保留这些数组作为结果场，持久化到工程，随后接入 out-of-core 分块、切面插值和视口上色。用户安排的候选顺序（2026-10-24 12:29 UTC+8）：结果场 → 混合单元 → OpenFOAM/CGNS → 时间序列 → 超大文件流式导入 → 结构网格 → Netgen 加密。

原计划三个 PR，PR 2 体量过大，2026-10-24 拆为四个（仍在已批准的本任务范围内）：

1. **领域模型与存储**（#33 已合并）：`panta_mesh` 结果场模型与 `.presult` sidecar；`panta-io` 解析 VTU PointData / CellData 与旧版 VTK 属性段；`panta-core` 导入写 sidecar、清单引用、只读访问。
2. **2a 分块与计算（panta-mesh）**：`.pchunk` v2 每块携带结果场值与块级范围，驻留与分块两条场剖面路径（节点场线性插值、单元场取单元值），IO 基准。
3. **2b 应用接入（panta-core）**：派生分块按「资产摘要 + 结果摘要」命名并带场构建，工程场剖面 API（驻留 / 分块），字段统计诊断；流式回退（无派生缓存）不支持场剖面，明确报 CommandInvalid。
4. **3 显示**：FFI / VTK 上色、QML 字段选择、色标（放在视口矩形之外，不遮挡模型）。需要 GUI / GPU 实机验收。

## 必读

- [网格架构](../architecture/mesh.md)「结果场」与 out-of-core 两节、[可视化架构](../architecture/visualization.md)「字段与显示」
- [Rust 规范](../standards/rust.md)、[注释规范](../standards/comments.md)、[测试规范](../standards/testing.md)、[仓库文件规范](../standards/repository-hygiene.md)、[提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)
- VTK 文件格式（VTK User's Guide / docs.vtk.org「VTK File Formats」，2026-10-24 查阅）：旧版属性段 `POINT_DATA n` / `CELL_DATA n` 后接 `SCALARS name type [numComp]` + 可选 `LOOKUP_TABLE`、`VECTORS` / `NORMALS name type`、`TENSORS` / `TENSORS6`、`TEXTURE_COORDINATES name dim type`、`COLOR_SCALARS name nValues`（二进制为 unsigned char）、`FIELD name numArrays` 后每个数组 `name numComponents numTuples type`；5.1 起名称以 `%XX` 转义空格等字符。

## 范围与非目标

- PR 1（本次）：
  - `panta_mesh::{ResultField, ResultSet, FieldSummary, FieldAssociation, ResultAssetReader, encode_result_asset}`。
  - VTU：`PointData` / `CellData` 中的 `DataArray`，1 或 3 分量，任意数值类型转 f64。
  - 旧版 VTK：`SCALARS`（1 分量）、`VECTORS`、`NORMALS`、`FIELD` 数组（1 或 3 分量）；`TENSORS` / `TENSORS6` / `TEXTURE_COORDINATES` / `COLOR_SCALARS` / 独立 `LOOKUP_TABLE` 读过并跳过；`METADATA` 跳过。
  - 跳过不阻止网格导入，原因写入 `ImportRecord.skipped_result_fields`。
  - 工程：`assets/results/<资产 ID>-r1.presult`，清单 `mesh_assets.<id>.results`；`ProjectService::result_fields_for_import` / `read_result_field`。
- 非目标（本任务内不做）：表面导入的结果场（STL / OBJ / PLY / VTP 表面导入不读取数组）；2、4–9 分量（张量）字段；多时间步（候选「时间序列」）；导出 VTU 时写回结果场；单位解析（VTK 无标准单位属性，单位留空）。

## 涉及分组与跨组依赖

沿既有方向：`panta-mesh`（模型与容器）→ `panta-io`（解析，已依赖 panta-mesh）→ `panta-core`（工程存储）。不新增跨组依赖和第三方依赖。

## 前置条件与待决策

- 已决（保守，2026-10-24）：**结果场写独立 sidecar，不升级 Mesh 资产到 v4**。Mesh 资产与摘要不变，旧工程、028 派生分块缓存（按资产摘要索引）不失效；一个网格可对应多份结果文件，适合后续时间序列。代价是多打开一个文件，读取按字段、MiB 级块，符合 Windows 少次大块读取要求。
- 已决：数值内存与磁盘都用 f64，无损、实现简单；PR 1 读取单个字段时整字段驻留，按块按需读取在 PR 2 随 `.pchunk` 实现。
- 已决：缺失值（任一分量非有限）原样保留并计数，不替换为 0；范围忽略缺失值，多分量按模长。
- 已决：属性段解析失败只丢弃其后的数组并记录原因，网格照常导入（网格段本身的错误仍整体拒绝）。

## 实施步骤

1. panta-mesh：结果场模型、`.presult` 编码与读取器（kind 3、流 6/7），Mesh / 结果读取器互斥。
2. panta-io：`RawGrid.fields / skipped_fields`、`offer_field` 统一校验；VTU 与旧版 VTK 解析。
3. panta-core：导入写 sidecar（tmp + rename、同内容复用、冲突换恢复路径、失败清理）、清单校验、只读访问。
4. 文档与测试；PR 2、PR 3 另行记录。

## 预计改动

- 新增：`crates/panta-mesh/src/asset/result.rs`、`result/tests.rs`、`crates/panta-core/src/project/results.rs`。
- 修改：`panta-mesh` asset `mod.rs` / `stream.rs`（新 kind 与流）；`panta-io` `formats/{mod,text,vtk_xml,vtk_legacy}.rs`、`import.rs`（ImportRecord 可选字段）；`panta-core` `project/{import,generated,mesh_assets,model,service}.rs`、`project.rs`。
- 持久化：清单新增可选 `results`，ImportRecord 新增可选 `skipped_result_fields`，均为缺省可省略字段，schema / 记录版本不变；新文件类型 `.presult`。

## 清理与兼容例外

无废弃项，无兼容例外（新增可选字段不是兼容分支：缺省即“无结果场”）。

## 验收标准

- [x] VTU 与旧版 VTK（ASCII / 二进制）的 1、3 分量节点 / 单元数组按来源顺序保留；NaN 保留并计入缺失；不支持的数组跳过并记录原因，网格照常导入。
- [x] 四个 tet_ball 夹具（VTU ASCII / appended zlib、旧版 4.2 ASCII / 5.1 二进制）都得到同一个 3 分量节点场 `Normals`。
- [x] `.presult` 往返位级一致；损坏的块只影响所在字段（`ChecksumMismatch`）；Mesh 与结果读取器互相拒绝对方文件。
- [x] 导入后重开工程可列出字段并读取数值；清单中的 results 引用经路径、版本、摘要校验；sidecar 与网格摘要 / 计数不符时拒绝。
- [x] PR 2a：`.pchunk` v2 携带结果场；驻留 `section_surface_field` 与分块 `fine_section_field` 几何与无场剖面位级一致、数值集合一致；节点场线性插值误差 < 1e-9，单元场每三角形三值相等，NaN 传播；块级范围落在全局范围内；同场再次剖面只多 1 次合并读；损坏的场值块只让场剖面失败；从资产 + `ResultAssetReader` 构建与驻留构建字节一致。
- [ ] PR 2b：应用接入、派生缓存带场、字段诊断。
- [ ] PR 3：视口上色、字段选择、色标（GUI / GPU 实机验收）。
- [ ] 每次 commit 的代码、测试、文档和 task 自洽；索引状态一致。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | Linux x86_64，rustc 1.99.0，cwd 仓库根，`cargo test -p panta-mesh --lib` | 结果 sidecar 5 项新测试通过 | 58 passed |
| 2026-10-24 | `cargo test -p panta-io --lib` | VTU / 旧版解析与夹具断言通过 | 33 passed |
| 2026-10-24 | `cargo test -p panta-core --lib results` | 导入、重开、损坏隔离、表面无结果 | 5 passed（含 2 项新测试） |
| 2026-10-24 | `cargo format`、`cargo lint clippy --check`、`cargo lint qmllint --check`、`cargo test --locked --workspace`、offscreen 启动 | 全部通过 | 格式与 Clippy 无告警；workspace 全部 Rust 测试通过，CTest 68/68；offscreen 启动 8 s 超时退出（124，预期） |

| 2026-10-24 | PR 2a：`cargo test -p panta-mesh --lib field` | 8 项分块场测试 + 既有场测试通过 | 11 passed |
| 2026-10-24 | PR 2a：`cargo quality`、`cargo test --locked --workspace`、offscreen 启动 | 全部通过 | 格式 / Clippy / qmllint 无告警；workspace 全通过，CTest 68/68；offscreen 124（预期） |
| 2026-10-24 | PR 2a：`ooc_bench --cells 20M --fields --reader pread`（8 线程，Linux，文件可能在页缓存） | 场剖面读调用为个位数、MiB 级 | 128 块 0.29 GiB；斜切面 40 块 40 万三角形：首次选矢量场 103.7 ms / 8 次读 / 30.1 MiB，同场换分量 7 次读 / 28.9 MiB，换标量节点场 8 次 / 10.5 MiB，换单元场 6 次 / 7.4 MiB；打开文件 1 |
| 2026-10-24 | PR 2a：`ooc_bench --cells 100M --fields --reader pread`（同上） | 亿级下场剖面仍为少次 MiB 级读 | 构建 14.75 s、512 块 1.45 GiB、209 次写、峰值 RSS 4.4 GiB（字段逐个驻留，单元场 0.8 GiB）；斜切面 102 块 117 万三角形：选矢量场 218.7 ms / 22 次读 / 91.1 MiB，换分量 21 次 / 87.7 MiB，换节点标量场 18 次 / 34.1 MiB，换单元场 9 次 / 23.8 MiB；z 向 128 块：选矢量场 225.0 ms / 33 次读 / 106.5 MiB；单次读平均约 3–4 MiB，打开文件 1。命中块在 Morton 序上不连续，读调用数随命中块分散度增长 |

## 风险与回退

- f64 结果场体积是网格节点数 × 分量 × 8 B，亿级网格单字段约 GiB 级；PR 1 读取整字段驻留，超大网格需等 PR 2 按块读取。回退：清单 `results` 可选，删除该引用即退回纯网格。
- 结果场写入失败会让整个导入失败（与网格写入同一提交闸门），已创建的 sidecar 与网格资产随守卫删除，不留孤立文件。

## 决策与工作记录

- 2026-10-24：创建任务，确定三 PR 拆分与 sidecar 方案（见前置条件）。
- 2026-10-24（PR 1）：实现结果场模型与 `.presult`、VTU / 旧版 VTK 解析、工程存储与只读访问；文档更新 [网格架构](../architecture/mesh.md)、[应用与存储](../architecture/application-and-storage.md)。现有 tet_ball 夹具带 `Normals` 节点数组，实体 3D 导入现在会生成 `.presult`。无独立 subagent 评审条件（本会话无法启动独立评审代理），由实现者自检并以测试覆盖失败路径。

- 2026-10-24（PR 2a）决策（保守）：
  - 结果场嵌入 `.pchunk` 并升级 FORMAT_VERSION 到 2，不另建分块场文件：只打开一个文件，块表一次读入；v1 文件打开失败，应用派生缓存会自动重建（派生缓存可丢弃，不需迁移）。
  - 场段按字段排列：每个字段先所有块的内部值块，再所有块的外表面值块（都按 Morton 序）。外表面值连续存放，选中一个场只需一次合并读；内部值每次剖面按命中块合并读取（相邻 1 MiB 内合并、单次 ≤ 64 MiB），不进 LRU。
  - 外表面值只常驻当前一个场（换场时替换），避免多场常驻放大内存。
  - 粗级别（拖动中）剖面不着色，松手精细剖面才上色；仅支持 1 / 3 分量。
  - 合成基准场是平滑函数，压缩率偏乐观；真实 CFD 场体积更大，读取字节数会相应上升，读调用次数不变。
  - 无独立 subagent 评审条件，由实现者自检并以测试覆盖失败路径。

## 完成摘要

未完成。
