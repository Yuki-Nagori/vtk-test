# 重要模块设计

[任务索引](../task-index.md) · [架构总览](../architecture/README.md) · [技术规范](../standards/README.md)

当前 crate、native target 与 QML 的依赖分组见[分层标准](../standards/layering.md)和[仓库布局](../architecture/repository-layout.md)；Rust 应用命令入口及 GUI/CLI 边界见[命令层设计](../architecture/command-layer.md)。

本目录保留现有模块的职责、数据流和演进设计。当前仓库聚焦 project 资产/场景与 VTK 渲染测试；STEP、体网格生成和网格修复仍需要 OCCT/Netgen。固定界面常量、中文文案与路径 是现有支撑能力，热重载和增量更新等未接入方案只作参考，实施与验证统一记入[任务索引](../task-index.md)。

| 方向 | 设计说明 | 能力边界 / 源码入口 |
|---|---|---|
| 复用组件与固定界面样式 | [组件与固定界面样式](qml-components-and-theme.md) | 实际控件与少量 QML 颜色 / 字号 / 尺寸常量 |
| 中文提示、系统动态效果偏好 | [中文界面](chinese-interface.md)、[应用与存储架构](../architecture/application-and-storage.md) | 中文直接声明，Settings 适配系统 reducedMotion 与界面布局偏好 |
| 界面文案与技术标识边界 | [中文界面](chinese-interface.md) | UTF-8 中文提示、稳定错误码和原始诊断 |
| 跨平台路径、逻辑资源地址与运行上下文 | [路径与运行时](paths-and-runtime.md) | `panta-core` 路径授权与工程会话相关性 |
| 静态库、QML 注册与状态重载 | [模块与热重载](qml-modules-and-reload.md) | 当前静态注册；热重载为候选设计 |
| 高 DPI 缩放与多显示屏 | [显示缩放与多屏](display-scaling-and-multi-monitor.md) | 窗口坐标、DPR 与多屏真实验收 |
| 基础格式、Clippy、QML lint 与行为测试 | [质量工具链](quality-tooling.md) | `tests/src/` 与聚合 Cargo 命令 |
| 已保存资产按需加载与取消 | [资产激活](asset-activation.md) | 后台读取、相关性、容量限制与失败保留 |
| Mesh 资产 revision、旁置存储与按需加载 | [工程存储](../architecture/application-and-storage.md)、[网格架构](../architecture/mesh.md) | `panta-mesh` sidecar 与工程资产索引 |
| 导入窗口与请求回执 | [导入交互](import-interaction.md) | QML 属性、预检请求与 Rust 提交闸门 |
| 软件内更新参考 | [软件内增量更新](incremental-updates.md) | 非当前范围；无更新器实现 |
| VTK/OCCT/Netgen 预编译 SDK 与受信制品 | [Native 依赖供给](native-dependency-supply.md) | 消费现有 Release；配对 OCCT/Netgen ABI |
| 性能诊断边界 | [性能诊断范围](performance.md) | 当前无专属基准；真实问题另建任务 |
