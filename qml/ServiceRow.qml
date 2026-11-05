// A row of the list Connect opens on: the service's tile, its name and one line. Sized from its parts, like `Button`.
import QtQuick
import QtQuick.Controls.Basic as T

T.Button {
    id: r

    required property var ui
    property string label: ""
    property string line: ""
    property string mark: ""
    /// Where the arrow keys are.
    property bool current: false

    focusPolicy: Qt.NoFocus
    hoverEnabled: true
    padding: 10
    implicitHeight: Math.max(tile.implicitHeight, words.implicitHeight) + 2 * padding

    contentItem: Item {
        ServiceIcon {
            id: tile
            visible: r.mark !== ""
            anchors.verticalCenter: parent.verticalCenter
            ui: r.ui
            mark: r.mark
            current: r.current
        }
        Column {
            id: words
            anchors.verticalCenter: parent.verticalCenter
            x: tile.visible ? tile.implicitWidth + 12 : 0
            width: parent.width - x
            spacing: 2
            Text {
                width: parent.width
                text: r.label
                textFormat: Text.PlainText
                elide: Text.ElideRight
                color: r.ui.model.fgBright
                font.family: r.ui.uiFont
                font.pixelSize: 15
                font.weight: Font.DemiBold
            }
            Text {
                visible: text !== ""
                width: parent.width
                text: r.line
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                color: r.ui.model.fgDim
                font.family: r.ui.uiFont
                font.pixelSize: 13
            }
        }
    }

    background: Rectangle {
        radius: r.ui.radius
        color: "transparent"
        border.width: 1
        border.color: r.current || r.hovered ? r.ui.model.accent : r.ui.model.bgLight
    }

    HoverHandler { cursorShape: Qt.PointingHandCursor }
}
