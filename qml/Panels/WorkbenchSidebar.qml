// 工作台左栏（task 036 / 037，034 Q4）：标题行 + 纵向 SplitView（上：对象树，下：上下文属性面板）。
// 树由 Rust 投影（ProjectViewModel.objectTree），本组件只持有会话内的选中项：单击只切换属性面板，
// 双击 / Enter 才激活页签。宽度与折叠状态由宿主写入用户设置（Settings）。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

PanelSurface {
    id: sidebar

    required property var projectModel
    property bool projectOpen: false
    property bool meshActive: false
    property var activeDocument: ({})

    signal openAnimationRequested
    signal collapseRequested
    // 选中诊断节点时请求宿主展开底部面板的诊断页。
    signal qualityRevealRequested

    readonly property var treeModel: projectModel.objectTree
    // 会话内选中的节点 ID；节点消失时回落到活动资产或工程根节点。
    property string selectedNodeId: ""
    property int treeRevision: 0
    readonly property var selectedNode: {
        sidebar.treeRevision;
        const chosen = sidebar.treeModel.node(sidebar.selectedNodeId);
        if (chosen.id !== undefined)
            return chosen;
        const active = sidebar.treeModel.node("asset:" + sidebar.projectModel.activeDocumentId);
        return active.id !== undefined ? active : sidebar.treeModel.node("project");
    }
    readonly property string selectedKind: selectedNode.kind ?? ""
    property int propertiesTab: 0
    // 播放中修改切面 / 着色会与逐帧刷新竞争；暂停后再改（034 Q7）。
    readonly property bool editLocked: projectModel.playbackPlaying
    readonly property string editLockedNote: "播放中不可修改，暂停后可用"

    function selectNode(nodeId: string) {
        sidebar.selectedNodeId = nodeId;
        const index = sidebar.treeModel.indexOfNode(nodeId);
        if (index.valid)
            objectTree.selectionModel.setCurrentIndex(index, ItemSelectionModel.ClearAndSelect);
        if (sidebar.treeModel.node(nodeId).kind === "quality")
            sidebar.qualityRevealRequested();
    }
    // 激活：资产及其子节点打开对应页签；工程 / 表面序列节点没有页签可激活。
    function activateNode(nodeId: string) {
        const node = sidebar.treeModel.node(nodeId);
        if ((node.assetId ?? "").length > 0)
            sidebar.projectModel.openImportRecord(node.assetId);
    }
    function toggleNodeVisible(nodeId: string) {
        const node = sidebar.treeModel.node(nodeId);
        if (node.kind === "asset")
            sidebar.projectModel.setAssetVisible(node.assetId, !node.visible);
        else if (node.kind === "section")
            sidebar.setSectionEnabled(!node.visible);
    }
    function setSectionEnabled(enabled: bool) {
        const model = sidebar.projectModel;
        model.setSection(enabled, model.sectionNormalX, model.sectionNormalY, model.sectionNormalZ, model.sectionFraction);
    }
    function setSectionNormal(axis: int, value: real) {
        const model = sidebar.projectModel;
        const normal = [model.sectionNormalX, model.sectionNormalY, model.sectionNormalZ];
        normal[axis] = value;
        model.setSection(model.sectionActive, normal[0], normal[1], normal[2], model.sectionFraction);
    }

    Connections {
        target: sidebar.treeModel
        function onNodesChanged() {
            sidebar.treeRevision += 1;
        }
        // 结构变化（资产增删、切面 / 字段 / 时间步节点出现或消失）后重新展开并恢复当前项。
        function onModelReset() {
            Qt.callLater(() => {
                objectTree.expandRecursively();
                const index = sidebar.treeModel.indexOfNode(sidebar.selectedNode.id ?? "");
                if (index.valid)
                    objectTree.selectionModel.setCurrentIndex(index, ItemSelectionModel.ClearAndSelect);
            });
        }
    }

    // 属性行：固定宽度标签列 + 内容区（034 Q4 属性面板）。
    component PropertyRow: RowLayout {
        id: propertyRow
        property string label: ""
        default property alias content: propertyContent.data
        Layout.fillWidth: true
        spacing: Theme.spacingSmall
        ThemedLabel {
            Layout.preferredWidth: Theme.propertyLabelWidth
            Layout.alignment: Qt.AlignTop
            text: propertyRow.label
            textColor: Theme.colorTextMuted
            elide: Text.ElideRight
        }
        RowLayout {
            id: propertyContent
            Layout.fillWidth: true
            spacing: Theme.spacingXSmall
        }
    }
    // 禁用控件旁的就地说明。
    component DisabledNote: ThemedLabel {
        Layout.fillWidth: true
        textColor: Theme.colorTextMuted
        textSize: Theme.fontSmall
        wrapMode: Text.Wrap
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            objectName: "sidebarHeader"
            Layout.fillWidth: true
            Layout.preferredHeight: Theme.panelHeaderHeight
            Layout.leftMargin: Theme.spacingMedium
            Layout.rightMargin: Theme.spacingXSmall
            spacing: Theme.spacingSmall
            Label {
                Layout.fillWidth: true
                text: "对象"
                font.bold: true
                elide: Text.ElideRight
            }
            ThemedToolButton {
                objectName: "sidebarCollapseButton"
                iconName: "panel-left"
                accessibleName: "折叠左栏"
                onClicked: sidebar.collapseRequested()
            }
        }

        SplitView {
            id: sidebarSplit
            objectName: "sidebarSplit"
            Layout.fillWidth: true
            Layout.fillHeight: true
            orientation: Qt.Vertical
            handle: Rectangle {
                implicitHeight: Theme.spacingXSmall
                color: SplitHandle.pressed || SplitHandle.hovered ? Theme.colorFocus : Theme.colorChrome
            }

            Item {
                objectName: "objectTreePane"
                // 默认给对象树约五分之二高度；拖动后由 SplitView 记住本次会话的比例。
                SplitView.preferredHeight: sidebarSplit.height * 0.4
                SplitView.minimumHeight: Theme.panelHeaderHeight * 2

                ThemedLabel {
                    objectName: "objectTreeEmpty"
                    anchors.fill: parent
                    anchors.margins: Theme.spacingMedium
                    visible: sidebar.treeModel.nodeCount <= 1
                    topPadding: sidebar.treeModel.nodeCount === 1 ? Theme.treeRowHeight : 0
                    text: sidebar.projectOpen ? "导入网格以查看。" : "新建或打开工程。"
                    textColor: Theme.colorTextMuted
                    wrapMode: Text.Wrap
                }
                TreeView {
                    id: objectTree
                    objectName: "objectTree"
                    anchors.fill: parent
                    anchors.leftMargin: Theme.spacingXSmall
                    anchors.rightMargin: Theme.spacingXSmall
                    clip: true
                    focus: true
                    model: sidebar.treeModel
                    keyNavigationEnabled: true
                    pointerNavigationEnabled: true
                    selectionBehavior: TableView.SelectRows
                    selectionMode: TableView.SingleSelection
                    selectionModel: ItemSelectionModel {}
                    boundsBehavior: Flickable.StopAtBounds
                    Accessible.role: Accessible.Tree
                    Accessible.name: "对象树"
                    ScrollBar.vertical: ScrollBar {}
                    Component.onCompleted: expandRecursively()
                    // 键盘上下 / 左右由 TreeView 处理；当前项变化即选中（只切换属性面板）。
                    Connections {
                        target: objectTree.selectionModel
                        function onCurrentChanged(current) {
                            const nodeId = sidebar.treeModel.nodeIdAt(current);
                            if (nodeId.length > 0 && nodeId !== sidebar.selectedNodeId)
                                sidebar.selectNode(nodeId);
                        }
                    }
                    Keys.onReturnPressed: sidebar.activateNode(sidebar.selectedNode.id ?? "")
                    Keys.onEnterPressed: sidebar.activateNode(sidebar.selectedNode.id ?? "")

                    delegate: Rectangle {
                        id: treeRow
                        required property TreeView treeView
                        required property bool expanded
                        required property bool hasChildren
                        required property int depth
                        required property int row
                        required property string display
                        required property string nodeId
                        required property string kind
                        required property string assetId
                        required property bool nodeVisible
                        required property bool canToggleVisibility
                        readonly property bool isSelected: sidebar.selectedNode.id === nodeId
                        readonly property bool isActiveAsset: kind === "asset" && assetId === sidebar.projectModel.activeDocumentId
                        objectName: "objectTreeRow_" + nodeId
                        implicitWidth: treeRow.treeView.width
                        implicitHeight: Theme.treeRowHeight
                        color: isSelected ? Theme.colorSelection : rowHover.hovered ? Theme.colorHover : Theme.colorTransparent
                        border.width: isSelected && objectTree.activeFocus ? Theme.borderWidth : 0
                        border.color: Theme.colorFocus
                        Accessible.role: Accessible.TreeItem
                        Accessible.name: display
                        Accessible.selected: isSelected

                        HoverHandler {
                            id: rowHover
                        }
                        TapHandler {
                            onTapped: {
                                objectTree.forceActiveFocus();
                                sidebar.selectNode(treeRow.nodeId);
                            }
                            onDoubleTapped: sidebar.activateNode(treeRow.nodeId)
                        }
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: treeRow.depth * Theme.treeIndent
                            anchors.rightMargin: Theme.spacingXSmall
                            spacing: Theme.spacingXSmall
                            ThemedToolButton {
                                objectName: "objectTreeExpand"
                                Layout.preferredWidth: Theme.iconSizeSmall + Theme.spacingXSmall
                                controlHeight: Theme.treeRowHeight
                                contentPadding: 0
                                iconName: "tree-expand"
                                iconSize: Theme.iconSizeCompact
                                rotation: treeRow.expanded ? 90 : 0
                                opacity: treeRow.hasChildren ? 1 : 0
                                enabled: treeRow.hasChildren
                                focusPolicy: Qt.NoFocus
                                accessibleName: treeRow.expanded ? "折叠" : "展开"
                                onClicked: treeRow.treeView.toggleExpanded(treeRow.row)
                            }
                            ThemedLabel {
                                Layout.fillWidth: true
                                text: treeRow.display
                                // 活动资产加粗；隐藏节点弱化文字，不只靠颜色（眼睛图标同时变化）。
                                font.bold: treeRow.isActiveAsset
                                textColor: treeRow.nodeVisible ? Theme.colorText : Theme.colorTextMuted
                                elide: Text.ElideRight
                            }
                            ThemedToolButton {
                                objectName: "objectTreeVisibility"
                                visible: treeRow.canToggleVisibility
                                controlHeight: Theme.treeRowHeight
                                contentPadding: Theme.spacingTiny
                                iconName: treeRow.nodeVisible ? "visibility-on" : "visibility-off"
                                iconSize: Theme.iconSizeSmall
                                focusPolicy: Qt.NoFocus
                                enabled: treeRow.kind !== "section" || (!sidebar.projectModel.animationActive && !sidebar.editLocked)
                                accessibleName: (treeRow.nodeVisible ? "隐藏" : "显示") + treeRow.display
                                onClicked: sidebar.toggleNodeVisible(treeRow.nodeId)
                            }
                        }
                    }
                }
            }

            ColumnLayout {
                objectName: "propertiesPanel"
                SplitView.fillHeight: true
                SplitView.minimumHeight: Theme.panelHeaderHeight * 2
                spacing: Theme.spacingXSmall

                ThemedLabel {
                    objectName: "propertiesTitle"
                    Layout.fillWidth: true
                    Layout.topMargin: Theme.spacingSmall
                    Layout.leftMargin: Theme.spacingMedium
                    Layout.rightMargin: Theme.spacingMedium
                    text: sidebar.selectedNode.label ?? "属性"
                    font.bold: true
                    elide: Text.ElideRight
                }
                RowLayout {
                    objectName: "propertiesTabs"
                    Layout.leftMargin: Theme.spacingSmall
                    spacing: Theme.spacingTiny
                    visible: sidebar.selectedKind.length > 0
                    Accessible.role: Accessible.PageTabList
                    Accessible.name: "属性页"
                    Repeater {
                        model: [["properties", "属性"], ["display", "显示"], ["info", "信息"]]
                        delegate: ThemedToolButton {
                            required property var modelData
                            required property int index
                            objectName: "propertiesTab_" + modelData[0]
                            text: modelData[1]
                            checkable: true
                            checked: sidebar.propertiesTab === index
                            highlighted: checked
                            borderColor: checked ? Theme.colorPanelLine : Theme.colorTransparent
                            Accessible.role: Accessible.PageTab
                            onClicked: {
                                sidebar.propertiesTab = index;
                                checked = Qt.binding(() => sidebar.propertiesTab === index);
                            }
                        }
                    }
                }
                // 属性内容可滚动：切面与着色全部展开时超过左栏下半部高度（task 035 / 036）。
                ScrollView {
                    id: sidebarDetailsScroll
                    objectName: "sidebarDetailsScroll"
                    Layout.leftMargin: Theme.spacingMedium
                    Layout.rightMargin: Theme.spacingMedium
                    Layout.bottomMargin: Theme.spacingMedium
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    contentWidth: availableWidth
                    rightPadding: sidebarDetailsScrollBar.size < 1 ? sidebarDetailsScrollBar.width : 0
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                    ScrollBar.vertical: ScrollBar {
                        id: sidebarDetailsScrollBar
                        objectName: "sidebarDetailsScrollBar"
                        parent: sidebarDetailsScroll
                        x: sidebarDetailsScroll.width - width
                        y: sidebarDetailsScroll.topPadding
                        height: sidebarDetailsScroll.availableHeight
                        // 内容超出时常显，提示可滚动（Basic 样式的 AsNeeded 不悬停时透明）。
                        policy: size < 1 ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
                    }

                    ColumnLayout {
                        id: sidebarDetails
                        objectName: "sidebarDetails"
                        width: sidebarDetailsScroll.availableWidth
                        spacing: Theme.spacingSmall

                        // ── 工程 · 属性 ──
                        ColumnLayout {
                            objectName: "projectProperties"
                            Layout.fillWidth: true
                            visible: sidebar.propertiesTab === 0 && sidebar.selectedKind === "project"
                            PropertyRow {
                                label: "工程"
                                ThemedLabel {
                                    Layout.fillWidth: true
                                    text: sidebar.projectModel.currentName
                                    elide: Text.ElideRight
                                }
                            }
                            // 表面序列（task 020）：只读帧序列播放；播放控件在视口下方的统一播放栏（task 033）。
                            PropertyRow {
                                label: "表面序列"
                                Button {
                                    objectName: "openAnimationButton"
                                    text: sidebar.projectModel.animationActive ? "关闭序列" : "打开序列…"
                                    onClicked: sidebar.projectModel.animationActive ? sidebar.projectModel.closeAnimation() : sidebar.openAnimationRequested()
                                    ToolTip.visible: hovered
                                    ToolTip.text: "选择编号序列中的任一帧文件（如 flow_0001.stl），自动发现同前缀的全部帧"
                                }
                            }
                        }
                        // ── 资产 · 属性 ── 加载中 / 失败时显示稳定诊断码对应的中文摘要与原始详情。
                        ColumnLayout {
                            id: assetProperties
                            objectName: "assetProperties"
                            Layout.fillWidth: true
                            visible: sidebar.propertiesTab === 0 && sidebar.selectedKind === "asset"
                            readonly property bool active: sidebar.selectedNode.assetId === sidebar.projectModel.activeDocumentId
                            PropertyRow {
                                label: "页签"
                                ThemedLabel {
                                    Layout.fillWidth: true
                                    visible: assetProperties.active
                                    text: "已激活"
                                    textColor: Theme.colorTextMuted
                                }
                                Button {
                                    objectName: "activateAssetButton"
                                    visible: !assetProperties.active
                                    text: "打开页签"
                                    onClicked: sidebar.activateNode(sidebar.selectedNode.id)
                                }
                            }
                            ThemedLabel {
                                objectName: "activeDocumentStatus"
                                Layout.fillWidth: true
                                visible: assetProperties.active && sidebar.meshActive && (sidebar.activeDocument.message ?? "").length > 0
                                text: sidebar.activeDocument.message ?? ""
                                textColor: (sidebar.activeDocument.diagnostic?.code ?? "").length > 0 ? Theme.colorError : Theme.colorTextMuted
                                wrapMode: Text.Wrap
                            }
                            ThemedLabel {
                                objectName: "activeDocumentDiagnostic"
                                Layout.fillWidth: true
                                visible: assetProperties.active && text.length > 0
                                text: {
                                    const diagnostic = sidebar.activeDocument.diagnostic ?? ({});
                                    const code = diagnostic.code ?? "";
                                    return code.length > 0 ? code + (diagnostic.detail ? "：" + diagnostic.detail : "") : "";
                                }
                                textColor: Theme.colorTextMuted
                                textSize: Theme.fontSmall
                                wrapMode: Text.WrapAnywhere
                            }
                        }
                        // ── 资产 / 切面 · 显示 ── 可见性只是会话显示状态，不写工程、不影响 dirty。
                        ColumnLayout {
                            objectName: "visibilityProperties"
                            Layout.fillWidth: true
                            visible: sidebar.propertiesTab === 1 && (sidebar.selectedNode.canToggleVisibility ?? false)
                            PropertyRow {
                                label: "可见"
                                CheckBox {
                                    objectName: "nodeVisibleToggle"
                                    checked: sidebar.selectedNode.visible ?? true
                                    enabled: sidebar.selectedKind !== "section" || (!sidebar.projectModel.animationActive && !sidebar.editLocked)
                                    onToggled: {
                                        sidebar.toggleNodeVisible(sidebar.selectedNode.id);
                                        checked = Qt.binding(() => sidebar.selectedNode.visible ?? true);
                                    }
                                }
                            }
                            DisabledNote {
                                visible: sidebar.selectedKind === "asset"
                                text: "表示方式、颜色与透明度当前渲染暂不支持。"
                            }
                        }
                        // ── 切面 · 属性 ── 任意平面切面（task 024）：X/Y/Z 预设只是把法向设为坐标轴。
                        ColumnLayout {
                            id: sectionProperties
                            objectName: "sectionProperties"
                            Layout.fillWidth: true
                            visible: sidebar.propertiesTab === 0 && sidebar.selectedKind === "section"
                            readonly property bool editable: !sidebar.projectModel.animationActive && !sidebar.editLocked
                            DisabledNote {
                                objectName: "sectionDisabledNote"
                                visible: !sectionProperties.editable
                                text: sidebar.projectModel.animationActive ? "打开表面序列时切面不可用，关闭序列后可用" : sidebar.editLockedNote
                            }
                            PropertyRow {
                                label: "启用"
                                CheckBox {
                                    objectName: "sectionToggle"
                                    enabled: sectionProperties.editable
                                    checked: sidebar.projectModel.sectionActive
                                    onToggled: {
                                        sidebar.setSectionEnabled(checked);
                                        checked = Qt.binding(() => sidebar.projectModel.sectionActive);
                                    }
                                }
                            }
                            PropertyRow {
                                label: "预设"
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
                                        // Basic 样式背景自带 100px 隐式宽度，会把左栏撑宽；按文字收紧（task 035）。
                                        implicitWidth: implicitContentWidth + leftPadding + rightPadding
                                        enabled: sectionProperties.editable
                                        text: modelData.label
                                        flat: true
                                        highlighted: sidebar.projectModel.sectionNormalX === modelData.normal[0] && sidebar.projectModel.sectionNormalY === modelData.normal[1] && sidebar.projectModel.sectionNormalZ === modelData.normal[2]
                                        onClicked: sidebar.projectModel.setSection(sidebar.projectModel.sectionActive, modelData.normal[0], modelData.normal[1], modelData.normal[2], sidebar.projectModel.sectionFraction)
                                    }
                                }
                            }
                            // 法向三元组：紧凑数字输入（−1..1，两位小数），零法向被拒绝时回显原有效值。
                            PropertyRow {
                                label: "法向"
                                Repeater {
                                    model: ["X", "Y", "Z"]
                                    delegate: ThemedTextField {
                                        id: normalField
                                        required property int index
                                        required property string modelData
                                        readonly property real component: index === 0 ? sidebar.projectModel.sectionNormalX : index === 1 ? sidebar.projectModel.sectionNormalY : sidebar.projectModel.sectionNormalZ
                                        objectName: "sectionNormal" + modelData
                                        Layout.fillWidth: true
                                        Layout.minimumWidth: 0
                                        implicitWidth: Theme.controlHeight * 2
                                        implicitHeight: Theme.controlHeight
                                        enabled: sectionProperties.editable
                                        horizontalAlignment: TextInput.AlignRight
                                        text: Number(component).toFixed(2)
                                        Accessible.name: "切面法向 " + modelData
                                        validator: DoubleValidator {
                                            bottom: -1
                                            top: 1
                                            decimals: 2
                                            notation: DoubleValidator.StandardNotation
                                        }
                                        onEditingFinished: {
                                            if (acceptableInput)
                                                sidebar.setSectionNormal(index, Number(text));
                                            text = Qt.binding(() => Number(normalField.component).toFixed(2));
                                        }
                                    }
                                }
                            }
                            PropertyRow {
                                label: "位置"
                                Slider {
                                    objectName: "sectionSlider"
                                    Layout.fillWidth: true
                                    Layout.minimumWidth: 0
                                    implicitWidth: Theme.controlHeight * 3
                                    enabled: sidebar.projectModel.sectionActive && !sidebar.editLocked
                                    from: 0
                                    to: 1
                                    value: sidebar.projectModel.sectionFraction
                                    Accessible.name: "切面位置"
                                    onMoved: sidebar.projectModel.setSection(true, sidebar.projectModel.sectionNormalX, sidebar.projectModel.sectionNormalY, sidebar.projectModel.sectionNormalZ, value)
                                }
                            }
                        }
                        // ── 着色 · 属性 ── 结果场上色（task 029）：只列出可切面的 1/3 分量字段；3 分量字段可选模或分量。
                        ColumnLayout {
                            id: fieldProperties
                            objectName: "fieldProperties"
                            Layout.fillWidth: true
                            visible: sidebar.propertiesTab === 0 && sidebar.selectedKind === "field"
                            readonly property var fieldOptions: [
                                {
                                    "label": "无",
                                    "name": "",
                                    "association": "",
                                    "components": 1
                                }
                            ].concat(sidebar.projectModel.resultFields.filter(field => field.sectionable).map(field => ({
                                        "label": field.name + (field.association === "cell" ? "（单元）" : "（节点）"),
                                        "name": field.name,
                                        "association": field.association,
                                        "components": field.components
                                    })))
                            readonly property int selectedIndex: fieldOptions.findIndex(option => option.name === sidebar.projectModel.sectionFieldName && option.association === sidebar.projectModel.sectionFieldAssociation)
                            DisabledNote {
                                visible: sidebar.editLocked
                                text: sidebar.editLockedNote
                            }
                            PropertyRow {
                                label: "字段"
                                ThemedComboBox {
                                    objectName: "sectionField"
                                    Layout.fillWidth: true
                                    Layout.minimumWidth: 0
                                    enabled: !sidebar.editLocked
                                    model: fieldProperties.fieldOptions
                                    textRole: "label"
                                    currentIndex: Math.max(0, fieldProperties.selectedIndex)
                                    Accessible.name: "上色字段"
                                    onActivated: index => {
                                        const option = fieldProperties.fieldOptions[index];
                                        sidebar.projectModel.setSectionField(option.name, option.association, -1);
                                    }
                                }
                            }
                            PropertyRow {
                                label: "分量"
                                visible: fieldProperties.fieldOptions[Math.max(0, fieldProperties.selectedIndex)].components === 3
                                ThemedComboBox {
                                    objectName: "sectionFieldComponent"
                                    Layout.fillWidth: true
                                    Layout.minimumWidth: 0
                                    enabled: !sidebar.editLocked
                                    model: ["模", "X", "Y", "Z"]
                                    currentIndex: sidebar.projectModel.sectionFieldComponent + 1
                                    Accessible.name: "上色分量"
                                    onActivated: index => sidebar.projectModel.setSectionField(sidebar.projectModel.sectionFieldName, sidebar.projectModel.sectionFieldAssociation, index - 1)
                                }
                            }
                            // 时间步结果（task 033）：默认全部时间步的全局范围，播放时色标不跳变。
                            PropertyRow {
                                objectName: "fieldRangeModeRow"
                                label: "色标范围"
                                visible: sidebar.projectModel.playbackSource === "timeSteps"
                                ThemedComboBox {
                                    objectName: "fieldRangeModeCombo"
                                    Layout.fillWidth: true
                                    Layout.minimumWidth: 0
                                    model: ["全部时间步", "当前步"]
                                    currentIndex: sidebar.projectModel.fieldRangeGlobal ? 0 : 1
                                    Accessible.name: "色标范围"
                                    onActivated: index => sidebar.projectModel.setFieldRangeGlobal(index === 0)
                                }
                            }
                        }
                        // ── 着色 · 显示 ── 小型数值图例：与视口查找表共用 Cool to Warm 控制点；位于视口之外。
                        ColumnLayout {
                            objectName: "fieldLegend"
                            Layout.fillWidth: true
                            visible: sidebar.propertiesTab === 1 && sidebar.selectedKind === "field"
                            spacing: Theme.spacingTiny
                            ThemedLabel {
                                Layout.fillWidth: true
                                visible: !sidebar.projectModel.fieldLegendVisible
                                text: "未选择着色字段。"
                                textColor: Theme.colorTextMuted
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: sidebar.projectModel.fieldLegendVisible
                                spacing: Theme.spacingTiny
                                ThemedLabel {
                                    Layout.fillWidth: true
                                    text: sidebar.projectModel.fieldLegendTitle + (sidebar.projectModel.fieldLegendUnits.length > 0 ? " (" + sidebar.projectModel.fieldLegendUnits + ")" : "")
                                    elide: Text.ElideRight
                                }
                                Rectangle {
                                    Layout.fillWidth: true
                                    implicitHeight: Theme.spacingLarge
                                    border.color: Theme.colorTextMuted
                                    gradient: Gradient {
                                        orientation: Gradient.Horizontal
                                        GradientStop {
                                            position: sidebar.projectModel.fieldLegendStops[0].position
                                            color: sidebar.projectModel.fieldLegendStops[0].color
                                        }
                                        GradientStop {
                                            position: sidebar.projectModel.fieldLegendStops[1].position
                                            color: sidebar.projectModel.fieldLegendStops[1].color
                                        }
                                        GradientStop {
                                            position: sidebar.projectModel.fieldLegendStops[2].position
                                            color: sidebar.projectModel.fieldLegendStops[2].color
                                        }
                                    }
                                }
                                RowLayout {
                                    Layout.fillWidth: true
                                    ThemedLabel {
                                        text: sidebar.projectModel.fieldLegendHasRange ? Number(sidebar.projectModel.fieldLegendMin).toPrecision(4) : "无有限值"
                                        textColor: Theme.colorTextMuted
                                    }
                                    Item {
                                        Layout.fillWidth: true
                                    }
                                    ThemedLabel {
                                        visible: sidebar.projectModel.fieldLegendHasRange
                                        text: Number(sidebar.projectModel.fieldLegendMax).toPrecision(4)
                                        textColor: Theme.colorTextMuted
                                    }
                                }
                                RowLayout {
                                    visible: sidebar.projectModel.fieldMissingVertices > 0
                                    Rectangle {
                                        implicitWidth: Theme.spacingLarge
                                        implicitHeight: Theme.spacingLarge
                                        color: sidebar.projectModel.fieldMissingColor
                                    }
                                    ThemedLabel {
                                        text: "缺失值顶点 " + sidebar.projectModel.fieldMissingVertices
                                        textColor: Theme.colorTextMuted
                                    }
                                }
                            }
                        }
                        // ── 时间序列 / 诊断 / 表面序列 · 属性 ──
                        ColumnLayout {
                            objectName: "timeStepsProperties"
                            Layout.fillWidth: true
                            visible: sidebar.propertiesTab === 0 && sidebar.selectedKind === "timeSteps"
                            DisabledNote {
                                text: "使用视口下方的播放栏浏览时间步。"
                            }
                        }
                        ColumnLayout {
                            objectName: "qualityProperties"
                            Layout.fillWidth: true
                            visible: sidebar.propertiesTab === 0 && sidebar.selectedKind === "quality"
                            DisabledNote {
                                text: "诊断明细见底部面板「诊断」页。"
                            }
                            Button {
                                objectName: "revealQualityButton"
                                text: "查看诊断"
                                onClicked: sidebar.qualityRevealRequested()
                            }
                        }
                        ColumnLayout {
                            objectName: "animationProperties"
                            Layout.fillWidth: true
                            visible: sidebar.propertiesTab === 0 && sidebar.selectedKind === "animation"
                            DisabledNote {
                                text: "表面序列只读播放；使用视口下方的播放栏浏览帧。"
                            }
                            Button {
                                objectName: "closeAnimationButton"
                                text: "关闭序列"
                                onClicked: sidebar.projectModel.closeAnimation()
                            }
                        }
                        // 其余节点的「显示」页没有可调项时给出说明，避免空白页。
                        DisabledNote {
                            objectName: "noDisplayNote"
                            visible: sidebar.propertiesTab === 1 && sidebar.selectedKind.length > 0 && sidebar.selectedKind !== "field" && !(sidebar.selectedNode.canToggleVisibility ?? false)
                            text: "该对象没有显示设置。"
                        }
                        // ── 信息 ── 由 Rust 投影的只读键值行。
                        Repeater {
                            model: sidebar.propertiesTab === 2 ? (sidebar.selectedNode.info ?? []) : []
                            delegate: PropertyRow {
                                required property var modelData
                                objectName: "infoRow"
                                label: modelData.label
                                ThemedLabel {
                                    Layout.fillWidth: true
                                    text: modelData.value
                                    wrapMode: Text.WrapAnywhere
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
