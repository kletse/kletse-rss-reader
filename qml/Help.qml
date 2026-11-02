// The keys, from the same table the keymaps are checked against (src/keys.rs).
import QtQuick

Panel {
    id: help

    readonly property var lines: {
        try {
            return JSON.parse(ui.model.helpLines)
        } catch (e) {
            return []
        }
    }

    title: "Keys"
    onBackdrop: ui.model.closeHelp()

    Grid {
        x: 14
        width: parent.width - 28
        topPadding: 14
        bottomPadding: 14
        columns: 2
        columnSpacing: 20
        rowSpacing: 6

        Repeater {
            model: help.lines.length * 2
            delegate: Text {
                required property int index
                readonly property bool isKey: index % 2 === 0
                width: isKey ? implicitWidth : help.panelWidth - 28 - 20 - help.keyWidth - 2
                text: help.lines[Math.floor(index / 2)][isKey ? 0 : 1]
                color: isKey ? help.ui.model.accent : help.ui.model.fg
                wrapMode: isKey ? Text.NoWrap : Text.Wrap
                font.family: help.ui.uiFont
                font.pixelSize: 14
            }
        }
    }

    // The widest key label, so the actions line up and wrap in their own column.
    FontMetrics {
        id: metrics
        font.family: help.ui.uiFont
        font.pixelSize: 14
    }
    readonly property real keyWidth: lines.reduce((w, line) => Math.max(w, metrics.advanceWidth(line[0])), 0)
}
