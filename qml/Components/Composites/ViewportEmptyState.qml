// 视口空状态引导（task 038）：无打开文档时原生视口表面隐藏，此处给出导入 / 打开 / 最近工程入口。
// 只发出意图信号，对话框与工程操作由宿主负责。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: emptyState

    property bool projectOpen: false
    property bool canCreateOrOpen: true
    property bool canImport: false
    property var recentProjects: []

    signal importRequested
    signal newProjectRequested
    signal openProjectRequested
    signal recentProjectRequested(string path)

    Accessible.role: Accessible.Pane
    Accessible.name: "开始使用"

    gradient: Gradient {
        GradientStop {
            position: 0
            color: Theme.colorViewportTop
        }
        GradientStop {
            position: 1
            color: Theme.colorViewportBottom
        }
    }

    component ViewportButton: ThemedButton {
        contentColor: primaryAction ? Theme.colorText : Theme.colorViewportText
        borderColor: primaryAction ? Theme.colorDialogPrimaryBorder : Theme.colorViewportLine
        hoverColor: Theme.colorViewportHover
    }

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(Theme.viewportEmptyStateWidth, emptyState.width - 2 * Theme.spacingXXLarge)
        spacing: Theme.spacingMedium

        ThemedLabel {
            Layout.fillWidth: true
            text: "开始使用"
            textColor: Theme.colorViewportText
            textSize: Theme.fontTitle
            font.bold: true
            horizontalAlignment: Text.AlignHCenter
        }
        ThemedLabel {
            objectName: "viewportEmptyHint"
            Layout.fillWidth: true
            text: emptyState.projectOpen ? "导入网格后在此查看模型。" : "新建或打开工程，然后导入网格。"
            textColor: Theme.colorViewportTextMuted
            wrapMode: Text.Wrap
            horizontalAlignment: Text.AlignHCenter
        }
        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: Theme.spacingXLarge
            spacing: Theme.dialogActionSpacing

            ViewportButton {
                objectName: "viewportEmptyImport"
                text: "导入网格…"
                primaryAction: emptyState.projectOpen
                enabled: emptyState.canImport
                onClicked: emptyState.importRequested()
            }
            ViewportButton {
                objectName: "viewportEmptyNew"
                text: "新建工程…"
                primaryAction: !emptyState.projectOpen
                enabled: emptyState.canCreateOrOpen
                onClicked: emptyState.newProjectRequested()
            }
            ViewportButton {
                objectName: "viewportEmptyOpen"
                text: "打开工程…"
                enabled: emptyState.canCreateOrOpen
                onClicked: emptyState.openProjectRequested()
            }
        }
        ThemedLabel {
            Layout.fillWidth: true
            Layout.topMargin: Theme.spacingLarge
            visible: recentRepeater.count > 0
            text: "最近工程"
            textColor: Theme.colorViewportTextMuted
            textSize: Theme.fontSmall
        }
        Repeater {
            id: recentRepeater
            model: emptyState.recentProjects.slice(0, Theme.viewportRecentProjectLimit)
            delegate: ThemedToolButton {
                required property string modelData
                required property int index
                objectName: "viewportEmptyRecent-" + index
                Layout.fillWidth: true
                clip: true
                // 只显示文件名，完整路径放进悬停提示与读屏名称。
                text: modelData.split(/[\\/]/).pop()
                accessibleName: "打开最近工程 " + modelData
                ToolTip.visible: hovered
                ToolTip.text: modelData
                contentColor: Theme.colorViewportText
                hoverColor: Theme.colorViewportHover
                enabled: emptyState.canCreateOrOpen
                onClicked: emptyState.recentProjectRequested(modelData)
            }
        }
    }
}
