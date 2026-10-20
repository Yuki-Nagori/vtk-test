# 组件与固定界面样式

[模块导航](README.md) · [QML 规范](../standards/qml.md)

## 当前工作台

`qml/App.qml` 提供最小工具栏、模型资产列表和中央视口：新建/打开工程、保存、STL 导入与资产激活。`ProjectViewModel` 是工程快照和异步结果的 Qt 投影；QML 不维护第二份已提交工程状态。当前布局与启用条件以源码和真实窗口验收为准。

`NewProjectDialog` 管理工程名称与目录候选，`ImportDialog` 管理文件、单位和网格类型候选及预检。确认操作经 ViewModel 进入 Rust；准入、完成和失败分别展示，失败保留已提交资产。`ViewportPane` 组合 `DocumentTabBar` 与 `CaeViewport`，页签和活动资产由 Rust 文档工作区提供，VTK 管理原生显示。初始、创建及重开工程均不创建默认文档；关闭最后一个资产页签后恢复空视口。

`qml/Themes/Theme.qml` 保留颜色、间距、字号及必要窗口/控件尺寸的只读门面。应用使用系统标题栏；主题值只影响界面，不决定资产身份、网格单位或业务规则。本仓库使用固定 QML 常量，不提供运行期主题切换或外部覆盖配置。

## 页面与组件边界

页面直接围绕真实工程操作使用 Qt Quick 布局和可访问控件。只提取当前有明确职责的组件，避免为单次布局建立空包装；独立原型不再作为本仓库构建或图标测试输入。

| 层次 | 位置 | 当前职责 |
|---|---|---|
| 主题 | `qml/Themes/Theme.qml` | 颜色、逻辑尺寸、间距、字体与窗口约束 |
| 原子组件 | `qml/Components/Atoms/` | `ThemedLabel`、`ThemedButton`、`ThemedToolButton`、`ThemedTextField`、`ThemedComboBox`、`ThemedIcon`、`PanelSurface` |
| 文档组件 | `qml/Components/Composites/DocumentTabBar.qml` | 页签选择、关闭与重排意图；已确认文档状态归 Rust |
| 弹窗外壳 | `qml/Components/Composites/` | `DialogWindow`、`DialogFrame`、`DialogTitleBar`、`DialogButtonRow` 组合通用窗口结构 |
| 工程弹窗 | `qml/Dialogs/` | `NewProjectDialog.qml`、`ImportDialog.qml` 的候选输入、预检和确认 |
| 视口 | `qml/Panels/ViewportPane.qml` | 文档页签、加载状态、诊断与 `CaeViewport` 装配 |
| 工作台 | `qml/App.qml`、`qml/AppNoBridge.qml` | 正常工程入口与无 Bridge 构建的最小资源入口 |

控件可封装 Qt Quick Controls，但应保留焦点、键盘、禁用和可访问性行为。App 的工具栏与资产列表可直接使用 Qt Controls；无需为了统一命名把简单控件再包一层。业务弹窗的输入和按钮按主题组合，职责约束见[QML 规范](../standards/qml.md#原子组件与主题参数项目约定)。

原子/组合组件公开语义明确的属性与信号。默认尺寸通过 Theme token 绑定，组合层显式传参数，不通过 parent 链查找业务状态。布局决定分配空间；组件不同时强制互相冲突的 anchors 与固定尺寸，也不用 imperative 赋值覆盖应保持的主题绑定。

`DialogWindow` 管理模态与居中，`DialogFrame` 装配标题和内容，`DialogTitleBar` 仅发移动/关闭意图。具体弹窗拥有草稿、打开条件和关闭守卫，Rust 裁决提交、取消及过期上下文。主窗口是否关闭不等于后台写入已取消。

## 文案与图标

界面文案直接使用中文，不调用 `qsTr()` / `qsTranslate()` 或加载翻译字典。错误按稳定代码适配中文摘要；用户文件名、资产名和底层原始诊断保持原值。自定义 Controls 使用应用固定的 Basic 样式。

保留图标位于 `qml/icons/`，由 CMake 注册到 `/qt/qml/Panta/Shell/icons/`。彩色资源可直接加载，单色工具图标通过 `panta-icons` provider 着色；图标的来源与格式见[图标规范](../standards/icons.md)。图标清单以现存资源为准，不与已移除的原型 symbol 对照。

## 尺寸与布局

Theme 集中定义 spacing、padding、radius、borderWidth、iconSize、fontSize、controlHeight 与 windowMinimumSize 等语义 token。业务几何尺寸和工程单位不属于 Theme；父布局计算出的宽高也不应被错误地改成固定主题尺寸。

界面使用 Qt 逻辑像素，不自行乘 DPR。资产列表分配必要宽度，视口占剩余空间；长工程名、错误文案、中文文案、字体变化和小窗口需要单独检查。页签选择、关闭和重排不得依赖显示名寻址，须保持稳定文档 ID。

## 验证与后续组件整理

功能测试围绕工程创建/打开/保存、预检失败、资产列表及文档选择验证，不保留商业面板的占位动作。组件测试覆盖固定样式下的保留控件；真实 VTK 窗口验收覆盖输入、尺寸变化与图形生命周期。无头截图只能作为布局证据。

公共样式归原子层，弹窗复用外壳；草稿初始化、校验反馈、异步结果和上下文检查留在具体弹窗。新增组件先确认现有控件不能满足独立职责，避免恢复无实际消费者的层次。

## 固定样式维护

`Theme.qml` 保留的 59 个固定常量均有实际消费者，集中颜色、字号、间距和尺寸；只保留固定样式。新属性先确认确有复用或独立语义，不为假设的主题扩展建立 schema、服务、ViewModel 或配置文件。删除组件时同步删除无消费者的常量。

控件通过属性绑定使用这些常量；内容测量与父布局的动态宽高仍由布局负责。样式调整检查真实操作流程、中文混排、焦点 / 禁用状态和显示缩放，不增加主题切换或主题偏好测试。
