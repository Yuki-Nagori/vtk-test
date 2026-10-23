// 最小渲染工作台：工程负责资产与文档，VTK 负责原生视口。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Panta.Bridge

ApplicationWindow {
    id: shellWindow
    objectName: "workbench"
    minimumWidth: Theme.windowMinimumWidth
    minimumHeight: Theme.windowMinimumHeight
    width: 1280
    height: 800
    title: projectOpen ? (projectModel.dirty ? "* " : "") + projectModel.currentName + " · 可视化渲染测试" : "可视化渲染测试"
    visible: true
    color: Theme.colorPanel

    readonly property bool projectOpen: projectModel.currentPath.length > 0
    readonly property bool busy: projectModel.projectOperationPending || projectModel.importPending || projectModel.exportPending
    readonly property bool meshActive: projectModel.activeDocumentId.length > 0
    // 导出 / 截图的一次性结果提示；新的错误或操作会覆盖它。
    property string notice: ""
    property bool noticeIsError: false

    // 活动文档的工作区快照条目（含状态与激活诊断）；无活动文档时为空对象。
    readonly property var activeDocument: {
        const documents = projectModel.openDocuments;
        for (let i = 0; i < documents.length; ++i) {
            if (documents[i].id === projectModel.activeDocumentId)
                return documents[i];
        }
        return ({});
    }
    readonly property bool canCreateOrOpen: !busy
    readonly property bool canSave: projectOpen && !busy
    readonly property bool canImport: projectOpen && !busy
    readonly property bool canExport: meshActive && !busy
    readonly property bool canScreenshot: !busy

    function showNotice(text: string, isError: bool) {
        notice = text;
        noticeIsError = isError;
    }

    ProjectViewModel {
        id: projectModel
        objectName: "projectModel"
    }

    NewProjectDialog {
        id: newProjectDialog
        ownerWindow: shellWindow
        projectModel: projectModel
    }

    ImportDialog {
        id: importDialog
        ownerWindow: shellWindow
        projectModel: projectModel
    }

    FileDialog {
        id: openProjectFileDialog
        objectName: "openProjectFileDialog"
        title: "打开工程"
        currentFolder: projectModel.defaultLocationUrl
        fileMode: FileDialog.OpenFile
        nameFilters: ["Panta 工程文件 (*.panta)"]
        onAccepted: projectModel.openProjectUrl(selectedFile)
    }

    Connections {
        target: projectModel
        function onExportFinished(path: string, succeeded: bool) {
            shellWindow.showNotice(succeeded ? "已导出：" + path : "", false);
        }
        function onErrorChanged() {
            if (projectModel.error.length > 0)
                shellWindow.showNotice("", false);
        }
    }

    FileDialog {
        id: exportMeshFileDialog
        objectName: "exportMeshFileDialog"
        title: "导出网格"
        currentFolder: projectModel.defaultLocationUrl
        fileMode: FileDialog.SaveFile
        defaultSuffix: "stl"
        nameFilters: ["STL 网格 (*.stl)", "VTK XML 非结构网格 (*.vtu)", "VTK XML 多边形数据 (*.vtp)", "VTK 旧版格式 (*.vtk)", "Wavefront OBJ (*.obj)", "PLY 网格 (*.ply)"]
        onAccepted: {
            shellWindow.showNotice("", false);
            projectModel.beginExportActiveMeshUrl(selectedFile);
        }
    }

    FileDialog {
        id: screenshotFileDialog
        objectName: "screenshotFileDialog"
        title: "保存视口截图"
        currentFolder: projectModel.defaultLocationUrl
        fileMode: FileDialog.SaveFile
        defaultSuffix: "png"
        nameFilters: ["PNG 图像 (*.png)"]
        onAccepted: {
            const failure = viewportPane.saveScreenshot(selectedFile);
            shellWindow.showNotice(failure.length > 0 ? failure : "已保存截图：" + projectModel.localPath(selectedFile), failure.length > 0);
        }
    }

    // 全局快捷键；启用条件与工具栏按钮一致，忙碌时不触发。
    Shortcut {
        sequences: [StandardKey.New]
        enabled: shellWindow.canCreateOrOpen
        onActivated: newProjectDialog.open()
    }
    Shortcut {
        sequences: [StandardKey.Open]
        enabled: shellWindow.canCreateOrOpen
        onActivated: openProjectFileDialog.open()
    }
    Shortcut {
        sequences: [StandardKey.Save]
        enabled: shellWindow.canSave
        onActivated: projectModel.saveProject()
    }
    Shortcut {
        sequence: "Ctrl+I"
        enabled: shellWindow.canImport
        onActivated: importDialog.open()
    }
    Shortcut {
        sequence: "Ctrl+E"
        enabled: shellWindow.canExport
        onActivated: exportMeshFileDialog.open()
    }
    Shortcut {
        sequence: "Ctrl+Shift+P"
        enabled: shellWindow.canScreenshot
        onActivated: screenshotFileDialog.open()
    }

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            anchors.margins: Theme.spacingSmall
            spacing: Theme.spacingSmall

            Button {
                objectName: "newProjectButton"
                text: "新建工程"
                enabled: shellWindow.canCreateOrOpen
                ToolTip.visible: hovered
                ToolTip.delay: 500
                ToolTip.text: "新建工程（Ctrl+N）"
                onClicked: newProjectDialog.open()
            }
            Button {
                objectName: "openProjectButton"
                text: "打开工程"
                enabled: shellWindow.canCreateOrOpen
                ToolTip.visible: hovered
                ToolTip.delay: 500
                ToolTip.text: "打开工程（Ctrl+O）"
                onClicked: openProjectFileDialog.open()
            }
            Button {
                objectName: "saveProjectButton"
                text: "保存工程"
                enabled: shellWindow.canSave
                ToolTip.visible: hovered
                ToolTip.delay: 500
                ToolTip.text: "保存工程（Ctrl+S）"
                onClicked: projectModel.saveProject()
            }
            Button {
                objectName: "importStlButton"
                text: "导入网格"
                enabled: shellWindow.canImport
                ToolTip.visible: hovered
                ToolTip.delay: 500
                ToolTip.text: "导入网格（Ctrl+I）"
                onClicked: importDialog.open()
            }
            Button {
                objectName: "exportMeshButton"
                text: projectModel.exportPending ? "正在导出…" : "导出网格"
                enabled: shellWindow.canExport
                ToolTip.visible: hovered
                ToolTip.delay: 500
                ToolTip.text: "导出活动网格（Ctrl+E）"
                onClicked: exportMeshFileDialog.open()
            }
            Button {
                objectName: "screenshotButton"
                text: "视口截图"
                enabled: shellWindow.canScreenshot
                ToolTip.visible: hovered
                ToolTip.delay: 500
                ToolTip.text: "保存视口 PNG 截图（Ctrl+Shift+P）"
                onClicked: screenshotFileDialog.open()
            }
            Label {
                objectName: "shellCaption"
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignRight
                elide: Text.ElideMiddle
                text: shellWindow.projectOpen ? projectModel.currentName : "可视化渲染测试"
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        ColumnLayout {
            objectName: "assetSidebar"
            // 侧栏固定宽度，剩余空间交给原生视口；不使用布局容器默认的横向扩展策略。
            Layout.fillWidth: false
            Layout.minimumWidth: 240
            Layout.maximumWidth: 240
            Layout.preferredWidth: 240
            Layout.fillHeight: true
            Layout.margins: Theme.spacingMedium
            spacing: Theme.spacingMedium

            Label {
                text: "模型资产"
                font.bold: true
            }
            Label {
                Layout.fillWidth: true
                visible: assetList.count === 0
                text: shellWindow.projectOpen ? "导入网格以查看。" : "新建或打开工程。"
                wrapMode: Text.Wrap
            }
            ListView {
                id: assetList
                objectName: "assetList"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: projectModel.importedPartIds
                delegate: ItemDelegate {
                    required property int index
                    required property string modelData
                    objectName: "importedPartEntry"
                    width: ListView.view.width
                    text: projectModel.importedPartNames[index] ?? modelData
                    highlighted: projectModel.activeDocumentId === modelData
                    onClicked: projectModel.openImportRecord(modelData)
                }
                ScrollBar.vertical: ScrollBar {}
            }
            // 活动网格状态：加载中 / 失败时显示稳定诊断码对应的中文摘要与原始详情。
            Label {
                objectName: "activeDocumentStatus"
                Layout.fillWidth: true
                visible: shellWindow.meshActive && (shellWindow.activeDocument.message ?? "").length > 0
                text: shellWindow.activeDocument.message ?? ""
                color: (shellWindow.activeDocument.diagnostic?.code ?? "").length > 0 ? Theme.colorError : Theme.colorTextMuted
                wrapMode: Text.Wrap
            }
            Label {
                objectName: "activeDocumentDiagnostic"
                Layout.fillWidth: true
                visible: text.length > 0
                text: {
                    const diagnostic = shellWindow.activeDocument.diagnostic ?? ({});
                    const code = diagnostic.code ?? "";
                    return code.length > 0 ? code + (diagnostic.detail ? "：" + diagnostic.detail : "") : "";
                }
                color: Theme.colorTextMuted
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.WrapAnywhere
            }
        }
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: Theme.borderWidth
            color: Theme.colorPanelLine
        }
        ViewportPane {
            id: viewportPane
            objectName: "viewportPane"
            Layout.minimumWidth: 0
            Layout.preferredWidth: 640
            Layout.fillWidth: true
            Layout.fillHeight: true
            meshSource: projectModel
            reducedMotion: Settings.reducedMotion
        }
    }

    footer: RowLayout {
        spacing: Theme.spacingSmall
        BusyIndicator {
            objectName: "busyIndicator"
            Layout.leftMargin: Theme.spacingSmall
            Layout.preferredWidth: 20
            Layout.preferredHeight: 20
            running: shellWindow.busy
            visible: running
        }
        Label {
            objectName: "statusMessage"
            Layout.fillWidth: true
            padding: Theme.spacingSmall
            text: projectModel.error.length > 0 ? projectModel.error : shellWindow.notice.length > 0 ? shellWindow.notice : shellWindow.busy ? "处理中…" : "就绪"
            color: projectModel.error.length > 0 || shellWindow.noticeIsError ? Theme.colorError : Theme.colorTextMuted
            elide: Text.ElideRight
        }
    }
}
