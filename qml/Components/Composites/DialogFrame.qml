// 公共标题与内容布局；内容显式注入，关闭请求交给窗口守卫。
import QtQuick
import QtQuick.Layouts

Rectangle {
    id: frame
    required property Window window
    property bool closeEnabled: true
    default property alias content: body.data
    anchors.fill: parent
    // 供窗口按内容收紧高度（task 035）。
    implicitHeight: body.implicitHeight + 2 * Theme.borderWidth
    implicitWidth: body.implicitWidth + 2 * Theme.borderWidth
    color: Theme.colorPanel
    // 对话框圆角 6 px 与细边框（034 视觉规范）；窗口底色透明，圆角外不绘制。
    radius: Theme.radiusDialog
    border.width: Theme.borderWidth
    border.color: Theme.colorPanelLine
    ColumnLayout {
        id: body
        anchors.fill: parent
        anchors.margins: Theme.borderWidth
        spacing: 0
        DialogTitleBar {
            Layout.fillWidth: true
            window: frame.window
            caption: frame.window.title
            closeEnabled: frame.closeEnabled
            onCloseRequested: frame.window.close()
        }
    }
}
