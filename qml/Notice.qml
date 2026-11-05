// What was wrong with the config file, with a yellow edge; nothing when there's nothing to say.
import QtQuick

Item {
    id: n

    required property var ui

    visible: ui.model.settingsProblem !== ""
    height: visible ? notice.height + 22 : 0

    Rectangle {
        width: 3
        height: notice.height
        color: n.ui.model.yellow
    }
    Text {
        id: notice
        x: 15
        width: parent.width - 15
        topPadding: 2
        bottomPadding: 2
        text: n.ui.model.settingsProblem
        color: n.ui.model.fg
        lineHeight: 1.4
        wrapMode: Text.WrapAnywhere
        font.family: n.ui.uiFont
        font.pixelSize: 14
    }
}
