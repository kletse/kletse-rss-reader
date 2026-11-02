// One settings field: a label over a text input in the theme's colours.
import QtQuick
import QtQuick.Controls.Basic as T

Column {
    id: f

    required property var ui
    property string label
    property string placeholder
    property bool secret: false
    property alias text: input.text
    property alias input: input
    signal accepted()

    spacing: 4

    Text {
        text: f.label
        color: f.ui.model.fgDim
        font.family: f.ui.uiFont
        font.pixelSize: 12
    }

    T.TextField {
        id: input
        width: f.width
        placeholderText: f.placeholder
        placeholderTextColor: f.ui.model.muted
        echoMode: f.secret ? TextInput.Password : TextInput.Normal
        inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhNoPredictiveText | (f.secret ? Qt.ImhSensitiveData : Qt.ImhUrlCharactersOnly)
        color: f.ui.model.fgBright
        selectionColor: f.ui.model.selection
        selectedTextColor: f.ui.model.fgBright
        font.family: f.ui.uiFont
        font.pixelSize: 14
        leftPadding: 10
        rightPadding: 10
        topPadding: 8
        bottomPadding: 8
        background: Rectangle {
            color: f.ui.model.bgDark
            border.color: input.activeFocus ? f.ui.model.accent : f.ui.model.bgLight
            radius: f.ui.radius
        }
        onAccepted: f.accepted()
    }
}
