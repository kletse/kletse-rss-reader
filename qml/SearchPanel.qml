// The search box (`/`): Enter searches every feed, read and unread (src/app.rs `search`); Esc goes back.
// Loaded only while it's open, so the field starts from the last search and gives up the keyboard when it closes.
import QtQuick

Panel {
    id: box

    title: "Search"
    panelWidth: Math.min(480, width - 32)
    onBackdrop: ui.model.cancelSearch()

    Column {
        x: 20
        width: parent.width - 40
        topPadding: 16
        bottomPadding: 16
        spacing: 12

        Field {
            id: field
            ui: box.ui
            width: parent.width
            label: "Words to look for, in every feed"
            placeholder: "Search"
            text: box.ui.model.searchQuery
            onAccepted: box.ui.model.search(field.text)
        }
        Text {
            width: parent.width
            text: "Enter to search · Esc to cancel"
            color: box.ui.model.fgDim
            wrapMode: Text.Wrap
            font.family: box.ui.uiFont
            font.pixelSize: 12
        }
    }

    Component.onCompleted: {
        field.input.forceActiveFocus()
        field.input.selectAll()
    }
}
