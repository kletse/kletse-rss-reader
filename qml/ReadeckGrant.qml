// The way into Readeck that needs no token: what the core says, and its buttons (src/grant.rs). The code and the address
// come from the server, so nothing here is ever taken for rich text.
import QtQuick

Column {
    id: g

    required property var ui
    readonly property var model: ui.model
    readonly property var view: JSON.parse(g.model.readeckGrant)
    /// What's in the Readeck URL field.
    property string address: ""

    function press(row) {
        g.model.readeckRow(row, g.address)
    }

    spacing: 8
    topPadding: 12
    Component.onCompleted: g.ui.grantPanel = g

    Text {
        width: parent.width
        text: g.view.text
        textFormat: Text.PlainText
        color: g.view.failed ? g.model.red : g.model.fgDim
        lineHeight: 1.3
        wrapMode: Text.Wrap
        font.family: g.ui.uiFont
        font.pixelSize: 12
    }
    TextEdit {
        visible: g.view.code !== ""
        width: parent.width
        text: g.view.code
        textFormat: TextEdit.PlainText
        readOnly: true
        selectByMouse: true
        activeFocusOnPress: false
        color: g.model.fgBright
        selectionColor: g.model.accent
        font.family: g.ui.uiFont
        font.pixelSize: 24
        font.weight: Font.Bold
        font.letterSpacing: 2
    }
    TextEdit {
        visible: g.view.address !== ""
        width: parent.width
        text: g.view.address
        textFormat: TextEdit.PlainText
        readOnly: true
        selectByMouse: true
        activeFocusOnPress: false
        color: g.model.fg
        selectionColor: g.model.accent
        wrapMode: TextEdit.Wrap
        font.family: g.ui.uiFont
        font.pixelSize: 14
    }
    Text {
        visible: text !== ""
        width: parent.width
        text: g.view.note
        textFormat: Text.PlainText
        color: g.model.fgDim
        lineHeight: 1.3
        wrapMode: Text.Wrap
        font.family: g.ui.uiFont
        font.pixelSize: 12
    }
    Flow {
        visible: g.view.rows.length > 0
        width: parent.width
        spacing: 8
        Repeater {
            model: g.view.rows
            delegate: Button {
                required property var modelData
                required property int index
                ui: g.ui
                primary: index === 0
                text: modelData.label
                onClicked: g.press(index)
            }
        }
    }
}
