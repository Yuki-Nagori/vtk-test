// 共用工具按钮：图文、悬停和键盘焦点由同一内容区域承载。
import QtQuick
import QtQuick.Controls

ToolButton {
    id: button
    hoverEnabled: true

    property int controlHeight: Theme.controlHeight
    property int contentPadding: Theme.spacingXSmall
    property real cornerRadius: Theme.radiusSmall
    property color contentColor: Theme.colorIcon
    property color hoverColor: Theme.colorHover
    property color pressedColor: Theme.colorTransparent
    property color borderColor: Theme.colorTransparent
    property bool primaryAction: false
    property string iconName: ""
    property bool preserveIconColors: false
    // 纯图标按钮由宿主提供中文名称，供读屏与悬停提示使用。
    property string accessibleName: text
    property int iconSize: Theme.iconSizeDefault

    Accessible.name: accessibleName
    ToolTip.visible: hovered && text === "" && accessibleName !== ""
    ToolTip.text: accessibleName
    ToolTip.delay: Theme.toolTipDelay

    font.family: Theme.fontFamily
    font.pixelSize: Theme.fontBody
    implicitHeight: controlHeight
    leftPadding: contentPadding
    rightPadding: contentPadding
    topPadding: 0
    bottomPadding: 0
    spacing: Theme.spacingXSmall
    // 统一弱化整个内容；禁用事件拦截沿用 ToolButton。
    opacity: enabled ? 1 : Theme.disabledOpacity

    background: Rectangle {
        implicitWidth: button.controlHeight
        // 强调状态不依赖悬停显示；键盘焦点使用独立轮廓。
        color: {
            if (button.highlighted) {
                return Theme.colorPanel;
            }
            if (button.primaryAction) {
                return Theme.colorDialogPrimary;
            }
            if (button.enabled && button.pressed && button.pressedColor.a > 0) {
                return button.pressedColor;
            }
            if (button.enabled && (button.hovered || button.visualFocus)) {
                return button.hoverColor;
            }
            return Theme.colorTransparent;
        }
        border.width: {
            if (button.visualFocus) {
                return Theme.focusBorderWidth;
            }
            return button.borderColor.a > 0 ? Theme.borderWidth : 0;
        }
        border.color: button.visualFocus ? Theme.colorFocus : button.borderColor
        radius: button.cornerRadius
    }

    contentItem: Item {
        implicitWidth: content.implicitWidth
        implicitHeight: content.implicitHeight

        Row {
            id: content
            objectName: "toolButtonContent"
            anchors.verticalCenter: parent.verticalCenter
            anchors.horizontalCenter: parent.horizontalCenter
            // 避免连续居中取整让普通字重的分数像素宽度偏离按钮中心。
            anchors.alignWhenCentered: false
            spacing: button.spacing

            ThemedIcon {
                anchors.verticalCenter: parent.verticalCenter
                visible: button.iconName !== ""
                name: button.iconName
                iconSize: button.iconSize
                color: button.contentColor
                preserveSourceColors: button.preserveIconColors
            }
            ThemedLabel {
                anchors.verticalCenter: parent.verticalCenter
                visible: button.text !== ""
                text: button.text
                textSize: button.font.pixelSize
                font.weight: button.font.weight
                textColor: button.contentColor
            }
        }
    }
}
