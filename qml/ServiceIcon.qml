// A service's mark on first run: `icon` from the table in src/providers.rs, on a tile in the theme's colours. It's a letter
// and not a logo, as neither project's terms clearly allow a logo here (see AGENTS.md), and it mustn't look like one: this
// file is all that changes if that ever does.
import QtQuick

Rectangle {
    id: tile

    required property var ui
    property string mark: ""
    property bool current: false

    implicitWidth: 40
    implicitHeight: 40
    radius: tile.ui.radius
    color: tile.ui.model.bgLight
    border.width: tile.current ? 1 : 0
    border.color: tile.ui.model.accent

    Text {
        anchors.centerIn: parent
        text: tile.mark
        textFormat: Text.PlainText
        color: tile.current ? tile.ui.model.accent : tile.ui.model.fg
        font.family: tile.ui.uiFont
        font.pixelSize: 18
        font.weight: Font.DemiBold
    }
}
