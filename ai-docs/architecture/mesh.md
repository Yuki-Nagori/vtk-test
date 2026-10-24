# 网格模型、Netgen 与 Mesh IR

[架构总览](README.md)

## 自有 Mesh IR

网格模块拥有独立于 Netgen 和 VTK 的中间表示（Mesh IR）。基本信息包括节点坐标、单元类型与连接关系、节点/单元 ID、区域与边界分组、单位、来源几何修订及生成参数。

混合单元必须明确各单元的类型和局部节点顺序；边界面、体单元及其对应关系应可验证。索引宽度、浮点精度和内存布局在实现时依据规模确定并写入格式说明，不能依赖某个库的隐式默认值。

当前权威 Mesh IR、STL 解码、体网格校验与总体积计算在 `crates/panta-mesh/`。`native/mesh/` 调用 Netgen，将库对象转换成明确命名的 native DTO，再经 CXX 委托 Rust 校验。C++ 只保留库结果转换、安全边界检查与局部节点顺序归一化；它不再保存第二套领域校验规则或重复计算网格总体积。STL 文件读取由 `panta-io` 编排、`panta-mesh` 解码，VTK 只接收已提交快照。职责与依赖方向见 [适配边界](native-domain-boundaries.md)。

## 从 STL 到显示和计算的路线（A 已有，B/C 规划）

```text
STL source asset → panta-io → panta-mesh::SurfaceMesh（Rust 权威）
                                      ├─ A. 直接显示：批量 CXX 快照 → C++ vtkPolyData → VTK
                                      ├─ B. 几何修复任务（规划）：C++ OCCT adapter → faceted B-rep
                                      │       → Rust 提交新 GeometryAsset / revision
                                      │       → OCCT 三角化 → vtkPolyData → VTK
                                      └─ C. 体网格任务（STL 路径规划）：C++ Netgen adapter → native DTO
                                              → panta-mesh 体 Mesh 校验 / 提交
                                              → vtkUnstructuredGrid → VTK
```

Rust 拥有资产身份、来源 revision、转换 / 网格任务策略、领域参数和成功提交；C++ adapter 调用 OCCT / Netgen，将领域选项映射为库参数、转换输入输出并管理 native 所有权；VTK 只承担显示对象与 GPU 生命周期。Netgen 可执行表面和体网格生成及相应网格优化；这些能力不等同于对原始 CAD 几何做 B-rep 修复。场景 A 使用单个批量快照，不让 QML 持有数组，也不将 VTK 类型穿过 Rust。当前 SurfaceMesh 以每个三角面三个 f64 顶点表示；C++ 直接生成面法线，绕过 VTK 上重复拆点 / 三角化的处理。

场景 B 从 STL 三角面重建的是离散面片构成的 faceted B-rep；这不会恢复原始 CAD 的解析曲面、参数和设计历史。需要曲面拟合或拓扑修复时，任务必须定义容差、诊断、实体有效性、失败保留旧 GeometryAsset 和映射失效行为。GeometryAsset 的 ID 与 revision 由 Rust 持有；C++ OCCT shape 是运行期 native 对象，不能当成跨 FFI 的 Rust 领域对象。

场景 C 的 STL 路径仍按明确 `SurfaceMesh` revision 与参数快照规划。固定 SDK Netgen 6.2.2604 的 `nglib.h` 提供 `Ng_STL_NewGeometry` / `Ng_STL_AddTriangle` / `Ng_STL_InitSTLGeometry` 接收三角面、`Ng_STL_GenerateSurfaceMesh` 生成表面网格，再由 `Ng_GenerateVolumeMesh` 生成体网格的 API 路径；这证明 C++ adapter 有直接传入三角面数据的 API 边界，但不证明任意 STL 都闭合、可定向或可成功生成体网格，需通过真实失败样例验收。Netgen 只在 C++ adapter 中运行；输出经 native DTO 交 Rust `panta-mesh` 校验，再按新资产 revision 提交为体 Mesh。

现有 STEP 冒烟路径位于 `native/geometry/` 和 `native/mesh/`：geometry adapter 一次读取并保留运行期 `TopoDS_Shape`，Netgen adapter 通过 `ShapeRef` 使用同一对象完成 OCC 体网格生成；Mesh DTO 另带连续边界组到导入 revision 内 `FaceId` 的映射。`ShapeRef` / `FaceId` 当前仅限 native 进程和该次导入，不是工程持久身份，Rust 资产服务接入前不能写入 `.panta`。当前领域体网格类型为 `TetMesh`，不维护第二份 Mesh IR。所有网格操作都不能覆盖其来源资产。

Netgen API 依据：[v6.2.2604 `nglib.h`](https://github.com/NGSolve/netgen/blob/v6.2.2604/nglib/nglib.h)。最终能否对特定输入直接体网格化，仍由未来网格任务的封闭性、方向、容差及失败诊断测试决定。

工程 schema 4 在 `.panta` 中保存 `mesh_assets` 索引，Mesh payload 以工程内相对路径旁置，打开工程不立即读取网格。`crates/panta-mesh/src/asset/mod.rs` 定义 sidecar v4（task 030；块布局与 v3 相同，只新增混合单元流，v3 继续可读）：56 B 头部（类型、毫米单位、目录 SHA-256）后接分块目录，数据块按 64 B 对齐、每块约 4 MiB 未压缩记录、独立 Zstandard 压缩（不可压缩时原样保存）并带块级 SHA-256；资产摘要为目录摘要。目录条目为 LOD 层级、时间步和块内过滤器预留字段，v3 / v4 要求为 0、读取端显式拒绝非 0 值。`MeshAssetReader` 先校验头部与目录，再按批读取数据块并多线程解码，批次间响应取消。v2（单 Zstandard 流 + 整段摘要）只读兼容见 [task 011](../task/011-mesh-asset-chunked-storage.md) 与清理任务 [012](../task/012-mesh-asset-v2-removal.md)。解码验证版本、摘要与范围后构造领域 Mesh；路径授权由工程服务负责。SurfaceMesh 保留完整三角面顶点语义，新导入不复制来源 STL，旧 schema 不自动迁移。

## 原生混合单元（task 030）

- IR：`TetMesh.cells: Option<Arc<CellLayout>>`。`None` 为纯四面体（单元 ID = 四面体下标）；`Some` 时 `CellLayout` 为 VTK 风格的 `types`（VTK 单元编码）、`offsets`（n + 1 项，首项 0，单调递增）与 `connectivity`（u32 节点下标），另附 `tet_cells`（每个拆分四面体所属原始单元，非递减）。支持 tet(10)、hex(12)、wedge(13)、pyramid(14)；polyhedron(42) 编码已预留，当前显式拒绝（需要面流布局，另立任务）。
- 局部节点顺序（与 VTK 9.x `vtkTetra` / `vtkHexahedron` / `vtkWedge` / `vtkPyramid` 一致，参考坐标下体积为正）：
  - tet `0 1 2 3`：`(0,1,2)` 的右手法向指向 3。
  - hex `0–7`：底面 `0 1 2 3` 右手法向指向顶面，`4 5 6 7` 分别位于 `0 1 2 3` 正上方。
  - wedge `0–5`：三角形 `0 1 2` 与 `3 4 5` 对应（`i` 与 `i+3` 相连）；`(0,1,2)` 的右手法向指向 `3 4 5`（VTK 注释写作“`(0,2,1)` 法向背离 `(3,4,5)`”）。四边形面 `0 1 4 3`、`1 2 5 4`、`2 0 3 5`。
  - pyramid `0–4`：底面 `0 1 2 3` 右手法向指向顶点 4。
- 拆分：切面、体积、质量与外表面在加载时把单元临时拆成四面体（不落盘）。四边形面对角线取全局最小节点下标所在的对角线（Dompierre 等的最小下标规则），因此相邻单元共享面拆分一致、结果共形；hex 按 64 种面对角线组合查表（实际可达 42 种，每种 5–6 个四面体），wedge 3 个，pyramid 2 个。输入方向整体为负的单元先镜像（tet `0 1 3 2`、hex `0 3 2 1 4 7 6 5`、wedge `0 2 1 3 5 4`、pyramid `0 3 2 1 4`）再拆分；正负混合（扭曲）或节点重复的单元拒绝。区域按单元记录，校验要求同一单元的四面体区域一致，并重算拆分与 `tet_cells` 比对。
- 资产 v4：纯四面体资产块布局与 v3 完全相同（仅头部版本为 4，目录摘要不变）。混合资产不写 `VolumeTets` 流，改写 `VolumeCells`（流 8，每单元 40 B：u32 VTK 编码、u32 区域、8 × u32 节点，不足补 `u32::MAX`）与 `VolumeCellSummary`（流 9，恰好 1 条 8 B 记录：拆分四面体数，供流式读取预估）。这两个流只允许出现在 v4 头部下，且与 `VolumeTets` 互斥。`.presult` 容器版本仍为 3，结果场单元数按原始单元计。`VolumeAssetChunks::visit_cell_tets` 逐块读取单元、就地拆分并给出所属单元 ID。
- 导入导出（030 PR 2）：VTU 与旧版 VTK 读取 tet(10)、voxel(11，按 `0 1 3 2 4 5 7 6` 改写为 hex)、hex(12)、wedge(13)、pyramid(14)；polyhedron(42) 与其他类型（含二次单元）以明确错误拒绝。纯四面体输入仍得到 `cells = None`。VTU 导出按原始单元写 `connectivity` / `offsets` / `types`（纯四面体输出逐字节不变）；单元未驻留时由 `VolumeAssetChunks::visit_cells` 流式写出，内存每单元额外 1 B（类型），与驻留导出逐字节一致。
- 单元对应：单元场长度、场剖面单元值、外表面所属单元、校验消息（`cell N (类型) sub-tetrahedron K ...`）都按原始单元号；拆分四面体只是内部计算单位。
- 质量（030 PR 2）：每类单元给 Verdict 风格缩放雅可比（tet `√2·6V/max 角点边长积`；hex 8 角点 + 中心主轴；wedge 6 角点 × `2/√3`；pyramid 4 个底角 × `√2`；理想单元为 1，≤ 0 为反转或退化），统计最小值、平均值与低于 0.2 的劣质数。最小二面角与长宽比只统计原生四面体（拆分出的四面体不计）。劣质单元总数 = 四面体按二面角 / 长宽比规则 + 其他类型按缩放雅可比规则。驻留、流式（逐原始单元）与分块路径共用同一累加器。
- 分块（PR 3，`.pchunk` v3）：混合资产同样生成派生分块文件。k-d 划分以原始单元为单位（按单元节点质心），一个单元的全部拆分四面体落在同一块；块表与块头加原始单元数。块内 `global_cells` 记每个拆分四面体所属的原始单元 ID（非降序），非纯四面体块在单元段末尾追加块内单元布局（u32 单元数、每单元 1 B VTK 编码、块内局部节点连接；纯四面体块写 0）。单元场的块内值、外表面值与场范围都按原始单元；逐类型质量在块内按原始单元计算。v2 文件因版本不符被拒绝，应用派生缓存自动重建；复用时另核对原始单元数。构建内存估算对混合资产另加每拆分四面体 4 B、每原始单元 41 B。流式写出 `write_volume_asset_streaming` 在 `cells != tets` 时写 `VolumeCells` / `VolumeCellSummary`，逐条校验区域、节点范围与可拆分性，并核对拆分四面体总数。

## 生成流程

应用服务确认几何有效，冻结输入修订和网格参数，创建后台任务，再由 Netgen 适配器生成输出。完成后转换为 Mesh IR、检查有效性、形成质量摘要，最后一次性提交新的网格资产。

最初提供全局尺寸等少量可解释参数；局部加密、曲率控制等选项在验证后逐步增加。界面显示参数单位、有效范围和预计代价，避免暴露未经封装的库内部配置。

生成中不能覆盖当前有效网格。取消、失败或输入几何已变更时，丢弃或隔离候选输出；成功提交才触发视口刷新、工程 dirty 状态和旧 Study/结果的依赖失效检查。

## 有效性与质量

至少检查连接索引范围、非有限坐标、重复/退化单元、单元方向、区域分组完整性，以及适用于目标分析的体积或面积条件。质量指标及阈值必须声明适用单元类型，不能用一个未定义的“quality”数字覆盖所有网格。

几何到网格的边界映射是分析配置的关键资产。记录每个分组对应的几何实体和网格实体；转换后验证分组没有意外丢失。生成完成不等于适合所有求解器，还需经过求解器能力和单元支持检查。

## 显示与缓存

体网格可派生外表面用于高效显示，裁剪和截面可按需要访问体单元。用于显示的抽稀结果或缓存不能回写为分析输入。缓存以网格修订和显示参数为键，在资产更换后失效。

验收覆盖一个简单闭合实体、失败/取消场景、边界映射和存储往返。性能指标在有代表性样例后设定，不在当前文档承诺未经测量的规模或速度。

## out-of-core 分块体网格（task 027 原型，task 028 接入应用）

- 模块 `panta_mesh::chunked`：导入时 k-d 分块（默认每块 ≤ 262144 四面体），块按 Morton 序写入单个 `.pchunk` 文件，每块独立 zstd 压缩。块表常驻，记录包围盒、场值范围表、16³ 占据体素；外表面与简化外表面在打开时一次读入常驻。
- 块内保留原始单元顺序，存全局单元 ID / 全局节点 ID，以后的结果场按全局 ID 对齐。新格式（结果场、混合单元、OpenFOAM / CGNS）都要能写成这一分块格式。
- 读取经 `ChunkSource` 接口：默认 `MmapChunkSource`（整文件只读映射，零拷贝，0 次读调用；task 028），映射不可用时回退 `FileChunkSource`（定位读）；以后可换远程源。映射的 unsafe 只在 `panta_mesh::mapped`。打开时校验长度、头尾与块表，每块解压时校验 zstd 校验和与计数；`ChunkCache` 是按字节计的 LRU，未命中的相邻块合并成 MiB 级读。`Prefetcher` 后台预取，新请求覆盖旧请求。
- 切面：拖动时粗级别只用常驻数据（体素 + 简化外表面），松手后精细级别对命中块并行求交，结果与 `section_surface` 位级一致。质量统计按块缓存。
- 内存：块缓存与按块批处理都受预算约束；实测见 [task 027](../task/027-out-of-core-block-prototype.md)。
- 应用接入（task 028）：体网格模式三选一——按阈值分块（默认 1000 万四面体）、按可用内存自动（阈值 = 可用物理内存 / 2 / 190 B 每四面体，限在阈值合法范围内；查询不到时用手设阈值）、禁用分块完整驻留。超过有效阈值的体网格激活时只驻留外表面（025 流式路径），随后在应用缓存目录 `volume-chunks/<资产摘要>.pchunk` 打开或生成派生分块文件；已有有效文件直接复用，生成前估算内存（每四面体 48 B + 节点 / 边界），超过可用内存一半时跳过，生成失败或取消都不影响激活，回退 025 流式诊断与切面。派生文件先写临时文件再 rename，按访问时间只保留最新 4 个。有分块时质量与切面走 `ChunkedVolume`（切面位置与驻留路径位级一致），导出仍读源资产。块缓存预算（64 MiB–1 TiB，默认 1 GiB）修改后立即作用于已打开的分块体网格。
- 完整驻留模式不使用映射：以 8 MiB 缓冲顺序读入进程自有内存，读完立即关闭文件、释放锁。映射只用于分块按需读，安全前提与残余风险见 `panta_mesh::mapped` 模块文档与 [task 028](../task/028-out-of-core-rollout.md)。
- Linux glibc：应用入口在未设置 `MALLOC_ARENA_MAX` 时调用 `mallopt(M_ARENA_MAX, 2)`，避免多 arena 滞留释放后的 MiB 级缓冲；Windows / macOS 不受影响。

## 结果场（task 029）

- 来源：「实体 3D」导入的 VTU 与旧版 VTK 中的 PointData / CellData。首期保留 1 或 3 分量数组：VTU `DataArray`（整数转 f64），旧版 VTK `SCALARS`（1 分量）、`VECTORS` / `NORMALS`、`FIELD` 数组。其他分量数、个数不符、名称非法或重复、无法解析的数组跳过，原因写入 ImportRecord `skipped_result_fields`，不影响网格导入。表面导入不读取结果场。
- 对齐：`tet_mesh_from_cells` 不重排节点与单元（只为定向交换四面体第 3、4 个节点），结果场下标与网格节点 / 单元一一对应。
- 存储：结果场写成独立的 `.presult` sidecar（`assets/results/<资产 ID>-r1.presult`），不改 Mesh 资产格式和摘要；容器沿用 v3 布局（56 B 头、块表、64 B 对齐 zstd 块、每块 SHA-256），kind = 3，Mesh 读取器与结果读取器互相拒绝对方文件。字段表记录名称、关联、分量数、单位、缺失数与范围，并绑定网格资产摘要和节点 / 单元数；数值为 f64，每个字段单独切块（约 4 MiB，块不跨字段），读取一个字段只读它自己的块。清单 `mesh_assets.<id>.results` 记录路径、摘要、格式版本与字段数。
- 缺失值：任一分量非有限即计为缺失，数值原样保留（不替换为 0）；范围按标量值或多分量模长统计，忽略缺失值。读取字段时重算摘要并与字段表逐位比较。
- 分块携带（029 PR 2a）：`.pchunk` v2 布局为 `头 16 B | 单元段 | 场段 | 表面段 | 块表（zstd）| 尾 32 B`。块表记录字段名、关联、分量数，每块每字段的范围（标量值 / 矢量模长，忽略 NaN，无有限值为 NaN）与值块位置。场段每个字段先放所有块的内部值（节点场按块内节点序、单元场按块内单元序），再放所有块的外表面值（与块外表面三角形同序：节点场每三角形 3 个顶点值，单元场 1 个所属单元值），每块一个 f64 小端 zstd 帧。构建入口 `build_chunked_volume_with_fields` / `build_chunked_from_asset_with_fields`，字段由 `FieldProvider` 逐个加载（`ResidentFields`、`ResultAssetReader`）。
- 场剖面：驻留 `section_surface_field` 与分块 `ChunkedVolume::fine_section_field` 的几何与无场剖面位级一致；节点场先按分量线性插值再归约（模长或指定分量），单元场取所在单元值，被裁剪外表面三角形取其所属四面体值；缺失值按 NaN 传播。分块路径外表面值只常驻当前一个字段，命中块内部值按次合并读取、不进 LRU。
- 应用接入（029 PR 2b）：激活请求携带清单中的 `.presult` 引用；派生分块文件在核对结果摘要、绑定网格摘要与计数后带场构建，文件名 `<资产摘要>-<结果摘要>.pchunk`，不符时按无场构建。工程 API：驻留 `section_plane_field`（字段经 `read_result_field` 整字段读取），单元未驻留 `streamed_section_plane_field`（只走派生分块；逐次流式回退返回 CommandInvalid）。
- 显示（029 PR 3）：FFI `project_service_result_fields` 只读 sidecar 头列出字段；`section_field_mesh_display` 返回切面坐标与逐顶点 f32 标量（范围 f64、缺失顶点数）。驻留路径服务级缓存一个字段。VTK 以 `field_color_stops`（Cool to Warm）构建 `vtkColorTransferFunction`，NaN 为黄色；QML 色标复用同一组控制点，位于视口矩形外。
