# 分层与依赖方向

更新日期：2026-10-20。来源：本仓库分层约定，对全部改动生效；违反即评审问题。当前实现与迁移顺序见 [重库适配与 Rust 领域模块](../architecture/native-domain-boundaries.md)。

## 结构

```text
QML → C++ ViewModel / Controller → CXX → Rust 应用服务 / 领域模型
                                         ↓ 自有后端接口
                                     CXX → C++ adapter → OCCT / Netgen

Rust 资产 / 显示快照 → CXX → C++ RenderScene / ViewportBackend → VTK
                           （原生窗口与逐帧显示留在 C++）
```

图示表示职责方向，不能据此推断规划 crate 已建立。当前 STEP shape 仓库和 Netgen 消费保留 native 适配入口；Netgen Mesh IR 接入 Rust 权威模型与校验，GUI / project 对 STEP 的完整业务接入仍需独立实现与验收。

## 当前分组与依赖

以下层级依据当前 `Cargo.toml` workspace dependencies、native `target_link_libraries` 与 QML module 声明核对。层号分别在 Rust 图和 Native 图内解释；Rust staticlib、CXX、Qt/QML 是显式跨边界连接，不用层号伪装成同一种依赖。表内“→”表示左侧依赖右侧。测试 target 可依赖被测 target 与测试框架，不反向进入生产依赖图。

### Rust 层级

| 层级 | 当前成员 | 当前仓库内依赖 | 说明 |
|---|---|---|---|
| Rust L1：基础与叶子库 | `panta-foundation`、`panta-build`、`panta-mesh` | 不依赖其他 workspace crate | 基础设施、构建工具与网格 IR / 解析 |
| Rust L2：领域适配与工具入口 | `panta-io` | `panta-io → panta-mesh` | 导入格式适配，不成为应用组合根 |
| Rust L3：应用 / 工程服务 | `panta-core` | `panta-core → panta-io, panta-mesh` | 编排工程状态、资产和领域服务 |
| Rust L4：语言边界 | `panta-ffi` | `panta-ffi → panta-core, panta-mesh, panta-foundation, panta-build` | DTO、错误、句柄转换与服务转发，不承载领域算法 |
| Rust L5：应用消费者与桌面启动组合根 | `panta-cli`、`panta-launcher` | `panta-cli → panta-core`；`panta-launcher → panta-ffi, panta-build` | CLI 适配应用命令；launcher 组装并启动 native app |
| 测试工具（生产层外） | `panta-tests`、`tests/` | `panta-tests → panta-build`；测试可依赖被测 crate | 只作为验证消费者，不得被生产 crate 依赖 |

### Native 层级

| 层级 | 当前成员 | 当前 Native target 依赖 | 跨语言边界 |
|---|---|---|---|
| Native L1：适配与基础服务 | `panta_foundation`、`panta_geometry`、`panta_qt_adapter`、`panta_visualization` | 各自依赖职责范围内的 Qt、OCCT、Netgen、VTK 或平台库 | 无 Rust/C++ 依赖逆转 |
| Native L2：领域适配与 UI bridge | `panta_mesh`、`panta_bridge` / `panta::bridge`、`panta_shell` / `Panta.Shell` | Mesh → `panta::geometry`；Bridge → `panta::visualization`、`panta::qt_adapter`；QML module 启用 Bridge 时 → `panta_bridge`、`panta_visualization` | `panta_mesh` 还链接 Rust FFI staticlib，属于显式 Rust/C++ 边界；Bridge 调用 Rust FFI；QML 只经 ViewModel/Bridge 使用服务 |
| Native L3：应用组合根 | `panta_native_app` | → `panta::foundation`、`panta::shell`、`panta_shellplugin`；启用 Bridge 时还 → `panta::visualization`、Bridge/Visualization plugins | 直接链接 Rust FFI staticlib，并通过 Qt/CXX 连接运行时 |
| 测试（生产层外） | native test targets | 可依赖被测生产 target，不进入生产依赖图 | 通过 CTest 与 Cargo 聚合执行 |

这些表用于人工理解现有职责与消费关系，不是自动检查的允许边清单。具体依赖以 Cargo manifest、CMake target links 与 QML module 声明为准；新增依赖时先判断职责，更新相关说明与任务，并通过构建和行为测试验证。

跨语言链接、native 所有权与线程关系仍须人工评审；构建成功不能替代边界设计检查，也不能用反向依赖掩盖职责混杂。

### 应用命令层

命令 / 查询的输入、领域校验、编排、结果与稳定错误由 Qt 无关的 Rust 应用服务提供。GUI 通过 `panta-ffi`/Bridge 适配 Qt 值和展示；CLI 将参数适配为同一命令，不复制业务规则。`panta-core::ProjectService` 提供服务入口；`panta-cli project inspect <path>` 与 GUI 打开工程共用 `ProjectCommand::Open`。详见[应用命令层](../architecture/command-layer.md)。

## 领域、适配与桥接

- 资产激活的过期裁决、缓存提交、attempt 收尾和工作区更新属于 core 的完整用例，FFI 不能拼接另一套提交流程。GUI 工程 I/O 通过独占请求的 begin / 后台 wait / 服务线程 finish 适配，分别通知准入与最终结果；CLI 同步入口复用相同事务。实现与回归以 `panta-core/src/project/`、`panta-ffi/src/project/` 和工程测试为准。

## Rust 导入职责

- `panta-io` 是格式入口，分 `import` / `export` 两个互不依赖的模块。`import`：识别 STEP / IGES / STL、读取 / 固定来源快照、按格式选择实际后端、持有预览与导入准备状态、记录格式专属诊断和导入选项。当前已接入 STL；STEP / IGES 只有格式识别，读取仍待 `panta-geom` 与 OCCT adapter 的真实业务任务。
- `panta-mesh` 拥有表面 / 体 Mesh IR、STL 字节解析、网格领域校验及总体积摘要。它不管理工程包、Qt、VTK 或 OCCT；STL 不被冒充为 B-rep。
- `panta-core` 拥有工程清单、资产写入、修订和原子提交。它调用 `panta-io` 取得已验证的输入，提交成功才替换当前网格。工程包事务不由格式解析器自行完成。
- 运行期 `SurfaceMesh` 的预算、LRU、驻留 ID 和按页签关闭释放由 `panta-core::ProjectService` 管理；其 `DocumentWorkspace` 拥有打开页签、活动 ID、加载状态、重排与关闭回退。ViewModel 转发粗粒度意图并批量投影工作区快照，只保留当前活动显示 DTO，不能另建页签选择权威或按文档维护 Mesh 缓存。
- 展开 Mesh 字节估算由 `panta-mesh` 根据编码布局提供，导入提醒策略由 `panta-io` 随预览返回；GUI / CLI 不重复公式或阈值。2 GiB 仅为非阻断提醒，不是导入或存储上限。
- 后续 `panta-geom` 拥有 Shape / Face / Edge 的领域身份及 STEP / IGES 结果契约；C++ OCCT adapter 真正执行这两种几何格式的读取、转换与所需底层算法。`panta-io` 负责分发，不在 Rust 复制 OCCT 解析器。
- 依赖方向为 `panta-core → panta-io → panta-mesh`；`panta-core` 在持有网格快照时可直接依赖 `panta-mesh`。`panta-ffi → panta-core / panta-mesh` 负责跨语言 DTO。反向依赖禁止。未来 `panta-io → panta-geom` 只在 STEP / IGES 服务接入时添加。
- 公共导入结果应保留格式、数据类别、单位、来源修订与诊断；共享导入编排不抹平 STL 表面网格和 STEP / IGES B-rep 的语义。新增格式需要实际后端和端到端消费者，不为统一名称创建空 parser。

## 显示与数据

- QML 表达用户意图，经 ViewModel / 原生视口提交；不得直接操作重库。ViewModel 负责 Qt 属性、信号与参数转换，不解析几何 / 网格文件或实现工程提交。
- VTK C++ 后端负责渲染对象缓存、投影与控件命中、相机插值、输入、定时器、窗口和 GPU 资源；Rust `ProjectService` 管领域 `SurfaceMesh` 缓存。`MeshSource::mesh_snapshot()` 只返回本地活动显示 DTO；导入、激活等粗粒度状态变化才跨 CXX 获取数据，渲染刷新与逐帧路径不得形成 QML → C++ → Rust → C++ → VTK 往返。
- Rust 管理可持久化的显示配置、字段语义、选择引用及工程相机书签；C++ `RenderScene` 是显示快照和可恢复 CPU 状态，不成为第二份工程领域模型。临时鼠标导航留在 C++，确需保存时提交快照。
- 几何解析与资产提交不放进 VTK 显示后端；STL 由 Rust 网格后端解码，VTK 只构造显示对象。格式与所有权边界见[职责审计](../architecture/native-domain-boundaries.md)。
- 数据按资产 / 修订批量跨边界，不逐点、逐单元调用 FFI。借用缓冲区必须证明有效期；异步工作拥有快照，先确保所有权再按测量优化复制。第三方缓冲区和借用不能逃逸到 QML。
- 大 Mesh 显示准备应先捕获 `Arc<SurfaceMesh>` 及其身份，在后台展开并构造拥有式 C++ DTO；最多一个在途准备，完成时按最新选择与 generation/revision 丢弃迟到结果。GUI 只发布有效快照，Qt / VTK / GPU 对象仍在原线程创建和释放，不新增完成轮询器。异步路径按实际实现验收过期结果丢弃、线程归属与响应时间，历史同步路径的性能数据不能替代。
- 长任务不阻塞 GUI。每个 native 操作明确线程、串行 / 并行与取消阶段；不能为跨线程转移句柄盲目声明 Send/Sync。工程关闭或修订改变后拒绝迟到结果，失败 / 取消不覆盖有效资产。

## 平台与配置

- `qml/Themes/Theme.qml` 只集中固定界面常量；工程数据由 Rust 服务校验和提交，不通过界面样式文件配置。
- `native/qt-adapter` 集中 Qt 封装的系统服务与操作系统 API（例如标准目录发现、系统偏好）；Bridge/ViewModel 消费稳定的 Qt 值/属性并对接 QML 或 Rust DTO，不直接承载 AppKit、Win32、D-Bus 等平台查询。该 target 不依赖 Rust、Bridge 或 VTK。
- 界面与提示直接使用中文；稳定错误码和 DTO 保持技术契约。Bridge `Settings` 暴露系统减少动态效果偏好与界面布局偏好，Qt adapter 负责平台查询，不引入语言目录或翻译安装服务。
- surface 坐标、窗口尺寸、设备像素以及 Wayland/Win32/Cocoa 对象归 C++ 平台适配层，不搬入主题 token 或 Rust 逐帧路径。
- Qt Quick 与 VTK hardware window 生命周期紧密耦合的原生 surface 代码归 `native/visualization`，不得为了平台 API 归类而让通用 Qt adapter 反向依赖 VTK。
- 当前 Settings 适配系统减少动态效果偏好与界面布局（task 036）：左栏宽度 / 折叠、底部面板高度 / 折叠，写入用户配置目录独立的 `workbench-layout.ini`，越界值夹到允许范围，非法值回落默认，不进入工程文件、不影响 dirty；未来实际新增的可持久化设置需先明确 schema、校验和平台存取职责，QML 不直接读写平台设置。
- 手写 unsafe 仅在登记的边界模块（当前 `panta-foundation::crash`），逐块说明安全前提；CXX 的 unsafe 声明与生成胶水按 FFI 规则审查，不扩散到业务调用方。

## 例外登记

新增例外须在本节登记或链接实际任务，不接受隐式例外。已知差距见[职责审计](../architecture/native-domain-boundaries.md)，兼容处理见对应适配器规范和实际登记任务，不能将目标设计当作已完成迁移。
