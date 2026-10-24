// 统一播放工具栏（task 033）：对照 ParaView 的 VCR 控件——首帧、上一帧、播放 / 暂停、
// 下一帧、末帧、循环、帧率与时间轴。来源为时序动画序列（task 020）或结果场时间步；
// 位于视口下方，不遮挡渲染。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

PanelSurface {
    id: bar

    property var model: null
    readonly property bool active: model !== null && model.playbackActive
    readonly property int lastFrame: active ? Math.max(0, model.playbackFrameCount - 1) : 0

    implicitHeight: content.implicitHeight + Theme.spacingSmall * 2
    visible: active || (model !== null && model.playbackUnavailable.length > 0)

    ColumnLayout {
        id: content
        anchors.fill: parent
        anchors.margins: Theme.spacingSmall
        spacing: 2

        RowLayout {
            Layout.fillWidth: true
            visible: bar.active
            spacing: 2
            // 图标为 Lucide SVG（task 035，Q9）；不依赖系统字体里的 ⏮⏭⏸ 字形。
            ThemedToolButton {
                objectName: "playbackFirstButton"
                iconName: "playback-first"
                accessibleName: "首帧"
                onClicked: bar.model.setPlaybackFrame(0)
            }
            ThemedToolButton {
                objectName: "playbackPreviousButton"
                iconName: "playback-previous"
                accessibleName: "上一帧"
                onClicked: bar.model.stepPlayback(-1)
            }
            ThemedToolButton {
                objectName: "playbackPlayButton"
                iconName: bar.active && bar.model.playbackPlaying ? "playback-pause" : "playback-play"
                accessibleName: bar.active && bar.model.playbackPlaying ? "暂停" : "播放"
                onClicked: bar.model.setPlaybackPlaying(!bar.model.playbackPlaying)
            }
            ThemedToolButton {
                objectName: "playbackNextButton"
                iconName: "playback-next"
                accessibleName: "下一帧"
                onClicked: bar.model.stepPlayback(1)
            }
            ThemedToolButton {
                objectName: "playbackLastButton"
                iconName: "playback-last"
                accessibleName: "末帧"
                onClicked: bar.model.setPlaybackFrame(bar.lastFrame)
            }
            // 开关状态由边框（形状）、图标颜色和 tooltip 文字共同表达，不只靠颜色。
            ThemedToolButton {
                objectName: "playbackLoopButton"
                iconName: "playback-loop"
                checkable: true
                checked: bar.active && bar.model.playbackLoop
                contentColor: checked ? Theme.colorFocus : Theme.colorIcon
                borderColor: checked ? Theme.colorFocus : Theme.colorTransparent
                accessibleName: checked ? "循环播放：开" : "循环播放：关（播到末帧停止）"
                onToggled: bar.model.setPlaybackLoop(checked)
            }
            Slider {
                objectName: "playbackFrameSlider"
                Layout.fillWidth: true
                from: 0
                to: bar.lastFrame
                stepSize: 1
                snapMode: Slider.SnapAlways
                value: bar.active ? Math.max(0, bar.model.playbackFrame) : 0
                Accessible.name: "时间轴"
                onMoved: bar.model.setPlaybackFrame(Math.round(value))
            }
            Label {
                objectName: "playbackTimeLabel"
                text: bar.active ? bar.model.playbackTimeLabel : ""
                elide: Text.ElideRight
                Layout.maximumWidth: 200
            }
            SpinBox {
                objectName: "playbackFpsSpin"
                from: 1
                to: 60
                editable: true
                value: bar.active ? bar.model.playbackFps : 10
                Accessible.name: "帧率 (fps)"
                ToolTip.visible: hovered
                ToolTip.text: "帧率 (fps)"
                onValueModified: bar.model.setPlaybackFps(value)
            }
        }
        // 播放中的长状态文字在状态栏「时间步」字段（task 036）；此处只说明为何不可播放。
        Label {
            objectName: "playbackStatus"
            Layout.fillWidth: true
            visible: !bar.active
            text: !bar.active && bar.model !== null ? bar.model.playbackUnavailable : ""
            color: Theme.colorTextMuted
            font.pixelSize: Theme.fontSmall
            elide: Text.ElideRight
        }
    }
}
