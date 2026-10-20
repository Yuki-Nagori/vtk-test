# 架构总览

[任务索引](../task-index.md) · [重要模块说明](../modules/README.md) · [技术规范](../standards/README.md)

## 主题导航

| 主题 | 文档 | 范围 |
|---|---|---|
| Rust 领域与 C++ 重库边界 | [适配边界与 crate 规划](native-domain-boundaries.md) | 本仓库；当前审计与目标 |
| 目录与模块归属 | [目录规划](repository-layout.md) | 本仓库 |
| Rust 应用命令层 | [GUI / CLI 共享命令设计](command-layer.md) | 当前服务边界与后续实施 |
| Cargo/CMake、运行与部署 | [构建与开发](build-and-development.md) | 本仓库 |
| QML、ViewModel 与交互 | [界面与桥接](ui-and-bridge.md) | 本仓库 |
| STEP、拓扑与 OCCT | [几何模型](geometry.md) | 本仓库 |
| Mesh IR、Netgen、资产 revision 与旁置存储 | [网格](mesh.md) | 工程资产、Mesh sidecar 与按需加载 |
| VTK、视口与字段 | [可视化](visualization.md) | 本仓库 |
| Rust、工程与持久化 | [应用平台与存储](application-and-storage.md) | 本仓库 |
| 资产加载、取消与相关性 | [资产激活](../modules/asset-activation.md) | 本仓库实际后台加载流程 |
| 网格、材料 / 边界标签与已求解结果 | [模型与结果组合](study-and-automation.md) | 后续结果导入与可视化；不接入求解器执行 |
| 本地耗时操作与测试作业 | [作业边界参考](background-jobs.md) | 不接入外部求解器 |
| 交付阶段与验收 | [里程碑与验证](milestones-and-validation.md) | 本仓库 |

## 文档状态与命名

本仓库是最小可视化渲染测试仓库，project 用于承载模型资产和场景。保留网格、材料 / 边界标签和已求解结果的组合设计，后续用于结果导入与可视化；当前不接入求解器执行或 Study 配置编辑框架。

已有 Cargo → CMake → Qt/QML → VTK WebGPU / 平台硬件窗口链路，以及工程创建、打开、保存、STL 预检、sidecar 存储和按需激活。STEP / OCCT 和 Netgen 适配器保留，用于继续验证 STEP 导入、体网格生成和网格修复；native 冒烟能力不等于工程端到端流程已经完成。实际验收以本次构建、测试和真实窗口记录为准，不沿用历史任务完成状态。

## 当前目标

围绕 project → 模型资产 → VTK 场景有序清理；保留 STEP、Surface / Volume Mesh 及修复所需的数据与重库边界。优先验证模型加载、显示、相机输入、窗口生命周期和工程持久化。Linux Wayland、跨显示器 DPR 等场景仍需实际桌面验证；无头类型创建不能替代 GPU 窗口验收。未来字段或求解展示须明确标注合成数据与真实结果。

## 分层和依赖方向

```text
QML → C++ ViewModel → CXX → Rust 应用服务 / 领域模型
                              ↓ 自有后端接口
                          CXX → C++ adapter → OCCT / Netgen
Rust 资产 / 显示快照 → CXX → C++ RenderScene / ViewportBackend → VTK
```

这是目标职责；现有 native STEP 摘要、Mesh IR 与 Rust 服务的接入状态见 [重库适配与 Rust 领域模块](native-domain-boundaries.md)。Rust 拥有业务、数据与编排，C++ 适配器实际调用重库并封装其类型 / 异常 / 生命周期。VTK 后端还负责窗口、输入与逐帧显示，不为每帧形成 QML → C++ → Rust → C++ → VTK 的往返。

进程级设施单独归 Rust 基础设施层：`panta-foundation` 实现崩溃信号、日志
等需要操作系统边界的能力，`panta-ffi` 只把安全入口映射到 CXX；`panta-core`
保持领域模型职责和无手写 `unsafe`。VTK 只存在于
`native/visualization/src/vtk/` 适配器，`RenderScene`、`ViewportBackend` 和
QML 公共头不暴露 VTK 类型。当前 `App.qml` 已创建 `CaeViewport`，但 Linux 当前锁定 Wayland，纯 X11 需要单独制品变体；模块加载/类型创建测试仍保留，供
构建和边界回归使用。旧 QQuickVTKItem、Qt OpenGL 场景图依赖和失效 SDK 引用已从代码与消费配置删除，不保留双路径。

## 技术基线

技术选型与依赖版本集中见[技术基线](../standards/baseline.md)，编码和库规则见[规范索引](../standards/README.md)。CXX 提供 Rust/C++ 桥接，`panta-foundation` 提供进程级设施，VTK 图形与线程约束见[VTK 规范](../standards/vtk.md)。架构文档只维护职责与交互，具体验证记入当前任务。

## 范围边界


本仓库不接入外部求解器或共享求解协议。保留的模型 / 结果组合用于后续导入已求解数据并渲染；实际完成范围仍以模型资产、几何网格操作与 VTK 显示的当前验收为准。
