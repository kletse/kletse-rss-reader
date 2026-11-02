// A short message at the bottom; errors stay up longer, to be read.
import QtQuick

Item {
    id: toast

    required property var ui
    readonly property var model: ui.model

    Rectangle {
        visible: toast.model.toastMessage !== ""
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 16
        width: Math.min(message.implicitWidth, toast.width - 32 - 28) + 28
        height: message.implicitHeight + 16
        color: toast.model.bgLight
        border.color: toast.model.toastError ? toast.model.red : toast.model.accent
        radius: toast.ui.radius

        Text {
            id: message
            anchors.centerIn: parent
            width: Math.min(implicitWidth, toast.width - 32 - 28)
            text: toast.model.toastMessage
            color: toast.model.fgBright
            wrapMode: Text.Wrap
            font.family: toast.ui.uiFont
            font.pixelSize: 14
        }
    }

    Timer {
        id: timer
        interval: toast.model.toastError ? 6000 : 2200
        onTriggered: toast.model.dismissToast(toast.model.toastSerial)
    }
    Connections {
        target: toast.model
        function onToastSerialChanged() { timer.restart() }
    }
}
