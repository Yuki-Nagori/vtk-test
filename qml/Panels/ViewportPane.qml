// CaeViewport 原生视口与视口文档页签；工程网格由 Rust 服务快照提供。
// 仅进入启用 Bridge 的构建变体，QML 无法覆盖原生视口表面。
import QtQuick
import QtQuick.Layouts
import Panta.Visualization

PanelSurface {
    id: panel

    property var meshSource: null
    property bool reducedMotion: false
    // 空状态引导（task 038）的启用条件与最近工程，由宿主按工具栏同一口径传入。
    property bool projectOpen: false
    property bool canCreateOrOpen: true
    property bool canImport: false
    property var recentProjects: []
    readonly property bool hasDocuments: meshSource !== null && meshSource.openDocuments.length > 0

    // 截图由原生视口读回当前帧；返回中文错误文案，空串表示成功。
    function saveScreenshot(url: url): string {
        return viewport.saveScreenshot(url);
    }
    // 视图工具（task 017）：预设视角与复位转发到原生视口。
    function setViewPreset(name: string): bool {
        return viewport.setViewPreset(name);
    }
    function resetView() {
        viewport.resetView();
    }

    signal pointPicked(bool hit, real x, real y, real z, int triangle)
    signal importRequested
    signal newProjectRequested
    signal openProjectRequested
    signal recentProjectRequested(string path)
    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // 页签在视口上方（task 036）；有文档时视口矩形内不叠放任何 QML 控件，
        // 无文档时原生表面隐藏，同一矩形显示空状态引导（task 038）。
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
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            CaeViewport {
                id: viewport
                objectName: "caeViewport"
                anchors.fill: parent
                // 无文档时隐藏原生表面（VtkViewport 随可见性隐藏 surface），让出区域给引导页。
                visible: panel.hasDocuments
                meshSource: panel.meshSource
                // 对象树可见性眼睛（task 037）：只隐藏显示，不改网格来源。
                meshVisible: panel.meshSource ? panel.meshSource.activeMeshVisible : true
                onPointPicked: (hit, x, y, z, triangle) => panel.pointPicked(hit, x, y, z, triangle)
            }
            ViewportEmptyState {
                objectName: "viewportEmptyState"
                anchors.fill: parent
                visible: !panel.hasDocuments
                projectOpen: panel.projectOpen
                canCreateOrOpen: panel.canCreateOrOpen
                canImport: panel.canImport
                recentProjects: panel.recentProjects
                onImportRequested: panel.importRequested()
                onNewProjectRequested: panel.newProjectRequested()
                onOpenProjectRequested: panel.openProjectRequested()
                onRecentProjectRequested: path => panel.recentProjectRequested(path)
            }
        }
    }
}
