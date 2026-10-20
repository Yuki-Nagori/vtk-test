# 界面、C++ 桥接与交互

[架构总览](README.md)

UI 与操作提示直接使用中文，文案与技术标识的边界见[中文界面](../modules/chinese-interface.md)。C++ 静态库与 QML 自动注册已落地，开发模式状态重载仍为规划，见[模块与热重载](../modules/qml-modules-and-reload.md)。

组件按原子控件、组合组件、具体界面和页面拼装；共用颜色、字号、间距和尺寸由 Theme 中的固定 QML 常量提供。只保留当前消费者需要的组件与常量，详细职责见[组件与固定界面样式](../modules/qml-components-and-theme.md)。

## 界面构成

最小工作台聚焦工程新建、打开、保存、STL 导入、资产列表、文档页签与 VTK 视口。界面展示已接入的工程命令与场景状态；显示与选择继续使用稳定资产 ID，具体界面以源码及本轮窗口验收为准。

QML 负责布局、主题、控件和状态绑定。`CaeViewport` 是嵌入 QML 的原生渲染组件，具体渲染集成见[可视化](visualization.md)。不能让各面板直接持有或操作 OCCT、Netgen、VTK 对象。

## ViewModel 契约

`QObject` 派生 ViewModel 暴露可观察属性、命令和事件。`native/bridge` 注册 `ProjectViewModel`，提供工程默认目录、创建/打开/保存、重命名命令和 dirty 状态。工程 / STL 操作错误使用 CXX 值 DTO：`ProjectDiagnostic` 独立携带 code / category / detail，code 为空才可消费结果 value；失败占位值不能覆盖旧状态。ViewModel 适配为 `errorCode` / `errorCategory` / `errorDetail` 与用户摘要，展示层按稳定码提供中文摘要，不解析异常文本。保存资产异步激活失败时，文档模型的 `diagnostic` 保存同一结构；成功或重试后清空。它只做 Qt 字符串/URL 与 Rust 应用服务的适配；实际网格生成仍按后续任务接入。

属性变化通过 NOTIFY 信号通知，错误以结构化错误加用户可读摘要呈现。长任务返回任务标识或提供可追踪的任务状态，不能仅用一个全局 busy 布尔值掩盖多个并发操作。按钮启用条件来自模型状态，例如没有有效几何时禁用网格生成。

导入窗口通过 QML 属性和请求回执组织交互，Rust 负责业务有效性、提交和取消结果；具体边界见[导入交互](../modules/import-interaction.md)。

ViewModel 将命令交给应用服务，不负责文件解析、持久化事务或数值计算。属性编辑先验证，再提交领域命令；失败时恢复显示并解释原因，避免界面显示已经修改而工程模型未接受的状态。

## 线程与生命周期

几何导入、网格生成和大文件读取不能阻塞 UI 线程。结果通过适当的线程间通知回到对象所属线程；工作线程不能直接更新 QML 或操作视口渲染对象。

工程异步操作直接使用 `ProjectService` 的请求与完成契约，由 `ProjectViewModel` 将结果投递到 GUI 线程。工程预检、激活、持久化和取消直接服务实际工程命令，模拟任务面板不属于工作台能力。

STL 预检由 Rust 后台执行，拥有式 request/completion 传递来源字节与 Mesh；取消、重新选择和工程切换使旧结果失效。C++ 只投影 pending / ready 及完成通知，QML 依据 ready 启用确认。读取按块检查取消，解析后的迟到快照由 Rust 释放。接口与提交边界见 `crates/panta-core/src/project/` 和 `native/bridge/src/project_view_model.cpp`。

Qt Quick 渲染线程、GUI 线程和后台任务线程之间需要明确数据移交边界。场景更新使用快照或排队命令，并验证底层库对线程的要求。关闭工程、销毁视口或取消任务后，迟到的回调必须被任务 ID 和工程修订检查拒绝。

所有权归属需写在实现接口中：谁创建 QObject、谁释放 native 资源、任务是否持有工程快照、视口重建时哪些资源失效。跨语言对象不能靠不明确的裸指针生命周期维持。

## 交互验收

导入过程显示任务进度或不确定进度，失败后可继续操作原工程；网格任务可取消或明确显示不支持取消的阶段；选择面/边/实体时工程树与属性面板一致。错误摘要面向用户，详细诊断进入 Console/日志；不能把实现细节当作主操作流程。
