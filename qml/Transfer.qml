// Importing or exporting a feed list. The core says what's on screen (`app::TransferView`); the file comes from a
// dialog, which the core asks for when the main button or Enter says so. Esc and a click beside it close this, and
// leave what's running to finish.
import QtQuick

Panel {
    id: t

    readonly property var view: {
        try {
            return JSON.parse(t.ui.model.transfer)
        } catch (e) {
            return null
        }
    }
    readonly property string stage: view?.stage ?? ""

    title: view?.title ?? ""
    panelWidth: Math.min(520, width - 32)
    onBackdrop: t.ui.model.cancelTransfer()

    Column {
        x: 20
        width: parent.width - 40
        topPadding: 16
        bottomPadding: 16
        spacing: 12

        Text {
            width: parent.width
            text: t.view?.text ?? ""
            textFormat: Text.PlainText
            color: t.stage === "done" ? t.ui.model.fg : t.ui.model.fgDim
            lineHeight: 1.3
            wrapMode: Text.Wrap
            font.family: t.ui.uiFont
            font.pixelSize: 14
        }
        Text {
            visible: text !== ""
            width: parent.width
            text: t.view?.error ?? ""
            textFormat: Text.PlainText
            color: t.ui.model.red
            lineHeight: 1.3
            wrapMode: Text.Wrap
            font.family: t.ui.uiFont
            font.pixelSize: 14
        }
        Row {
            anchors.right: parent.right
            spacing: 8
            Button {
                ui: t.ui
                text: t.stage === "asking" ? "Cancel" : "Close"
                onClicked: t.ui.model.cancelTransfer()
            }
            Button {
                ui: t.ui
                visible: t.stage === "asking"
                primary: true
                text: t.view?.action ?? ""
                onClicked: t.ui.model.transferAction()
            }
        }
    }
}
