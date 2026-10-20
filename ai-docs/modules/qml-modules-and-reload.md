# C++/QML 模块与热重载（规划）

[模块导航](README.md)

## 当前实现与目标边界

当前 `native/bridge` 是 `Panta.Bridge` QML 模块的静态 backing target，`qt_add_qml_module` 生成 typeinfo 和静态 plugin，`qml/` 提供 `Panta.Shell` 的 STATIC NO_PLUGIN 资源模块（Qt 将资源对象直接链接到消费者，避免 Windows 丢弃无符号引用的资源 DLL）。默认 `cargo build --locked` 通过 launcher 的 `bridge-module` feature 显式链接并导入 Bridge plugin，运行时和 qmllint 共用模块注册结果；当前已移除 main.cpp 的手动注册路径。诊断构建可使用 `cargo build --locked --no-default-features`，此时 launcher 传递 `PANTA_ENABLE_BRIDGE_MODULE=OFF`，裁剪 bridge 子目录并加载无 ViewModel 的 `AppNoBridge` 入口，验证 Shell 资源链路没有悬空 import。

借鉴用户提出的 Quickshell 思路，分三层推进：静态库管编译边界，Qt 类型注册管 QML 可见性，Reloadable 契约管运行期状态延续。不是所有底层静态库都需要对应 QML URI；只有公开 UI API 的 bridge 模块需要注册，领域与适配器继续隐藏在服务后。

Qt 的 `qt_add_qml_module` 支持自动生成类型注册和 typeinfo；静态插件可能需要显式链接和 `Q_IMPORT_QML_PLUGIN`。宏本身不能保证链接器保留注册代码，需验证 backing target、plugin target、import scan 与资源部署的组合。[Qt 官方模块文档](https://doc.qt.io/qt-6/qt-add-qml-module.html)（查阅 2026-09-16，Qt 6.11）。

Quickshell 的具体引擎替换顺序、对象匹配和窗口移交仍需在实现开发热重载时固定上游 revision 并审阅源码；本次访问上游源码未成功，不把用户摘要当作已核实的源码结论。下述是 panta 的候选设计，不能保证与 Quickshell 实现一致。

## 编译期模块

Bridge 是可被 lint 和运行时识别的正式模块，保持现有 URI，使用 `QML_ELEMENT` 等声明；只有确实是单例且所有权清晰的类型才使用 `QML_SINGLETON`。`PANTA_ENABLE_BRIDGE_MODULE` 是当前唯一的公开模块裁剪开关，默认开启；关闭只保留最小 Shell 资源入口，不预建其它无用途示例模块。

构建依赖采用单向图：Shell → UI bridge → 应用服务；C++ 业务库不反向引用 Shell。CMake 拥有 native 构建图，Cargo 仍是统一入口。替换实现时删除被替代的手动注册、过期注释和重复构建配置；无需保留双注册兼容路径。

## 运行期重载契约

热重载方案首期限定开发模式的 QML/资源更新，发布默认读取打包资源。修改 C++ ABI、类型定义或静态库仍需重新编译并重启，静态链接不是运行期动态卸载机制。

Reloadable（示意）用稳定 reload key、状态类型和版本匹配对象；对象树遍历顺序不能充当 ID。先覆盖面板选中项、滚动位置和编辑缓冲等可序列化状态。应用服务与工程会话在引擎外存活，ViewModel 从服务重新绑定；QQmlEngine 拥有的 JS 值、QObject 和图形资源不能简单跨引擎搬运。

候选流程：监听文件并合并事件 → 建立重载 generation → 记录旧 UI 状态 → 创建隔离候选引擎/对象树 → 检查创建结果与状态匹配 → 恢复状态 → 提交显示和服务连接 → 销毁旧树与引擎。候选阶段不得重复启动后台任务或产生工程写入；需门控组件初始化副作用，单纯隐藏窗口不足以隔离。失败则销毁候选，继续使用旧 UI，并恢复暂停的交互。

重复 key、未知状态版本、必需对象缺失或恢复失败均放弃本次重载；新增可选对象使用默认值。提交阶段禁止再次执行可失败的状态转换，连接切换失败时必须有回退路径。连续保存合并处理，已过时候选不得覆盖新 generation；重载期间关闭工程/窗口优先取消候选并清理监听器。

## 窗口和渲染限制

首期不承诺原生窗口句柄保持或完全无闪烁。QQuickWindow、场景图与 VTK 上下文涉及渲染线程和销毁顺序；VTK 原生视口下的热重载必须验证安全重建与服务状态恢复。若需原生窗口复用，应单独评估稳定宿主窗口方案、平台资源所有权和三平台实测，不照搬 Wayland 专用代理。

中文界面直接声明文案；不引入语言切换或翻译刷新路径。热重载只消费已稳定的上下文，不能改变服务事务语义。
