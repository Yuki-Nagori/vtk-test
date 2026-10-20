# QML 组件与声明式界面

更新日期：2026-10-20。QML 只提供工程 / 测试资产操作、导入与 VTK 视口所需的轻量界面。Qt 6.11.2 与静态模块 typeinfo 由现有构建链提供；实际模块、资源与测试清单以源码为准。

适用于 `qml/`；Qt 对象模型参见 [Qt](qt.md)，视口参见 [VTK](vtk.md)，高 DPI 与多显示屏参见[显示缩放模块](../modules/display-scaling-and-multi-monitor.md)。

## 官方依据

Qt 建议分离界面与业务逻辑，并使用资源系统组织应用资源；QML 文件与模块目录的关系影响 implicit imports。[QML Best Practices](https://doc.qt.io/qt-6/qtquick-bestpractices.html)

`qt_add_qml_module` 管理 QML 文件、资源和相关构建工具集成。[qt_add_qml_module](https://doc.qt.io/qt-6/qt-add-qml-module.html)

## 项目规则

- 组件文件 `PascalCase.qml`，id/属性/信号使用 `camelCase`；命名反映界面语义，不把库名当作用户功能名。
- 每个可复用组件声明输入属性与输出信号，通过显式属性注入 ViewModel；避免依赖任意上层 id 或散布全局 context properties。
- 状态优先用属性绑定表达；命令式赋值前检查是否会破坏原绑定。组件不直接读写工程文件，也不调用 OCCT、Netgen、VTK 算法。
- 长列表使用有明确角色的模型和 delegate；不将大型网格数组转换成 JavaScript 对象供 UI 管理。
- 将 `qt_add_qml_module` 放在与模块 QML 布局一致的位置，由 native 顶层纳入构建；若必须跨目录，必须验证 import 和资源别名，不能仅靠开发机 import path 可用。
- 图标和内置 QML 通过模块资源定位；用户输入文件通过应用服务处理 URL/path 转换。避免当前工作目录相关的相对资源路径。
- 固定颜色、字号、间距和尺寸集中管理；文字直接使用中文；焦点、键盘操作与禁用原因可理解。布局统一使用 Qt 逻辑像素，不在 QML 里乘 `devicePixelRatio` 或缓存屏幕比例；跨屏和 framebuffer 转换集中由 C++/渲染边界处理。仅为可验证的可视化操作增加界面，不提前堆砌业务面板。
- 主窗口最小尺寸由 `Theme.windowMinimumWidth` / `Theme.windowMinimumHeight` 统一定义；验证该下限与常用窗口尺寸下的操作区、工程列表及视口布局，不继承旧商业 Shell 的固定尺寸要求。
- 自绘控件样式（自定义 background 等）不能运行于原生 Controls 样式；应用入口以 `QQuickStyle::setStyle("Basic")` 固定非原生样式，测试环境保持同源。
- 图标遵循 [图标设计规范](icons.md)，使用模块内 SVG 资源并经 `qt_add_resources` 登记；Mono 图标由 `panta-icons` provider 使用 QtSvg 渲染并着色，普通 SVG Image 解码使用 qsvg 插件。不在 QML 里用 Shapes/Canvas 逐个绘制静态图标。

## 原子组件与主题参数（项目约定）

- UI 按原子组件 → 组合组件 → 业务界面（Panels / Dialogs）→ 页面拼装四层组织；Theme 为各层提供视觉参数，不计入组件层级。组件库只沉淀实际复用或具有独立职责的控件，原子层不接业务服务。
- 原子组件按单一控件语义划分，可封装 Qt Quick Controls 及其图标、文字和指示器，保留焦点、键盘及可访问性行为；不按内部 Item 数量拆分。原子组件提供 implicit size，外部 Layout 的空间分配由调用方负责。
- 组合组件负责表单、标题栏、按钮区等复用结构；具体 Dialog / Panel 负责业务草稿、上下文快照、校验反馈和命令编排，业务规则与持久化仍经 ViewModel 进入 Rust。公共弹窗结构可封装模态、标题、关闭入口和定位，不持有业务 ViewModel 或保存状态。
- 每个组件以明确属性接收内容、状态和尺寸，以信号输出意图；组合组件向子组件传参，不读取任意父级对象。
- 共用固定视觉尺寸（间距、内边距、圆角、边框、字号、图标、控件高度及面板/窗口最小尺寸等）统一由 `qml/Themes/Theme.qml` 暴露，组件输入属性默认绑定 token。调用方覆盖也使用 token 或布局计算，不散布视觉数值。
- 内容测量、父布局分配和纯算法常量无需伪装成主题尺寸；UI 尺寸使用逻辑像素语义，不能把工程几何单位混入 Theme。
- Theme 只提供当前使用的只读 QML 常量；不增加主题切换、外部覆盖或扩展配置服务。删除控件时同步清理无消费者的常量。
- 固定界面样式不属于工程数据，不影响工程 dirty 状态。

详见[组件与固定界面样式](../modules/qml-components-and-theme.md)。只保留实际复用的组件与常量，不为样式配置增加额外层次。

## 验证

组件库需验证固定样式与显式输入参数、长文本及 1.0/1.25/1.5/2.0 DPR 下布局，禁止以隐藏溢出代替正确布局。

使用所锁定 Qt 提供的 qmllint/格式工具检查实际模块；验证必需属性、导入、绑定循环、窗口缩放和键盘焦点。打包后的资源加载必须独立验证，不能只用源码目录启动成功作为证据。

### 性能基准

性能优化必须先有可复现的问题和测量依据。当前没有专属 QML 或 GPU 基准；确有需要时按[性能诊断范围](../modules/performance.md)另建任务，明确工具、真实输入和测量边界，不为静态布局变化机械建立基准。

CPU 构造、资源加载和更新成本可用 offscreen 场景测量；可见内容的帧呈现与动画使用实际渲染场景。离屏 GPU 时间、CPU 提交耗时与真实窗口呈现间隔分别记录，不混称 GPU 内核耗时。静态截图不能替代性能测量。

测量任务记录 Qt 版本、平台/图形后端、构建配置、输入规模、采样方法和限制；前后比较保持同环境与同输入。当前行为验收仍通过组件测试、资源加载、布局及真实窗口，不引入性能数值门禁。

VS Code 的 Qt Qml 扩展通过 [工作区配置](../../.vscode/settings.json) 使用 Cargo 供给的 qmlls，并传入 `target/native/debug` 构建目录与 Qt staging 的 QML 导入路径。先执行 `cargo build --locked` 生成模块类型信息；编辑器不自行触发 CMake。若出现 `color was not found [import]` 等导入诊断，先对照 `cargo lint qmllint --check`，再检查 QML Language Server 日志中的工具路径和参数，必要时重启语言服务；不关闭 import 诊断。
