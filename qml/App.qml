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
            return lastPickMissed ? "未命中模型。" : "左键单击模型拾取点；连续两点显示距离。";
        const a = pickedPoints[0];
        let text = "点 A " + formatPoint(a) + " 面 #" + a.triangle;
        if (pickedPoints.length === 2) {
            const b = pickedPoints[1];
            const d = Math.sqrt((b.x - a.x) ** 2 + (b.y - a.y) ** 2 + (b.z - a.z) ** 2);
            text += "\n点 B " + formatPoint(b) + " 面 #" + b.triangle + "\n距离 " + d.toFixed(3) + " mm";
        }
        return text + (lastPickMissed ? "\n（最近一次未命中）" : "");
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
            showNotice("不支持的文件类型：仅支持 .panta 工程与 STL/VTU/VTP/VTK/OBJ/PLY 网格。", true);
        }
        if (urls.length > 1)
            showNotice("一次只处理一个文件，已使用第一个。", false);
    }

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
                model: [["front", "前视图"], ["back", "后视图"], ["left", "左视图"], ["right", "右视图"], ["top", "顶视图"], ["bottom", "底视图"], ["iso", "轴测图"]]
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
            // 时序动画（task 020）：只读序列播放，帧未就绪时停在上一帧等待。
            RowLayout {
                Layout.fillWidth: true
                Label {
                    Layout.fillWidth: true
                    text: "时序动画"
                    font.bold: true
                }
                Button {
                    objectName: "openAnimationButton"
                    text: projectModel.animationActive ? "关闭序列" : "打开序列…"
                    onClicked: projectModel.animationActive ? projectModel.closeAnimation() : openAnimationFileDialog.open()
                    ToolTip.visible: hovered
                    ToolTip.text: "选择编号序列中的任一帧文件（如 flow_0001.stl），自动发现同前缀的全部帧"
                }
            }
            ColumnLayout {
                objectName: "animationPanel"
                Layout.fillWidth: true
                visible: projectModel.animationActive
                RowLayout {
                    Layout.fillWidth: true
                    Button {
                        text: "◀"
                        onClicked: projectModel.stepAnimation(-1)
                    }
                    Button {
                        objectName: "animationPlayButton"
                        text: projectModel.animationPlaying ? "暂停" : "播放"
                        onClicked: projectModel.setAnimationPlaying(!projectModel.animationPlaying)
                    }
                    Button {
                        text: "▶"
                        onClicked: projectModel.stepAnimation(1)
                    }
                    SpinBox {
                        objectName: "animationFpsSpin"
                        from: 1
                        to: 60
                        value: projectModel.animationFps
                        onValueModified: projectModel.setAnimationFps(value)
                        ToolTip.visible: hovered
                        ToolTip.text: "帧率 (fps)"
                    }
                }
                Slider {
                    objectName: "animationFrameSlider"
                    Layout.fillWidth: true
                    from: 0
                    to: Math.max(0, projectModel.animationFrameCount - 1)
                    stepSize: 1
                    value: Math.max(0, projectModel.animationFrame)
                    onMoved: projectModel.setAnimationFrame(Math.round(value))
                }
                Label {
                    objectName: "animationStatus"
                    Layout.fillWidth: true
                    text: projectModel.animationStatus
                    color: Theme.colorTextMuted
                    wrapMode: Text.WrapAnywhere
                }
            }
            // 体网格（task 021）：质量统计与单切面显示；切面只替换视口，不写工程。
            ColumnLayout {
                objectName: "volumePanel"
                Layout.fillWidth: true
                visible: shellWindow.meshActive && projectModel.volumeAvailable
                Label {
                    text: "体网格"
                    font.bold: true
                }
                Label {
                    objectName: "volumeInfo"
                    Layout.fillWidth: true
                    text: projectModel.volumeInfo
                    color: Theme.colorTextMuted
                    wrapMode: Text.WrapAnywhere
                }
                // 任意平面切面（task 024）：X/Y/Z 预设只是把法向设为坐标轴；法向分量可编辑（−1..1，步长 0.05）。
                RowLayout {
                    Layout.fillWidth: true
                    CheckBox {
                        objectName: "sectionToggle"
                        text: "切面"
                        enabled: !projectModel.animationActive
                        checked: projectModel.sectionActive
                        onToggled: projectModel.setSection(checked, projectModel.sectionNormalX, projectModel.sectionNormalY, projectModel.sectionNormalZ, sectionSlider.value)
                    }
                    Repeater {
                        model: [
                            {
                                "label": "X",
                                "normal": [1, 0, 0]
                            },
                            {
                                "label": "Y",
                                "normal": [0, 1, 0]
                            },
                            {
                                "label": "Z",
                                "normal": [0, 0, 1]
                            }
                        ]
                        delegate: Button {
                            required property var modelData
                            objectName: "sectionPreset" + modelData.label
                            text: modelData.label
                            flat: true
                            checkable: false
                            highlighted: projectModel.sectionNormalX === modelData.normal[0] && projectModel.sectionNormalY === modelData.normal[1] && projectModel.sectionNormalZ === modelData.normal[2]
                            onClicked: projectModel.setSection(projectModel.sectionActive, modelData.normal[0], modelData.normal[1], modelData.normal[2], sectionSlider.value)
                        }
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    Label {
                        text: "法向"
                        color: Theme.colorTextMuted
                    }
                    Repeater {
                        model: ["X", "Y", "Z"]
                        delegate: SpinBox {
                            id: normalSpin
                            required property int index
                            required property string modelData
                            readonly property real valueScale: 100
                            readonly property real component: index === 0 ? projectModel.sectionNormalX : index === 1 ? projectModel.sectionNormalY : projectModel.sectionNormalZ
                            objectName: "sectionNormal" + modelData
                            Layout.fillWidth: true
                            editable: true
                            from: -100
                            to: 100
                            stepSize: 5
                            value: Math.round(component * valueScale)
                            Accessible.name: "切面法向 " + modelData
                            textFromValue: (value, locale) => Number(value / valueScale).toLocaleString(locale, "f", 2)
                            valueFromText: (text, locale) => Math.round(Number.fromLocaleString(locale, text) * valueScale)
                            validator: DoubleValidator {
                                bottom: -1
                                top: 1
                                decimals: 2
                            }
                            onValueModified: {
                                const normal = [projectModel.sectionNormalX, projectModel.sectionNormalY, projectModel.sectionNormalZ];
                                normal[index] = value / valueScale;
                                projectModel.setSection(projectModel.sectionActive, normal[0], normal[1], normal[2], sectionSlider.value);
                                // 用户编辑会打断绑定；重新绑定到模型值，零法向被拒绝时即回显原有效值。
                                value = Qt.binding(() => Math.round(normalSpin.component * normalSpin.valueScale));
                            }
                        }
                    }
                }
                Slider {
                    id: sectionSlider
                    objectName: "sectionSlider"
                    Layout.fillWidth: true
                    enabled: projectModel.sectionActive
                    from: 0
                    to: 1
                    value: projectModel.sectionFraction
                    onMoved: projectModel.setSection(true, projectModel.sectionNormalX, projectModel.sectionNormalY, projectModel.sectionNormalZ, value)
                }
                // 结果场上色（task 029）：只列出可切面的 1/3 分量字段；3 分量字段可选模或 X/Y/Z 分量。
                RowLayout {
                    id: sectionFieldRow
                    readonly property var fieldOptions: [
                        {
                            "label": "无",
                            "name": "",
                            "association": "",
                            "components": 1
                        }
                    ].concat(projectModel.resultFields.filter(field => field.sectionable).map(field => ({
                                "label": field.name + (field.association === "cell" ? "（单元）" : "（节点）"),
                                "name": field.name,
                                "association": field.association,
                                "components": field.components
                            })))
                    readonly property int selectedIndex: fieldOptions.findIndex(option => option.name === projectModel.sectionFieldName && option.association === projectModel.sectionFieldAssociation)
                    Layout.fillWidth: true
                    visible: fieldOptions.length > 1
                    Label {
                        text: "字段"
                        color: Theme.colorTextMuted
                    }
                    ComboBox {
                        objectName: "sectionField"
                        Layout.fillWidth: true
                        model: sectionFieldRow.fieldOptions
                        textRole: "label"
                        currentIndex: Math.max(0, sectionFieldRow.selectedIndex)
                        Accessible.name: "切面上色字段"
                        onActivated: index => {
                            const option = sectionFieldRow.fieldOptions[index];
                            projectModel.setSectionField(option.name, option.association, -1);
                        }
                    }
                    ComboBox {
                        objectName: "sectionFieldComponent"
                        readonly property var option: sectionFieldRow.fieldOptions[Math.max(0, sectionFieldRow.selectedIndex)]
                        visible: option.components === 3
                        model: ["模", "X", "Y", "Z"]
                        currentIndex: projectModel.sectionFieldComponent + 1
                        Accessible.name: "切面上色分量"
                        onActivated: index => projectModel.setSectionField(projectModel.sectionFieldName, projectModel.sectionFieldAssociation, index - 1)
                    }
                }
                // 色标：渐变与视口查找表共用同一组 Cool to Warm 控制点；位于视口之外，不遮挡渲染。
                ColumnLayout {
                    objectName: "fieldLegend"
                    Layout.fillWidth: true
                    visible: projectModel.fieldLegendVisible
                    spacing: 2
                    Label {
                        Layout.fillWidth: true
                        text: projectModel.fieldLegendTitle + (projectModel.fieldLegendUnits.length > 0 ? " (" + projectModel.fieldLegendUnits + ")" : "")
                        elide: Text.ElideRight
                    }
                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 12
                        border.color: Theme.colorTextMuted
                        gradient: Gradient {
                            orientation: Gradient.Horizontal
                            GradientStop {
                                position: projectModel.fieldLegendStops[0].position
                                color: projectModel.fieldLegendStops[0].color
                            }
                            GradientStop {
                                position: projectModel.fieldLegendStops[1].position
                                color: projectModel.fieldLegendStops[1].color
                            }
                            GradientStop {
                                position: projectModel.fieldLegendStops[2].position
                                color: projectModel.fieldLegendStops[2].color
                            }
                        }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        Label {
                            text: projectModel.fieldLegendHasRange ? Number(projectModel.fieldLegendMin).toPrecision(4) : "无有限值"
                            color: Theme.colorTextMuted
                        }
                        Item {
                            Layout.fillWidth: true
                        }
                        Label {
                            visible: projectModel.fieldLegendHasRange
                            text: Number(projectModel.fieldLegendMax).toPrecision(4)
                            color: Theme.colorTextMuted
                        }
                    }
                    RowLayout {
                        visible: projectModel.fieldMissingVertices > 0
                        Rectangle {
                            implicitWidth: 12
                            implicitHeight: 12
                            color: projectModel.fieldMissingColor
                        }
                        Label {
                            text: "缺失值顶点 " + projectModel.fieldMissingVertices
                            color: Theme.colorTextMuted
                        }
                    }
                }
            }
            // 大网格 LOD 提示（task 019/025）：显示的是简化网格，拾取为近似值；导出流式读取源资产，表面诊断不可用。
            Label {
                objectName: "activeMeshLodStatus"
                Layout.fillWidth: true
                visible: shellWindow.meshActive && text.length > 0
                text: projectModel.activeMeshLodStatus
                color: Theme.colorTextMuted
                wrapMode: Text.Wrap
            }
            // 网格质量诊断面板：复用 task 006 的表面诊断，后台执行。
            RowLayout {
                Layout.fillWidth: true
                visible: shellWindow.meshActive
                Label {
                    Layout.fillWidth: true
                    text: "网格质量"
                    font.bold: true
                }
                Button {
                    objectName: "diagnoseMeshButton"
                    text: projectModel.diagnosticsPending ? "诊断中…" : "诊断"
                    enabled: shellWindow.canDiagnose
                    onClicked: projectModel.beginDiagnoseActiveMesh()
                    ToolTip.visible: hovered
                    ToolTip.text: "检查重复顶点、退化面、开放边界、非流形边与法向一致性 (Ctrl+D)"
                }
            }
            ColumnLayout {
                objectName: "meshQualityPanel"
                Layout.fillWidth: true
                visible: shellWindow.meshActive && shellWindow.activeQuality.documentId !== undefined
                spacing: 2
                Repeater {
                    model: [["结论", shellWindow.activeQuality.clean ? "无问题" : "存在问题", !shellWindow.activeQuality.clean], ["三角面", shellWindow.activeQuality.triangleCount, false], ["焊接后顶点", shellWindow.activeQuality.weldedVertexCount, false], ["近重复顶点", shellWindow.activeQuality.nearDuplicateVertexCount, shellWindow.activeQuality.nearDuplicateVertexCount > 0], ["退化三角面", shellWindow.activeQuality.degenerateTriangleCount, shellWindow.activeQuality.degenerateTriangleCount > 0], ["开放边界边", shellWindow.activeQuality.boundaryEdgeCount, shellWindow.activeQuality.boundaryEdgeCount > 0], ["孔洞", shellWindow.activeQuality.holeCount, shellWindow.activeQuality.holeCount > 0], ["非流形边", shellWindow.activeQuality.nonManifoldEdgeCount, shellWindow.activeQuality.nonManifoldEdgeCount > 0], ["法向不一致边", shellWindow.activeQuality.inconsistentOrientationEdgeCount, shellWindow.activeQuality.inconsistentOrientationEdgeCount > 0], ["焊接容差", Number(shellWindow.activeQuality.weldTolerance ?? 0).toExponential(2), false]].concat(shellWindow.activeQuality.volumeAvailable ? [["体网格节点", shellWindow.activeQuality.nodeCount, false], ["四面体", shellWindow.activeQuality.tetCount, false], ["最小二面角", Number(shellWindow.activeQuality.minDihedralDegrees ?? 0).toFixed(2) + "°", false], ["最大长宽比", Number(shellWindow.activeQuality.maxAspectRatio ?? 0).toFixed(2), false], ["平均长宽比", Number(shellWindow.activeQuality.meanAspectRatio ?? 0).toFixed(2), false], ["劣质四面体", shellWindow.activeQuality.poorTetCount, shellWindow.activeQuality.poorTetCount > 0]] : [])
                    delegate: Label {
                        required property var modelData
                        Layout.fillWidth: true
                        text: String(modelData[0]) + "：" + String(modelData[1])
                        color: modelData[2] ? Theme.colorError : Theme.colorTextMuted
                        font.pixelSize: Theme.fontSmall
                    }
                }
            }
            // 视图与测量（task 017）：原生视口覆盖 QML，工具放在侧栏而非浮层。
            Label {
                visible: shellWindow.meshActive
                text: "视图与测量"
                font.bold: true
            }
            GridLayout {
                objectName: "viewPresetGrid"
                Layout.fillWidth: true
                visible: shellWindow.meshActive
                columns: 4
                columnSpacing: 2
                rowSpacing: 2
                Repeater {
                    model: [["front", "前"], ["back", "后"], ["left", "左"], ["right", "右"], ["top", "顶"], ["bottom", "底"], ["iso", "轴测"]]
                    delegate: Button {
                        required property var modelData
                        objectName: "viewPreset_" + modelData[0]
                        Layout.fillWidth: true
                        text: modelData[1]
                        onClicked: viewportPane.setViewPreset(modelData[0])
                    }
                }
                Button {
                    objectName: "resetViewButton"
                    Layout.fillWidth: true
                    text: "复位"
                    onClicked: viewportPane.resetView()
                }
            }
            Label {
                objectName: "pickReadout"
                Layout.fillWidth: true
                visible: shellWindow.meshActive
                text: shellWindow.formatPick()
                color: Theme.colorTextMuted
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.Wrap
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
            onPointPicked: (hit, x, y, z, triangle) => shellWindow.recordPick(hit, x, y, z, triangle)
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
        // task 016：当前后台任务的百分比与取消；导入优先，其次导出、诊断。
        Label {
            objectName: "taskProgressLabel"
            visible: shellWindow.taskProgress >= 0
            text: shellWindow.taskName + " " + Math.round(shellWindow.taskProgress * 100) + "%"
            color: Theme.colorTextMuted
        }
        ProgressBar {
            objectName: "taskProgressBar"
            Layout.preferredWidth: 160
            visible: shellWindow.taskProgress >= 0
            from: 0
            to: 1
            value: Math.max(0, shellWindow.taskProgress)
        }
        Button {
            objectName: "taskCancelButton"
            Layout.rightMargin: Theme.spacingSmall
            visible: shellWindow.taskProgress >= 0
            text: "取消"
            onClicked: {
                if (projectModel.generationPending)
                    projectModel.cancelGeneration();
                else if (projectModel.importPending)
                    projectModel.cancelImport();
                else if (projectModel.exportPending)
                    projectModel.cancelExport();
                else
                    projectModel.cancelDiagnostics();
            }
        }
    }
}
