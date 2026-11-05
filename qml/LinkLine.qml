// The link just opened, as text. `xdg-open` says it worked whether or not a browser came up, so the address stays here
// to take by hand.
import QtQuick

Column {
    id: l

    required property var ui

    visible: ui.model.shownLink !== ""
    bottomPadding: visible ? 16 : 0
    spacing: 4

    Text {
        width: parent.width
        text: "If your browser didn't open, here's the address:"
        color: l.ui.model.fgDim
        wrapMode: Text.Wrap
        font.family: l.ui.uiFont
        font.pixelSize: 12
    }
    Item {
        width: parent.width
        height: Math.max(address.height, copy.height)

        TextEdit {
            id: address
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width - copy.width - 8
            text: l.ui.model.shownLink
            wrapMode: TextEdit.WrapAnywhere
            readOnly: true
            selectByMouse: true
            activeFocusOnPress: false
            color: l.ui.model.fg
            selectionColor: l.ui.model.selection
            selectedTextColor: l.ui.model.fgBright
            font.family: l.ui.uiFont
            font.pixelSize: 14
        }
        Button {
            id: copy
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            ui: l.ui
            text: "Copy"
            onClicked: {
                address.selectAll()
                address.copy()
                address.deselect()
            }
        }
    }
}
