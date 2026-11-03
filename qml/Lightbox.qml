// The image viewer (`v`, or a click on an image): one of the article's images over the whole window, as large as
// fits but at most twice its own size. ← / → go between them, o opens one in the browser (src/app.rs takes the
// keys); a click or Esc closes it.
import QtQuick

Item {
    id: box

    required property var ui
    readonly property var model: ui.model

    Rectangle {
        anchors.fill: parent
        color: box.model.bgDark
        opacity: 0.94
    }
    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: box.model.closeViewer()
    }

    Image {
        id: img
        anchors.horizontalCenter: parent.horizontalCenter
        y: 24 + (parent.height - 48 - footer.height - height) / 2
        // The same source and fill mode as in the article, so Qt's image cache has it already.
        source: box.model.viewerSrc
        asynchronous: true
        cache: true
        fillMode: Image.PreserveAspectFit
        smooth: true
        mipmap: true
        width: Math.min(parent.width - 48, implicitWidth * 2)
        height: Math.min(parent.height - 48 - footer.height, implicitHeight * 2)
    }

    Text {
        anchors.centerIn: img
        visible: img.status !== Image.Ready
        text: img.status === Image.Error ? "The image didn't load" : "Loading…"
        color: box.model.fgDim
        font.family: box.ui.uiFont
        font.pixelSize: 14
    }

    Column {
        id: footer
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 16
        x: 24
        width: parent.width - 48
        spacing: 4

        Text {
            visible: box.model.viewerAlt !== ""
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            text: box.model.viewerAlt
            textFormat: Text.PlainText
            color: box.model.fg
            wrapMode: Text.Wrap
            maximumLineCount: 3
            elide: Text.ElideRight
            font.family: box.ui.uiFont
            font.pixelSize: 14
        }
        Text {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            text: [box.model.viewerPosition, box.model.viewerPosition !== "" ? "← / → between images" : "",
                   "o opens it in the browser", "Esc closes"].filter(p => p !== "").join(" · ")
            color: box.model.fgDim
            wrapMode: Text.Wrap
            font.family: box.ui.uiFont
            font.pixelSize: 12
        }
    }
}
