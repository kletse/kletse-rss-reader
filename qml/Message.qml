// "Loading…", "All caught up": centred, a fifth of the way down.
import QtQuick

Column {
    id: m

    required property var ui
    property string title: ""
    property string text: ""
    property bool logo: false

    topPadding: ui.height * 0.2
    spacing: 16

    Logo {
        visible: m.logo
        anchors.horizontalCenter: parent.horizontalCenter
        color: m.ui.model.accent
    }
    Text {
        visible: m.title !== ""
        width: m.width
        horizontalAlignment: Text.AlignHCenter
        text: m.title
        color: m.ui.model.fg
        font.family: m.ui.headingFont
        font.pixelSize: 28
        font.weight: Font.Bold
        wrapMode: Text.Wrap
    }
    Text {
        width: m.width
        horizontalAlignment: Text.AlignHCenter
        text: m.text
        color: m.ui.model.fgDim
        font.family: m.ui.uiFont
        font.pixelSize: 14
        wrapMode: Text.Wrap
    }
}
