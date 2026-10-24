// 状态栏（task 036）：消息 | 拾取读数 | 单位 | LOD/流式 | 时间步 | 后台任务进度与取消。
// 各字段单行省略，完整文字放在 tooltip 中；内容由宿主计算后传入。
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

PanelSurface {
    id: statusBar

    property bool busy: false
    property string message: ""
    // 消息级别，取值同 Theme.statusColor；空串为普通状态（就绪 / 处理中）。
    property string messageLevel: ""
    property string pickText: ""
    property bool meshActive: false
    property string lodStatus: ""
    property string timeStep: ""
    property string timeStepDetail: ""
    property string taskName: ""
    // < 0 表示没有后台任务。
    property real taskProgress: -1

    signal cancelTaskRequested

    implicitHeight: Theme.panelHeaderHeight

    component StatusField: Label {
        // 悬停提示文字；showDetailAlways 为 false 时仅在被省略时提示。
        property string detail: text
        property bool showDetailAlways: false
        Layout.maximumWidth: Theme.statusFieldMaximumWidth
        color: Theme.colorTextMuted
        font.pixelSize: Theme.fontCaption
        font.features: Theme.fontFeaturesNumeric
        elide: Text.ElideRight
        maximumLineCount: 1
        ToolTip.visible: hover.hovered && detail.length > 0 && (truncated || showDetailAlways)
        ToolTip.text: detail
        ToolTip.delay: Theme.toolTipDelay
        HoverHandler {
            id: hover
        }
    }
    component FieldSeparator: Rectangle {
        Layout.fillHeight: true
        Layout.topMargin: Theme.spacingXSmall
        Layout.bottomMargin: Theme.spacingXSmall
        implicitWidth: Theme.borderWidth
        color: Theme.colorPanelLine
    }

    Rectangle {
        anchors.top: parent.top
        width: parent.width
        height: Theme.borderWidth
        color: Theme.colorPanelLine
    }
    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Theme.spacingMedium
        anchors.rightMargin: Theme.spacingMedium
        spacing: Theme.spacingMedium

        BusyIndicator {
            objectName: "busyIndicator"
            Layout.preferredWidth: Theme.iconSizeDefault
            Layout.preferredHeight: Theme.iconSizeDefault
            running: statusBar.busy
            visible: running
        }
        ThemedIcon {
            objectName: "statusMessageIcon"
            visible: name.length > 0 && !statusBar.busy
            name: Theme.statusIcon(statusBar.messageLevel)
            iconSize: Theme.iconSizeSmall
            color: Theme.statusColor(statusBar.messageLevel)
        }
        Label {
            objectName: "statusMessage"
            Layout.fillWidth: true
            text: statusBar.message
            color: Theme.statusColor(statusBar.messageLevel)
            font.pixelSize: Theme.fontCaption
            elide: Text.ElideRight
            maximumLineCount: 1
            ToolTip.visible: truncated && messageHover.hovered
            ToolTip.text: text
            ToolTip.delay: Theme.toolTipDelay
            HoverHandler {
                id: messageHover
            }
        }
        FieldSeparator {
            visible: statusBar.meshActive
        }
        StatusField {
            objectName: "pickReadout"
            visible: statusBar.meshActive
            text: statusBar.pickText
        }
        FieldSeparator {
            visible: statusBar.meshActive
        }
        // 工程内部长度单位固定为 mm（task 017 测距同口径）。
        StatusField {
            objectName: "statusUnits"
            visible: statusBar.meshActive
            text: "单位 mm"
        }
        FieldSeparator {
            visible: lodField.visible
        }
        // 大网格 LOD / 流式提示（task 019/025）：完整说明见 tooltip。
        StatusField {
            id: lodField
            objectName: "activeMeshLodStatus"
            visible: statusBar.meshActive && statusBar.lodStatus.length > 0
            text: statusBar.lodStatus
            showDetailAlways: true
        }
        FieldSeparator {
            visible: timeField.visible
        }
        StatusField {
            id: timeField
            objectName: "statusTimeStep"
            visible: statusBar.timeStep.length > 0
            text: statusBar.timeStep
            detail: statusBar.timeStepDetail
            showDetailAlways: true
        }
        FieldSeparator {
            visible: statusBar.taskProgress >= 0
        }
        // task 016：当前后台任务的百分比与取消；多任务明细见底部面板「任务」页。
        Label {
            objectName: "taskProgressLabel"
            visible: statusBar.taskProgress >= 0
            text: statusBar.taskName + " " + Math.round(statusBar.taskProgress * 100) + "%"
            color: Theme.colorTextMuted
            font.pixelSize: Theme.fontCaption
            font.features: Theme.fontFeaturesNumeric
        }
        ProgressBar {
            objectName: "taskProgressBar"
            Layout.preferredWidth: Theme.statusProgressWidth
            visible: statusBar.taskProgress >= 0
            from: 0
            to: 1
            value: Math.max(0, statusBar.taskProgress)
        }
        ThemedToolButton {
            objectName: "taskCancelButton"
            visible: statusBar.taskProgress >= 0
            iconName: "close"
            accessibleName: "取消" + statusBar.taskName
            onClicked: statusBar.cancelTaskRequested()
        }
    }
}
