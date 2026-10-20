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
    title: projectOpen ? projectModel.currentName + " · 可视化渲染测试" : "可视化渲染测试"
    visible: true
    color: Theme.colorPanel

    readonly property bool projectOpen: projectModel.currentPath.length > 0
    readonly property bool busy: projectModel.projectOperationPending || projectModel.importPending

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

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            anchors.margins: Theme.spacingSmall
            spacing: Theme.spacingSmall

            Button {
                objectName: "newProjectButton"
                text: "新建工程"
                enabled: !shellWindow.busy
                onClicked: newProjectDialog.open()
            }
            Button {
                objectName: "openProjectButton"
                text: "打开工程"
                enabled: !shellWindow.busy
                onClicked: openProjectFileDialog.open()
            }
            Button {
                objectName: "saveProjectButton"
                text: "保存工程"
                enabled: shellWindow.projectOpen && !shellWindow.busy
                onClicked: projectModel.saveProject()
            }
            Button {
                objectName: "importStlButton"
                text: "导入 STL"
                enabled: shellWindow.projectOpen && !shellWindow.busy
                onClicked: importDialog.open()
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
                text: shellWindow.projectOpen ? "导入 STL 网格以查看。" : "新建或打开工程。"
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
        }
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: Theme.borderWidth
            color: Theme.colorPanelLine
        }
        ViewportPane {
            objectName: "viewportPane"
            Layout.minimumWidth: 0
            Layout.preferredWidth: 640
            Layout.fillWidth: true
            Layout.fillHeight: true
            meshSource: projectModel
            reducedMotion: Settings.reducedMotion
        }
    }

    footer: Label {
        objectName: "statusMessage"
        padding: Theme.spacingSmall
        text: projectModel.error.length > 0 ? projectModel.error : shellWindow.busy ? "处理中…" : "就绪"
        color: projectModel.error.length > 0 ? Theme.colorError : Theme.colorTextMuted
        elide: Text.ElideRight
    }
}
