// An overlay: the window dimmed, and a bordered panel near the top with a title row and its content.
import QtQuick

Item {
    id: overlay

    required property var ui
    property string title: ""
    /// Extra things on the title row (buttons), after the title.
    default property alias content: body.data
    property alias actions: actionRow.data
    property real panelWidth: Math.min(560, width - 32)
    property real panelY: height * 0.08
    readonly property real panelHeight: panel.height
    /// A click outside the panel.
    signal backdrop()

    Rectangle {
        anchors.fill: parent
        color: overlay.ui.model.bgDark
        opacity: 0.7
        MouseArea {
            anchors.fill: parent
            onClicked: overlay.backdrop()
        }
    }

    Rectangle {
        id: panel
        x: (overlay.width - width) / 2
        y: overlay.panelY
        width: overlay.panelWidth
        height: Math.min(column.implicitHeight + 2, overlay.height * 0.8)
        color: overlay.ui.model.bg
        border.color: overlay.ui.model.accent
        radius: overlay.ui.radius
        clip: true
        // Clicks on the panel don't reach the backdrop.
        MouseArea { anchors.fill: parent }

        Column {
            id: column
            x: 1
            y: 1
            width: parent.width - 2

            Item {
                visible: overlay.title !== ""
                width: parent.width
                height: 40
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    x: 14
                    text: overlay.title
                    color: overlay.ui.model.accent
                    font.family: overlay.ui.uiFont
                    font.pixelSize: 14
                }
                Row {
                    id: actionRow
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.right: parent.right
                    anchors.rightMargin: 14
                    spacing: 8
                }
                Rectangle {
                    anchors.bottom: parent.bottom
                    width: parent.width
                    height: 1
                    color: overlay.ui.model.bgLight
                }
            }

            Item {
                id: body
                width: parent.width
                height: childrenRect.height
            }
        }
    }
}
