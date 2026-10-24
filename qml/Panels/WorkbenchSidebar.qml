// 工作台左栏（task 036，034 Q4）：标题行 + 纵向 SplitView（上：资产列表，下：属性面板容器）。
// 宽度与折叠状态由宿主写入用户设置（Settings）；本组件不导入 Bridge，属性面板先承载原侧栏控件。
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
                text: "模型资产"
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

            ColumnLayout {
                objectName: "assetListPane"
                // 默认给资产列表约三分之一高度；拖动后由 SplitView 记住本次会话的比例。
                SplitView.preferredHeight: sidebarSplit.height / 3
                SplitView.minimumHeight: Theme.panelHeaderHeight * 2
                spacing: Theme.spacingSmall

                Label {
                    Layout.fillWidth: true
                    visible: assetList.count === 0
                    Layout.leftMargin: Theme.spacingMedium
                    Layout.rightMargin: Theme.spacingMedium
                    text: sidebar.projectOpen ? "导入网格以查看。" : "新建或打开工程。"
                    wrapMode: Text.Wrap
                }
                ListView {
                    id: assetList
                    objectName: "assetList"
                    Layout.leftMargin: Theme.spacingMedium
                    Layout.rightMargin: Theme.spacingMedium
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    model: sidebar.projectModel.importedPartIds
                    delegate: ItemDelegate {
                        required property int index
                        required property string modelData
                        objectName: "importedPartEntry"
                        width: ListView.view.width
                        text: sidebar.projectModel.importedPartNames[index] ?? modelData
                        highlighted: sidebar.projectModel.activeDocumentId === modelData
                        onClicked: sidebar.projectModel.openImportRecord(modelData)
                    }
                    ScrollBar.vertical: ScrollBar {}
                }
            }

            ColumnLayout {
                objectName: "propertiesPanel"
                SplitView.fillHeight: true
                SplitView.minimumHeight: Theme.panelHeaderHeight * 2
                spacing: Theme.spacingSmall

                Label {
                    Layout.topMargin: Theme.spacingSmall
                    Layout.leftMargin: Theme.spacingMedium
                    text: "属性"
                    font.bold: true
                }
                // 属性内容可滚动：体网格、色标、切面与动画全部展开时超过左栏下半部高度（task 035 / 036）。
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
                        // 内容超出时常显，提示侧栏可滚动（Basic 样式的 AsNeeded 不悬停时透明）。
                        policy: size < 1 ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
                    }

                    ColumnLayout {
                        id: sidebarDetails
                        objectName: "sidebarDetails"
                        width: sidebarDetailsScroll.availableWidth
                        spacing: Theme.spacingMedium

                        // 活动网格状态：加载中 / 失败时显示稳定诊断码对应的中文摘要与原始详情。
                        Label {
                            objectName: "activeDocumentStatus"
                            Layout.fillWidth: true
                            visible: sidebar.meshActive && (sidebar.activeDocument.message ?? "").length > 0
                            text: sidebar.activeDocument.message ?? ""
                            color: (sidebar.activeDocument.diagnostic?.code ?? "").length > 0 ? Theme.colorError : Theme.colorTextMuted
                            wrapMode: Text.Wrap
                        }
                        Label {
                            objectName: "activeDocumentDiagnostic"
                            Layout.fillWidth: true
                            visible: text.length > 0
                            text: {
                                const diagnostic = sidebar.activeDocument.diagnostic ?? ({});
                                const code = diagnostic.code ?? "";
                                return code.length > 0 ? code + (diagnostic.detail ? "：" + diagnostic.detail : "") : "";
                            }
                            color: Theme.colorTextMuted
                            font.pixelSize: Theme.fontSmall
                            wrapMode: Text.WrapAnywhere
                        }
                        // 时序动画（task 020）：只读序列播放；播放控件在视口下方的统一播放栏（task 033）。
                        RowLayout {
                            Layout.fillWidth: true
                            Label {
                                Layout.fillWidth: true
                                text: "时序动画"
                                font.bold: true
                            }
                            Button {
                                objectName: "openAnimationButton"
                                text: sidebar.projectModel.animationActive ? "关闭序列" : "打开序列…"
                                onClicked: sidebar.projectModel.animationActive ? sidebar.projectModel.closeAnimation() : sidebar.openAnimationRequested()
                                ToolTip.visible: hovered
                                ToolTip.text: "选择编号序列中的任一帧文件（如 flow_0001.stl），自动发现同前缀的全部帧"
                            }
                        }
                        // 体网格（task 021）：质量统计与单切面显示；切面只替换视口，不写工程。
                        ColumnLayout {
                            objectName: "volumePanel"
                            Layout.fillWidth: true
                            visible: sidebar.meshActive && sidebar.projectModel.volumeAvailable
                            Label {
                                text: "体网格"
                                font.bold: true
                            }
                            Label {
                                objectName: "volumeInfo"
                                Layout.fillWidth: true
                                text: sidebar.projectModel.volumeInfo
                                color: Theme.colorTextMuted
                                wrapMode: Text.WrapAnywhere
                            }
                            // 任意平面切面（task 024）：X/Y/Z 预设只是把法向设为坐标轴；法向分量可编辑（−1..1，步长 0.05）。
                            RowLayout {
                                Layout.fillWidth: true
                                CheckBox {
                                    objectName: "sectionToggle"
                                    text: "切面"
                                    enabled: !sidebar.projectModel.animationActive
                                    checked: sidebar.projectModel.sectionActive
                                    onToggled: sidebar.projectModel.setSection(checked, sidebar.projectModel.sectionNormalX, sidebar.projectModel.sectionNormalY, sidebar.projectModel.sectionNormalZ, sectionSlider.value)
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
                                        // Basic 样式背景自带 100px 隐式宽度，会把整列侧栏撑宽；按文字收紧（task 035）。
                                        implicitWidth: implicitContentWidth + leftPadding + rightPadding
                                        text: modelData.label
                                        flat: true
                                        checkable: false
                                        highlighted: sidebar.projectModel.sectionNormalX === modelData.normal[0] && sidebar.projectModel.sectionNormalY === modelData.normal[1] && sidebar.projectModel.sectionNormalZ === modelData.normal[2]
                                        onClicked: sidebar.projectModel.setSection(sidebar.projectModel.sectionActive, modelData.normal[0], modelData.normal[1], modelData.normal[2], sectionSlider.value)
                                    }
                                }
                            }
                            // 三个分量纵向排布：240px 侧栏放不下三个并排的 SpinBox（task 035）。
                            Label {
                                text: "法向"
                                color: Theme.colorTextMuted
                            }
                            Repeater {
                                model: ["X", "Y", "Z"]
                                delegate: RowLayout {
                                    id: normalRow
                                    required property int index
                                    required property string modelData
                                    Layout.fillWidth: true
                                    Label {
                                        Layout.preferredWidth: Theme.controlHeight
                                        text: normalRow.modelData
                                        color: Theme.colorTextMuted
                                        horizontalAlignment: Text.AlignHCenter
                                    }
                                    SpinBox {
                                        id: normalSpin
                                        readonly property int index: normalRow.index
                                        readonly property string modelData: normalRow.modelData
                                        readonly property real valueScale: 100
                                        readonly property real component: index === 0 ? sidebar.projectModel.sectionNormalX : index === 1 ? sidebar.projectModel.sectionNormalY : sidebar.projectModel.sectionNormalZ
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
                                            const normal = [sidebar.projectModel.sectionNormalX, sidebar.projectModel.sectionNormalY, sidebar.projectModel.sectionNormalZ];
                                            normal[index] = value / valueScale;
                                            sidebar.projectModel.setSection(sidebar.projectModel.sectionActive, normal[0], normal[1], normal[2], sectionSlider.value);
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
                                enabled: sidebar.projectModel.sectionActive
                                from: 0
                                to: 1
                                value: sidebar.projectModel.sectionFraction
                                onMoved: sidebar.projectModel.setSection(true, sidebar.projectModel.sectionNormalX, sidebar.projectModel.sectionNormalY, sidebar.projectModel.sectionNormalZ, value)
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
                                ].concat(sidebar.projectModel.resultFields.filter(field => field.sectionable).map(field => ({
                                            "label": field.name + (field.association === "cell" ? "（单元）" : "（节点）"),
                                            "name": field.name,
                                            "association": field.association,
                                            "components": field.components
                                        })))
                                readonly property int selectedIndex: fieldOptions.findIndex(option => option.name === sidebar.projectModel.sectionFieldName && option.association === sidebar.projectModel.sectionFieldAssociation)
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
                                        sidebar.projectModel.setSectionField(option.name, option.association, -1);
                                    }
                                }
                                ComboBox {
                                    objectName: "sectionFieldComponent"
                                    readonly property var option: sectionFieldRow.fieldOptions[Math.max(0, sectionFieldRow.selectedIndex)]
                                    visible: option.components === 3
                                    model: ["模", "X", "Y", "Z"]
                                    currentIndex: sidebar.projectModel.sectionFieldComponent + 1
                                    Accessible.name: "切面上色分量"
                                    onActivated: index => sidebar.projectModel.setSectionField(sidebar.projectModel.sectionFieldName, sidebar.projectModel.sectionFieldAssociation, index - 1)
                                }
                            }
                            // 色标：渐变与视口查找表共用同一组 Cool to Warm 控制点；位于视口之外，不遮挡渲染。
                            ColumnLayout {
                                objectName: "fieldLegend"
                                Layout.fillWidth: true
                                visible: sidebar.projectModel.fieldLegendVisible
                                spacing: 2
                                Label {
                                    Layout.fillWidth: true
                                    text: sidebar.projectModel.fieldLegendTitle + (sidebar.projectModel.fieldLegendUnits.length > 0 ? " (" + sidebar.projectModel.fieldLegendUnits + ")" : "")
                                    elide: Text.ElideRight
                                }
                                Rectangle {
                                    Layout.fillWidth: true
                                    implicitHeight: 12
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
                                    Label {
                                        text: sidebar.projectModel.fieldLegendHasRange ? Number(sidebar.projectModel.fieldLegendMin).toPrecision(4) : "无有限值"
                                        color: Theme.colorTextMuted
                                    }
                                    Item {
                                        Layout.fillWidth: true
                                    }
                                    Label {
                                        visible: sidebar.projectModel.fieldLegendHasRange
                                        text: Number(sidebar.projectModel.fieldLegendMax).toPrecision(4)
                                        color: Theme.colorTextMuted
                                    }
                                }
                                // 时间步结果（task 033）：默认全部时间步的全局范围，播放时色标不跳变。
                                RowLayout {
                                    objectName: "fieldRangeModeRow"
                                    Layout.fillWidth: true
                                    visible: sidebar.projectModel.playbackSource === "timeSteps"
                                    Label {
                                        text: "色标范围"
                                        color: Theme.colorTextMuted
                                    }
                                    ComboBox {
                                        objectName: "fieldRangeModeCombo"
                                        Layout.fillWidth: true
                                        model: ["全部时间步", "当前步"]
                                        currentIndex: sidebar.projectModel.fieldRangeGlobal ? 0 : 1
                                        Accessible.name: "色标范围"
                                        onActivated: index => sidebar.projectModel.setFieldRangeGlobal(index === 0)
                                    }
                                }
                                RowLayout {
                                    visible: sidebar.projectModel.fieldMissingVertices > 0
                                    Rectangle {
                                        implicitWidth: 12
                                        implicitHeight: 12
                                        color: sidebar.projectModel.fieldMissingColor
                                    }
                                    Label {
                                        text: "缺失值顶点 " + sidebar.projectModel.fieldMissingVertices
                                        color: Theme.colorTextMuted
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
