# 组件与固定界面样式

[模块导航](README.md) · [QML 规范](../standards/qml.md)

## 当前工作台

`qml/App.qml` 组装工作台：顶部 `WorkbenchToolBar`（图标 + tooltip 分组：工程 | 导入 / 导出 | 视图预设 复位 | 切面 | 截图）、左侧 `WorkbenchSidebar`（标题行 + 纵向 `SplitView`：上为对象树「工程 → 资产 → 切面 / 着色 / 时间序列 / 质量诊断」与表面序列节点，下为按节点类型切换的「属性 / 显示 / 信息」属性面板；单击只选中，双击 / Enter 激活所属资产页签，眼睛只切换会话内显示（task 037）；可拖动宽度 260–480 px，可折叠为只含展开按钮的窄轨）、中央视口列（视口上方页签、视口、视口下方播放栏；无播放源时播放栏为禁用细条）与其下可折叠的 `BottomPanel`（诊断 / 消息 / 任务），底部 `WorkbenchStatusBar` 单行显示消息、拾取读数、单位、LOD / 流式状态、时间步与后台任务进度。左栏宽度 / 折叠与底部面板高度 / 折叠由 Bridge `Settings` 写入用户设置，不进工程（task 036）。`ProjectViewModel` 是工程快照和异步结果的 Qt 投影；QML 不维护第二份已提交工程状态。当前布局与启用条件以源码和真实窗口验收为准。

`NewProjectDialog` 管理工程名称与目录候选，`ImportDialog` 管理文件、单位和网格类型候选及预检。确认操作经 ViewModel 进入 Rust；准入、完成和失败分别展示，失败保留已提交资产。`ViewportPane` 自上而下组合 `DocumentTabBar` 与 `CaeViewport`；原生视口压在 Qt Quick 之上，视口矩形内不叠放 QML 控件，菜单使用 `Popup.Window`，页签和活动资产由 Rust 文档工作区提供，VTK 管理原生显示。初始、创建及重开工程均不创建默认文档；关闭最后一个资产页签后恢复空视口。

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
| 视口 | `qml/Panels/ViewportPane.qml` | 文档页签与 `CaeViewport` 装配 |
| 工作台面板 | `qml/Panels/` | `WorkbenchToolBar`、`WorkbenchSidebar`、`BottomPanel`、`WorkbenchStatusBar`、`PlaybackBar`；只经属性与信号和 App 交互，不导入 Bridge 的面板也进入无 Bridge 构建 |
| 工作台 | `qml/App.qml`、`qml/AppNoBridge.qml` | 正常工程入口与无 Bridge 构建的最小资源入口 |

控件可封装 Qt Quick Controls，但应保留焦点、键盘、禁用和可访问性行为。面板内部可直接使用 Qt Controls；无需为了统一命名把简单控件再包一层。业务弹窗的输入和按钮按主题组合，职责约束见[QML 规范](../standards/qml.md#原子组件与主题参数项目约定)。

原子/组合组件公开语义明确的属性与信号。默认尺寸通过 Theme token 绑定，组合层显式传参数，不通过 parent 链查找业务状态。布局决定分配空间；组件不同时强制互相冲突的 anchors 与固定尺寸，也不用 imperative 赋值覆盖应保持的主题绑定。

`DialogWindow` 管理模态与居中，`DialogFrame` 装配标题和内容，`DialogTitleBar` 仅发移动/关闭意图。具体弹窗拥有草稿、打开条件和关闭守卫，Rust 裁决提交、取消及过期上下文。主窗口是否关闭不等于后台写入已取消。

弹窗内校验或提交失败时，错误紧贴出错字段显示；该弹窗打开期间状态栏不重复同一错误，关闭后状态栏恢复显示。可变内容的弹窗（如导入）按 `DialogFrame` 的隐式高度收紧窗口高度，警告或错误出现时自动增高，不以固定高度留白或裁切按钮。导入单位旁注明来源（默认 / CFD 结果按 SI 米制 / 用户选择）。文件与文件夹选择器保持 `options` 为 0：平台有原生实现（Windows / macOS 系统对话框，Linux 经 xdg-desktop-portal 或 GTK3 平台主题）时使用原生对话框，否则由 Qt Quick 回退实现；安装产物随 `plugins/platformthemes` 部署两个 Linux 平台主题插件（task 035）。

## 文案与图标

占位文字使用 `Theme.colorTextPlaceholder`，与已填写内容（`colorText`）及说明文字（`colorTextMuted`）区分。界面文案直接使用中文，不调用 `qsTr()` / `qsTranslate()` 或加载翻译字典。错误按稳定代码适配中文摘要；用户文件名、资产名和底层原始诊断保持原值。自定义 Controls 使用应用固定的 Basic 样式。

保留图标位于 `qml/icons/`，由 CMake 注册到 `/qt/qml/Panta/Shell/icons/`。彩色资源可直接加载，单色工具图标通过 `panta-icons` provider 着色；图标的来源与格式见[图标规范](../standards/icons.md)。图标清单以现存资源为准，不与已移除的原型 symbol 对照。

## 视觉规范（task 039）

按 [034](../task/034-ui-redesign.md)「视觉规范」落地，浅色界面 + 深色视口：

- 颜色：`colorWindow` #f3f4f6（窗口底色、工具栏、分隔柄、对话框标题栏）、`colorPanel` #ffffff、`colorPanelAlt` #f7f8fa（属性分组标题带）、`colorPanelLine` #d5d8de、`colorText` #1f2328、`colorTextMuted` #59636e、`colorTextPlaceholder` #8c959f、`colorAccent` = `colorFocus` #2563eb、`colorSelectionBg` #dbe7fd、`colorWarning` #9a5b00、`colorError` #c62828、`colorSuccess` #1a7f37。
- 对比度（WCAG 2.x 相对亮度公式，task 039 计算）：正文 15.8:1（白）/ 14.4:1（窗口底色）；次要文字 6.1 / 5.6 / 4.9（选中底色）；警告 5.4 / 4.9；错误 5.6 / 5.1；成功 5.1 / 4.6；强调色 5.2 / 4.7。占位文字 3.0:1 为记录在案的例外（只作示例，字段标签常显）。焦点环用 `colorFocus`，对白底 5.2:1（非文字元素要求 ≥ 3:1）。
- 状态：`Theme.statusColor(level)` / `Theme.statusIcon(level)` 按 `info` / `success` / `warning` / `error` 给出颜色与 `status-*` 图标；状态栏消息和底部消息列表同时显示图标与着色文字，不只靠颜色。
- 字体：`Theme.fontFamily` 按平台取 Microsoft YaHei UI / PingFang SC / Noto Sans CJK SC（缺失时 Qt 回退系统字体），主窗口与原子组件统一设置；数值显示（状态栏字段、任务进度、消息、播放时间）用 `Theme.fontFeaturesNumeric`（`tnum`）。字号阶梯 11 说明 / 状态栏、12 属性标签、13 正文与控件、14 分组标题（DemiBold）、16 对话框与空状态标题。
- 间距：2 / 4 / 8 / 12 / 16 / 24（`spacingTiny` … `spacingXXLarge`）；对话框内容区四边 `dialogContentPadding` 16 px，按钮间距 8 px。
- 尺寸：常规控件 26 px，树行 22 px，工具按钮 28 px（工具栏图标 20 px、播放栏图标 16 px）；圆角 4 px，对话框 6 px（`DialogFrame` 带 1 px 边框，标题栏同步圆上角）。所有对话框高度随内容收紧（`Math.max(minimumHeight, frame.implicitHeight)`）。
- 颜色图：`field_color_maps`（`native/visualization` 的 `mesh_source.hpp`）提供冷暖（默认）、Viridis、彩虹（蓝-红）、灰度四个预设，视口查找表、视口色标与属性面板图例共用；在「着色」属性中切换，属会话内视图状态，不写工程。

## 尺寸与布局

Theme 集中定义 spacing、padding、radius、borderWidth、iconSize、fontSize、controlHeight 与 windowMinimumSize 等语义 token。业务几何尺寸和工程单位不属于 Theme；父布局计算出的宽高也不应被错误地改成固定主题尺寸。

界面使用 Qt 逻辑像素，不自行乘 DPR。左栏宽度取自用户设置并夹在 260–480 px，视口列占剩余空间且保留 `Theme.viewportMinimumWidth` / `viewportMinimumHeight`；长工程名、错误文案、中文文案、字体变化和小窗口需要单独检查。页签选择、关闭和重排不得依赖显示名寻址，须保持稳定文档 ID。

## 验证与后续组件整理

功能测试围绕工程创建/打开/保存、预检失败、资产列表及文档选择验证，不保留商业面板的占位动作。组件测试覆盖固定样式下的保留控件；真实 VTK 窗口验收覆盖输入、尺寸变化与图形生命周期。无头截图只能作为布局证据。

公共样式归原子层，弹窗复用外壳；草稿初始化、校验反馈、异步结果和上下文检查留在具体弹窗。新增组件先确认现有控件不能满足独立职责，避免恢复无实际消费者的层次。

## 固定样式维护

`Theme.qml` 的固定常量均有实际消费者（task 039 逐个复核：除 `colorAccent` 经 `colorFocus`、状态色经 `statusColor()` 间接使用外，其余都被组件直接引用），集中颜色、字号、间距和尺寸；只保留固定样式。新属性先确认确有复用或独立语义，不为假设的主题扩展建立 schema、服务、ViewModel 或配置文件。删除组件时同步删除无消费者的常量。

控件通过属性绑定使用这些常量；内容测量与父布局的动态宽高仍由布局负责。样式调整检查真实操作流程、中文混排、焦点 / 禁用状态和显示缩放，不增加主题切换或主题偏好测试。
