// 任务 026 / 028：大网格设置。表面 LOD 阈值/预算与体网格分块阈值（界面以“万”为单位）、
// 体网格模式与块缓存预算（MiB）；校验在 Rust，修改只影响之后打开的网格（块缓存预算立即生效）。
import QtQuick
import QtQuick.Layouts
import Panta.Bridge

DialogWindow {
    id: dialog
    objectName: "largeMeshSettingsDialog"

    required property ProjectViewModel projectModel

    width: Theme.newProjectDialogWidth
    // 与导入窗口一致（task 039）：高度随内容收紧，不以固定高度留白或裁切按钮。
    height: Math.max(minimumHeight, largeMeshFrame.implicitHeight)
    minimumWidth: Theme.newProjectDialogMinimumWidth
    minimumHeight: Theme.newProjectDialogMinimumHeight
    title: "大网格设置"

    function toWan(count) {
        return String(count / 10000);
    }

    function fromWan(text) {
        const value = Number(text);
        return text.length > 0 && isFinite(value) ? Math.round(value * 10000) : -1;
    }

    function load() {
        trianglesField.text = dialog.toWan(dialog.projectModel.lodActivationTriangles);
        budgetField.text = dialog.toWan(dialog.projectModel.lodBudgetTriangles);
        cellsField.text = dialog.toWan(dialog.projectModel.volumeStreamCells);
        modeCombo.currentIndex = dialog.projectModel.volumeMode;
        cacheField.text = String(dialog.projectModel.chunkCacheMib);
    }

    function cacheMib(text) {
        const value = Number(text);
        return text.length > 0 && isFinite(value) ? value : -1;
    }

    function open() {
        dialog.projectModel.clearError();
        dialog.load();
        centerOnOwner();
        visible = true;
        requestActivate();
        trianglesField.forceActiveFocus();
    }

    function submit() {
        if (!visible)
            return;
        if (dialog.projectModel.setLargeMeshLimits(dialog.fromWan(trianglesField.text), dialog.fromWan(budgetField.text), dialog.fromWan(cellsField.text), modeCombo.currentIndex, dialog.cacheMib(cacheField.text)))
            dialog.close();
    }

    function restoreDefaults() {
        dialog.projectModel.resetLargeMeshLimits();
        dialog.projectModel.clearError();
        dialog.load();
    }

    DialogFrame {
        id: largeMeshFrame
        window: dialog

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: Theme.dialogContentPadding
            spacing: Theme.spacingMedium

            ThemedLabel {
                Layout.fillWidth: true
                text: "超过阈值的表面网格以 LOD 显示（导出仍读取完整源资产）；超过阈值的体网格只驻留外表面，诊断和切面按块读取（派生分块文件放在缓存目录）。“按可用内存自动”按可用物理内存的一半推算阈值；“禁用分块”始终完整读入内存。修改只影响之后打开的网格。"
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
                    text: "表面 LOD 阈值 (万面)"
                    textSize: Theme.fontBody
                }
                ThemedTextField {
                    id: trianglesField
                    objectName: "lodTrianglesField"
                    Layout.fillWidth: true
                    Layout.preferredHeight: Theme.controlHeight
                    placeholderText: "100 – 200000"
                    Accessible.name: "表面 LOD 阈值"
                    selectByMouse: true
                    invalid: dialog.projectModel.errorDetail === "surface_triangles"
                    onAccepted: dialog.submit()
                }

                ThemedLabel {
                    text: "LOD 显示预算 (万面)"
                    textSize: Theme.fontBody
                }
                ThemedTextField {
                    id: budgetField
                    objectName: "lodBudgetField"
                    Layout.fillWidth: true
                    Layout.preferredHeight: Theme.controlHeight
                    placeholderText: "10 – 2000，且小于阈值"
                    Accessible.name: "LOD 显示预算"
                    selectByMouse: true
                    invalid: dialog.projectModel.errorDetail === "surface_budget"
                    onAccepted: dialog.submit()
                }

                ThemedLabel {
                    text: "体网格模式"
                    textSize: Theme.fontBody
                }
                ThemedComboBox {
                    id: modeCombo
                    objectName: "volumeModeCombo"
                    Layout.fillWidth: true
                    model: ["按阈值分块", "按可用内存自动", "禁用分块（完整驻留）"]
                    Accessible.name: "体网格模式"
                }

                ThemedLabel {
                    text: "体网格分块阈值 (万单元)"
                    textSize: Theme.fontBody
                }
                ThemedTextField {
                    id: cellsField
                    objectName: "volumeCellsField"
                    Layout.fillWidth: true
                    Layout.preferredHeight: Theme.controlHeight
                    placeholderText: "100 – 1000000"
                    Accessible.name: "体网格分块阈值"
                    selectByMouse: true
                    enabled: modeCombo.currentIndex !== 2
                    invalid: dialog.projectModel.errorDetail === "volume_cells"
                    onAccepted: dialog.submit()
                }

                ThemedLabel {
                    text: "块缓存预算 (MiB)"
                    textSize: Theme.fontBody
                }
                ThemedTextField {
                    id: cacheField
                    objectName: "chunkCacheField"
                    Layout.fillWidth: true
                    Layout.preferredHeight: Theme.controlHeight
                    placeholderText: "64 – 1048576"
                    Accessible.name: "块缓存预算"
                    selectByMouse: true
                    enabled: modeCombo.currentIndex !== 2
                    invalid: dialog.projectModel.errorDetail === "chunk_cache_mib"
                    onAccepted: dialog.submit()
                }
            }

            ThemedLabel {
                Layout.fillWidth: true
                visible: dialog.projectModel.error.length > 0
                text: dialog.projectModel.error
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
                    objectName: "largeMeshDefaults"
                    text: "恢复默认"
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    borderColor: Theme.colorPanelLine
                    onClicked: dialog.restoreDefaults()
                }
                ThemedButton {
                    objectName: "largeMeshAccept"
                    text: "应用"
                    primaryAction: true
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    onClicked: dialog.submit()
                }
                ThemedButton {
                    objectName: "largeMeshCancel"
                    text: "取消"
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    borderColor: Theme.colorPanelLine
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
