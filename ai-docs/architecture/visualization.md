# 可视化、RenderScene 与原生视口

[架构总览](README.md)

## 视口定位

UI 使用 QML，3D 视口使用 C++ native Qt Quick component。VTK 是 V1 渲染后端，但不成为应用公共 API。原生视口集中处理图形上下文、窗口尺寸、设备像素比、渲染线程及资源销毁。

VTK 采用 WebGPU render window；macOS 使用 `vtkCocoaHardwareWindow`/`vtkCocoaHardwareView` 暴露的原生 Cocoa view 与 Metal layer，再由原生视口桥接到 Qt Quick 窗口；Windows 使用 Qt Quick 原生 HWND 创建 VTK child window；Wayland 使用 Qt Quick 的 `wl_display`/`wl_compositor` 创建 `wl_subsurface`，再把该子 surface 交给 VTK。三端的 render window 均只绑定 `CaeViewport` 的原生区域，不把整个 Qt 顶层 surface 交给 VTK。三种嵌入（Cocoa 子视图、Win32 子 HWND、Wayland subsurface）都使原生渲染层位于 Qt Quick 内容之上：QML 元素无法叠放在视口区域上方，色标、工具栏等 overlay 必须避开视口矩形或另择承载方式（视口色标由 VTK `vtkScalarBarActor` 在 render window 内绘制，task 038）；无打开文档时 `CaeViewport` 隐藏、原生表面随之隐藏，同一矩形显示 QML 空状态引导（task 038，三端须在目标平台验证）；叠加与输入协调验证以此为前提。Qt Quick 不承载 VTK 的 OpenGL scenegraph 集成，`QQuickVTKItem` 不属于目标架构。第一里程碑的代码路径已落地，仍需在目标平台验证叠加、尺寸/高 DPI 同步和输入事件协调。

默认视口为空场景，不创建品牌字样、固定欢迎文案或默认文档页签。只有真实资产加载成功后才建立对应显示对象；关闭最后一个文档释放其 Mesh 快照并恢复空视口。窗口、相机和输入生命周期由 VTK adapter 管理。

当前适配器在 GUI 事件循环中合并场景、相机和几何刷新；重复提交相同外观不渲染。滚轮围绕焦点缩放，右键拖动旋转，左键点击定位六面体；方向切换以四元数插值在 260 ms 内完成。定时器只在过渡期间运行，完成、用户接管、隐藏、尺寸重置或资源销毁时停表。祖先平移只同步原生区域位置。跨窗口时释放旧 surface/GPU 资源后按最新 CPU 状态重建；析构先断开条目自身的窗口回调，避免基类析构信号访问已释放状态。原生区域当前仍按轴对齐矩形映射，不支持任意 QML 旋转/裁剪叠加。

`native/visualization/src/vtk/navigation/` 集中相机姿态、输入映射和方向标记；`VtkViewport` 负责原生窗口、GUI 定时器和刷新编排。相机与命中计算在 CPU 执行，WebGPU 执行场景绘制。无窗口测试验证相机不变量和实际面投影命中，真实窗口另验证输入与图形生命周期；性能诊断如有需要按[独立任务](../modules/performance.md)接入。

## RenderScene 契约

当前 `native/visualization/include/panta/visualization/render_scene.hpp` 的 `RenderScene` 接收修订化 mesh snapshot，另含背景与模型可见性；没有有效网格快照时保持空白。工程路径解析、STL 读取和提交归 Rust。字段、裁剪、时间步与实体拾取仍为规划；`addMesh`、`showScalar`、`setClipPlane`、`setTimeStep`、`pick` 是示意接口，尚未实现。

Rust 规划的 `panta-visualization` 拥有后端中立的显示配置、字段和选择语义；C++ RenderScene 消费 `MeshSource` 提供的拥有内存快照并重建显示状态。VTK 只将点坐标转换为 VTK 数据对象，不读文件、不解析 STL、不执行网格领域规则；具体函数审计与依赖方向见 [适配边界](native-domain-boundaries.md)。相机插值、方向控件命中、GUI 定时器与 GPU 生命周期留在 C++。

接口参数使用 Mesh/Field ID、自有选项和结果类型；不暴露 `vtkActor`、`vtkDataSet` 或 Qt Quick 内部对象。VTK 适配器负责转换与缓存，未来更换渲染后端时尽量保持应用和 QML 调用语义稳定，但不承诺无需任何迁移。

## 字段与显示

字段至少记录名称、单位、分量数、节点/单元关联、网格修订和时间步。标量上色前检查数组长度和关联关系；向量显示或分量提取应显式指定。NaN、缺失值和无效区域采用独立显示策略，不能伪装为零值。

色标需展示单位、范围和映射方式。区分自动范围、固定范围及跨时间步统一范围，避免时间动画中颜色相同却表示不同数值。裁剪只改变显示，默认不修改工程网格或求解数据。

合成数据可使用简单位置函数生成压力/温度样例，但必须标为“演示/合成数据”，附带单位与生成规则，不能冒充求解结果。

## 选择与生命周期

拾取结果应返回实体 ID、命中位置和选择类型，并处理逻辑坐标与物理像素转换。几何面/边选择与网格单元选择是不同模式，需保留各自映射关系。

后台线程可准备 CPU 数据，图形资源的创建和释放必须遵守 VTK WebGPU 与 Cocoa view/layer 的线程约束。检查窗口缩放、最小化、原生 view/layer 重建、工程关闭和连续加载，防止旧场景资源或回调访问已释放对象。

## V1 验收

空视口启动 → 简单测试网格 → 几何显示 → 表面/体网格 → 拾取高亮 → 相机操作 → 裁剪 → 合成标量与色标。大结果采用按时间步或按需读取，避免默认把所有数据复制到内存和 GPU。
