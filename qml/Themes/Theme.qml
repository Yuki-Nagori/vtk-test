// 工作台、导入窗口与文档页签共用的只读视觉参数。
pragma Singleton

import QtQuick

QtObject {
    readonly property color colorPanel: "#ffffff"
    readonly property color colorTransparent: "transparent"
    // 视觉规范（task 039 / 034「视觉规范」）：窗口底色、面板、交替底色与分隔线。
    readonly property color colorWindow: "#f3f4f6"
    readonly property color colorPanelAlt: "#f7f8fa"
    readonly property color colorPanelLine: "#d5d8de"
    readonly property color colorText: "#1f2328"
    // 对白色面板 6.1:1、对 colorWindow 5.6:1、对选中底色 4.9:1（task 039 复核）。
    readonly property color colorTextMuted: "#59636e"
    // 占位文字与已填写内容明显区分（task 035 / 034 Q 节）；对白色面板 3.0:1，
    // 为 4.5:1 规则的记录例外：占位只作示例，字段标签另以 colorText 常显。
    readonly property color colorTextPlaceholder: "#8c959f"
    readonly property color colorIcon: "#4a4a4a"
    readonly property color colorHover: "#e5f1fb"
    readonly property color colorAccent: "#2563eb"
    readonly property color colorFocus: colorAccent
    readonly property color colorSelectionBg: "#dbe7fd"
    // 状态文字对白色 / colorWindow 均 ≥ 4.5:1（task 039 复核）：警告 5.4 / 4.9，
    // 错误 5.6 / 5.1，成功 5.1 / 4.6；状态同时配图标，不只靠颜色。
    readonly property color colorWarning: "#9a5b00"
    readonly property color colorError: "#c62828"
    readonly property color colorSuccess: "#1a7f37"
    readonly property color colorCloseHover: "#e81123"
    readonly property color colorDocumentBand: "#e7e4e1"
    readonly property color colorDocumentBandLine: "#c8c4c0"
    readonly property color colorDocumentHover: "#f0eeec"
    readonly property color colorDocumentClosePressed: "#d5e3ef"
    readonly property color colorDialogPrimary: "#e5f1fb"
    readonly property color colorDialogPrimaryBorder: "#5d9fc7"
    // 字族按平台集中声明（034 视觉规范）；缺失时由 Qt 回退到系统字体。
    readonly property string fontFamily: Qt.platform.os === "windows" ? "Microsoft YaHei UI" : Qt.platform.os === "osx" ? "PingFang SC" : "Noto Sans CJK SC"
    // 数值列启用等宽数字（tabular figures），字体不支持该特性时按原字形显示。
    readonly property var fontFeaturesNumeric: ({
            "tnum": 1
        })
    // 字号阶梯：11 说明 / 状态栏，12 属性标签和表格，13 正文和控件，14 分组标题，16 对话框标题。
    readonly property int fontCaption: 11
    readonly property int fontSmall: 12
    readonly property int fontBody: 13
    readonly property int fontHeading: 14
    readonly property int fontTitle: 16
    readonly property int spacingTiny: 2
    readonly property int spacingXSmall: 4
    readonly property int spacingMedium: 8
    readonly property int spacingLarge: 12
    readonly property int spacingXLarge: 16
    readonly property int spacingXXLarge: 24
    readonly property int iconSizeSmall: 16
    readonly property int iconSizeDefault: 18
    // 工具栏与播放栏按钮 28 px；工具栏图标 20 px，播放栏图标 16 px。
    readonly property int toolButtonSize: 28
    readonly property int toolbarIconSize: 20
    readonly property int iconSizeCompact: 12
    readonly property int radiusSmall: 4
    readonly property int borderWidth: 1
    readonly property int focusBorderWidth: 2
    readonly property int radiusDialog: 6
    readonly property int comboBoxHeight: 26
    readonly property int controlHeight: 26
    readonly property int paneCloseSize: 20
    readonly property int titlebarHeight: 30
    readonly property int documentTabRadius: 8
    readonly property int documentTabCloseRadius: 5
    readonly property int documentTabBottomLineHeight: 3
    readonly property int documentTabWidth: 130
    readonly property int documentTabBarHeight: 39
    readonly property int documentTabContentInset: 11
    readonly property int documentTabCloseSize: 19
    readonly property int documentTabCloseRightInset: 3
    readonly property int dialogActionWidth: 92
    readonly property int dialogActionSpacing: 8
    // 对话框内容区四边统一 16 px（task 039）。
    readonly property int dialogContentPadding: spacingXLarge
    readonly property int documentTabDragThreshold: 4
    readonly property int documentTabAnimationDuration: 150
    readonly property int toolTipDelay: 600
    // 工作台布局（task 036）：底部面板标题行与状态栏同高；视口列保底高度。
    readonly property int panelHeaderHeight: 30
    readonly property int viewportMinimumHeight: 240
    readonly property int viewportMinimumWidth: 320
    readonly property int statusFieldMaximumWidth: 360
    readonly property int statusProgressWidth: 120
    readonly property int qualityCellWidth: 240
    readonly property int taskNameWidth: 96
    readonly property int playbackTimeLabelMaximumWidth: 200
    readonly property real disabledOpacity: 0.4
    // 对象树与属性面板（task 037）：行高、每级缩进、属性标签列宽、选中行底色。
    readonly property int treeRowHeight: 22
    readonly property int treeIndent: 16
    readonly property int propertyLabelWidth: 96
    // 视口深色画布（task 038）：与 VTK 渐变背景同色，无文档时的引导页沿用。
    readonly property color colorViewportTop: "#2b3442"
    readonly property color colorViewportBottom: "#11161d"
    readonly property color colorViewportText: "#e6e9ee"
    readonly property color colorViewportTextMuted: "#9aa4b2"
    readonly property color colorViewportHover: "#3a4556"
    readonly property color colorViewportLine: "#5a6678"
    readonly property int viewportEmptyStateWidth: 360
    readonly property int viewportRecentProjectLimit: 5
    readonly property int windowDefaultWidth: 1280
    readonly property int windowDefaultHeight: 800
    readonly property int windowMinimumWidth: 1280
    readonly property int windowMinimumHeight: 720
    readonly property int newProjectDialogWidth: 620
    readonly property int newProjectDialogMinimumWidth: 520
    readonly property int newProjectDialogMinimumHeight: 320
    readonly property int importDialogWidth: 680
    readonly property int importDialogMinimumWidth: 580
    readonly property int importDialogMinimumHeight: 240

    // 消息级别（task 039）："" 普通 / "info" / "success" / "warning" / "error"。
    // 状态不只靠颜色：每个级别同时配图标，文字由调用方给出。
    function statusColor(level: string): color {
        return level === "error" ? colorError : level === "warning" ? colorWarning : level === "success" ? colorSuccess : colorTextMuted;
    }
    function statusIcon(level: string): string {
        return level.length > 0 ? "status-" + level : "";
    }
}
