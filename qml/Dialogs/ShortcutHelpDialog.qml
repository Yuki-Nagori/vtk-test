// 「帮助 → 快捷键与鼠标操作」（task 040，034 Q7）：快捷键表与宿主 Shortcut 同源，鼠标映射保持现状并在此说明。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

DialogWindow {
    id: dialog
    objectName: "shortcutHelpDialog"

    // 每项为 { group, label, keys }，keys 为平台显示文字（macOS 显示 ⌘ 等符号）。
    property var shortcuts: []
    // 每项为 [操作, 说明]。
    property var mouseActions: []
    property var treeKeys: []

    width: Theme.helpDialogWidth
    height: Theme.helpDialogHeight
    minimumWidth: Theme.helpDialogWidth
    minimumHeight: Theme.newProjectDialogMinimumHeight
    title: "快捷键与鼠标操作"

    function open() {
        centerOnOwner();
        visible = true;
        requestActivate();
        closeButton.forceActiveFocus();
    }

    component SectionTitle: ThemedLabel {
        Layout.fillWidth: true
        Layout.topMargin: Theme.spacingMedium
        textSize: Theme.fontHeading
        font.weight: Font.DemiBold
    }
    component KeyRow: RowLayout {
        id: keyRow
        property string keys: ""
        property string label: ""
        Layout.fillWidth: true
        spacing: Theme.spacingMedium
        ThemedLabel {
            Layout.preferredWidth: Theme.helpKeyColumnWidth
            text: keyRow.keys
            textColor: Theme.colorTextMuted
        }
        ThemedLabel {
            Layout.fillWidth: true
            text: keyRow.label
            wrapMode: Text.Wrap
        }
    }

    DialogFrame {
        window: dialog
        closeEnabled: true

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: Theme.dialogContentPadding
            spacing: Theme.spacingMedium

            ScrollView {
                objectName: "shortcutHelpScroll"
                Layout.fillWidth: true
                Layout.fillHeight: true
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    spacing: Theme.spacingXSmall

                    SectionTitle {
                        text: "快捷键"
                    }
                    Repeater {
                        objectName: "shortcutHelpRows"
                        model: dialog.shortcuts
                        delegate: KeyRow {
                            required property var modelData
                            keys: modelData.keys
                            label: modelData.group + " · " + modelData.label
                        }
                    }
                    SectionTitle {
                        text: "视口鼠标"
                    }
                    Repeater {
                        model: dialog.mouseActions
                        delegate: KeyRow {
                            required property var modelData
                            keys: modelData[0]
                            label: modelData[1]
                        }
                    }
                    SectionTitle {
                        text: "对象树与焦点"
                    }
                    Repeater {
                        model: dialog.treeKeys
                        delegate: KeyRow {
                            required property var modelData
                            keys: modelData[0]
                            label: modelData[1]
                        }
                    }
                }
            }

            DialogButtonRow {
                Layout.fillWidth: true
                ThemedButton {
                    id: closeButton
                    objectName: "shortcutHelpClose"
                    text: "关闭"
                    primaryAction: true
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    onClicked: dialog.close()
                }
            }
        }
    }

    Shortcut {
        enabled: dialog.visible
        sequence: "Esc"
        onActivated: dialog.close()
    }
}
