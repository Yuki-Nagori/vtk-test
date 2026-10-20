# 几何模型与 OpenCASCADE 适配

[架构总览](README.md)

## 职责

几何模块负责 STEP 导入、几何资产管理、拓扑枚举、显示所需离散数据以及可被网格模块引用的几何身份。OCCT 作为实现后端，封装在 `native/geometry/src/occt/`；公共接口使用自有句柄、ID 和诊断类型。

V1 先保证导入与检查闭环，不预设完整 CAD 建模、布尔编辑或自动修复能力。原始输入和派生显示数据应分开管理，不能把三角化表面当成完整 B-rep 替代品。

STEP 导入由 `native/geometry/src/occt/step_reader.cpp` 调用 OCCT 并持有 native shape，返回带 revision 的进程内句柄供摘要查询和 Netgen 使用。句柄必须显式释放，不能作为工程持久资产 ID。Rust 几何资产服务尚未接入；工程身份、修订与提交规则仍应归 Rust，详见[适配边界](native-domain-boundaries.md)。

## 从 STL 表面构造 B-rep

未来的 STL 修复路线以 Rust `panta-mesh::SurfaceMesh` 作为来源资产。Rust 领域任务根据几何摘要、用户选择和领域约束决定是否转换、采用哪类操作及其可移植选项；C++ OCCT adapter 将这些选项映射到 OCCT 专属参数，将三角片构造成 faceted B-rep、执行明确范围的 B-rep 修复并返回结构化诊断。Rust 验证结果后提交一个新的 `GeometryAsset` ID / revision，记录它来源于哪个 Mesh revision；失败、取消或来源 revision 变化不能覆盖旧 Mesh 或已提交几何。

由三角片重建的 faceted B-rep 不会恢复原始 STEP/IGES 中的解析曲面、尺寸约束或设计历史。`ShapeFix` / `ShapeUpgrade` 的具体能力要按操作和输入 B-rep 验证；`BRepMesh` 负责从 B-rep 生成离散三角化，不等同于对任意 STL 表面网格做通用质量优化。曲面拟合、闭合判定、修复容差、拓扑身份映射和可逆性必须由单独任务定义，不可将 OCCT 接口调用描述为可无损“修复 STL”。提交后的 B-rep 需要独立的数据文件格式 / native shape 所有权策略；Mesh sidecar Mesh sidecar 只负责 Surface / Volume Mesh 资产，不持久化 OCCT 对象。

## 导入流程

1. 校验路径可访问性，建立后台任务并记录来源。
2. 通过 OCCT 适配器读取 STEP，收集警告和失败诊断。
3. 确定单位、包围盒、部件与拓扑层级；遇到未知单位时显式要求用户确认或给出可追踪策略。
4. 建立工程内几何实体 ID，生成可视化所需离散表示。
5. 仅在成功后提交工程变更，失败时保留此前有效状态。

大型或异常模型要支持进度反馈，必要时分阶段取消。导入单位转换必须一致影响坐标、长度参数与网格尺寸，避免界面看似正确但数值相差数量级。

## 拓扑身份与选择

至少区分实体、面、边以及显示用图元。当前 native `FaceId` 是导入 revision 内由去重拓扑遍历生成的零起 ID；常规 shape 摘要查询只复制缓存元数据，只有显式请求面级信息时才遍历并返回面边界盒。Netgen 边界分组通过拓扑对象相等性映射回同 revision 的 `FaceId`。它用于本次运行期转换，不保证跨导入或拓扑修改稳定，也不是工程持久 ID。视口拾取结果最终需要映射回 Rust 工程实体 ID，而不是保存 VTK actor 指针或 OCCT 对象地址。

重新导入或拓扑修改可能破坏原有面/边映射。没有稳定映射证据时，将相关 Study 边界选择标记为失效，要求重新确认；不能假设数组索引不变。几何修订号用于识别派生网格和缓存是否过期。

## 与网格和显示的边界

几何模块提供适合网格生成器使用的输入访问，但不负责 Netgen 参数和质量统计。几何三角化供视口显示，分析网格由[网格模块](mesh.md)拥有，两者不能因外观相似而混用。

验收使用可分发的小 STEP 样例，检查单位、拓扑数量、包围盒、选中映射，以及无效文件的错误恢复。源文件缺失后的工程行为由[存储策略](application-and-storage.md)决定。
