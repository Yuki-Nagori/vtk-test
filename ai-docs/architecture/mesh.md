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

工程 schema 4 在 `.panta` 中保存 `mesh_assets` 索引，Mesh payload 以工程内相对路径旁置，打开工程不立即读取网格。`crates/panta-mesh/src/asset/mod.rs` 定义 sidecar v3：56 B 头部（类型、毫米单位、目录 SHA-256）后接分块目录，数据块按 64 B 对齐、每块约 4 MiB 未压缩记录、独立 Zstandard 压缩（不可压缩时原样保存）并带块级 SHA-256；资产摘要为目录摘要。目录条目为 LOD 层级、时间步和块内过滤器预留字段，v3 要求为 0、读取端显式拒绝非 0 值。`MeshAssetReader` 先校验头部与目录，再按批读取数据块并多线程解码，批次间响应取消。v2（单 Zstandard 流 + 整段摘要）只读兼容见 [task 011](../task/011-mesh-asset-chunked-storage.md) 与清理任务 [012](../task/012-mesh-asset-v2-removal.md)。解码验证版本、摘要与范围后构造领域 Mesh；路径授权由工程服务负责。SurfaceMesh 保留完整三角面顶点语义，新导入不复制来源 STL，旧 schema 不自动迁移。

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

## out-of-core 分块体网格（task 027，原型，未接入应用）

- 模块 `panta_mesh::chunked`：导入时 k-d 分块（默认每块 ≤ 262144 四面体），块按 Morton 序写入单个 `.pchunk` 文件，每块独立 zstd 压缩。块表常驻，记录包围盒、场值范围表、16³ 占据体素；外表面与简化外表面在打开时一次读入常驻。
- 块内保留原始单元顺序，存全局单元 ID / 全局节点 ID，以后的结果场按全局 ID 对齐。新格式（结果场、混合单元、OpenFOAM / CGNS）都要能写成这一分块格式。
- 读取经 `ChunkSource` 接口：默认 `MmapChunkSource`（整文件只读映射，零拷贝，0 次读调用；task 028），映射不可用时回退 `FileChunkSource`（定位读）；以后可换远程源。映射的 unsafe 只在 `panta_mesh::mapped`。打开时校验长度、头尾与块表，每块解压时校验 zstd 校验和与计数；`ChunkCache` 是按字节计的 LRU，未命中的相邻块合并成 MiB 级读。`Prefetcher` 后台预取，新请求覆盖旧请求。
- 切面：拖动时粗级别只用常驻数据（体素 + 简化外表面），松手后精细级别对命中块并行求交，结果与 `section_surface` 位级一致。质量统计按块缓存。
- 内存：块缓存与按块批处理都受预算约束；实测见 [task 027](../task/027-out-of-core-block-prototype.md)。
