// 主工具栏（task 036）：图标 + tooltip，按 034 分组——工程 | 导入/导出 | 视图预设 复位 | 切面 | 截图。
// 启用条件由宿主计算后传入，与菜单和快捷键共用；工具栏只发出意图信号。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ToolBar {
    id: toolBar

    property bool canCreateOrOpen: false
    property bool canSave: false
    property bool canImport: false
    property bool canExport: false
    property bool exportPending: false
    property bool canScreenshot: false
    property bool canUseView: false
    property bool canToggleSection: false
    property bool sectionActive: false
    // 视图预设：[[id, 名称], …]，与「视图」菜单共用同一份列表。
    property var viewPresets: []

    signal newProjectRequested
    signal openProjectRequested
    signal saveProjectRequested
    signal importRequested
    signal exportRequested
    signal viewPresetRequested(string name)
    signal resetViewRequested
    signal sectionToggled(bool active)
    signal screenshotRequested

    // 工具栏按钮 28 px、图标 20 px（034 视觉规范）。
    component ToolbarButton: ThemedToolButton {
        controlHeight: Theme.toolButtonSize
        iconSize: Theme.toolbarIconSize
    }
    component GroupSeparator: Rectangle {
        Layout.fillHeight: true
        Layout.topMargin: Theme.spacingXSmall
        Layout.bottomMargin: Theme.spacingXSmall
        Layout.leftMargin: Theme.spacingXSmall
        Layout.rightMargin: Theme.spacingXSmall
        implicitWidth: Theme.borderWidth
        color: Theme.colorPanelLine
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Theme.spacingMedium
        anchors.rightMargin: Theme.spacingMedium
        spacing: Theme.spacingTiny

        ToolbarButton {
            objectName: "newProjectButton"
            iconName: "project-new"
            accessibleName: "新建工程（Ctrl+N）"
            enabled: toolBar.canCreateOrOpen
            onClicked: toolBar.newProjectRequested()
        }
        ToolbarButton {
            objectName: "openProjectButton"
            iconName: "project-open"
            accessibleName: "打开工程（Ctrl+O）"
            enabled: toolBar.canCreateOrOpen
            onClicked: toolBar.openProjectRequested()
        }
        ToolbarButton {
            objectName: "saveProjectButton"
            iconName: "project-save"
            accessibleName: "保存工程（Ctrl+S）"
            enabled: toolBar.canSave
            onClicked: toolBar.saveProjectRequested()
        }
        GroupSeparator {}
        ToolbarButton {
            objectName: "importStlButton"
            iconName: "mesh-import"
            accessibleName: "导入网格（Ctrl+I）"
            enabled: toolBar.canImport
            onClicked: toolBar.importRequested()
        }
        ToolbarButton {
            objectName: "exportMeshButton"
            iconName: "mesh-export"
            accessibleName: toolBar.exportPending ? "正在导出…" : "导出活动网格（Ctrl+E）"
            enabled: toolBar.canExport
            onClicked: toolBar.exportRequested()
        }
        GroupSeparator {}
        ToolbarButton {
            id: viewPresetButton
            objectName: "viewPresetMenuButton"
            iconName: "view-preset"
            accessibleName: "视图预设"
            enabled: toolBar.canUseView
            onClicked: viewPresetMenu.popup(viewPresetButton, 0, viewPresetButton.height)

            // 独立窗口弹出，避免被原生视口表面遮挡（同菜单栏）。
            Menu {
                id: viewPresetMenu
                objectName: "viewPresetMenu"
                popupType: Popup.Window
                Repeater {
                    model: toolBar.viewPresets
                    delegate: MenuItem {
                        required property var modelData
                        objectName: "viewPreset_" + modelData[0]
                        text: modelData[1]
                        onTriggered: toolBar.viewPresetRequested(modelData[0])
                    }
                }
            }
        }
        ToolbarButton {
            objectName: "resetViewButton"
            iconName: "view-reset"
            accessibleName: "复位视图"
            enabled: toolBar.canUseView
            onClicked: toolBar.resetViewRequested()
        }
        GroupSeparator {}
        // 开关状态由边框、图标颜色和 tooltip 共同表达，不只靠颜色（同播放栏循环按钮）。
        ToolbarButton {
            objectName: "toolbarSectionButton"
            iconName: "section"
            checkable: true
            checked: toolBar.sectionActive
            enabled: toolBar.canToggleSection
            contentColor: checked ? Theme.colorFocus : Theme.colorIcon
            borderColor: checked ? Theme.colorFocus : Theme.colorTransparent
            accessibleName: checked ? "切面：开" : "切面：关"
            onToggled: {
                toolBar.sectionToggled(checked);
                // 用户点击会打断绑定；重新绑定到模型状态，切面被拒绝时即回显。
                checked = Qt.binding(() => toolBar.sectionActive);
            }
        }
        GroupSeparator {}
        ToolbarButton {
            objectName: "screenshotButton"
            iconName: "screenshot"
            accessibleName: "保存视口 PNG 截图（Ctrl+Shift+P）"
            enabled: toolBar.canScreenshot
            onClicked: toolBar.screenshotRequested()
        }
        Item {
            Layout.fillWidth: true
        }
    }
}
