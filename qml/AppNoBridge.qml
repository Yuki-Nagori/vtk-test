// 关闭 Bridge 时保留资源模块的启动诊断窗口。
import QtQuick
import QtQuick.Controls

ApplicationWindow {
    minimumWidth: Theme.windowMinimumWidth
    minimumHeight: Theme.windowMinimumHeight
    visible: true
    title: "可视化渲染测试"
    Label {
        objectName: "shellCaption"
        anchors.centerIn: parent
        text: "未启用工程服务"
    }
}
