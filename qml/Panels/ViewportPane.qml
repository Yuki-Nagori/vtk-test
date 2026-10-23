// CaeViewport 原生视口与视口文档页签；工程网格由 Rust 服务快照提供。
// 仅进入启用 Bridge 的构建变体，QML 无法覆盖原生视口表面。
import QtQuick
import QtQuick.Layouts
import Panta.Visualization

PanelSurface {
    id: panel

    property var meshSource: null
    property bool reducedMotion: false

    // 截图由原生视口读回当前帧；返回中文错误文案，空串表示成功。
    function saveScreenshot(url: url): string {
        return viewport.saveScreenshot(url);
    }
    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            CaeViewport {
                id: viewport
                objectName: "caeViewport"
                anchors.fill: parent
                meshSource: panel.meshSource
            }
        }

        DocumentTabBar {
            objectName: "documentTabBar"
            Layout.fillWidth: true
            documents: panel.meshSource ? panel.meshSource.openDocuments : []
            activeDocumentId: panel.meshSource ? panel.meshSource.activeDocumentId : ""
            reducedMotion: panel.reducedMotion
            onActivateDocument: documentId => panel.meshSource.activateDocument(documentId)
            onCloseDocument: documentId => panel.meshSource.closeDocument(documentId)
            onMoveDocument: (fromIndex, toIndex) => panel.meshSource.moveDocument(fromIndex, toIndex)
        }
    }
}
