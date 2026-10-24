# 重库适配与 Rust 领域模块

[架构总览](README.md) · [分层规则](../standards/layering.md)

更新日期：2026-10-20。本文说明最小渲染测试仓库的职责边界：project 承载模型资产与场景，Rust 管理身份、修订和事务，C++ 调用 OCCT、Netgen 与 VTK。现有 STL、Mesh sidecar 和按需激活以实际源码为准，STEP 工程闭环、体网格与修复流程仍须逐项集成验证。

## 分层与调用

```text
QML → C++ ViewModel → CXX → Rust 应用服务 / 领域模型
                                  ↓ 通过注入的后端接口提交粗粒度操作
                              CXX → C++ adapter → OCCT / Netgen

Rust 已提交的资产 / 显示配置 → CXX → C++ RenderScene / ViewportBackend
                                             ↓
                                C++ VTK 后端 → VTK / WebGPU
                                （窗口、输入、逐帧更新在本地完成）
```

Rust 管理工程身份、单位、修订、引用、任务与成功提交，C++ 适配器负责实际调用重库、转换输入输出及管理 native 资源。CXX 声明受支持的类型和签名、生成胶水，不决定业务行为。调用流程不等同于 crate 依赖图；领域层通过自有后端接口接收能力，不反向依赖桥接 crate。

Qt 是体量较大的原生 GUI 框架，在这条边界中单独作为**界面与平台运行时**看待，不与 OCCT / Netgen 的几何算法适配角色混为一谈。QML / Qt Quick 继续负责界面组合、布局、输入、动画和即时显示，使用 Qt scene graph、Controls、模型视图等已有的渲染与交互能力；C++ ViewModel / Controller 负责 Qt 对象生命周期、信号属性、模型 / 委托接口、线程亲和性、平台对话框及 QML 与服务 DTO 之间的类型适配。`native/qt-adapter` 集中标准目录、系统偏好等需要选择 Qt 平台 API 或操作系统 API 的服务，并以稳定值和 Qt 属性交给上层；该 target 不依赖 Rust、Bridge 或 VTK。Qt Quick 与 VTK hardware window 紧耦合的 surface 创建和同步仍由 Visualization 管理，避免把 VTK 窗口所有权抽到通用平台层。继续使用当前 Qt 基线和其已供给的界面、绘制及平台能力，包括 Qt 已有的高性能场景图和适用的绘图 / 可视化组件；不为语言统一而在 Rust 重写 Qt 的图形、事件循环或工具能力。

Qt 边界不改变领域权威归属：工程身份与持久化、导入解析、单位 / 修订 / 引用规则、成功提交和业务任务由 Rust 服务管理；QML 表达展示与用户意图，C++ ViewModel / Controller 只做 Qt 适配并转发粗粒度命令。界面和操作提示直接使用中文；Bridge `Settings` 只适配系统减少动态效果偏好。Rust 不引入 Qt 类型或事件循环。Rust 服务应按用例返回拥有内存的批量快照 / DTO；C++ 可把 DTO 映射为 Qt 属性或 `QAbstractItemModel` 角色供 QML 使用，但不能让 ViewModel 通过逐行 FFI 查询拼装业务对象，也不能把 Rust 领域状态藏入 QObject 或 QML 作为第二份权威数据。跨过 CXX 边界后，传给 Qt 的数据必须由 C++ 值对象或具有明确共享所有权的不可变快照承载；不能把借用的 Rust 缓冲区直接放进排队信号。跨线程信号参数须满足 Qt queued connection 的类型和生命周期要求，并按 Qt 要求注册元类型；具体 payload 大小时，在对应任务中确定复制、移动或共享快照策略。

运行期 Mesh 驻留策略也属于 Rust 工程服务：`ProjectService` 持有解析后的 `SurfaceMesh`，决定缓存预算、LRU 淘汰、活动文档 pin、缓存命中和关闭释放；其 `DocumentWorkspace` 拥有打开页签、当前活动文档、关闭回退和加载状态，向 Bridge 批量报告工作区与驻留快照。ViewModel 只投影 Qt 属性，并在视口活动时持有单份 C++ 显示 DTO。激活结果的过期判断、缓存提交、attempt 收尾与工作区更新应由 core 的完整用例处理；FFI 只转换最终结果，不编排这些业务步骤。`MeshSource::mesh_snapshot()` 是本地共享指针读取，VTK / Qt Quick 的刷新与逐帧路径不得通过此 getter 调用 CXX/Rust；跨边界复制仅在导入、选中、关闭或异步激活等粗粒度状态变化时发生。C++ VTK 后端负责把活动 DTO 转成 `vtkPolyData` 和管理显示资源；不得在 ViewModel 再维护第二份按文档索引的 Mesh 缓存或自行实施预算 / LRU。

大网格的显示准备属于粗粒度数据适配，不应占用 GUI 线程。服务线程捕获拥有共享所有权的 `Arc<SurfaceMesh>` 和文档身份，在后台展开坐标并构造拥有数据的 C++ 显示 DTO；最多保留一个在途显示准备，完成后以最新选择及 generation/revision 校验拒绝迟到结果，再处理最新待显示选择。该过程不移动 QObject、VTK 或 GPU 资源，不改变它们的原线程生命周期，也不建立第二套领域 Mesh 缓存。资产激活与 sidecar 已记录的同步物化测量是迁移前基线，不能当作异步实现的性能结论。

鼠标悬停、当前行高亮和即时交互反馈属于 Qt / QML / VTK 显示状态。若用户选择的实体将影响网格操作、边界条件或工程提交，稳定实体 ID、所属修订和引用有效性由 Rust 建模与校验；Qt 保留命中反馈并把完整、粗粒度的选择意图交给服务。选择意图应携带稳定实体 ID 集合、所属修订和选择模式（如替换、追加、移除或清空）；不能只把易变化的行号、显示索引或像素位置当成领域身份。纯显示高亮则无需因此升级为 Rust 领域状态。

Qt 对象、GUI 事件循环和线程亲和性留在 C++。仅当 Rust 调用是有明确上界的内存内元数据操作、不包含文件 / 网络 I/O、格式解析、重库调用、网格生成或等待锁 / 后台任务，并且能证明在一个 GUI 帧预算内完成时，才可同步调用 Rust；默认要求在项目支持的最低配置上 p95 不超过 1 ms，未测量或超出预算时走异步路径。文件读取、格式解析、网格生成及其他不可预测耗时的用例应通过 Rust 后台任务执行，并以拥有内存的结果 DTO / 任务事件异步返回，通过 Qt 队列交付到 GUI 线程后更新 QObject/QML。GUI 线程不能同步等待长任务，后台任务不能访问 QObject；任务契约应定义取消何时生效及提交截止点，QML 只能表达取消意图，不能自行假设后台任务已停止。失败若保留旧状态，应继续暴露仍有效的旧快照 / 实体及其修订；具体由任务 / 领域服务定义，并拒绝过期修订提交。既有同步路径由对应功能任务跟踪迁移，不作为新增功能的默认模式。

继续使用仓库当前固定版本已经链接的 Qt 运行时模块：Qt Core / Gui / Qml / Quick、Quick Controls、Quick Dialogs 和 Svg；Qt Quick scene graph 是界面渲染能力，当前视口的几何显示由 VTK 承担。继续使用 Qt Declarative / Tools 提供且仓库实际接入的开发工具（如 `qmllint`、`qmlformat`）。Qt Charts / Graphs、Qt 3D、WebEngine 等其他运行时模块不因属于 Qt 而默认获准；新增模块必须由实际产品任务说明能力需求、已有模块是否可满足、固定版本、构建 / 打包 / CI 成本、平台覆盖和线程 / 渲染边界。也不能只因存在 Rust 替代项就放弃适合的 Qt 实现。Rust 不依赖 Qt 类型、对象或事件循环。

Rust 服务和领域操作失败时应通过结构化错误 DTO 返回稳定错误码、类别 / 严重级别及必要上下文（例如相关实体 ID 与修订）；不得只返回供界面解析的拼接文本。C++ ViewModel 将结构化字段适配为 Qt 属性 / 模型角色，并提供适合界面的本地化消息；QML 可按稳定错误码选择交互，但不得通过拆分、匹配错误文案来推断错误类型或领域状态。若操作失败但保留了旧状态，服务应继续返回当前有效的旧快照 / 实体及其修订，让界面能同时表达失败和仍可用的数据；错误 DTO 本身不取代该状态。

“不直接链接 OCCT/Netgen”在此指 Rust 业务代码不得直接声明或调用重库 ABI、不得暴露其对象布局；桌面最终产物仍需链接重库。当前最终链接由 CMake 拥有，CXX 不消除链接和运行库部署要求。

适配器对外使用小型值 DTO、连续数组或自有 opaque 所有者。大型 B-rep 无须强行完整复制到 Rust：C++ adapter 的进程内仓库持有并显式释放 native shape，STEP 读取返回摘要及 `ShapeRef`，Netgen 可复用同一个 shape。`ShapeId`、revision 和该 revision 的 `FaceId` 仅在当前进程有效，不是工程几何资产持久化或 Rust 几何领域模型。后续工程资产通路应由 Rust 保存稳定领域身份和修订，跨 FFI 传 ID、修订和操作结果，不泄漏裸句柄，并拒绝旧修订。

## 适配器允许做什么

| 内容 | 责任 | 不允许 |
|---|---|---|
| Qt Quick / QML 界面、窗口、输入、场景图渲染及 Qt 工具链 | Qt / QML 提供平台集成、事件循环、控件、布局、即时图形能力和开发检查工具；C++ ViewModel 适配 QObject 属性、信号和 Rust 值 DTO | Rust 依赖 Qt；ViewModel 或 QML 承担工程持久化、导入 / 几何解析、领域校验或重库算法；为迁移语言重写 Qt 已提供的图形 / 工具能力；无任务论证而新增 Qt 运行时模块 |
| STEP 读取、拓扑操作、几何修复、几何三角化 | C++ adapter 调用 OCCT；Rust 决定领域操作选项和是否提交，C++ 映射 OCCT 专属参数 | 适配层重写几何修复算法或自行决定领域提交 |
| 网格生成、库提供的网格优化 | C++ adapter 调用 Netgen；Rust 选择领域参数、输入修订与任务策略，C++ 映射 Netgen 参数 | 适配层维护独立于领域层的网格有效性规则 |
| 异常归一化、类型转换、数组展开、索引起点与单元节点顺序转换 | C++ adapter，属于接口语义转换 | 对外泄漏 OCCT/Netgen 类型或未声明所有权的指针 |
| 工程网格有效性、应用质量阈值、区域 / 边界引用、资产失效 | Rust 领域层；边界仍保留必要长度 / 溢出 / 范围检查 | Rust 重写重库的几何修复或网格生成算法 |
| 运行期 `SurfaceMesh` 缓存预算、LRU、驻留查询和关闭释放 | Rust `ProjectService`，其 `DocumentWorkspace` 管活动文档；C++ ViewModel 转发意图并投影批量工作区 / 驻留快照 | 在 ViewModel 按 ID 保留 Mesh 集合、计算预算、决定淘汰次序或另建活动文档权威 |
| VTK 数据对象、mapper/filter/actor、相机及显示命中、GPU 资源 | C++ 显示后端；调用 VTK 的绘制和数据处理能力 | 由 Rust 驱动逐帧导航或管理 VTK/GPU 生命周期 |
| 导入格式解析、领域网格 / 字段、显示选项与选择状态 | Rust；轻量解析 / 校验可自有实现 | 用泛型解析接口抹平不同格式的语义 |

“薄适配”限制领域策略与重库算法重写，不禁止必要转换中的循环和计算。Netgen 输出索引归一化、异常封装和 native 锁是适配职责。VTK 的 C++ 模块还包含原生显示控制器；将每个鼠标事件、像素命中和相机插值搬到 Rust 不会使接口更清楚。

## 工程配置数据与当前界面边界

`panta-core::project` 继续保存材料选择、分析序列、`FillSettings` 与 `GateLocationSettings` / `GateLocatorAlgorithm` 等领域值、校验和工程清单字段。保留这些 schema 与持久化测试是工程存储边界，不表示最小工作台提供材料、工艺或浇口配置编辑，也不表示相关算法或求解执行已经接入。

当前 QML / Bridge 仅围绕创建、打开、保存工程，STL 预检 / 导入与模型资产显示工作。CXX 不再投影材料 / 分析序列目录、方案草稿或配置确认请求；`ProjectViewModel` 不维护这些字段的 Qt 模型、计时器、pending 状态或完成信号。真实工程操作输入仅携带操作 kind、路径和名称。创建、打开和保存使用后台 I/O：begin 表示准入，拥有式请求在后台 wait，服务线程 finish 校验作用域并发布结果；准入不等于保存成功。

网格类型目录和导入默认值仍由 `panta-io` 提供，Qt 缓存实际导入表单需要的 `meshTypes` / `defaultMeshType`。`ImportRecord.mesh_type` 是导入时保存的选择，不证明体网格或其他派生网格已经生成。显示快照只在工程、导入和活动文档改变时读取，不进入逐帧渲染；源码入口为 `crates/panta-io/src/lib.rs` 和 `crates/panta-core/src/project/`。

导入资源提醒由 Rust 定义：`panta-mesh` 按未压缩 Mesh 资产布局估算字节数，`panta-io::StlImportPreview` 返回 `expanded_mesh_bytes` 和 `large_mesh_warning`。GUI 与 CLI 展示同一预览结果，不分别计算面数公式或维护阈值。严格超过 2 GiB 只提醒，不阻止导入；估算不代表进程 RSS、来源文件大小或 GPU 内存。阈值与估算通过工程服务回归验证。

## 当前 VTK 审计

路径均相对于仓库根。以下表保留职责审计的设计背景；已替换符号仅用于解释迁移边界，当前实现以 `crates/` 与 `native/` 源码为准。

| 现有位置 / 符号 | 结论与理由 |
|---|---|
| `crates/panta-mesh/src/lib.rs`：`parse_stl` | Rust 统一读取语义与数据校验，返回后端中立 `SurfaceMesh`；native 显示不另读来源路径 |
| `native/visualization/src/vtk/surface_mesh.cpp` 的数据转换 | 保留 C++：只把 Mesh 坐标 / 连接关系转换成 `vtkPoints`、`vtkCellArray`、`vtkPolyData`，检查索引转换和缓冲区有效期 |
| `RenderScene::mesh_path`、`CaeViewport::setMeshPath`、`VtkViewport::apply_state` / `update_mesh_actor` | 当前采用修订化 Mesh 显示快照；工程路径解析、I/O 和提交归 Rust，此处旧符号仅说明替换边界 |
| `navigation/viewport_camera.cpp`：`ViewportCamera`、`ViewportCameraTransition` | 保留 C++：操作 VTK 相机，状态仅用于即时导航；数学虽可移植，但当前无独立 Rust 消费者，不增加每帧 FFI |
| `navigation/viewport_input.cpp`：`classify_viewport_input` | 保留 C++：把 VTK 事件映射到本地动作。若将来支持可配置按键，Rust 管理偏好数据，C++ 使用一次下发的配置 |
| `navigation/viewport_orientation.cpp`：`pick_cube_face`、`ViewportOrientation` | 保留 C++：方向控件的显示命中反馈、坐标投影和 VTK 标记同步；影响工程操作的实体 ID、修订和引用校验未来归 Rust |
| `vtk_viewport.cpp` 的 timer / refresh / camera / window 管理；`vtk_native_surface.*` | 保留 C++：Qt 线程、像素尺度、VTK/GPU 生命周期与平台窗口强耦合 |
| 空文档工作区与无 Mesh 快照 | Rust 管理零文档和活动 ID 为空的状态；VTK adapter 保持空场景，不生成默认装饰几何 |

当前 STL 解析统一由 `panta-mesh::parse_stl` 执行，ASCII 路径拒绝额外字段并使用严格 UTF-8。预览、导入与显示复用解析结果或同一内容修订的缓存；旧 native 解析器不再是接受规则来源。新增格式应保留来源快照和提交一致性，并用合法与非法输入验证。

## STEP / IGES / STL 的导入扩展

Rust 工作区由 `panta-io` 承担统一的**导入服务契约**，不要求三种格式共用一个文本或几何解析器。请求记录格式、来源资产、用户确认的长度单位策略和工程修订；结果包含可提交的资产引用、数据类别、来源与诊断。`panta-io` 负责格式识别、来源快照、预览 / 导入准备和格式诊断；`panta-core` 负责工程修订、失败保留旧资产和持久化。格式解码器由拥有数据模型的领域 crate 持有：STL 解码器在 `panta-mesh`，未来 STEP/IGES 读取通过 `panta-geom` 的后端契约调用 OCCT。按实际消费者添加格式路由，禁止一个 `parse<T>` 之类的空泛框架遮盖三者完全不同的语义。可共享的是来源快照、单位策略、修订与失败保留旧资产的导入事务，不是解析接口；STL 返回 `SurfaceMesh`，STEP 和 IGES 分别返回带格式诊断的 `GeometryAsset`。

| 输入 | Rust 领域归属 | 实际读取 / 算法 | 输出契约 |
|---|---|---|---|
| STL | `panta-io` 分发，`panta-mesh` 持有网格及解码器 | `panta-mesh` 解析 binary / ASCII，得到表面三角网格；不假装 STL 包含 B-rep 拓扑或体网格 | `SurfaceMesh`、原始单位解释、摘要与解析诊断 |
| STEP | `panta-io` 分发，`panta-geom`（规划）持有几何 | C++ OCCT STEP reader / translator；装配、名称、颜色能力按所选路径明确 | 几何资产身份、单位、拓扑引用及按需显示三角化 |
| IGES | `panta-io` 分发，`panta-geom`（规划）持有几何 | C++ OCCT IGES reader / translator；实测后再确定修复与实体支持范围 | 与 STEP 同一几何领域契约，但保留格式特有诊断 |

跨格式共享的是导入编排和工程资产元数据，不是所有文件都先化成同一种 Mesh。STEP/IGES 的 B-rep 由 C++ adapter 的 native shape 所有者管理并释放，Rust 只保存稳定领域 ID 与修订号；跨 FFI 通过 ID 寻址，修订不匹配则拒绝操作。可视化三角化是派生缓存，不能替代几何资产。STL 是表面网格，若未来用于体网格生成，应经明确的转换 / 网格任务，不把它伪装成原生 OCCT shape。

从 STL 派生显示、修复几何和生成体网格时，目标职责保持如下；表中 B / C 尚无端到端实现：

| 阶段 | Rust 权威 | C++ / 重库适配 | 提交结果 |
|---|---|---|---|
| 读取 STL | `panta-io` 编排来源快照，`panta-mesh` 解析并拥有 `SurfaceMesh` | 无格式解析或业务选择 | 原始 source asset 与 Surface Mesh revision 分别标识 |
| A. 直接显示 | 选择活动 Mesh revision，返回拥有内存的批量快照 | C++ 将三角面转换为 `vtkPolyData`，VTK 管 renderer / WebGPU 生命周期 | 无工程写入，仅显示派生数据 |
| B. 修复为几何 | 决定是否启动转换任务、领域操作选项和来源 revision；校验并提交新 `GeometryAsset` | C++ adapter 将领域选项映射到 OCCT 参数，重建 faceted B-rep、执行范围明确的 B-rep 修复并返回诊断 | 新几何 ID / revision 与来源 Mesh revision 关联；原 Mesh 保留 |
| C. 生成体网格 | 选择 `SurfaceMesh` revision、领域网格参数和任务语义；`panta-mesh` 校验 `TetMesh` 并提交 | C++ adapter 调用 Netgen 的 STL 几何入口与适用的表面 / 体网格优化步骤，转换输出 DTO，不持有 Rust 领域规则 | 新 Volume Mesh ID / revision 与来源 Mesh 和参数关联 |

读取解析、OCCT 修复和 Netgen 生成由 Rust 后台流程编排，C++ adapter 映射领域参数并管理 native 所有权。具体入口见 `native/geometry/`、`native/mesh/` 和 `crates/panta-mesh/`；修复、取消与过期结果必须通过实际输入验证。Mesh sidecar 只持久化 Surface / Volume Mesh IR，不保存 OCCT B-rep，见[工程存储](application-and-storage.md)。

格式分发以显式声明或扩展名为入口，再由实际解析器验证内容；扩展名本身不保证格式有效。各格式返回结构化错误，公共流程只决定是否提交，不吞掉 OCCT / STL 的专门诊断。新的导入格式在有端到端使用者时才加入路由、bridge 和测试；若新增本构、网格生成或格式修复算法，仍调用相应重库或另立明确的轻量功能任务。

[资产激活](../modules/asset-activation.md) 使用直接的后台读取、校验与结果流程，Rust 校验工程代次、记录身份、attempt 与迟到结果。该只读加载不修改工程清单；STEP 或网格写入仍由 `panta-core` 的事务提交闸门裁决。

## OCCT / Netgen 当前边界

- `native/geometry/src/occt/step_reader.cpp` 的 `import_step` 一次读取 STEP、生成摘要并保存同一个 `TopoDS_Shape`。native 仓库返回的 `ShapeId + revision` 只是在当前进程中寻址的句柄，不是工程资产身份；调用方显式释放。该仓库负责 OCCT 对象的运行期所有权，不取代未来 Rust `panta-geom` 的资产与修订模型。
- `native/mesh/src/netgen/netgen_mesher.cpp` 的 `generate_tet_mesh` 从 geometry 仓库借用 shape 构造 Netgen OCC 几何，不再重复解析 STEP。借用期间持有 OCCT 串行锁；Netgen 面通过拓扑对象映射到导入 revision 的 `FaceId`，Mesh DTO 的连续边界组另带组到 FaceId 的映射。压缩组序号本身不作为几何身份。
- `panta-mesh` 负责校验与总体积计算：实现位于 `panta-mesh`。`native/mesh/include/panta/mesh/mesh_ir.hpp` 的 `Native*Dto` 和 `validate_native_mesh_with_rust` 只是 CXX 适配输入与结果，不是第二套 Mesh IR 领域模型；native mesh 保留 Netgen 转换、为满足 Rust 契约而调整节点顺序及必要的数组长度检查。私有 geometry→mesh shape 访问契约只供 native adapter 内部使用，不进入安装头文件或 CXX。Netgen 资源释放保留版本特定的 `COMPAT` 处理，退出条件见[Netgen 清理兼容跟踪](../task/002-netgen-cleanup-compatibility.md)。

## Rust crate 划分

五个领域名称可以采用，按实际功能逐步建立。`panta-io` 是已落地 STL 消费者的跨格式导入入口，负责来源快照和解析分发，不代替领域数据 crate。`geom` 表示几何领域，`bc` 明确表示 boundary conditions；公共文档和接口注释使用完整含义。

| crate | 拥有的内容 | 领域依赖上限 | 创建触发条件 |
|---|---|---|---|
| `panta-io` | `import`：格式识别、来源快照、预览、选项与导入准备；`export`：各格式写出 | `panta-mesh`；STEP / IGES 落地后按需依赖 `panta-geom` | STL 工程导入已有实际消费者 |
| `panta-geom`（规划） | Shape/Face/Edge ID、几何修订、单位、导入请求与摘要、拓扑引用有效性、后端操作契约 | 无 | 实现 STEP 导入到工程资产的完整通路；不存 `TopoDS_Shape` |
| `panta-mesh` | 表面 / 体 Mesh IR、连接关系、分组、来源几何修订、STL 解码、体网格校验与总体积摘要 | `panta-geom`，仅当实际需要来源几何契约时 | 统一 STL 解析与显示数据通路，并承接 Netgen DTO 校验 |
| `panta-material`（规划） | 材料定义、属性及单位、来源版本、表格数据和校验 | 无 | 首次材料编辑 / 持久化功能；不实现外部求解器的本构计算 |
| `panta-bc`（规划） | 边界条件定义、目标实体 / 分组引用、参数单位、修订与客户端校验 | `panta-geom` / `panta-mesh`，仅当实际需要目标引用契约时 | 首次 Study 边界配置；不负责组装方程或施加数值边界 |
| `panta-visualization`（规划） | 后端中立显示配置、字段关联与范围策略、实体选择语义、可保存相机书签 | `panta-geom` / `panta-mesh`，仅当实际需要显示来源契约时 | 首次字段显示 / 选择 / 显示配置持久化的真实消费者；不含 VTK/GPU/Qt 或逐帧导航 |

网格区域到材料的分配、边界条件与网格版本的组合由 Study 应用模型拥有；材料库不依赖网格，网格也不依赖材料或边界条件。VTK 收到已解析的显示快照，不理解求解器参数。

### 编译依赖方向

以下是目标上限，只有实际使用类型才添加依赖：

```text
panta-ffi（组装后端实现、DTO 转换） → Rust 应用服务
Rust 应用服务（当前 panta-core） → import / mesh；未来 → geom / material / bc / visualization
import → mesh；STEP / IGES 接入后 → geom
mesh → geom（仅来源几何契约）
bc → geom / mesh（仅目标引用契约）
visualization → geom / mesh（仅显示来源契约）
geom、material → 无其他上述领域 crate
```

当前 `panta-core` 包含 project/path/task 和工程存储实现，不能同时被当成底层共享类型库又依赖这些领域 crate。过渡期 `panta-core` 只保留应用服务与工程存储，领域 crate **不得反向依赖 `panta-core`**。ID、单位和错误等类型先留在各自的拥有者中；出现第二个实际消费者、且确认语义相同时才考虑抽出低层契约 crate。禁止为将来可能复用而提前建立 `panta-common`。后续随真实消费关系拆分 project/workflow/storage，再收缩 core。

`panta-ffi` 当前依赖 `panta-core`，因此不能让 core 再依赖 ffi 来调用重库。规划做法是在领域 / 服务层声明窄后端接口，ffi 的组装层实现该接口并注入服务；CXX 模块只声明绑定，组装函数只转发与转换。编译依赖无环，运行时服务仍可请求 native 工作。具体接口、线程和最终链接须在首个实现任务验证。

## 实施顺序与门槛

1. `panta-mesh` 提供 STL 解析、表面网格、体网格校验及总体积计算；批量快照交付 C++ VTK 转换，native DTO 不重复定义领域规则。
2. STEP 工程闭环和 Netgen 后台服务接入应明确 native shape 所有权、取消、安全串行、失败保留旧资产及修订检查。
3. 仅随真实使用者拆分新 crate；材料、Study 和求解方向作为扩展参考，不为最小测试仓库预建空模块。
