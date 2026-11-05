// A titled box of fields, as an HTML fieldset draws it.
import QtQuick

Item {
    id: fs

    required property var ui
    property string title
    property string note
    default property alias fields: fieldColumn.data

    width: parent ? parent.width : 0
    height: box.height + legend.height / 2 + 18

    Rectangle {
        id: box
        y: legend.height / 2
        width: parent.width
        height: fieldColumn.height + legend.height / 2 + 18
        color: "transparent"
        border.color: fs.ui.model.bgLight
        radius: fs.ui.radius
    }
    // The legend sits on the border, as a fieldset's does.
    Rectangle {
        x: 10
        width: Math.min(legend.width + 12, fs.width - 20)
        height: legend.height
        color: fs.ui.model.bg
        Text {
            id: legend
            x: 6
            width: Math.min(implicitWidth, fs.width - 32)
            text: fs.title + (fs.note === "" ? "" : "<font color='" + fs.ui.model.fgDim + "'> · " + fs.note + "</font>")
            textFormat: Text.StyledText
            color: fs.ui.model.accent
            elide: Text.ElideRight
            font.family: fs.ui.uiFont
            font.pixelSize: 14
        }
    }
    Column {
        id: fieldColumn
        x: 16
        y: legend.height + 4
        width: parent.width - 32
        spacing: 10
    }
}
