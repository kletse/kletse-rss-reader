// The logo in the theme's accent, so it follows the desktop: two dots and an O, the articles to come and the one you're
// reading. The sizes match packaging/logo, cropped to the mark.
import QtQuick

Item {
    id: logo

    required property color color
    readonly property real unit: width / 360

    implicitWidth: 120
    implicitHeight: width * 204 / 360

    Rectangle {
        x: 0
        anchors.verticalCenter: parent.verticalCenter
        width: 40 * logo.unit
        height: width
        radius: width / 2
        color: logo.color
        opacity: 0.5
    }
    Rectangle {
        x: 60 * logo.unit
        anchors.verticalCenter: parent.verticalCenter
        width: 64 * logo.unit
        height: width
        radius: width / 2
        color: logo.color
        opacity: 0.75
    }
    Rectangle {
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        width: 204 * logo.unit
        height: width
        radius: width / 2
        color: "transparent"
        border.color: logo.color
        border.width: 52 * logo.unit
    }
}
