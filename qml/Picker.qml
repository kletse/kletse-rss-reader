// The feeds/categories overlay: "All unread", categories, and their feeds with unread counts. In history mode the
// same, with read counts. The cursor and the rows come from the app; a click picks a row.
import QtQuick
import QtQuick.Controls.Basic as T

Panel {
    id: picker

    readonly property var model: ui.model
    readonly property var rows: {
        try {
            return JSON.parse(model.pickerRows)
        } catch (e) {
            return []
        }
    }

    readonly property real rowHeight: Math.ceil(metrics.height) + 10

    titleHeight: 32

    FontMetrics {
        id: metrics
        font.family: picker.ui.uiFont
        font.pixelSize: 14
    }

    onBackdrop: model.pickerBackdrop()

    // A word that lights up under the mouse, and when chosen stands on the title row's line in the accent colour.
    component ModeTab: Item {
        id: tab
        property string text
        property string tip
        property bool selected
        signal clicked()

        implicitWidth: tabLabel.implicitWidth + 12
        implicitHeight: picker.titleHeight
        T.ToolTip.visible: tabHover.hovered
        T.ToolTip.text: tip
        T.ToolTip.delay: 600

        Text {
            id: tabLabel
            anchors.verticalCenter: parent.verticalCenter
            text: tab.text
            color: tab.selected || tabHover.hovered ? picker.model.fgBright : picker.model.fgDim
            font.family: picker.ui.uiFont
            font.pixelSize: 14
            font.weight: Font.DemiBold
        }
        Rectangle {
            anchors.bottom: parent.bottom
            width: tabLabel.width
            height: 2
            color: tab.selected ? picker.model.accent : "transparent"
        }
        HoverHandler { id: tabHover; cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: tab.clicked() }
    }

    titleControls: [
        ModeTab {
            text: "Unread"
            selected: !picker.model.pickerHistory
            tip: "Unread items (h)"
            onClicked: picker.model.press("unread")
        },
        ModeTab {
            text: "History"
            selected: picker.model.pickerHistory
            tip: "Read items, newest first (h)"
            onClicked: picker.model.press("history")
        }
    ]

    actions: [
        Button {
            ui: picker.ui
            visible: picker.model.refreshes
            iconPath: "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8 M21 3v5h-5 M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16 M8 16H3v5"
            tip: "Refresh feeds (r)"
            onClicked: picker.model.press("refresh")
        },
        Button {
            ui: picker.ui
            iconPath: "M21 21l-4.3-4.3 M19 11a8 8 0 1 1-16 0a8 8 0 1 1 16 0z"
            tip: "Search all feeds (/)"
            onClicked: picker.model.press("search")
        }
    ]

    footer: [
        Button {
            ui: picker.ui
            visible: !picker.model.pickerHistory
            anchors.verticalCenter: parent.verticalCenter
            iconPath: "M3 16l4 4 4-4 M7 20V4 M21 8l-4-4-4 4 M17 4v16"
            text: picker.model.newestFirst ? "Newest first" : "Oldest first"
            tip: picker.model.newestFirst ? "Unread items newest first (s)" : "Unread items oldest first (s)"
            onClicked: picker.model.press("order")
        },
        // History has only the one order, so this says it rather than offering a choice.
        Text {
            visible: picker.model.pickerHistory
            anchors.verticalCenter: parent.verticalCenter
            text: "Newest first"
            color: picker.model.fgDim
            font.family: picker.ui.uiFont
            font.pixelSize: 14
        },
        Row {
            anchors.verticalCenter: parent.verticalCenter
            anchors.right: parent.right
            spacing: 8
            Button {
                ui: picker.ui
                iconPath: "M10 8h.01 M12 12h.01 M14 8h.01 M16 12h.01 M18 8h.01 M6 8h.01 M7 16h10 M8 12h.01 M2 6a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2z"
                tip: "Keys (?)"
                onClicked: picker.model.press("help")
            }
            Button {
                ui: picker.ui
                iconPath: "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z M15 12a3 3 0 1 1-6 0a3 3 0 1 1 6 0z"
                tip: picker.model.samples ? "Connect (,)" : "Settings (,)"
                onClicked: picker.model.press("settings")
            }
        }
    ]

    Text {
        visible: picker.model.pickerState !== "ready" || picker.rows.length === 0
        width: parent.width
        leftPadding: 14
        rightPadding: 14
        topPadding: 11
        bottomPadding: 11
        wrapMode: Text.Wrap
        text: picker.model.pickerState === "loading" ? "Loading…"
            : picker.model.pickerState === "error" ? picker.model.pickerError
            : picker.model.pickerHistory ? "Nothing read yet. Press h for unread items."
            : picker.model.refreshes ? "Nothing unread. Press r to refresh, or h for history."
            : "Nothing unread. Press h for history."
        color: picker.model.pickerState === "error" ? picker.model.red : picker.model.fgDim
        font.family: picker.ui.uiFont
        font.pixelSize: 14
    }

    ListView {
        id: list
        visible: picker.model.pickerState === "ready" && picker.rows.length > 0
        width: parent.width
        // From the rows rather than contentHeight, which depends on the height through the delegates made.
        height: visible ? Math.min(picker.rows.reduce((h, r, i) => h + picker.rowHeight + (r.depth === 0 && i > 0 ? 4 : 0), 12),
                                   picker.height * 0.8 - 82) : 0
        topMargin: 6
        bottomMargin: 6
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        model: picker.rows
        currentIndex: picker.model.pickerCursor
        highlightFollowsCurrentItem: false
        onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)
        onCountChanged: positionViewAtIndex(currentIndex, ListView.Contain)

        delegate: Rectangle {
            id: row
            required property var modelData
            required property int index
            readonly property bool current: index === picker.model.pickerCursor

            width: list.width
            height: picker.rowHeight + (modelData.depth === 0 && index > 0 ? 4 : 0)
            color: current ? picker.model.selection : (hover.hovered ? picker.model.bgLight : "transparent")

            Text {
                id: label
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 5
                x: row.modelData.depth === 1 ? 32 : 14
                width: parent.width - x - count.width - 26
                text: row.modelData.label
                textFormat: Text.PlainText
                elide: Text.ElideRight
                color: row.current || row.modelData.depth === 0 ? picker.model.fgBright : picker.model.fg
                font.family: picker.ui.uiFont
                font.pixelSize: 14
            }
            Text {
                id: count
                anchors.baseline: label.baseline
                anchors.right: parent.right
                anchors.rightMargin: 14
                // null: the server can't count these (read items on a Google Reader API server).
                text: row.modelData.count ?? ""
                color: picker.model.fgDim
                font.family: picker.ui.uiFont
                font.pixelSize: 14
            }
            HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
            TapHandler { onTapped: picker.model.chooseRow(row.index) }
        }
    }
}
