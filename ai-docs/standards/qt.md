# Qt 对象、线程与部署

更新日期：2026-10-20。适用于现有 Qt 应用与 Bridge；本仓库的窗口和部署验证需记录实际平台与结果。

适用于 Qt 6 C++ bridge、应用入口和资源部署。具体 Qt 版本见[技术基线](baseline.md)与供给脚本。显示缩放和多屏状态遵循[显示缩放与多屏模块](../modules/display-scaling-and-multi-monitor.md)。

## 官方依据

QObject 有线程归属，事件在对象所属线程处理；QObject 可重入不等于同一实例可被任意线程并发操作。[Threads and QObjects](https://doc.qt.io/qt-6/threads-qobject.html)

屏幕、窗口和 DPI 变化通过 `QScreen`/`QWindow` 的公开信号观察；逻辑像素与物理像素转换使用运行期 `devicePixelRatioF()`，不能将 `QScreen*`、屏幕下标或整数缩放比写入持久化状态。[QScreen](https://doc.qt.io/qt-6/qscreen.html)、[High DPI](https://doc.qt.io/qt-6/highdpi.html)

C++ 类型可以通过 QML 注册宏与模块构建暴露到 QML；属性、方法与信号需符合类型系统要求。[C++ QML Types](https://doc.qt.io/qt-6/qtqml-cppintegration-definetypes.html)

QML 部署脚本参与 install 流程，其平台支持和行为依 Qt 版本而定。[QML Deployment](https://doc.qt.io/qt-6/qt-generate-deploy-qml-app-script.html)

## 项目规则

- 当前导入窗口使用 QML 属性和 ViewModel 请求回执，见[导入交互](../modules/import-interaction.md)。后台请求的成功、取消和提交由 Rust 服务裁决，Qt 本地状态和窗口关闭不能充当业务完成回执。

## 验证

Qt 改动验证属性通知、资源加载、线程归属与关闭时序；部署改动另验证无开发环境路径的安装产物。Qt Quick 图形线程不能按 GUI 线程规则直接推断，另读 [VTK](vtk.md)。
