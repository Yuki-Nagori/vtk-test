// 任务 023：从 STEP 生成体网格（Netgen）。提交后对话框关闭，进度与取消在状态栏。
import QtQuick
import QtQuick.Dialogs
import QtQuick.Layouts
import Panta.Bridge

DialogWindow {
    id: dialog
    objectName: "generateVolumeDialog"

    required property ProjectViewModel projectModel

    width: Theme.newProjectDialogWidth
    // 与导入窗口一致（task 039）：高度随内容收紧，不以固定高度留白或裁切按钮。
    height: Math.max(minimumHeight, volumeFrame.implicitHeight)
    minimumWidth: Theme.newProjectDialogMinimumWidth
    minimumHeight: Theme.newProjectDialogMinimumHeight
    title: "从 STEP 生成体网格"

    function open() {
        if (dialog.projectModel.generationPending)
            return;
        dialog.projectModel.clearError();
        maxLengthField.text = "0";
        centerOnOwner();
        visible = true;
        requestActivate();
        stepField.forceActiveFocus();
    }

    function submit() {
        if (!visible || dialog.projectModel.generationPending)
            return;
        const maxLength = Number(maxLengthField.text);
        if (dialog.projectModel.beginGenerateVolumeFromStep(stepField.text, isFinite(maxLength) ? maxLength : -1))
            dialog.close();
    }

    function errorMessage() {
        if (dialog.projectModel.errorCode === "project.import_file_missing")
            return "STEP 文件不存在。";
        if (dialog.projectModel.errorCode === "project.no_project")
            return "请先打开或创建工程。";
        return dialog.projectModel.error;
    }

    DialogFrame {
        id: volumeFrame
        window: dialog

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: Theme.dialogContentPadding
            spacing: Theme.spacingMedium

            ThemedLabel {
                Layout.fillWidth: true
                text: "读取 STEP 实体并用 Netgen 在后台生成四面体体网格，完成后作为体网格部件加入工程。"
                textSize: Theme.fontSmall
                textColor: Theme.colorTextMuted
                wrapMode: Text.Wrap
            }

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Theme.spacingMedium
                rowSpacing: Theme.spacingMedium

                ThemedLabel {
                    text: "STEP 文件"
                    textSize: Theme.fontBody
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacingMedium

                    ThemedTextField {
                        id: stepField
                        objectName: "generateStepField"
                        Layout.fillWidth: true
                        Layout.preferredHeight: Theme.controlHeight
                        placeholderText: "选择 .step / .stp 文件"
                        Accessible.name: "STEP 文件"
                        selectByMouse: true
                        invalid: dialog.projectModel.errorCode === "project.import_file_missing"
                        onAccepted: dialog.submit()
                    }
                    ThemedButton {
                        text: "浏览"
                        iconName: "open"
                        preserveIconColors: true
                        contentPadding: Theme.spacingMedium
                        hoverColor: Theme.colorHover
                        borderColor: Theme.colorPanelLine
                        onClicked: stepFileDialog.open()
                    }
                }

                ThemedLabel {
                    text: "最大单元尺寸 (mm)"
                    textSize: Theme.fontBody
                }
                ThemedTextField {
                    id: maxLengthField
                    objectName: "generateMaxLengthField"
                    Layout.fillWidth: true
                    Layout.preferredHeight: Theme.controlHeight
                    placeholderText: "0 = 自动"
                    Accessible.name: "最大单元尺寸"
                    selectByMouse: true
                    validator: DoubleValidator {
                        bottom: 0
                        top: 1000000
                        notation: DoubleValidator.StandardNotation
                    }
                    invalid: dialog.projectModel.errorCode === "project.volume_generation_failed"
                    onAccepted: dialog.submit()
                }
            }

            ThemedLabel {
                Layout.fillWidth: true
                visible: dialog.projectModel.error.length > 0
                text: dialog.errorMessage()
                textSize: Theme.fontSmall
                textColor: Theme.colorError
                wrapMode: Text.Wrap
            }

            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
            }

            DialogButtonRow {
                Layout.fillWidth: true
                spacing: Theme.spacingMedium

                ThemedButton {
                    objectName: "generateVolumeAccept"
                    enabled: !dialog.projectModel.generationPending && stepField.text.length > 0
                    text: "生成"
                    primaryAction: true
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    onClicked: dialog.submit()
                }
                ThemedButton {
                    objectName: "generateVolumeCancel"
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
        id: stepFileDialog
        title: "选择 STEP 文件"
        fileMode: FileDialog.OpenFile
        nameFilters: ["STEP 文件 (*.step *.stp)"]
        onAccepted: {
            const selectedPath = dialog.projectModel.localPath(selectedFile);
            if (selectedPath.length > 0) {
                stepField.text = selectedPath;
                dialog.projectModel.clearError();
            }
        }
    }

    Shortcut {
        enabled: dialog.visible
        sequence: "Esc"
        onActivated: dialog.close()
    }
}
