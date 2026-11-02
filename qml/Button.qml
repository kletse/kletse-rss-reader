// A button in the theme's colours: outlined, the accent on hover; `primary` fills it with the accent.
// An icon is a glyph (`glyph`) or a 24×24 SVG path drawn as an outline in the accent colour (`iconPath`).
import QtQuick
import QtQuick.Shapes
import QtQuick.Controls.Basic as T

T.Button {
    id: b

    required property var ui
    property bool primary: false
    property string glyph: ""
    property string iconPath: ""
    property string tip: ""

    focusPolicy: Qt.NoFocus
    hoverEnabled: true
    padding: 0
    leftPadding: 10
    rightPadding: 10
    topPadding: 4
    bottomPadding: 4
    font.family: ui.uiFont
    font.pixelSize: 14
    opacity: enabled ? 1 : 0.6

    T.ToolTip.visible: hovered && tip !== ""
    T.ToolTip.text: tip
    T.ToolTip.delay: 600

    // Sized from the parts' natural widths only; the label elides when the button is given less room.
    contentItem: Item {
        readonly property real iconWidth: (b.glyph !== "" ? glyphText.implicitWidth + 6 : 0) + (b.iconPath !== "" ? 20 : 0)
        implicitWidth: iconWidth + label.implicitWidth
        implicitHeight: Math.max(label.implicitHeight, 14)

        Row {
            anchors.verticalCenter: parent.verticalCenter
            spacing: 6
            Text {
                id: glyphText
                visible: b.glyph !== ""
                anchors.verticalCenter: parent.verticalCenter
                text: b.glyph
                color: b.primary ? b.ui.model.bg : b.ui.model.accent
                font: b.font
            }
            Shape {
                visible: b.iconPath !== ""
                anchors.verticalCenter: parent.verticalCenter
                width: 14
                height: 14
                preferredRendererType: Shape.CurveRenderer
                ShapePath {
                    scale: Qt.size(14 / 24, 14 / 24)
                    strokeColor: b.ui.model.accent
                    strokeWidth: 2
                    fillColor: "transparent"
                    joinStyle: ShapePath.RoundJoin
                    capStyle: ShapePath.RoundCap
                    PathSvg { path: b.iconPath }
                }
            }
            Text {
                id: label
                anchors.verticalCenter: parent.verticalCenter
                width: Math.min(implicitWidth, b.contentItem.width - b.contentItem.iconWidth)
                text: b.text
                elide: Text.ElideRight
                color: b.primary ? b.ui.model.bg : (b.hovered ? b.ui.model.fgBright : b.ui.model.fg)
                font: b.font
            }
        }
    }

    background: Rectangle {
        radius: b.ui.radius
        color: b.primary ? b.ui.model.accent : "transparent"
        border.width: 1
        border.color: b.primary || b.hovered ? b.ui.model.accent : b.ui.model.bgLight
        opacity: b.primary && b.hovered ? 0.9 : 1
    }

    HoverHandler { cursorShape: b.enabled ? Qt.PointingHandCursor : Qt.BusyCursor }
}
