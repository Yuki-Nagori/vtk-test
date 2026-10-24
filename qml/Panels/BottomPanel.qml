// 底部面板（task 036）：诊断 / 消息 / 任务三页，可折叠为一行标题。
// 折叠状态与展开高度由宿主写入用户设置（Settings），不进入工程。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

PanelSurface {
    id: panel

    required property var projectModel
    property bool collapsed: true
    property bool meshActive: false
    property bool canDiagnose: false
    // 活动页签的诊断报告；不属于活动页签时为空对象。
    property var quality: ({})
    // [{kind, name, progress}]：正在进行的后台任务。
    property var tasks: []
    property int currentIndex: 0
    readonly property int headerHeight: Theme.panelHeaderHeight
    readonly property int messageCount: messages.count

    signal collapseToggled(bool collapsed)
    signal diagnoseRequested
    signal cancelTaskRequested(string kind)

    // 切到指定页签并展开（诊断有新结果、任务出错时调用）。
    function reveal(index: int) {
        currentIndex = index;
        if (collapsed)
            collapseToggled(false);
    }
    // 消息历史只保存在本次会话内存中，最多保留最近 200 条。
    function appendMessage(text: string, isError: bool) {
        if (text.length === 0)
            return;
        messages.append({
            "time": Qt.formatTime(new Date(), "HH:mm:ss"),
            "message": text,
            "isError": isError
        });
        if (messages.count > 200)
            messages.remove(0);
    }

    ListModel {
        id: messages
    }

    // 质量报告按行展示：[名称, 值, 是否超阈值]。
    readonly property var qualityRows: {
        const q = panel.quality;
        if (q.documentId === undefined)
            return [];
        const rows = [["结论", q.clean ? "无问题" : "存在问题", !q.clean], ["三角面", q.triangleCount, false], ["焊接后顶点", q.weldedVertexCount, false], ["近重复顶点", q.nearDuplicateVertexCount, q.nearDuplicateVertexCount > 0], ["退化三角面", q.degenerateTriangleCount, q.degenerateTriangleCount > 0], ["开放边界边", q.boundaryEdgeCount, q.boundaryEdgeCount > 0], ["孔洞", q.holeCount, q.holeCount > 0], ["非流形边", q.nonManifoldEdgeCount, q.nonManifoldEdgeCount > 0], ["法向不一致边", q.inconsistentOrientationEdgeCount, q.inconsistentOrientationEdgeCount > 0], ["焊接容差", Number(q.weldTolerance ?? 0).toExponential(2), false]];
        if (!q.volumeAvailable)
            return rows;
        const volume = [["体网格节点", q.nodeCount, false], ["原生四面体", q.tetCount, false], ["最小二面角", Number(q.minDihedralDegrees ?? 0).toFixed(2) + "°", false], ["最大长宽比", Number(q.maxAspectRatio ?? 0).toFixed(2), false], ["平均长宽比", Number(q.meanAspectRatio ?? 0).toFixed(2), false], ["原始单元", q.cellCount ?? 0, false], ["劣质单元", q.poorTetCount, q.poorTetCount > 0], ["按类型缩放雅可比", q.cellQualitySummary ?? "", false]];
        const face = q.faceQualitySummary ? [["面邻接质量（checkMesh 口径）", q.faceQualitySummary, q.faceQualityWarning === true]] : [];
        return rows.concat(volume).concat(face);
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: Theme.borderWidth
            color: Theme.colorPanelLine
        }
        RowLayout {
            objectName: "bottomPanelHeader"
            Accessible.role: Accessible.PageTabList
            Accessible.name: "底部面板"
            Layout.fillWidth: true
            Layout.preferredHeight: panel.headerHeight - Theme.borderWidth
            Layout.leftMargin: Theme.spacingSmall
            Layout.rightMargin: Theme.spacingSmall
            spacing: Theme.spacingTiny

            // 页签用共用工具按钮：选中页用面板底色 + 边框表达，不只靠颜色。
            Repeater {
                model: [["diagnostics", "诊断"], ["messages", "消息"], ["tasks", "任务"]]
                delegate: ThemedToolButton {
                    required property var modelData
                    required property int index
                    objectName: "bottomTab_" + modelData[0]
                    text: modelData[1]
                    checkable: true
                    checked: panel.currentIndex === index
                    highlighted: checked && !panel.collapsed
                    borderColor: highlighted ? Theme.colorPanelLine : Theme.colorTransparent
                    Accessible.role: Accessible.PageTab
                    // 点击页签同时展开面板；已展开时只切换页签。
                    onClicked: {
                        panel.reveal(index);
                        checked = Qt.binding(() => panel.currentIndex === index);
                    }
                }
            }
            Item {
                Layout.fillWidth: true
            }
            ThemedToolButton {
                objectName: "bottomPanelToggle"
                iconName: "panel-bottom"
                checkable: true
                checked: !panel.collapsed
                contentColor: checked ? Theme.colorFocus : Theme.colorIcon
                accessibleName: panel.collapsed ? "展开底部面板" : "折叠底部面板"
                onToggled: {
                    panel.collapseToggled(!checked);
                    checked = Qt.binding(() => !panel.collapsed);
                }
            }
        }
        StackLayout {
            objectName: "bottomPanelBody"
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: !panel.collapsed
            currentIndex: panel.currentIndex

            // 诊断：网格质量结果表（task 006 表面诊断 + 021/032 体网格指标）。
            ColumnLayout {
                objectName: "diagnosticsPage"
                spacing: Theme.spacingXSmall
                RowLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: Theme.spacingMedium
                    Layout.rightMargin: Theme.spacingMedium
                    ThemedLabel {
                        Layout.fillWidth: true
                        text: !panel.meshActive ? "打开网格后可进行质量诊断。" : panel.qualityRows.length === 0 ? "尚未诊断活动网格。" : "网格质量"
                        textColor: Theme.colorTextMuted
                    }
                    Button {
                        objectName: "diagnoseMeshButton"
                        implicitWidth: implicitContentWidth + leftPadding + rightPadding
                        text: panel.projectModel.diagnosticsPending ? "诊断中…" : "诊断"
                        enabled: panel.canDiagnose
                        onClicked: panel.diagnoseRequested()
                        ToolTip.visible: hovered
                        ToolTip.delay: Theme.toolTipDelay
                        ToolTip.text: "检查重复顶点、退化面、开放边界、非流形边与法向一致性（Ctrl+D）"
                    }
                }
                ScrollView {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    contentWidth: availableWidth
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                    Flow {
                        objectName: "meshQualityPanel"
                        width: parent.width
                        visible: panel.meshActive && panel.qualityRows.length > 0
                        leftPadding: Theme.spacingMedium
                        rightPadding: Theme.spacingMedium
                        spacing: Theme.spacingXSmall
                        Repeater {
                            model: panel.qualityRows
                            delegate: Label {
                                required property var modelData
                                width: Theme.qualityCellWidth
                                text: String(modelData[0]) + "：" + String(modelData[1])
                                color: modelData[2] ? Theme.colorError : Theme.colorTextMuted
                                font.pixelSize: Theme.fontSmall
                                elide: Text.ElideRight
                                ToolTip.visible: truncated && hoverHandler.hovered
                                ToolTip.text: text
                                HoverHandler {
                                    id: hoverHandler
                                }
                            }
                        }
                    }
                }
            }
            // 消息：本次会话的状态与错误历史，新消息在底部。
            ListView {
                id: messageList
                objectName: "messageList"
                clip: true
                model: messages
                onCountChanged: positionViewAtEnd()
                ScrollBar.vertical: ScrollBar {}
                delegate: Label {
                    required property string time
                    required property string message
                    required property bool isError
                    width: ListView.view.width
                    leftPadding: Theme.spacingMedium
                    rightPadding: Theme.spacingMedium
                    text: time + "  " + message
                    color: isError ? Theme.colorError : Theme.colorTextMuted
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                }
                ThemedLabel {
                    anchors.centerIn: parent
                    visible: messageList.count === 0
                    text: "暂无消息。"
                    textColor: Theme.colorTextMuted
                }
            }
            // 任务：正在进行的后台任务与各自的取消入口。
            ColumnLayout {
                objectName: "tasksPage"
                spacing: Theme.spacingXSmall
                ThemedLabel {
                    Layout.leftMargin: Theme.spacingMedium
                    visible: panel.tasks.length === 0
                    text: "当前没有后台任务。"
                    textColor: Theme.colorTextMuted
                }
                Repeater {
                    model: panel.tasks
                    delegate: RowLayout {
                        id: taskRow
                        required property var modelData
                        objectName: "taskRow_" + modelData.kind
                        Layout.fillWidth: true
                        Layout.leftMargin: Theme.spacingMedium
                        Layout.rightMargin: Theme.spacingMedium
                        ThemedLabel {
                            Layout.preferredWidth: Theme.taskNameWidth
                            text: taskRow.modelData.name
                        }
                        ProgressBar {
                            Layout.fillWidth: true
                            from: 0
                            to: 1
                            value: Math.max(0, taskRow.modelData.progress)
                        }
                        ThemedLabel {
                            text: Math.round(Math.max(0, taskRow.modelData.progress) * 100) + "%"
                            textColor: Theme.colorTextMuted
                        }
                        Button {
                            objectName: "taskCancel_" + taskRow.modelData.kind
                            implicitWidth: implicitContentWidth + leftPadding + rightPadding
                            text: "取消"
                            onClicked: panel.cancelTaskRequested(taskRow.modelData.kind)
                        }
                    }
                }
                Item {
                    Layout.fillHeight: true
                }
            }
        }
    }
}
