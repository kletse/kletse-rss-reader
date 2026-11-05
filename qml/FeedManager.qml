// The feeds panel: every feed and category, for changing them. The core says what's on screen (`manage::ManageView`):
// the rows, what can be done on the one the cursor's on, and the field on the stages that have one. Keys, clicks and
// what's typed go back to it, and it asks before anything is lost.
import QtQuick

Panel {
    id: m

    readonly property var model: ui.model
    readonly property var view: {
        try {
            return JSON.parse(m.model.manage)
        } catch (e) {
            return null
        }
    }
    readonly property string stage: view?.stage ?? ""
    readonly property var rows: view?.rows ?? []
    readonly property real rowHeight: Math.ceil(metrics.height) + 10

    title: view?.title ?? ""
    panelWidth: Math.min(560, width - 32)
    onBackdrop: m.model.cancelManage()

    FontMetrics {
        id: metrics
        font.family: m.ui.uiFont
        font.pixelSize: 14
    }

    // The field comes and goes with its stage, and takes the keyboard when it comes.
    Component {
        id: fieldComponent
        Field {
            id: field
            ui: m.ui
            label: m.view?.field?.label ?? ""
            text: m.view?.field?.value ?? ""
            onAccepted: m.model.submitManage(field.text)
            Component.onCompleted: {
                field.input.forceActiveFocus()
                field.input.selectAll()
            }
        }
    }

    Column {
        width: parent.width
        topPadding: 12
        bottomPadding: 12
        spacing: 10

        Text {
            visible: text !== ""
            x: 14
            width: parent.width - 28
            text: m.stage === "loading" && m.view?.error === "" ? "Loading…" : m.view?.text ?? ""
            textFormat: Text.PlainText
            color: m.model.fgDim
            lineHeight: 1.3
            wrapMode: Text.Wrap
            font.family: m.ui.uiFont
            font.pixelSize: 14
        }
        Loader {
            id: fieldLoader
            x: 14
            width: parent.width - 28
            active: m.stage === "field"
            sourceComponent: fieldComponent
            onLoaded: item.width = width
        }
        Text {
            visible: text !== ""
            x: 14
            width: parent.width - 28
            text: m.view?.error ?? ""
            textFormat: Text.PlainText
            color: m.model.red
            lineHeight: 1.3
            wrapMode: Text.Wrap
            font.family: m.ui.uiFont
            font.pixelSize: 14
        }
        ListView {
            id: list
            visible: m.rows.length > 0
            width: parent.width
            // From the rows rather than contentHeight, which depends on the height through the delegates made.
            height: visible ? Math.min(m.rows.length * m.rowHeight, m.height * 0.8 - 170) : 0
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: m.rows
            currentIndex: m.view?.cursor ?? 0
            highlightFollowsCurrentItem: false
            onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)
            onCountChanged: positionViewAtIndex(currentIndex, ListView.Contain)

            delegate: Rectangle {
                id: row
                required property var modelData
                required property int index
                readonly property bool current: index === (m.view?.cursor ?? 0)

                width: list.width
                height: m.rowHeight
                color: current ? m.model.selection : (hover.hovered ? m.model.bgLight : "transparent")

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    x: row.modelData.depth === 1 ? 32 : 14
                    width: parent.width - x - note.width - 26
                    text: row.modelData.label
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    color: row.current || row.modelData.depth === 0 ? m.model.fgBright : m.model.fg
                    font.family: m.ui.uiFont
                    font.pixelSize: 14
                }
                Text {
                    id: note
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.right: parent.right
                    anchors.rightMargin: 14
                    text: row.modelData.note
                    color: m.model.fgDim
                    font.family: m.ui.uiFont
                    font.pixelSize: 14
                }
                HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: m.model.manageRow(row.index) }
            }
        }
        Text {
            visible: text !== "" && (m.stage === "list" || m.stage === "working")
            x: 14
            width: parent.width - 28
            text: m.view?.notice ?? ""
            textFormat: Text.PlainText
            color: m.view?.worrying ? m.model.red : m.model.fgDim
            lineHeight: 1.3
            wrapMode: Text.Wrap
            font.family: m.ui.uiFont
            font.pixelSize: 14
        }
        Flow {
            x: 14
            width: parent.width - 28
            spacing: 8
            layoutDirection: Qt.RightToLeft

            Button {
                ui: m.ui
                visible: m.stage === "field"
                primary: true
                text: m.view?.field?.action ?? ""
                onClicked: m.model.submitManage(fieldLoader.item.text)
            }
            Button {
                ui: m.ui
                text: m.stage === "list" || m.stage === "loading" || m.stage === "working" ? "Close" : "Back"
                onClicked: m.model.cancelManage()
            }
            Repeater {
                // Laid out from the right, as the main button goes there.
                model: (m.view?.keys ?? []).slice().reverse()
                delegate: Button {
                    required property var modelData
                    ui: m.ui
                    text: modelData.label + " (" + modelData.key + ")"
                    onClicked: m.model.press(modelData.button)
                }
            }
        }
    }
}
