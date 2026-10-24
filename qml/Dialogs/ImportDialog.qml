// 导入窗口只提交意图；预检由 Rust 后台完成，工程提交由服务持有。
import QtQuick
import QtQuick.Dialogs
import QtQuick.Layouts
import Panta.Bridge

DialogWindow {
    id: dialog
    objectName: "importDialog"

    required property ProjectViewModel projectModel
    property string sourcePath: ""
    property var pendingImportRequestId: 0

    width: Theme.importDialogWidth
    // task 035：高度随内容收紧，警告/错误出现时自动增高，不留大片空白也不裁切按钮。
    height: Math.max(minimumHeight, importFrame.implicitHeight)
    minimumWidth: Theme.importDialogMinimumWidth
    minimumHeight: Theme.importDialogMinimumHeight
    title: "导入"

    readonly property var meshTypes: projectModel.meshTypes
    readonly property var unitValues: ["millimeters", "centimeters", "meters", "inches"]
    // 单位来源（task 035）："default" 默认值 / "cfd" CFD 结果按 SI 米制 / "user" 用户选择。
    property string unitSource: "default"

    onVisibleChanged: {
        if (!visible) {
            pendingImportRequestId = 0;
            projectModel.cancelImportPreview();
            projectModel.cancelImport();
        }
    }

    function resetFields() {
        projectModel.cancelImportPreview();
        projectModel.clearError();
        sourcePath = "";
        meshTypeCombo.currentIndex = meshTypes.findIndex(entry => entry.id === projectModel.defaultMeshType);
        unitsCombo.currentIndex = 0;
        unitSource = "default";
    }

    function open() {
        resetFields();
        pendingImportRequestId = 0;
        centerOnOwner();
        visible = true;
        requestActivate();
    }

    // 拖放导入（task 018）：打开对话框并预填源文件，仍需用户确认网格类型与单位。
    // OpenFOAM 算例与 CGNS（task 032）：CFD 结果按 SI 米制，默认按实体 3D 体网格导入，用户仍可改选。
    function isCfdPath(path: string): bool {
        const lower = path.toLowerCase();
        return lower.endsWith(".foam") || lower.endsWith(".openfoam") || lower.endsWith(".cgns") || /[\\/]system[\\/]controldict$/.test(lower);
    }

    // ParaView 集合 .pvd（task 033）：只有体导入保留时间步，默认实体 3D；单位不推断，沿用当前选择。
    function selectSource(path: string) {
        sourcePath = path;
        const cfd = isCfdPath(path);
        if (cfd) {
            unitsCombo.currentIndex = unitValues.indexOf("meters");
            unitSource = "cfd";
        } else if (unitSource === "cfd") {
            // 由 CFD 换成普通网格时，回到默认单位，避免沿用自动设置的米制而来源标注失真。
            unitsCombo.currentIndex = 0;
            unitSource = "default";
        }
        if (cfd || path.toLowerCase().endsWith(".pvd")) {
            const solid = meshTypes.findIndex(entry => entry.id === "solid-3d");
            if (solid >= 0)
                meshTypeCombo.currentIndex = solid;
        }
        projectModel.inspectStl(path);
    }

    function openWithFile(path: string) {
        open();
        if (!path)
            return;
        selectSource(path);
    }

    function chooseFile() {
        fileDialog.open();
    }

    function submit() {
        if (!sourcePath || !projectModel.importPreviewReady)
            return;
        if (projectModel.beginImportStl(sourcePath, meshTypes[meshTypeCombo.currentIndex].id, unitValues[unitsCombo.currentIndex], false))
            pendingImportRequestId = projectModel.importRequestId;
    }

    function errorMessage() {
        if (projectModel.errorCode === "project.import_file_missing")
            return "所选网格文件不存在。";
        if (projectModel.errorCode === "project.import_invalid_file")
            return "请选择要导入的网格文件。";
        if (projectModel.errorCode === "project.import_parse_failed")
            return "无法读取所选网格文件。";
        if (projectModel.errorCode === "project.import_asset_write_failed")
            return "无法将网格资产保存到工程中。";
        return "导入操作无法完成。";
    }

    function unitSourceText(): string {
        if (unitSource === "cfd")
            return "CFD 结果按 SI 米制";
        if (unitSource === "user")
            return "用户选择";
        return "默认";
    }

    DialogFrame {
        id: importFrame
        window: dialog
        closeEnabled: true

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: Theme.spacingLarge
            Layout.rightMargin: Theme.spacingLarge
            Layout.topMargin: Theme.spacingMedium
            Layout.bottomMargin: Theme.spacingLarge
            spacing: Theme.spacingSmall

            ThemedLabel {
                Layout.fillWidth: true
                text: "导入网格文件前请选择网格类型和单位。"
                textSize: Theme.fontSmall
                textColor: Theme.colorTextMuted
                wrapMode: Text.Wrap
            }

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Theme.spacingMedium
                rowSpacing: Theme.spacingSmall

                ThemedLabel {
                    text: "文件"
                    textSize: Theme.fontBody
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacingSmall

                    ThemedTextField {
                        id: sourceField
                        Layout.fillWidth: true
                        Layout.preferredHeight: Theme.controlHeight
                        text: dialog.sourcePath
                        readOnly: true
                        placeholderText: "选择网格文件"
                        Accessible.name: "网格文件"
                    }
                    ThemedButton {
                        text: "浏览"
                        enabled: !dialog.projectModel.importPending
                        iconName: "open"
                        preserveIconColors: true
                        contentPadding: Theme.spacingSmall
                        hoverColor: Theme.colorHover
                        borderColor: Theme.colorPanelLine
                        onClicked: dialog.chooseFile()
                    }
                }

                ThemedLabel {
                    text: "网格类型"
                    textSize: Theme.fontBody
                }
                ThemedComboBox {
                    id: meshTypeCombo
                    Layout.fillWidth: true
                    enabled: !dialog.projectModel.importPending
                    model: dialog.meshTypes.map(entry => entry.sourceText)
                    Accessible.name: "网格类型"
                }

                ThemedLabel {
                    text: "单位"
                    textSize: Theme.fontBody
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacingSmall

                    ThemedComboBox {
                        id: unitsCombo
                        Layout.fillWidth: true
                        enabled: !dialog.projectModel.importPending
                        model: ["毫米", "厘米", "米", "英寸"]
                        Accessible.name: "单位"
                        onActivated: dialog.unitSource = "user"
                    }
                    ThemedLabel {
                        objectName: "importUnitSource"
                        text: "来源：" + dialog.unitSourceText()
                        textSize: Theme.fontSmall
                        textColor: Theme.colorTextMuted
                    }
                }

                ThemedLabel {
                    text: "近似尺寸"
                    textSize: Theme.fontBody
                }
                ThemedLabel {
                    Layout.fillWidth: true
                    text: dialog.projectModel.importPreviewReady && dialog.sourcePath.length > 0 ? dialog.projectModel.importPreviewDimensions + " " + unitsCombo.currentText : "请选择网格文件"
                    textSize: Theme.fontBody
                    textColor: Theme.colorTextMuted
                }
            }

            ThemedLabel {
                Layout.fillWidth: true
                visible: dialog.projectModel.importPreviewLargeMeshWarning
                text: "预计解压后的网格数据会超过 2 GiB。仍可继续导入，但可能耗时较长并占用大量内存和磁盘空间；继续前请确认计算机和工程所在磁盘有足够资源。"
                textSize: Theme.fontSmall
                textColor: Theme.colorError
                wrapMode: Text.Wrap
            }

            ThemedLabel {
                Layout.fillWidth: true
                visible: dialog.projectModel.importPending || dialog.projectModel.importPreviewPending
                // 细粒度进度（task 019）：预检读取 0–50% / 解析 50–100%；导入复核、写资产按块上报。
                text: dialog.projectModel.importPending ? "正在将网格文件添加到工程… " + Math.round(Math.max(0, dialog.projectModel.importProgress) * 100) + "%" : "正在读取并解析网格… " + Math.round(Math.max(0, dialog.projectModel.importPreviewProgress) * 100) + "%"
                textSize: Theme.fontSmall
                textColor: Theme.colorTextMuted
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

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacingSmall

                Item {
                    Layout.fillWidth: true
                }

                ThemedButton {
                    text: dialog.projectModel.importPending ? "正在导入…" : "确定"
                    objectName: "importAccept"
                    primaryAction: true
                    enabled: dialog.sourcePath.length > 0 && dialog.projectModel.importPreviewReady && !dialog.projectModel.importPending
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    onClicked: dialog.submit()
                }
                ThemedButton {
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
        title: "选择网格文件"
        fileMode: FileDialog.OpenFile
        nameFilters: ["网格文件 (*.stl *.vtu *.vtp *.vtk *.pvd *.obj *.ply *.foam *.openfoam controlDict *.cgns)", "STL 文件 (*.stl)", "VTK 文件 (*.vtu *.vtp *.vtk *.pvd)", "OpenFOAM 算例 (*.foam *.openfoam controlDict)", "CGNS 文件 (*.cgns)", "OBJ 文件 (*.obj)", "PLY 文件 (*.ply)"]
        onAccepted: {
            if (!dialog.visible)
                return;
            const selectedPath = dialog.projectModel.localPath(selectedFile);
            if (!selectedPath)
                return;
            dialog.selectSource(selectedPath);
        }
    }

    Connections {
        target: dialog.projectModel
        function onProjectImportFinished(requestId, succeeded) {
            if (succeeded && dialog.visible && requestId === dialog.pendingImportRequestId) {
                dialog.pendingImportRequestId = 0;
                dialog.close();
            }
        }
    }
}
