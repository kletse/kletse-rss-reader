// A yes/no question; Enter / y says yes, Esc / n no (src/app.rs takes the keys).
import QtQuick

Panel {
    id: confirm

    panelWidth: Math.min(480, width - 32)
    // Over the picker (Shift+A on a row), lower down, clear of the top of the list it's about.
    panelY: ui.model.pickerShown ? Math.max(height * 0.08, (height - panelHeight) / 2) : height * 0.08

    Column {
        x: 20
        width: parent.width - 40
        topPadding: 16
        bottomPadding: 16
        spacing: 12

        Text {
            width: parent.width
            text: confirm.ui.model.confirmText
            color: confirm.ui.model.fg
            wrapMode: Text.Wrap
            font.family: confirm.ui.uiFont
            font.pixelSize: 14
        }
        Text {
            width: parent.width
            text: "Enter / y to confirm · Esc / n to cancel"
            color: confirm.ui.model.fgDim
            wrapMode: Text.Wrap
            font.family: confirm.ui.uiFont
            font.pixelSize: 12
        }
    }
}
