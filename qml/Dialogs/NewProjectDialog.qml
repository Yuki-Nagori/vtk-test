// 独立的新建工程窗口；工程命令通过显式 ProjectViewModel 注入。
import QtQuick
import QtQuick.Dialogs
import QtQuick.Layouts
import Panta.Bridge

DialogWindow {
    id: dialog
    objectName: "newProjectDialog"

    required property ProjectViewModel projectModel
    property bool submitting: false
    signal projectCreated(string path)

    width: Theme.newProjectDialogWidth
    height: Theme.newProjectDialogHeight
    minimumWidth: Theme.newProjectDialogMinimumWidth
    minimumHeight: Theme.newProjectDialogMinimumHeight
    title: "新建工程"
    onClosing: event => {
        if (submitting)
            event.accepted = false;
    }

    function resetFields() {
        projectModel.clearError();
        projectNameField.text = "";
        locationField.text = projectModel.defaultLocation;
    }

    function open() {
        if (projectModel.projectOperationPending)
            return;
        resetFields();
        centerOnOwner();
        visible = true;
        requestActivate();
        projectNameField.forceActiveFocus();
    }

    function submit() {
        if (!visible || submitting || projectModel.projectOperationPending)
            return;
        submitting = projectModel.createProject(projectNameField.text, locationField.text);
        if (!submitting)
            projectNameField.forceActiveFocus();
    }

    Connections {
        target: dialog.projectModel
        function onProjectCreated(path) {
            if (!dialog.submitting)
                return;
            dialog.submitting = false;
            dialog.projectCreated(path);
            dialog.close();
        }
        function onProjectOperationFinished(operation, succeeded) {
            if (operation === "create" && !succeeded && dialog.submitting) {
                dialog.submitting = false;
                projectNameField.forceActiveFocus();
            }
        }
    }

    function errorMessage() {
        if (projectModel.errorCode === "project.empty_name" || projectModel.errorCode === "project.invalid_name") {
            return "请输入有效的工程名称。";
        }
        if (projectModel.errorCode === "project.location_empty" || projectModel.errorCode === "project.location_not_absolute") {
            return "请选择绝对工程路径。";
        }
        if (projectModel.errorCode === "project.location_create_failed") {
            return "无法创建工程位置。";
        }
        if (projectModel.errorCode === "project.already_exists") {
            return "同名工程已经存在。";
        }
        if (projectModel.errorCode === "project.manifest_invalid" || projectModel.errorCode === "project.unsupported_schema") {
            return "所选文件夹不是受支持的 panta 工程。";
        }
        if (projectModel.errorCode === "project.no_project") {
            return "请先打开或创建工程。";
        }
        if (projectModel.errorCode === "project.command_invalid") {
            return "工程修改无效。";
        }
        return "工程操作无法完成。";
    }

    DialogFrame {
        window: dialog
        closeEnabled: !dialog.submitting

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: Theme.spacingLarge
            Layout.rightMargin: Theme.spacingLarge
            Layout.topMargin: Theme.spacingMedium
            Layout.bottomMargin: Theme.spacingMedium
            spacing: Theme.spacingMedium

            ThemedLabel {
                Layout.fillWidth: true
                text: "创建工程目录并开始一个空工作区。"
                textSize: Theme.fontSmall
                textColor: Theme.colorTextMuted
                wrapMode: Text.Wrap
            }

            GridLayout {
                Layout.fillWidth: true
                enabled: !dialog.projectModel.projectOperationPending
                columns: 2
                columnSpacing: Theme.spacingMedium
                rowSpacing: Theme.spacingMedium

                ThemedLabel {
                    text: "工程名称"
                    textSize: Theme.fontBody
                }
                ThemedTextField {
                    id: projectNameField
                    objectName: "projectNameField"
                    Layout.fillWidth: true
                    Layout.preferredHeight: Theme.controlHeight
                    placeholderText: "我的工程"
                    Accessible.name: "工程名称"
                    invalid: dialog.projectModel.errorCode === "project.empty_name" || dialog.projectModel.errorCode === "project.invalid_name"
                    onAccepted: dialog.submit()
                }

                ThemedLabel {
                    text: "创建位置"
                    textSize: Theme.fontBody
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacingSmall

                    ThemedTextField {
                        id: locationField
                        objectName: "projectLocationField"
                        Layout.fillWidth: true
                        Layout.preferredHeight: Theme.controlHeight
                        placeholderText: "选择文件夹"
                        Accessible.name: "工程位置"
                        selectByMouse: true
                        invalid: dialog.projectModel.errorCode === "project.location_empty" || dialog.projectModel.errorCode === "project.location_not_absolute"
                    }
                    ThemedButton {
                        text: "浏览"
                        iconName: "open"
                        preserveIconColors: true
                        contentPadding: Theme.spacingSmall
                        hoverColor: Theme.colorHover
                        borderColor: Theme.colorPanelLine
                        onClicked: folderDialog.open()
                    }
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

            ThemedLabel {
                Layout.fillWidth: true
                text: "如果工程文件夹和 .panta 文件不存在，将自动创建。"
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
                spacing: Theme.spacingSmall

                ThemedButton {
                    objectName: "newProjectAccept"
                    enabled: !dialog.projectModel.projectOperationPending
                    text: "确定"
                    primaryAction: true
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    onClicked: dialog.submit()
                }
                ThemedButton {
                    objectName: "newProjectCancel"
                    enabled: !dialog.submitting
                    text: "取消"
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    borderColor: Theme.colorPanelLine
                    onClicked: dialog.close()
                }
            }
        }
    }

    FolderDialog {
        id: folderDialog
        title: "选择工程位置"
        currentFolder: dialog.projectModel.defaultLocationUrl
        onAccepted: {
            if (dialog.projectModel.projectOperationPending)
                return;
            const selectedPath = dialog.projectModel.localPath(selectedFolder);
            if (selectedPath.length > 0) {
                locationField.text = selectedPath;
                dialog.projectModel.clearError();
            }
        }
    }

    Shortcut {
        enabled: dialog.visible && !dialog.submitting
        sequence: "Esc"
        onActivated: dialog.close()
    }
}
