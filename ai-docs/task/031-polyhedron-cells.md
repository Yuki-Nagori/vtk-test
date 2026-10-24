# 031 — 多面体单元（VTK_POLYHEDRON 42）：面流布局、拆分、资产 v5、`.pchunk` v4、导入导出与质量

- 状态：in-progress
- 阶段：PR 1（IR、拆分、质量、资产 v5）待合并
- 依赖：[030](030-mixed-cells.md)
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

**属于已批准范围**：用户 2026-10-24 16:14 UTC+8 确认“polyhedron 本来就在用户批准的混合单元范围内（tet/prism/pyramid/hex/polyhedron）”，要求在 OpenFOAM / CGNS 之前完成。030 把 polyhedron 列为非目标（需要面流布局）；本任务是 030 的延续，单独立档便于追踪。

OpenFOAM 网格本质是基于面的多面体，snappyHexMesh 与 STAR-CCM+ 导出的网格含大量多面体。完成后：VTU（`faces` / `faceoffsets`，以及 VTK 9.4 的 `polyhedron_to_faces` 等数组）与旧版 VTK（CELLS 中的序列化面流）里的 42 号单元可以导入、显示外表面、切面、统计体积与质量、按原单元 ID 绑定单元场，并原样导出；大网格经 `.pchunk` 分块按需读取。

## 必读

- [网格架构](../architecture/mesh.md)「原生混合单元」「多面体单元」两节（格式说明）
- [资产激活](../modules/asset-activation.md)、[性能](../modules/performance.md)
- [提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)
- VTK XML 文件格式（docs.vtk.org v9.7.0，多面体 `faces` / `faceoffsets` 示例，2026-10-24 查阅）；VTK `IO/XML/vtkXMLUnstructuredDataReader.cxx`（`polyhedron_offsets` / `polyhedron_to_faces` / `face_offsets` / `face_connectivity`）与 `IO/Legacy/vtkUnstructuredGridWriter.cxx`（旧版格式把多面体写成序列化面流 `GetSerializedCellFaces`），master 分支，2026-10-24 查阅
- Fluent 用户手册的 orthogonal quality 定义；OpenFOAM `checkMesh` 的 face pyramids / non-orthogonality / skewness 定义（质量指标取舍依据）

## 范围与非目标

- PR 1（`panta-mesh` 为主）：`CellType::Polyhedron`、规范面流布局、拓扑校验与确定性共形拆分、体积 / 切面 / 外表面、节点场在辅助节点上的取值、逐单元正交质量、资产 v5（独立面流流）及其流式读写；下游 `.pchunk` 构建、VTU 导出、VTU / 旧版 VTK 导入暂时明确拒绝多面体。
- PR 2：`.pchunk` v4（块内原始单元布局带面流）、分块质量 / 切面 / 场切面，合成多面体网格与基准（读调用次数、字节、打开文件数），nightly 加入多面体基准。
- PR 3（`panta-io` / FFI）：VTU 与旧版 VTK 读写 42（FFI 类型名“多面体”与“正交质量”文案已随 PR 1 加入）。
- 非目标：邻接单元间的非正交性 / 偏斜度（需要面—单元邻接，留待 OpenFOAM 导入时用 owner / neighbour 直接计算）、非星形（凹）多面体的面中心细分拆分、二次单元、Netgen 输出多面体、Fluent `.cas`。

## 涉及分组与跨组依赖

`panta-mesh`（IR、资产、分块）、`panta-io`（导入导出）、`panta-core`（激活与派生缓存）、`panta-ffi`（文案）。不新增跨组依赖，不改 C++ / QML 结构。

## 前置条件与待决策

已决定（保守方案，均在已批准范围内，无需用户拍板）：

1. **规范布局**：多面体在 `CellLayout.connectivity` 中占 `[apex, nFaces, n0, ids…, n1, ids…]`，即 VTK 旧版序列化面流前加一个“辅助中心节点”。面必须是闭合、一致定向的二维流形（每条有向边恰好与其反向边配对一次）；导入时整体反向的面流会被翻转为外法向。
2. **拆分**：每个面按最小全局节点号扇形三角化（四边形面即 030 的最小下标对角线，因此与相邻 hex / wedge / pyramid 共形），每个三角形与单元中心组成一个四面体。不用面中心：面中心需要跨单元去重才能共形，而且与相邻六面体的对角线拆分不一致。
3. **辅助中心节点落盘**：单元中心（三角化表面的体积质心）作为普通节点追加在节点数组末尾，第 k 个多面体的中心节点号 = 源节点数 + k。这样拆分保持纯拓扑、流式读取不需要坐标。源节点数 = 节点数 − 多面体数；节点场长度、导出节点、显示节点数都按源节点计，中心节点的节点场值取该单元唯一节点的算术平均。
4. **凹单元**：存在非正体积拆分四面体（中心看不到全部面，类似 OpenFOAM checkMesh 的 face pyramids 错误）的单元与 030 的扭曲单元同样由领域校验拒绝，整网格不部分导入。风险见下。
5. **资产 v5**：新写一律 v5；v3 / v4 继续可读。`VolumeCells` 仍为 40 B 记录，多面体记录为 `[42, region, apex, 面流字数, u32::MAX × 6]`；面流写入新流 `VolumePolyFaces`（10，u32 记录），与 `VolumeCells` 块一一配对（第 i 个面流块存第 i 个单元块中多面体的面流，按单元顺序），因此任一单元块可独立解码。`VolumeCellSummary` 增加第二条记录：多面体数。含多面体的资产单元块按 16384 单元切分以限制配对面流块大小。
6. **质量**：ParaView / Verdict 不支持多面体的 Mesh Quality。按“成本低、价值高”先做逐单元可算的正交质量（Fluent 定义中面面积向量与“单元中心→面中心”向量夹角余弦的最小值；不含邻接单元项），劣质阈值取 ANSYS 推荐的 0.15；非正交性与偏斜度需要邻接，见非目标。

## 实施步骤

1. PR 1：IR、拆分、校验、资产 v5 与流式读写，单元测试。
2. PR 2：`.pchunk` v4、合成多面体网格、基准与 nightly。
3. PR 3：VTU / 旧版 VTK 导入导出、质量指标、FFI 文案；收尾文档与汇报。

## 预计改动

`crates/panta-mesh/src/{cells,volume,lib,cell_quality,field_section,synthetic}.rs`、`asset/{mod,stream}.rs`、`chunked/*`；`crates/panta-io/src/import/formats/*`、`export*.rs`；`crates/panta-core/src/derived_chunks.rs`；`crates/panta-ffi/src/volume.rs`；`ai-docs/architecture/mesh.md`。资产 v5 与 `.pchunk` v4 为持久化格式变更：旧资产继续可读，旧派生缓存因版本不符自动重建。

## 清理与兼容例外

无废弃项；无兼容例外（v3 / v4 资产读取是既有格式支持，不是新增兼容分支）。

## 验收标准

- [ ] 多面体立方体、六棱柱、与六面体共面的多面体网格拆分共形、体积精确、外表面闭合；凹单元与开放 / 定向不一致的面流被拒绝并指出单元号。
- [ ] 资产 v5 往返一致，流式读取（`visit_cells` / `visit_cell_tets`）与整体解码一致；v4 资产仍可读。
- [ ] `.pchunk` v4 分块切面 / 质量 / 场切面与驻留结果一致，旧缓存自动重建；基准报告读调用次数、字节与打开文件数。
- [ ] VTU / 旧版 VTK 的 42 号单元导入导出往返（含单元场与节点场）。
- [ ] 全量检查、CI 与 nightly 通过。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | PR 1：`cargo test -p panta-mesh`（新增 `poly_tests` 8 项：立方体内外向、共形与六棱柱体积、开放 / 不一致 / 凹 / 篡改拒绝、资产 v5 驻留与流式往返（含 26³ 单元跨 16384 块配对与 0 记录面流块，流式写出与整体编码逐字节一致）、`visit_cells` / `visit_cell_tets`、流式质量 = 驻留质量、v4 资产夹具可读、`.pchunk` 构建拒绝、节点场经中心节点插值） | 全部通过 | 91 passed |
| 2026-10-24 | PR 1：`cargo format`、`cargo quality`、`cargo test --locked --workspace`（CTest 69/69）、offscreen 启动 8 s | 全部通过 | 通过；启动 exit 124（超时即正常） |

## 风险与回退

- 凹多面体（snappyHexMesh 偶见）会使整网格导入失败。回退：报错信息给出单元号；后续可对这类单元改用面中心细分（需跨单元去重面中心），属于本任务范围内的增强。
- 中心节点使节点数组比源文件多（每个多面体 24 B）；节点场按源节点计，任何把 `nodes.len()` 当作源节点数的旧代码都会出错，已逐一改为 `source_node_count()`。

## 决策与工作记录

- 2026-10-24：创建任务（用户 16:14 UTC+8 指示先做 polyhedron），记录上面的保守方案。
- 2026-10-24：PR 1 实现。正交质量提前到 PR 1（只依赖单元自身几何）。v4 兼容夹具 `crates/panta-mesh/tests/data/mixed-v4.pmesh` 由 main（7af8be6）的编码器生成（030 的混合单元测试网格）。流式写出与驻留编码采用同一块切分（含多面体时单元块固定 16384 单元），两条路径产出逐字节相同的资产。`MeshDisplayInput.streamed` 改为 `Arc`：`VolumeStreamCounts` 增加 `polyhedra` 后输入超过 clippy `boxed_local` 的体积阈值。结果场 sidecar 的节点数、核对与 `.presult` 打开改按源节点数。

## 完成摘要

未完成。
