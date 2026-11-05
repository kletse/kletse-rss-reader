// The fields of a form, as the table in src/providers.rs lists them. What's typed goes into `values`, which belongs to
// whoever holds the form and has every field, on screen or not.
import QtQuick

Column {
    id: list

    required property var ui
    /// `[{ key, label, placeholder, secret, help }, …]`
    property var fields: []
    property var values: ({})
    signal accepted()

    spacing: 10

    // The first empty field, or the last.
    function focusFirst() {
        const rows = []
        for (let i = 0; i < repeater.count; i++)
            rows.push(repeater.itemAt(i))
        const first = rows.find(r => r.field.text === "") ?? rows[rows.length - 1]
        if (first)
            first.field.input.forceActiveFocus()
    }

    onValuesChanged: {
        for (let i = 0; i < repeater.count; i++)
            repeater.itemAt(i).load()
    }

    Repeater {
        id: repeater
        model: list.fields
        delegate: Column {
            id: row
            required property var modelData
            readonly property alias field: field

            function load() {
                field.text = list.values[row.modelData.key] ?? ""
            }

            width: list.width
            spacing: 4

            Field {
                id: field
                ui: list.ui
                width: parent.width
                label: row.modelData.label
                placeholder: row.modelData.placeholder
                secret: row.modelData.secret
                onAccepted: list.accepted()
                onTextChanged: list.values[row.modelData.key] = text
            }
            Text {
                visible: text !== ""
                width: parent.width
                text: row.modelData.help
                color: list.ui.model.fgDim
                lineHeight: 1.3
                wrapMode: Text.Wrap
                font.family: list.ui.uiFont
                font.pixelSize: 12
            }
            Component.onCompleted: load()
        }
    }
}
