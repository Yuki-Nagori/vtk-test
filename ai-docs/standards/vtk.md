# VTK WebGPU 硬件窗口与渲染数据

更新日期：2026-10-24。仓库消费本仓库自产的 VTK 9.7.0 WebGPU 硬件窗口 SDK（任务 041 迁入生产管线）；native 工程使用原生 surface/view/layer 桥接，真实窗口验收按本仓库实际平台和场景记录。

适用于 VTK adapter 和 `CaeViewport`；VTK 不是公共数据模型。QML 不直接操作
VTK 对象，原生视口负责 VTK WebGPU 生命周期、平台 surface 与 Qt Quick 的叠加。

## 消费约定

- 消费统一走 `panta_require_sdk("vtk")`：manifest 固定 Release `sdk-vtk-9.7.0-webgpu`（C++20、`VTK_ENABLE_WEBGPU=ON`、Dawn 同包，required targets 为 `VTK::RenderingWebGPU`/`VTK::RenderingUI`/`VTK::RenderingAnnotation`/`dawn::webgpu_dawn`；RenderingAnnotation 供视口色标 `vtkScalarBarActor`；Linux 制品 Wayland-only，`VTK_USE_X=OFF`，纯 X11 需另行生产变体）。
- 链接边界：VTK/Dawn 全部 PRIVATE（公共头只有 Qt 类型，`Qt6::Quick` 因公共头基类保持 PUBLIC），静态库符号经 LINK_ONLY 传播给最终链接者；消费 `VTK::RenderingWebGPU` 的 target 必须调用 `vtk_module_autoinit`（缺省时 render window 可创建、帧可提交，但 renderer/mapper 工厂未注册，场景空白）。
- 运行库形态：Windows 制品为 DLL（`vtk*-9.7.dll`，Dawn 静态编入 VTK DLL），测试进程 PATH 须含 SDK bin 目录（`native_test_env` 注入）；macOS/Linux dylib/so 为 `@rpath` 安装名，构建 rpath 自动解析。
- 适配层结构：公共契约在 `include/panta/visualization/`，第三方头收敛在 `src/vtk/`（与 `geometry/src/occt`、`mesh/src/netgen` 同构）。

## 9.7.0 WebGPU 硬件窗口路线（以新 SDK 头文件为准）

- SDK 必须提供 `VTK::RenderingWebGPU`、`VTK::RenderingUI` 以及对应头文件；生产配置启用 `VTK_ENABLE_WEBGPU=ON`，关闭 Qt 组，不链接 `GUISupportQtQuick`/`RenderingQt`。
- VTK render window 使用 `vtkWebGPURenderWindow`；macOS 使用 `vtkCocoaHardwareWindow`/`vtkCocoaHardwareView` 承载原生 Cocoa surface，并通过 Metal layer 与 Qt Quick 窗口协调；Linux 使用 `vtkWaylandHardwareWindow` 的 Wayland surface，但嵌入 Qt Quick 时必须挂到 `wl_subsurface`，不能直接复用覆盖整个窗口的顶层 `wl_surface`；当前构建关闭 X11；Windows 使用 `vtkWin32HardwareWindow` 子 HWND。
- `CaeViewport` 与 `ViewportBackend` 保持公共边界，`src/vtk/` 适配器和原生 view/layer/surface 桥接隐藏 VTK、AppKit、Win32 和 Wayland 类型；QML 公共头不暴露第三方类型。
- Qt Quick 不负责创建 VTK 的 OpenGL scenegraph 资源；不得重新引入 `QQuickVTKItem`、Qt OpenGL scenegraph 适配或双路径兼容实现。
- 视口实现必须验证原生 view/layer 的尺寸、高 DPI、隐藏/恢复、重建、输入事件和窗口销毁顺序；offscreen 测试只覆盖模块加载/类型创建等不需要真实 GPU surface 的边界。

## 历史 Qt/OpenGL 诊断

旧 SDK 的 QQuickVTKItem 原型在 macOS 26（arm64，Qt 6.11.2 + OpenGLRhi）下，basic
循环因同步阶段没有当前 GL 上下文导致 glad 空表并在首次 Render 段错误；threaded
循环在 `NSOpenGLContext setView` 阶段被 AppKit 断言。该结果是路线废止依据，不是
新 WebGPU 硬件窗口实现的验收证据。

## 官方依据

`vtkWebGPURenderWindow` 负责 WebGPU context/swapchain，硬件窗口负责原生 surface；
仓库适配器在 GUI 线程串行创建/更新/销毁 render window，并由平台桥接借用 Qt Quick
窗口的原生 surface，不允许 VTK 创建脱离 Qt 界面的额外顶层窗口。具体初始化和销毁顺序
以锁定 SDK 的 release 头文件为准。
[vtkWebGPURenderWindow](https://vtk.org/doc/nightly/html/classvtkWebGPURenderWindow.html)
与 [VTK macOS hardware window](https://docs.vtk.org/en/latest/release_details/9.7/hardware-windows-and-wayland.html)
仅作为 API 设计依据，不能替代新制品核对。

VTK 的 C++ 接入文档使用 CMake 与模块依赖。[Using C++ and CMake](https://docs.vtk.org/en/latest/getting_started/using_cpp.html)

Qt Quick 场景图存在不同渲染循环与图形资源生命周期。[Qt Quick Scene Graph](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph.html)

## 职责边界（2026-09-23 项目约定）

Rust 拥有领域数据、显示选项与选择引用；C++ VTK 后端保留渲染对象、输入与相机导航、控件命中、窗口和 GPU 生命周期。不得为每帧导航引入 Rust 往返。 当前实现与迁移项见 [重库适配审计](../architecture/native-domain-boundaries.md)，执行 [分层规则](layering.md)；规划不代表已落地。

## 项目规则

- nightly 资料仅用于设计线索，接口变更必须对照锁定 SDK 的实际头文件/API；若接口不同，记录事实并修订任务，不临时切到未锁定版本。
- 初始化时先确定 WebGPU adapter/device、平台 surface 和所需格式，验证与对应原生窗口/Qt Quick 生命周期一致；不默认 Qt 的 scenegraph backend 可直接复用。
- 通过 VTK WebGPU render window 与硬件窗口的正式生命周期操作视口状态，GUI/worker 不直接修改 actor、mapper 或 pipeline；所有跨线程请求必须由适配器串行化并可取消。
- 平台 view/layer/surface 的线程约束、Metal layer（macOS）的承载关系和 Qt Quick 原生叠加方式必须以实际 API 和本仓库的运行证据固定。
- RenderScene 保存可恢复的应用状态，节点重建时重建渲染对象；任务完成回调携带场景修订以拒绝迟到更新。
- CMake 显式声明所需模块，核实静态/动态构建所需模块初始化；不为简化链接而无条件拉入整个 VTK。
- 使用 `VTK::RenderingWebGPU` 的消费 target 必须调用 `vtk_module_autoinit`；仅能创建 render window 或提交 WebGPU 帧不代表 renderer/mapper 工厂已经注册。
- Mesh/Field 转换检查整数范围、点/单元关联、分量数和数据长度。零拷贝须证明底层数组有效期，否则优先有界复制。
- VTK 引用计数资源按 VTK 规则管理；显示裁剪和抽稀不修改分析输入。相机、色标、单位和缺失值策略由应用模型定义。

## 验证

空视口保持空白，不创建默认模型或欢迎字样；使用明确传入的三角网格验证几何显示、材质与清除时 mapper 资源释放。检查 resize、高 DPI、隐藏/恢复、场景重建、关闭时排队命令，以及退出时无访问已释放资源。软件/无头图形测试不能替代真实目标平台的视口冒烟。
