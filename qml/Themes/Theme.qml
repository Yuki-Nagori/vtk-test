// 工作台、导入窗口与文档页签共用的只读视觉参数。
pragma Singleton

import QtQuick

QtObject {
    readonly property color colorPanel: "#ffffff"
    readonly property color colorTransparent: "transparent"
    readonly property color colorChrome: "#e9e9e9"
    readonly property color colorPanelLine: "#c9c9c9"
    readonly property color colorText: "#1c1c1c"
    readonly property color colorTextMuted: "#5a5a5a"
    readonly property color colorIcon: "#4a4a4a"
    readonly property color colorHover: "#e5f1fb"
    readonly property color colorFocus: "#2e7ce0"
    readonly property color colorError: "#c62828"
    readonly property color colorCloseHover: "#e81123"
    readonly property color colorDocumentBand: "#e7e4e1"
    readonly property color colorDocumentBandLine: "#c8c4c0"
    readonly property color colorDocumentHover: "#f0eeec"
    readonly property color colorDocumentClosePressed: "#d5e3ef"
    readonly property color colorDialogPrimary: "#e5f1fb"
    readonly property color colorDialogPrimaryBorder: "#5d9fc7"
    readonly property int fontTitle: 13
    readonly property int fontBody: 13
    readonly property int fontSmall: 12
    readonly property int spacingTiny: 2
    readonly property int spacingXSmall: 4
    readonly property int spacingSmall: 6
    readonly property int spacingMedium: 8
    readonly property int spacingLarge: 12
    readonly property int iconSizeSmall: 16
    readonly property int iconSizeDefault: 18
    readonly property int iconSizeCompact: 12
    readonly property int radiusSmall: 3
    readonly property int borderWidth: 1
    readonly property int focusBorderWidth: 2
    readonly property int comboBoxHeight: 30
    readonly property int controlHeight: 24
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
    readonly property int dialogActionSpacing: 5
    readonly property int documentTabDragThreshold: 4
    readonly property int documentTabAnimationDuration: 150
    readonly property int toolTipDelay: 600
    readonly property real disabledOpacity: 0.4
    readonly property int windowMinimumWidth: 1280
    readonly property int windowMinimumHeight: 720
    readonly property int newProjectDialogWidth: 620
    readonly property int newProjectDialogHeight: 360
    readonly property int newProjectDialogMinimumWidth: 520
    readonly property int newProjectDialogMinimumHeight: 320
    readonly property int importDialogWidth: 680
    readonly property int importDialogHeight: 430
    readonly property int importDialogMinimumWidth: 580
    readonly property int importDialogMinimumHeight: 360
}
