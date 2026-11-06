// First run: the three things the first screen offers, the server form, and what to do with no server. It has the whole
// window, as there's nothing behind it yet. The core keeps the step and all the words (src/app.rs, src/providers.rs);
// a key or a click goes there, and Esc steps back. What's typed stays here, so stepping back doesn't lose it, while the
// fields themselves are made when Connect opens: a hidden field would keep the keyboard. Connect starts from an address
// and shows what the core says that server needs (`app::ConnectView`).
import QtQuick
import QtQuick.Controls.Basic as T

Rectangle {
    id: o

    required property var ui
    readonly property var model: ui.model
    readonly property var words: JSON.parse(o.model.firstRun)
    readonly property string step: o.model.onboardingStep
    // Every field's value by its key, on screen or not: a Readeck saved earlier stays what it was.
    property var values: ({})
    property string server: "miniflux"

    color: o.model.bg

    function submit() {
        o.model.submitSettings(o.server, JSON.stringify(o.values))
    }
    function back() {
        o.model.key("Escape", false, false, false, false, false, false, false, false)
    }
    // The saved values, once: see Settings.qml.
    function fill() {
        o.server = o.model.server
        o.values = JSON.parse(o.model.formValues)
    }
    Connections {
        target: o.model
        function onSettingsSerialChanged() { o.fill() }
        function onOnboardingStepChanged() { page.contentY = 0 }
    }
    Component.onCompleted: if (o.model.settingsReady) o.fill()

    Flickable {
        id: page
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.height
        boundsBehavior: Flickable.StopAtBounds
        clip: true
        T.ScrollBar.vertical: T.ScrollBar { width: 8 }

        Column {
            id: column
            x: (page.width - width) / 2
            width: Math.min(560, page.width - 32)
            topPadding: Math.max(24, o.height * 0.08)
            bottomPadding: 32
            spacing: 0

            Logo {
                color: o.model.accent
                visible: o.step === "welcome"
            }
            Text {
                width: parent.width
                topPadding: o.step === "welcome" ? 20 : 0
                bottomPadding: 8
                text: o.step === "welcome" ? o.words.welcome_title
                    : o.step === "connect" ? o.words.connect_title : o.words.no_server.title
                color: o.model.fgBright
                font.family: o.ui.headingFont
                font.pixelSize: 28
                font.weight: Font.Bold
                wrapMode: Text.Wrap
            }
            Text {
                width: parent.width
                bottomPadding: 24
                text: o.step === "welcome" ? o.words.welcome_text
                    : o.step !== "connect" ? o.words.no_server.text
                    : o.model.connectStage === "by_hand" ? o.words.by_hand_text : o.words.connect_text
                color: o.model.fgDim
                lineHeight: 1.4
                wrapMode: Text.Wrap
                font.family: o.ui.uiFont
                font.pixelSize: 14
            }
            // What was wrong with the config file, on whichever step you're at.
            Notice {
                ui: o.ui
                width: parent.width
            }
            Loader {
                width: parent.width
                sourceComponent: o.step === "connect" ? connect : o.step === "no_server" ? noServer : welcome
            }
        }
    }

    Component {
        id: welcome
        Column {
            spacing: 10
            Repeater {
                model: o.words.welcome
                delegate: Button {
                    required property var modelData
                    required property int index
                    ui: o.ui
                    width: parent.width
                    height: 38
                    quiet: modelData.quiet
                    primary: !quiet && index === o.model.onboardingCursor
                    current: index === o.model.onboardingCursor
                    text: modelData.label
                    onClicked: o.model.chooseOnboarding(index)
                }
            }
        }
    }

    // Connect's stages share what's under them: what was said, the rows, the buttons. The fields go with their stage.
    Component {
        id: connect
        Column {
            spacing: 0
            Loader {
                width: parent.width
                sourceComponent: o.model.connectStage === "by_hand" ? byHand : address
            }
            LinkLine {
                ui: o.ui
                width: parent.width
            }
            Text {
                width: parent.width
                height: Math.max(implicitHeight, 20)
                text: o.model.settingsError
                color: o.model.red
                lineHeight: 1.3
                wrapMode: Text.Wrap
                font.family: o.ui.uiFont
                font.pixelSize: 14
            }
            Flow {
                width: parent.width
                topPadding: 8
                spacing: 8
                Repeater {
                    model: JSON.parse(o.model.connectRows)
                    delegate: Button {
                        required property var modelData
                        required property int index
                        ui: o.ui
                        enabled: !o.model.settingsChecking
                        text: modelData.label
                        tip: modelData.url ?? ""
                        onClicked: o.model.chooseOnboarding(index)
                    }
                }
            }
            Row {
                anchors.right: parent.right
                topPadding: 12
                spacing: 8
                Button {
                    ui: o.ui
                    enabled: !o.model.settingsChecking
                    text: "Back"
                    tip: "Esc"
                    onClicked: o.back()
                }
                Button {
                    ui: o.ui
                    primary: true
                    enabled: !o.model.settingsChecking && !o.model.connectProbing
                    text: o.model.settingsChecking ? "Checking…" : o.model.connectProbing ? "Looking…" : o.model.connectAction
                    onClicked: o.submit()
                }
            }
        }
    }

    Component {
        id: byHand
        ServerForm {
            ui: o.ui
            values: o.values
            server: o.server
            onChosen: name => o.server = name
            onAccepted: o.submit()
        }
    }

    Component {
        id: address
        Column {
            id: stage
            readonly property var asked: JSON.parse(o.model.connectFields)
            spacing: 12
            bottomPadding: 12

            // What's new takes the keyboard: the first thing asked, or the box when nothing is.
            function focusFirst() {
                if (stage.asked.length > 0)
                    askedList.focusFirst()
                else
                    box.focusFirst()
            }
            onAskedChanged: Qt.callLater(stage.focusFirst)
            Component.onCompleted: Qt.callLater(stage.focusFirst)

            Connections {
                target: o
                function onValuesChanged() { Qt.callLater(stage.focusFirst) }
            }

            FieldList {
                id: box
                ui: o.ui
                width: parent.width
                fields: [o.words.address]
                values: o.values
                readOnly: o.model.settingsChecking
                onAccepted: o.submit()
                onEdited: (key, text) => o.model.addressEdited(text)
            }
            Text {
                visible: text !== ""
                width: parent.width
                text: o.model.connectText
                color: o.model.fg
                lineHeight: 1.4
                wrapMode: Text.Wrap
                font.family: o.ui.uiFont
                font.pixelSize: 14
            }
            Text {
                visible: text !== ""
                width: parent.width
                text: o.model.connectNote
                color: o.model.fgDim
                lineHeight: 1.3
                wrapMode: Text.Wrap
                font.family: o.ui.uiFont
                font.pixelSize: 12
            }
            FieldList {
                id: askedList
                ui: o.ui
                width: parent.width
                fields: stage.asked
                values: o.values
                readOnly: o.model.settingsChecking
                onAccepted: o.submit()
            }
        }
    }

    Component {
        id: noServer
        Column {
            id: panel
            readonly property var words: o.words.no_server
            spacing: 4

            Repeater {
                model: panel.words.offers
                delegate: Fieldset {
                    required property var modelData
                    required property int index
                    ui: o.ui
                    title: modelData.name
                    Text {
                        width: parent.width
                        text: modelData.price
                        color: o.model.fgBright
                        wrapMode: Text.Wrap
                        font.family: o.ui.uiFont
                        font.pixelSize: 14
                    }
                    Text {
                        width: parent.width
                        text: modelData.text
                        color: o.model.fg
                        lineHeight: 1.4
                        wrapMode: Text.Wrap
                        font.family: o.ui.uiFont
                        font.pixelSize: 14
                    }
                    Button {
                        ui: o.ui
                        current: index === o.model.onboardingCursor
                        text: modelData.link.label
                        tip: modelData.link.url
                        onClicked: o.model.chooseOnboarding(index)
                    }
                }
            }
            Text {
                width: parent.width
                bottomPadding: 12
                text: panel.words.note
                color: o.model.fgDim
                lineHeight: 1.4
                wrapMode: Text.Wrap
                font.family: o.ui.uiFont
                font.pixelSize: 14
            }
            LinkLine {
                ui: o.ui
                width: parent.width
            }
            Item {
                width: parent.width
                height: more.height
                Button {
                    id: more
                    ui: o.ui
                    current: o.model.onboardingCursor === panel.words.offers.length
                    text: panel.words.more.label
                    tip: panel.words.more.url
                    onClicked: o.model.chooseOnboarding(panel.words.offers.length)
                }
                Button {
                    anchors.right: parent.right
                    ui: o.ui
                    text: "Back"
                    tip: "Esc"
                    onClicked: o.back()
                }
            }
        }
    }
}
