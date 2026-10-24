# OpenCASCADE / OCCT 几何适配

更新日期：2026-10-24。OCCT 8.0.1 制品消费与 STEP 适配已存在；SDK 自任务 041 起由本仓库生产与发布（配置与原 Panta 一致），当前平台验证单独记录。

适用于 `native/geometry/src/occt/`（适配层与 `mesh/src/netgen`、`visualization/src/vtk` 同构）。OCCT 固定 8.0.1（`sdk-provision.cmake` 清单，Release `sdk-occt-netgen-8.0.1-6.2.2604`）；在线文档重定向后的 API 不能直接当成本地已可用 API，以锁定制品头文件为准。

## 官方依据

基础 STEP translator 与 XDE 路径的元数据能力不同；XDE 用于名称、颜色等附属信息。读取与转换是需检查结果的多个阶段。[OCCT STEP Translator](https://dev.opencascade.org/doc/overview/html/occt_user_guides__step.html)

OCCT 有自己的基础类型、内存与 handle 机制，不能以通用裸指针所有权习惯替代。[OCCT Foundation Classes](https://dev.opencascade.org/doc/overview/html/occt_user_guides__foundation_classes.html)

## 职责边界（2026-09-23 项目约定）

Rust 拥有导入业务、几何身份与修订；C++ adapter 实际调用 OCCT 算法，处理异常 / 所有权 / 数据转换，不自行重写几何内核。 当前实现与迁移项见 [重库适配审计](../architecture/native-domain-boundaries.md)，执行 [分层规则](layering.md)；规划不代表已落地。

## 项目规则

- 所有 OCCT include 和类型限定在 native adapter 内；公共几何契约使用自有句柄、单位、修订和诊断。现有 `shape_repository_internal.hpp` 是 geometry 与 mesh adapter 之间的私有借用契约，不进入公共 include 或 CXX；其 `ShapeRef` 只是进程内运行期句柄，不代表工程 ID。
- 适配选型：现有 adapter 使用 STEPControl 基础路径，装配/名称/颜色等 XDE 附属元数据不保留（已记录取舍）；完整导入业务如需元数据再评估 STEPCAF 路线，不向用户承诺完整 STEP 语义保真。
- 每次导入检查读取状态、转换结果、有效 shape 数与空形状；记录单位和公差策略。修复行为必须留诊断，不能默默改变模型再报告无误。
- 正确使用所选版本的 handle 类型；复制拓扑句柄不能自动视为深拷贝。修改共享 shape 前明确共享关系，避免后台操作改变当前场景。
- 导入采用独立上下文并保守串行（适配器内进程级互斥锁）；不并发修改进程级 translator 参数。允许并发时逐项记录版本、API 和参数隔离证据。
- 持久拓扑 ID 由工程领域管理，OCCT 对象地址与遍历序号不作为持久化标识。当前 native `FaceId` 仅在 `ShapeRef` revision 内供同一次 Netgen 映射使用，重新导入后必须重新映射所有边界引用。
- B-rep、显示三角化与分析网格分开；公差与显示精度均记录单位，禁止通过调大公差掩盖任意坏模型。

## 集成验证与消费约定

- 消费统一走 `panta_require_sdk("occt")`：下载/SHA256 校验/原子发布 staging 后执行 `find_package(OpenCASCADE CONFIG)` 并断言 required targets。OCCT targets 无命名空间（`TKernel`、`TKDESTEP` 等）；imported targets 为目录作用域——跨目录消费需在消费目录重新 `panta_require_sdk("occt")`（缓存 marker 命中，不重复下载）。
- OCCT 8 起句柄类型为 `occ::handle<T>`；自有公共契约（`panta/geometry/step_import.hpp`）不出现任何 OCCT 类型，第三方库一律 PRIVATE 链接（静态库符号经 LINK_ONLY 传播）。
- 单位契约：坐标与包围盒一律毫米。源文件单位用 `STEPControl_Reader::FileUnits()` 识别（小写 ISO 名，如 `millimetre`）；该接口在损坏文件上会段错误，只在 `ReadFile == RetDone` 后调用。`StepModel::LocalLengthUnit`/`IsInitializedUnit` 是转移目标单位（恒 mm、转移前未初始化），不代表源单位。
- 包围盒：`BRepBndLib::Add` + `Bnd_Box::Get` 返回含形状公差 gap 的保守包络（典型 `Precision::Confusion` ≈ 1e-7 mm）；对外报告该包络并在契约注明，不通过调大公差掩盖坏模型。
- 异常边界：OCCT 异常（`Standard_Failure` 等）在适配器边界捕获转结构化错误状态；实测缺失文件为 `RetError`，垃圾/空文件为 `RetFail` 并伴随 StepFile 解析错误直接输出 stderr（上游 printer 行为）。
- 运行库定位：macOS/Linux 的 dylib/so 为 `@rpath` 安装名，CMake 构建 rpath 自动解析；Windows DLL 在嵌套 `win64/vc14/bin/`，测试进程经 `native_test_env` 递归收集 bin 目录注入 PATH。模块单测如需夹具路径宏，注入裸路径 + 代码内字符串化。

## 验证

小模型测试覆盖读取失败、空输出、单位和装配/颜色保留策略；检查 Netgen 与桌面使用的 OCCT ABI/版本一致性。adapter 和样例可用不能证明 GUI / project 已完成 STEP 导入，完整业务路径需单独实现并验证。
