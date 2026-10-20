// 对话框窗口约束与定位；打开条件和关闭守卫由具体业务窗口处理。
import QtQuick

Window {
    property Window ownerWindow
    color: Theme.colorTransparent
    modality: Qt.ApplicationModal
    flags: Qt.Dialog | Qt.FramelessWindowHint
    transientParent: ownerWindow
    function centerOnOwner() {
        if (ownerWindow) {
            x = ownerWindow.x + Math.round((ownerWindow.width - width) / 2);
            y = ownerWindow.y + Math.round((ownerWindow.height - height) / 2);
        }
    }
}
