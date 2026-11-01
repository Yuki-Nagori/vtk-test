// 视口截图选项（task 040，034「导出」）：分辨率倍率与是否包含色标，确认后再选择保存位置。
// 透明背景未提供：WebGPU 背景 alpha 读回能力尚未在目标平台确认（见 task 040 记录）。
import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Panta.Bridge

DialogWindow {
    id: dialog
    objectName: "screenshotDialog"

    required property ProjectViewModel projectModel
    // 选定文件后由宿主执行保存并显示结果（CaeViewport::saveScreenshot 返回中文错误，空串表示成功）。
    signal saveRequested(url fileUrl, int scale, bool includeLegend)

    readonly property var scales: [1, 2, 3]
    readonly property int scale: scales[scaleCombo.currentIndex]
    readonly property bool includeLegend: legendCheck.checked

    width: Theme.screenshotDialogWidth
    height: Math.max(minimumHeight, screenshotFrame.implicitHeight)
    minimumWidth: Theme.screenshotDialogWidth
    minimumHeight: Theme.screenshotDialogMinimumHeight
    title: "视口截图"

    function open() {
        centerOnOwner();
        visible = true;
        requestActivate();
        scaleCombo.forceActiveFocus();
    }
    function chooseFile() {
        fileDialog.open();
    }

    DialogFrame {
        id: screenshotFrame
        window: dialog
        closeEnabled: true

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: Theme.dialogContentPadding
            spacing: Theme.spacingMedium

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Theme.spacingMedium
                rowSpacing: Theme.spacingMedium

                ThemedLabel {
                    text: "分辨率倍率"
                    textSize: Theme.fontBody
                }
                ThemedComboBox {
                    id: scaleCombo
                    objectName: "screenshotScaleCombo"
                    Layout.fillWidth: true
                    model: dialog.scales.map(value => value + " 倍")
                    Accessible.name: "分辨率倍率"
                }

                ThemedLabel {
                    text: "色标"
                    textSize: Theme.fontBody
                }
                CheckBox {
                    id: legendCheck
                    objectName: "screenshotLegendCheck"
                    text: "包含色标"
                    checked: true
                    font.pixelSize: Theme.fontBody
                }
            }

            ThemedLabel {
                Layout.fillWidth: true
                text: "按视口当前物理像素尺寸乘以倍率输出 PNG。透明背景暂不支持。"
                textSize: Theme.fontSmall
                textColor: Theme.colorTextMuted
                wrapMode: Text.Wrap
            }

            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
            }

            DialogButtonRow {
                Layout.fillWidth: true

                ThemedButton {
                    objectName: "screenshotAccept"
                    text: "保存…"
                    primaryAction: true
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    onClicked: dialog.chooseFile()
                }
                ThemedButton {
                    objectName: "screenshotCancel"
                    text: "取消"
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    borderColor: Theme.colorPanelLine
                    onClicked: dialog.close()
                }
            }
        }
    }

    FileDialog {
        id: fileDialog
        objectName: "screenshotFileDialog"
        title: "保存视口截图"
        currentFolder: dialog.projectModel.defaultLocationUrl
        fileMode: FileDialog.SaveFile
        defaultSuffix: "png"
        nameFilters: ["PNG 图像 (*.png)"]
        onAccepted: {
            dialog.close();
            dialog.saveRequested(selectedFile, dialog.scale, dialog.includeLegend);
        }
    }

    Shortcut {
        enabled: dialog.visible
        sequence: "Esc"
        onActivated: dialog.close()
    }
}
