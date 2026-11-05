// The bar along the top: the selection (opens the picker), a History, Search or Starred badge, where you are (with
// a star when the article is starred), and buttons.
import QtQuick

Rectangle {
    id: bar

    required property var ui
    readonly property var model: ui.model

    height: 40
    color: model.bg

    // In settings the bar keeps its height but shows nothing, as before.
    Item {
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        visible: bar.model.screen !== "settings"

        Row {
            id: left
            anchors.verticalCenter: parent.verticalCenter
            spacing: 12

            Button {
                ui: bar.ui
                logo: true
                text: bar.model.selectionLabel !== "" ? bar.model.selectionLabel : "Feeds"
                // The samples' button below needs the room.
                width: Math.min(implicitWidth, bar.model.samples ? Math.max(100, bar.width * 0.2) : bar.width * 0.4)
                tip: "Feeds (p)"
                onClicked: bar.model.press("picker")
            }
            Rectangle {
                visible: bar.model.history || bar.model.searching || bar.model.starredList
                anchors.verticalCenter: parent.verticalCenter
                width: badge.implicitWidth + 12
                height: badge.implicitHeight + 2
                color: "transparent"
                border.color: bar.model.accent
                radius: bar.ui.radius
                Text {
                    id: badge
                    anchors.centerIn: parent
                    text: bar.model.searching ? "SEARCH" : bar.model.starredList ? "STARRED" : "HISTORY"
                    color: bar.model.accent
                    font.family: bar.ui.uiFont
                    font.pixelSize: 11
                    font.letterSpacing: 0.9
                }
            }
            // The samples aren't your feeds, and this is the way out of them.
            Button {
                visible: bar.model.samples
                ui: bar.ui
                text: bar.width >= 860 ? "Sample articles · Connect" : "Samples · Connect"
                tip: "Connect your server (,)"
                onClicked: bar.model.press("settings")
            }
        }

        Text {
            id: position
            visible: !bar.model.samples || bar.width >= 640
            anchors.centerIn: parent
            text: bar.model.position
            color: bar.model.fgDim
            font.family: bar.ui.uiFont
            font.pixelSize: 14
        }
        // The article on screen is starred (*).
        Text {
            visible: bar.model.starred && bar.model.screen === "article"
            anchors.right: position.left
            anchors.rightMargin: 6
            anchors.baseline: position.baseline
            text: "★"
            color: bar.model.accent
            font.family: bar.ui.uiFont
            font.pixelSize: 14
        }

        Row {
            anchors.verticalCenter: parent.verticalCenter
            anchors.right: parent.right
            spacing: 6

            Button {
                ui: bar.ui
                visible: bar.model.readeck && bar.width >= 560
                iconPath: "M6 3h12v18l-6-4-6 4z M12 7v6 M9 10h6"
                text: "Readeck"
                tip: "Save to Readeck (b)"
                onClicked: bar.model.press("readeck")
            }
            Button { ui: bar.ui; text: "▲"; tip: "Previous (k)"; onClicked: bar.model.press("prev") }
            Button { ui: bar.ui; text: "▼"; tip: "Next, mark read (j)"; onClicked: bar.model.press("next") }
        }
    }

    Rectangle {
        anchors.bottom: parent.bottom
        width: parent.width
        height: 1
        color: bar.model.bgLight
    }
}
