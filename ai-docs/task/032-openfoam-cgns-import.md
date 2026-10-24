# 032 — OpenFOAM / CGNS 导入与基于面邻接的非正交性 / 偏斜度

- 状态：in-progress
- 阶段：PR 1（任务文档 + OpenFOAM polyMesh / 结果场 / 多区域 / patch + 米单位）已合并；PR 2（非正交性 / 偏斜度）实施中；PR 3（CGNS，经纯 Rust HDF5）待实施
- 依赖：[029](029-result-fields.md)、[030](030-mixed-cells.md)、[031](031-polyhedron-cells.md)
- 优先级：P1
- 负责人：Grok Bot
- 创建 / 更新：2026-10-24 / 2026-10-24

## 目标与背景

用户 2026-10-24 17:16 UTC+8 指示：做 OpenFOAM / CGNS 导入（允许 HDF5，不做 Fluent `.cas`）；先写本文档并对照 ParaView 的 OpenFOAM 与 CGNS reader 评估范围；OpenFOAM 优先 polyMesh + 结果场、多区域与边界 patch；CGNS 优先非结构 Zone 与 FlowSolution；并用 owner / neighbour 补上 031 留下的非正交性与偏斜度。按 PR 拆分，CI 绿即合并；需要用户拍板的重大取舍（依赖体积、许可证）停下汇报。

现状：体网格导入只接受 VTU / 旧版 VTK（030 混合单元、031 多面体、029 结果场）。OpenFOAM 与 CGNS 是 CFD 求解器最常见的原生输出，用户目前必须先用 ParaView / foamToVTK / cgns 工具转成 VTU。完成后：在导入对话框直接选择 OpenFOAM 算例（`*.foam` / `*.OpenFOAM` 占位文件或 `system/controlDict`）或 `.cgns`（HDF5）文件，按「实体 3D」导入为原生混合 / 多面体体网格，带区域（多区域 / 多 Zone）与边界组（patch / BC）、单元或节点结果场；质量面板给出 checkMesh 口径的非正交性与偏斜度。

## 必读

- [网格架构](../architecture/mesh.md)「原生混合单元」「多面体单元」两节；[导入交互](../modules/import-interaction.md)
- [Rust 规范](../standards/rust.md)（unsafe 例外清单、依赖准入）、[依赖获取](../standards/dependency-acquisition.md)（Cargo 是唯一入口、不得源码构建 C 库）
- [测试规范](../standards/testing.md)（夹具放 `tests/fixtures/<模块>/`，记录来源与预期）
- [提交规范](../standards/commits.md)、[代码生命周期](../standards/code-lifecycle.md)
- 外部参考（2026-10-24 查阅）：OpenFOAM 用户指南 4.1 / 5.1（polyMesh 文件、FoamFile 头、ASCII / binary、`.gz`）、OpenFOAM `primitiveMeshTools::faceOrthogonality` / `faceSkewness`（checkMesh 判据：非正交 > 70° 记 severe、偏斜度 > 4 记 high）；CGNS SIDS 与 Mid-Level Library 4.x（`ElementStartOffset`、MIXED / NGON_n / NFACE_n）；CGNS HDF5 文件映射（节点属性 `label` / `type` 与数据集 `" data"`）；ParaView 5.13 `vtkOpenFOAMReader`、`vtkCGNSReader` 选项列表；Fluent 用户手册 orthogonal quality 定义。

## 对照 ParaView 的范围评估

| 能力 | ParaView vtkOpenFOAMReader | 本任务（OpenFOAM） |
|---|---|---|
| 网格：ASCII / binary、`.gz`、label 32/64、scalar 32/64 | 支持 | **支持**（FoamFile 头 `format` / `arch`） |
| 单元：hex / prism / tet / pyramid / 多面体 | 支持（多面体可选分解） | **支持**：按面拓扑识别四类标准单元，其余为 031 多面体（凹多面体整网格拒绝，同 031） |
| 多区域（`constant/<region>/polyMesh`） | 支持，逐区域选择 | **支持**：全部区域合并为一个体网格，区域 → `TetMesh` region |
| 边界 patch | 每 patch 一个块，可勾选 | **支持**：patch → 边界组（名称写入 ImportRecord）；不做 GUI 勾选 |
| volScalar / volVector 单元场 | 支持（另可插值到点） | **支持**：最新时间步 internalField（uniform / nonuniform、ASCII / binary），单位由 `dimensions` 推出 |
| 张量 / 对称张量 / pointField / surfaceField / patch 值 | 支持 | 非目标：记入 skipped_result_fields 或忽略（surfaceField 不进快照） |
| 时间序列 | 支持 | 非目标（候选 task：时间序列结果场） |
| decomposed（processor*）/ collated | 支持 | 非目标：无 polyMesh 时报错提示先 `reconstructPar` |
| cellZones / faceZones / pointZones、lagrangian、动网格 | 支持 | 非目标（后续） |

| 能力 | ParaView vtkCGNSReader | 本任务（CGNS，PR 3） |
|---|---|---|
| 文件后端 HDF5 / ADF | 二者（经 CGNS 库） | **HDF5**（纯 Rust 读取）；ADF 识别后拒绝并提示 `adf2hdf` |
| 非结构 Zone：固定单元 section | 支持 | **支持**：TETRA_4 / PYRA_5 / PENTA_6 / HEXA_8；二次单元整体拒绝（同 030） |
| MIXED | 支持 | **支持**：4.x `ElementStartOffset` 与 3.x 无偏移两种布局 |
| NGON_n / NFACE_n | 支持 | **支持** → 031 多面体 |
| 结构 Zone | 支持 | 非目标（候选 task：结构网格） |
| FlowSolution Vertex / CellCenter | 支持（含 Rind） | **支持**：Vertex → 节点场、CellCenter → 单元场；`XxxX/Y/Z` 合成矢量（与 ParaView 默认一致）；Rind ≠ 0 跳过并记录 |
| 多 Base / 多 Zone | 支持 | 首个 3D Base；多 Zone → 区域；节点不跨 Zone 合并（与 ParaView 一致） |
| ZoneBC / 边界面 section | 支持（patch 块） | **支持**：二维面 section 与 ZoneBC（PointList / PointRange，FaceCenter / Vertex）→ 边界组 |
| 时间序列（BaseIterativeData） | 支持 | 非目标 |
| 1to1 / Family | 支持 | 非目标 |

结论：两种格式的「首期」覆盖到 ParaView 默认读取路径（网格 + 单元 / 节点场 + 区域 + patch），但不做多时间步、分区算例、结构网格与 zone / family 选择 UI。

## HDF5 依赖评估（CGNS 需要）

| 方案 | 结论 |
|---|---|
| VTK SDK 自带 hdf5 / cgns | 不可用：当前 `sdk-vtk-9.7.0-webgpu` 未打包 IOCGNSReader / hdf5 模块 |
| 新出预编译 libhdf5（+ libcgns）SDK | 需新的 SDK 生产 workflow 与 Release 授权，属于用户级决策，首期不走 |
| `hdf5-metno`（`static` feature） | 本地编译 HDF5 C 源码，违反「不得静默源码构建」 |
| **`hdf5-pure` 0.47.0（选用）** | 纯 Rust、MIT OR Apache-2.0、无 build script；新增依赖树很小（`flate2` 已在 workspace）；已在本机用 libcgns 3.4.0 写出的 HDF5 文件验证可读 CGNS 节点属性与数据集 |

`hdf5-pure` 风险：首发 2026-03、单一维护者、约 12.7 万行、crate 内约 20 处 `unsafe`（第三方依赖，不进入本仓库 unsafe 例外清单）。只用于只读解析内存字节（`File::from_bytes`），出错整体拒绝导入。备选 `hdf5-reader`（只读）、`rust-hdf5`、`rustyhdf5`。判断：体积与许可证均无重大问题，不需用户拍板；在最终汇报中说明。

## 范围与非目标

PR 1（OpenFOAM）：

- 选择文件：`*.foam` / `*.OpenFOAM`（ParaView 约定的空占位文件）或 `system/controlDict`；算例根目录由此推出。
- 快照：把所需文件按确定顺序读入一个内存包（头 + 各条目「相对路径 + 原始字节」），摘要与「源已变化」检查覆盖全部文件；导入预检与提交复用现有会话。
- 网格：`constant/polyMesh` 的 points / faces / owner / neighbour / boundary（ASCII / binary、faceList / faceCompactList、`.gz`）；`constant/<region>/polyMesh` 存在时按目录名排序导入全部区域并忽略默认区域（多区域算例里默认区域常是拆分前的整网格，同时导入会重叠）。
- 结果场：最新数值时间目录（多区域为 `<time>/<region>/`）中的 volScalarField / volVectorField internalField → 单元场；区域缺某场时该区域为 NaN（缺失值）；单位由 `dimensions` 推出（SI 字符串，如 `m s^-1`）。
- patch：非空 patch → 边界组；多区域名称为 `<region>/<patch>`；名称与区域名写入 ImportRecord 新增可选字段。
- 单位：新增「米」（`meters`，OpenFOAM / CGNS 为 SI），Rust、QML 与 C++ 单位表同步；选中 OpenFOAM / CGNS 时默认单位切到米。
- 表面导入（非「实体 3D」）：所有 patch 面三角化为表面。
- 补 031 文档缺失的 PR 3 CI / nightly 记录。

PR 2（质量）：基于面邻接逐面计算非正交角与 OpenFOAM 偏斜度（内部面按 owner / neighbour 单元中心；边界面按面中心 / 单元中心变体），单元取所属面最大值；正交质量补邻接项（Fluent 定义）；OpenFOAM 直接用 owner / neighbour，其他来源按排序面节点哈希得到邻接。

PR 3（CGNS）：见上表。

非目标：Fluent `.cas` / `.dat`；时间序列；decomposed / collated 算例；结构网格；cellZones 等集合；动网格；GUI 中逐 patch / 区域显隐；张量场。

## 涉及分组与跨组依赖

`panta-io`（格式解析）、`panta-mesh`（质量）、`panta-core`（导入接入、记录字段）、`panta-ffi` / `native/bridge` / `qml`（扩展名、单位表、质量文案）。不新增跨组依赖；PR 3 新增第三方 crate `hdf5-pure`（只被 `panta-io` 使用）。

## 前置条件与待决策

- 夹具由本机 OpenFOAM v1912（Debian `openfoam 1912.200626`）与 libcgns 3.4.0（Debian `libcgns3.4`）生成，系统包只用于生成夹具与交叉核对，不进入构建配置。
- 若 `hdf5-pure` 读不出某类真实 CGNS 文件，记录并拒绝导入，不回退到 C 库。

## 实施步骤

1. PR 1：本文档；`panta-io::import::formats::openfoam`（FoamFile 词法 / 二进制列表、polyMesh、cell 形状识别、场）；`ImportFormat::OpenFoam` + 内存包快照；`RawGrid` 增加区域与边界面并在 `take_mesh` 中映射；ImportRecord 可选字段；米单位与 UI 扩展名；测试与夹具；nightly。
2. PR 2：`panta-mesh` 面邻接质量与汇总接入质量面板；与 checkMesh 交叉核对。
3. PR 3：`hdf5-pure` + `formats::cgns`；夹具由 libcgns 生成；与 `cgnsview` / h5dump 核对。

## 预计改动

- 新增：`crates/panta-io/src/import/formats/openfoam/{mod,lexer,bundle,tests}.rs`、`…/cgns.rs`（PR 3）；`tests/fixtures/mesh/openfoam/`、`tests/fixtures/mesh/cgns/`。
- 修改：`crates/panta-io/src/import.rs`（格式、快照、单位、记录字段）、`formats/mod.rs`（RawGrid）、`crates/panta-core/src/project/import.rs`、`qml/App.qml`、`qml/Dialogs/ImportDialog.qml`、`native/bridge/src/project_view_model.cpp`、`crates/panta-mesh/src/cell_quality.rs`（PR 2）。
- 持久化：ImportRecord 新增可选字段（serde default，记录版本不变）；网格资产格式不变。

## 清理与兼容例外

无废弃项；无兼容例外。

## 验收标准

- [ ] OpenFOAM ASCII、binary、`.gz`、多面体、多区域夹具导入后单元数 / 类型分布与 checkMesh 一致，区域数与 patch 边界组正确，单元场值与源文件一致、缺失为 NaN。
- [ ] 源包任一文件变化时提交报「源已变化」；缺 polyMesh、decomposed 算例、损坏文件给出明确错误。
- [ ] 非正交性最大 / 平均值与偏斜度最大值与 checkMesh 一致（1e-6 相对）。
- [ ] CGNS 夹具（HEXA_8、MIXED（含 / 不含偏移）、NGON / NFACE、两 Zone、ZoneBC、Vertex / CellCenter FlowSolution）导入正确；ADF 拒绝并提示。
- [ ] 每个 PR：`cargo format`、`cargo quality`、`cargo test --locked --workspace`、offscreen 启动通过；涉及 IO / C++ / QML 的 PR nightly 全绿。

## 验证计划与结果

| 日期 | 环境 / 命令或场景 | 预期 | 实际结果 / 证据 |
|---|---|---|---|
| 2026-10-24 | PR 1：`cargo test -p panta-io openfoam`（12）、`cargo test -p panta-core openfoam` | ascii / binary / gz 多面体 / 多区域 / 表面 / 坏输入 / 源变化 | ascii 12 hex、体积 1e-4 m³、3 组（12 / 28 / 24 三角形）；binary 与 ascii 一致；poly 160 点、12 hex + 36 多面体、体积 1e-4（与 checkMesh 一致）；multi 区域 [fluid, solid]，T 范围 300–400 K、单位 K；张量场记入 skipped；缺 owner / 坏 owner / decomposed 均明确报错；改 `0.01/p` 后提交报「源已变化」 |
| 2026-10-24 | PR 1：`cargo format`、`cargo quality`、`cargo test --locked --workspace`（CTest 69/69）、offscreen 启动 8 s | 全部通过 | 通过；启动 exit 124（超时即正常） |
| 2026-10-24 | PR 1：CI 40083065576 与 nightly 40083070826（Windows / macOS）（本行由 PR 2 补记） | 全绿 | 全绿；squash 合并为 main 7a52c06（2026-10-24 17:54 UTC+8） |
| 2026-10-24 | PR 2：`cargo test -p panta-io face_quality`，夹具 `skew`（扰动 4×3×2 blockMesh）、`skewdual`（其 polyDualMesh）、`poly` 与 OpenFOAM v1912 checkMesh 对照 | 内部面数一致，最大 / 平均非正交角与最大偏斜度相对误差 ≤ 1e-6 | skew：46 内部面，32.663972° / 21.620442°，偏斜 1.4240861；skewdual：133 内部面，41.626226° / 21.297343°，偏斜 2.4449241；poly：104 内部面，0° / 0°，偏斜 1.0000001——全部在 1e-6 内一致 |
| 2026-10-24 | PR 2：`cargo format`、`cargo quality`、`cargo test --locked --workspace`（CTest 69/69）、offscreen 启动 8 s；`panta-ffi` 诊断测试（带 `face_quality` 时摘要与结论）、`panta-core` 多区域算例记录 `face_quality` | 全部通过 | 通过；启动 exit 124（超时即正常） |

## 风险与回退

- OpenFOAM 文件语法变体多（宏、`#include`、`$var`）：polyMesh 与场文件由工具写出、不含宏；遇到无法识别的语法整体拒绝，回退为用户先 `foamToVTK`。
- `hdf5-pure` 年轻：出错整体拒绝；可在不改上层的前提下替换为备选 crate。
- 多区域合并后区域间界面面在两侧各保留一份边界三角形（两区域节点不共享），与 OpenFOAM 的 mapped patch 语义一致。

## 决策与工作记录

- 2026-10-24：创建任务（用户 17:16 UTC+8 指示），完成范围与 HDF5 评估。
- 2026-10-24（PR 1）：
  - 快照为内存包（`PANTA-FOAM-CASE 1` 头 + 「相对路径 + 原始字节」条目），预检 / 提交逐字节比较整包，因此任一文件变化都报「源已变化」；`.gz` 在解析时解压，包内保留原始压缩字节。场文件只按 8 KiB 头判断类别，非 volScalar / volVector 的 `vol*Field` 记为 skipped 条目，`surface*Field` / `point*Field` 不进包。
  - 只取最新数值时间目录；无时间目录时只导入网格。不读 `boundaryField`（patch 值），单元场仅 internalField。
  - 单元形状：按面数 / 面顶点数与面集合复核识别 tet / pyramid / wedge / hex，并按 VTK 局部顺序重排；识别失败一律落到 031 多面体（保守：不猜测退化单元）。
  - **保守决策**：OpenFOAM 面常含共线节点（polyDualMesh、加密边中点），031 的「最小节点扇形」在最小节点位于共线边上时产生零面积三角形与零体积四面体（`poly` 夹具 42 个）。不改 `panta-mesh` 的纯拓扑拆分规则（流式 / 分块 / 资产重载多处依赖其只由拓扑决定），而在 OpenFOAM 导入侧把这类面改写为显式三角形子面：按节点号升序找第一个扇形无退化（相对面积阈值 1e-8）的起点；起点只由面节点号与坐标决定，owner / neighbour 两侧一致，保持共形；含改写面的单元一律作多面体。找不到合格起点时保留原面，由领域校验报告。
  - 边界组：每个三角形按「包含其三个节点的源 patch 面」归组；未覆盖的三角形归入 `unassigned` 组而非导入失败（保守），未使用的组丢弃并重新编号。多区域组名为 `区域/patch`；零面 patch 跳过。
  - 多区域：同名场跨区域合并，缺该场的区域填 NaN；区域间节点不共享，界面两侧各保留一份边界（与 mapped patch 语义一致）。
  - 单位新增「米」（`meters`，×1000 到毫米）；OpenFOAM 选中后对话框默认「米 + 实体 3D」，用户可改。记录尺寸沿用来源单位，与其他格式一致。显示名取算例目录名。
  - ImportRecord 新增可选 `region_names` / `boundary_group_names`（serde default、空时不写），记录版本不变，旧工程可读。
  - 动画序列打开对话框与导出格式不加 OpenFOAM（时间序列归后续「播放 + 时序结果场」task）。

- 2026-10-24（PR 2）：
  - 质量在导入时按 OpenFOAM 原生 owner / neighbour 与原始多边形面计算（逐区域、节点偏移前），不用导入侧改写后的三角形子面，使面数与平均值和 checkMesh 一致；结果跨区域合并（保留余弦和与计数，平均值按全部内部面计）。
  - 偏斜度边界面归一化系数取 0.4（内部面 0.2），与 checkMesh 的 boundaryFaceSkewness 一致；夹具最大偏斜度逐位对上验证了内部面与边界面两种口径。
  - **保守决策**：只给全网格汇总（最大 / 平均 / 超阈值计数），不做逐单元「所属面最大值」字段与着色，也不为 VTU / VTK 等来源按面节点哈希建邻接——两者都需要新增持久化数组或在诊断时重建邻接（大网格内存与时间成本），列为候选后续 task，未实施。
  - **保守决策**：结果存入 ImportRecord 可选字段 `face_quality`（serde default，记录版本不变），诊断面板直接读取、不重算；旧工程与非 OpenFOAM 来源不显示该行。严重非正交 / 偏高偏斜 / 劣质正交质量任一计数非 0 时质量结论为「存在问题」。
  - 正交质量（Fluent 定义）checkMesh 不报告，无法交叉核对；`poly` 对偶网格的边界单元约 0.148（逐单元正交质量同一网格最小 0.100），量级一致，判断为对偶网格边界单元的真实特征。测试只断言其落在 (0, 1]。

## 完成摘要

未完成。
