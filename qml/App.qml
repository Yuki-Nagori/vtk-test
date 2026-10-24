// 最小渲染工作台：工程负责资产与文档，VTK 负责原生视口。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Panta.Bridge

ApplicationWindow {
    id: shellWindow

    // 拾取与两点测距（task 017）：保留最近两次命中点，单位与工程一致（mm）。
    property var pickedPoints: []
    property bool lastPickMissed: false
    function recordPick(hit: bool, x: real, y: real, z: real, triangle: int) {
        if (!hit) {
            lastPickMissed = true;
            return;
        }
        lastPickMissed = false;
        const points = pickedPoints.length >= 2 ? [] : pickedPoints.slice();
        points.push({
            x: x,
            y: y,
            z: z,
            triangle: triangle
        });
        pickedPoints = points;
    }
    function formatPoint(p: var): string {
        return "(" + p.x.toFixed(3) + ", " + p.y.toFixed(3) + ", " + p.z.toFixed(3) + ")";
    }
    function formatPick(): string {
        if (pickedPoints.length === 0)
            return lastPickMissed ? "未命中模型" : "左键拾取点，连续两点测距";
        const a = pickedPoints[0];
        let text = "点 A " + formatPoint(a) + " 面 #" + a.triangle;
        if (pickedPoints.length === 2) {
            const b = pickedPoints[1];
            const d = Math.sqrt((b.x - a.x) ** 2 + (b.y - a.y) ** 2 + (b.z - a.z) ** 2);
            text += "；点 B " + formatPoint(b) + " 面 #" + b.triangle + "；距离 " + d.toFixed(3) + " mm";
        }
        return text + (lastPickMissed ? "（最近一次未命中）" : "");
    }
    objectName: "workbench"
    minimumWidth: Theme.windowMinimumWidth
    minimumHeight: Theme.windowMinimumHeight
    width: 1280
    height: 800
    title: projectOpen ? (projectModel.dirty ? "* " : "") + projectModel.currentName + " · 可视化渲染测试" : "可视化渲染测试"
    visible: true
    color: Theme.colorPanel

    readonly property bool projectOpen: projectModel.currentPath.length > 0
    readonly property real taskProgress: projectModel.generationPending ? projectModel.generationProgress : projectModel.importPending ? projectModel.importProgress : projectModel.exportPending ? projectModel.exportProgress : projectModel.diagnosticsPending ? projectModel.diagnosticsProgress : -1
    readonly property string taskName: projectModel.generationPending ? "生成体网格" : projectModel.importPending ? "导入" : projectModel.exportPending ? "导出" : "诊断"
    readonly property bool busy: projectModel.projectOperationPending || projectModel.generationPending || projectModel.importPending || projectModel.exportPending || projectModel.diagnosticsPending
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
    readonly property bool canExport: meshActive && !busy && !projectModel.animationActive && !projectModel.sectionActive
    readonly property bool canScreenshot: !busy
    readonly property bool canDiagnose: meshActive && !busy && !projectModel.animationActive && !projectModel.sectionActive && projectModel.activeMeshLodStatus.length === 0
    // 仅当诊断报告属于活动页签时展示，切换页签后需重新诊断。
    readonly property var activeQuality: projectModel.meshQuality.documentId === projectModel.activeDocumentId ? projectModel.meshQuality : ({})
    readonly property bool canToggleSection: meshActive && projectModel.volumeAvailable && !projectModel.animationActive
    // 底部面板「任务」页：所有进行中的后台任务（状态栏只显示优先级最高的一项）。
    readonly property var pendingTasks: {
        const tasks = [];
        if (projectModel.generationPending)
            tasks.push({
                "kind": "generation",
                "name": "生成体网格",
                "progress": projectModel.generationProgress
            });
        if (projectModel.importPending)
            tasks.push({
                "kind": "import",
                "name": "导入",
                "progress": projectModel.importProgress
            });
        if (projectModel.exportPending)
            tasks.push({
                "kind": "export",
                "name": "导出",
                "progress": projectModel.exportProgress
            });
        if (projectModel.diagnosticsPending)
            tasks.push({
                "kind": "diagnostics",
                "name": "诊断",
                "progress": projectModel.diagnosticsProgress
            });
        return tasks;
    }
    // task 035：错误已在打开的对话框字段旁显示时，状态栏不重复。
    readonly property bool errorShownInDialog: newProjectDialog.visible || importDialog.visible || generateVolumeDialog.visible || largeMeshSettingsDialog.visible
    readonly property bool showError: projectModel.error.length > 0 && !errorShownInDialog

    // 关闭确认（task 018）：有未保存修改或后台任务时先询问。
    property bool closeConfirmed: false
    property bool quitAfterSave: false
    onClosing: close => {
        if (closeConfirmed || (!projectModel.dirty && !busy))
            return;
        close.accepted = false;
        closeConfirmDialog.open();
    }
    function confirmClose() {
        projectModel.cancelGeneration();
        projectModel.cancelImport();
        projectModel.cancelExport();
        projectModel.cancelDiagnostics();
        closeConfirmed = true;
        shellWindow.close();
    }

    // 拖放：.panta 打开工程，网格文件进入导入对话框；忙碌或无工程时给出提示。
    function handleDrop(urls: list<url>) {
        if (urls.length === 0)
            return;
        const kind = projectModel.classifyDroppedUrl(urls[0]);
        if (kind === "project") {
            if (!canCreateOrOpen)
                showNotice("后台任务进行中，稍后再打开工程。", true);
            else
                projectModel.openProjectUrl(urls[0]);
        } else if (kind === "mesh") {
            if (!canImport)
                showNotice(projectOpen ? "后台任务进行中，稍后再导入。" : "请先新建或打开工程，再拖入网格。", true);
            else
                importDialog.openWithFile(projectModel.localPath(urls[0]));
        } else {
            showNotice("不支持的文件类型：仅支持 .panta 工程与 STL/VTU/VTP/VTK/PVD/OBJ/PLY 网格、OpenFOAM 算例（.foam / controlDict）及 CGNS（.cgns）。", true);
        }
        if (urls.length > 1)
            showNotice("一次只处理一个文件，已使用第一个。", false);
    }

    function showNotice(text: string, isError: bool) {
        notice = text;
        noticeIsError = isError;
        bottomPanel.appendMessage(text, isError);
    }
    function setSectionActive(active: bool) {
        projectModel.setSection(active, projectModel.sectionNormalX, projectModel.sectionNormalY, projectModel.sectionNormalZ, projectModel.sectionFraction);
    }
    function cancelTask(kind: string) {
        if (kind === "generation")
            projectModel.cancelGeneration();
        else if (kind === "import")
            projectModel.cancelImport();
        else if (kind === "export")
            projectModel.cancelExport();
        else
            projectModel.cancelDiagnostics();
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

    GenerateVolumeDialog {
        id: generateVolumeDialog
        ownerWindow: shellWindow
        projectModel: projectModel
    }

    LargeMeshSettingsDialog {
        id: largeMeshSettingsDialog
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

    FileDialog {
        id: openAnimationFileDialog
        objectName: "openAnimationFileDialog"
        title: "打开时序序列（选择任一帧）"
        fileMode: FileDialog.OpenFile
        nameFilters: ["网格帧 (*.stl *.vtu *.vtp *.vtk *.obj *.ply)"]
        onAccepted: projectModel.openAnimationUrl(selectedFile)
    }

    MessageDialog {
        id: closeConfirmDialog
        objectName: "closeConfirmDialog"
        title: "退出确认"
        text: projectModel.dirty ? "工程有未保存的修改。" : "后台任务仍在进行。"
        informativeText: shellWindow.busy ? "退出将取消正在进行的导入/导出/诊断。" : "是否在退出前保存？"
        buttons: projectModel.dirty && !shellWindow.busy ? (MessageDialog.Save | MessageDialog.Discard | MessageDialog.Cancel) : (MessageDialog.Discard | MessageDialog.Cancel)
        onButtonClicked: (button, role) => {
            if (button === MessageDialog.Save) {
                shellWindow.quitAfterSave = projectModel.saveProject();
            } else if (button === MessageDialog.Discard) {
                shellWindow.confirmClose();
            }
        }
    }

    Connections {
        target: projectModel
        function onProjectOperationPendingChanged() {
            if (!shellWindow.quitAfterSave || projectModel.projectOperationPending)
                return;
            shellWindow.quitAfterSave = false;
            if (!projectModel.dirty)
                shellWindow.confirmClose();
        }
        function onExportFinished(path: string, succeeded: bool) {
            shellWindow.showNotice(succeeded ? "已导出：" + path : "", false);
        }
        function onErrorChanged() {
            if (projectModel.error.length === 0)
                return;
            shellWindow.showNotice("", false);
            bottomPanel.appendMessage(projectModel.error, true);
            // 对话框内已显示的错误不再额外弹出底部面板（task 035 同口径）。
            if (!shellWindow.errorShownInDialog)
                bottomPanel.reveal(1);
        }
        // 诊断出结果时自动展开一次「诊断」页；用户再次折叠后保持折叠，直到下一次结果。
        function onMeshQualityChanged() {
            if (!projectModel.diagnosticsPending && shellWindow.activeQuality.documentId !== undefined)
                bottomPanel.reveal(0);
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
        sequence: "Ctrl+D"
        enabled: shellWindow.canDiagnose
        onActivated: projectModel.beginDiagnoseActiveMesh()
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

    // 菜单栏（task 018）：Menu 使用独立窗口弹出，避免被原生视口表面遮挡。
    menuBar: MenuBar {
        Menu {
            title: "文件(&F)"
            popupType: Popup.Window
            Action {
                text: "新建工程…"
                enabled: shellWindow.canCreateOrOpen
                onTriggered: newProjectDialog.open()
            }
            Action {
                text: "打开工程…"
                enabled: shellWindow.canCreateOrOpen
                onTriggered: openProjectFileDialog.open()
            }
            Menu {
                id: recentMenuItems
                objectName: "recentProjectsMenu"
                title: "最近工程"
                popupType: Popup.Window
                enabled: shellWindow.canCreateOrOpen
                Instantiator {
                    model: projectModel.recentProjects
                    delegate: MenuItem {
                        required property string modelData
                        text: modelData
                        onTriggered: projectModel.openProject(modelData)
                    }
                    onObjectAdded: (index, object) => recentMenuItems.insertItem(index, object)
                    onObjectRemoved: (index, object) => recentMenuItems.removeItem(object)
                }
                MenuSeparator {}
                Action {
                    text: projectModel.recentProjects.length > 0 ? "清空列表" : "（无）"
                    enabled: projectModel.recentProjects.length > 0
                    onTriggered: projectModel.clearRecentProjects()
                }
            }
            Action {
                text: "保存工程"
                enabled: shellWindow.canSave
                onTriggered: projectModel.saveProject()
            }
            MenuSeparator {}
            Action {
                text: "导入网格…"
                enabled: shellWindow.canImport
                onTriggered: importDialog.open()
            }
            Action {
                text: "从 STEP 生成体网格…"
                enabled: shellWindow.canImport
                onTriggered: generateVolumeDialog.open()
            }
            Action {
                text: "导出网格…"
                enabled: shellWindow.canExport
                onTriggered: exportMeshFileDialog.open()
            }
            Action {
                text: "视口截图…"
                enabled: shellWindow.canScreenshot
                onTriggered: screenshotFileDialog.open()
            }
            MenuSeparator {}
            Action {
                text: "退出"
                onTriggered: shellWindow.close()
            }
        }
        Menu {
            title: "视图(&V)"
            popupType: Popup.Window
            Repeater {
                model: shellWindow.viewPresets
                delegate: MenuItem {
                    required property var modelData
                    text: modelData[1]
                    enabled: shellWindow.meshActive
                    onTriggered: viewportPane.setViewPreset(modelData[0])
                }
            }
            MenuItem {
                text: "复位视图"
                enabled: shellWindow.meshActive
                onTriggered: viewportPane.resetView()
            }
            MenuSeparator {}
            MenuItem {
                objectName: "sidebarMenuItem"
                text: "左栏"
                checkable: true
                checked: !Settings.sidebarCollapsed
                onTriggered: Settings.sidebarCollapsed = !checked
            }
            MenuItem {
                objectName: "bottomPanelMenuItem"
                text: "底部面板"
                checkable: true
                checked: !Settings.bottomPanelCollapsed
                onTriggered: Settings.bottomPanelCollapsed = !checked
            }
        }
        Menu {
            title: "网格(&M)"
            popupType: Popup.Window
            Action {
                text: "质量诊断"
                enabled: shellWindow.canDiagnose
                onTriggered: projectModel.beginDiagnoseActiveMesh()
            }
            MenuSeparator {}
            Action {
                text: "大网格设置…"
                onTriggered: largeMeshSettingsDialog.open()
            }
        }
    }

    readonly property var viewPresets: [["front", "前视图"], ["back", "后视图"], ["left", "左视图"], ["right", "右视图"], ["top", "顶视图"], ["bottom", "底视图"], ["iso", "轴测图"]]

    header: WorkbenchToolBar {
        objectName: "mainToolBar"
        canCreateOrOpen: shellWindow.canCreateOrOpen
        canSave: shellWindow.canSave
        canImport: shellWindow.canImport
        canExport: shellWindow.canExport
        exportPending: projectModel.exportPending
        canScreenshot: shellWindow.canScreenshot
        canUseView: shellWindow.meshActive
        canToggleSection: shellWindow.canToggleSection
        sectionActive: projectModel.sectionActive
        viewPresets: shellWindow.viewPresets
        onNewProjectRequested: newProjectDialog.open()
        onOpenProjectRequested: openProjectFileDialog.open()
        onSaveProjectRequested: projectModel.saveProject()
        onImportRequested: importDialog.open()
        onExportRequested: exportMeshFileDialog.open()
        onViewPresetRequested: name => viewportPane.setViewPreset(name)
        onResetViewRequested: viewportPane.resetView()
        onSectionToggled: active => shellWindow.setSectionActive(active)
        onScreenshotRequested: screenshotFileDialog.open()
    }

    DropArea {
        objectName: "fileDropArea"
        anchors.fill: parent
        keys: ["text/uri-list"]
        onDropped: drop => {
            if (!drop.hasUrls)
                return;
            drop.acceptProposedAction();
            shellWindow.handleDrop(drop.urls);
        }
    }

    // 左栏与中心区横向分割；左栏宽度与折叠状态写入用户设置（task 036，034 Q4）。
    SplitView {
        id: mainSplit
        objectName: "mainSplit"
        anchors.fill: parent
        orientation: Qt.Horizontal
        handle: Rectangle {
            implicitWidth: Theme.spacingXSmall
            color: SplitHandle.pressed || SplitHandle.hovered ? Theme.colorFocus : Theme.colorChrome
        }
        onResizingChanged: {
            if (!resizing && !Settings.sidebarCollapsed)
                Settings.sidebarWidth = Math.round(workbenchSidebar.width);
        }

        WorkbenchSidebar {
            id: workbenchSidebar
            objectName: "assetSidebar"
            visible: !Settings.sidebarCollapsed
            SplitView.preferredWidth: Settings.sidebarWidth
            SplitView.minimumWidth: Settings.sidebarMinimumWidth
            SplitView.maximumWidth: Settings.sidebarMaximumWidth
            projectModel: projectModel
            projectOpen: shellWindow.projectOpen
            meshActive: shellWindow.meshActive
            activeDocument: shellWindow.activeDocument
            onOpenAnimationRequested: openAnimationFileDialog.open()
            onCollapseRequested: Settings.sidebarCollapsed = true
        }
        // 左栏折叠后只留一条窄轨，放展开按钮；宽度固定，不可拖动。
        PanelSurface {
            objectName: "sidebarRail"
            visible: Settings.sidebarCollapsed
            implicitWidth: sidebarExpandButton.implicitWidth + Theme.spacingXSmall * 2
            SplitView.minimumWidth: implicitWidth
            SplitView.maximumWidth: implicitWidth
            ThemedToolButton {
                id: sidebarExpandButton
                objectName: "sidebarExpandButton"
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.top: parent.top
                anchors.topMargin: Theme.spacingXSmall
                iconName: "panel-left"
                accessibleName: "展开左栏"
                onClicked: Settings.sidebarCollapsed = false
            }
        }
        // 视口列与底部面板纵向分割；面板高度与折叠状态写入用户设置（task 036）。
        SplitView {
            id: centerSplit
            objectName: "centerSplit"
            SplitView.fillWidth: true
            SplitView.minimumWidth: Theme.viewportMinimumWidth
            orientation: Qt.Vertical
            handle: Rectangle {
                implicitHeight: Theme.spacingXSmall
                color: SplitHandle.pressed || SplitHandle.hovered ? Theme.colorFocus : Theme.colorChrome
            }
            // 折叠时面板最小=最大高度，分隔条拖不动，只保留标题行。
            onResizingChanged: {
                if (!resizing && !Settings.bottomPanelCollapsed)
                    Settings.bottomPanelHeight = Math.round(bottomPanel.height);
            }

            ColumnLayout {
                SplitView.fillHeight: true
                SplitView.minimumHeight: Theme.viewportMinimumHeight
                spacing: 0
                ViewportPane {
                    id: viewportPane
                    objectName: "viewportPane"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    meshSource: projectModel
                    reducedMotion: Settings.reducedMotion
                    onPointPicked: (hit, x, y, z, triangle) => shellWindow.recordPick(hit, x, y, z, triangle)
                }
                PlaybackBar {
                    objectName: "playbackBar"
                    Layout.fillWidth: true
                    model: projectModel
                }
            }
            BottomPanel {
                id: bottomPanel
                objectName: "bottomPanel"
                readonly property int expandedHeight: Settings.bottomPanelHeight
                SplitView.preferredHeight: collapsed ? headerHeight : expandedHeight
                SplitView.minimumHeight: collapsed ? headerHeight : Settings.bottomPanelMinimumHeight
                SplitView.maximumHeight: collapsed ? headerHeight : Settings.bottomPanelMaximumHeight
                projectModel: projectModel
                collapsed: Settings.bottomPanelCollapsed
                meshActive: shellWindow.meshActive
                canDiagnose: shellWindow.canDiagnose
                quality: shellWindow.activeQuality
                tasks: shellWindow.pendingTasks
                onCollapseToggled: value => Settings.bottomPanelCollapsed = value
                onDiagnoseRequested: projectModel.beginDiagnoseActiveMesh()
                onCancelTaskRequested: kind => shellWindow.cancelTask(kind)
            }
        }
    }

    footer: WorkbenchStatusBar {
        objectName: "statusBar"
        busy: shellWindow.busy
        message: shellWindow.showError ? projectModel.error : shellWindow.notice.length > 0 ? shellWindow.notice : shellWindow.busy ? "处理中…" : "就绪"
        messageIsError: shellWindow.showError || (shellWindow.notice.length > 0 && shellWindow.noticeIsError)
        meshActive: shellWindow.meshActive
        pickText: shellWindow.formatPick()
        lodStatus: projectModel.activeMeshLodStatus
        timeStep: projectModel.playbackActive ? "时间步 " + (projectModel.playbackFrame + 1) + "/" + projectModel.playbackFrameCount : ""
        timeStepDetail: projectModel.playbackActive ? projectModel.playbackStatus : ""
        taskName: shellWindow.taskName
        taskProgress: shellWindow.taskProgress
        onCancelTaskRequested: {
            if (projectModel.generationPending)
                shellWindow.cancelTask("generation");
            else if (projectModel.importPending)
                shellWindow.cancelTask("import");
            else if (projectModel.exportPending)
                shellWindow.cancelTask("export");
            else
                shellWindow.cancelTask("diagnostics");
        }
    }
}
