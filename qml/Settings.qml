// Settings: a feed server is required (Miniflux, or a Google Reader API server such as FreshRSS), Readeck optional.
// Saving checks both servers before anything is written. Shown in place of the article; Esc cancels (src/app.rs). First
// run has a screen of its own (Onboarding.qml), around the same `ServerForm`.
import QtQuick

Item {
    id: s

    required property var ui
    readonly property var model: ui.model
    // Every field's value by its key (src/providers.rs), whichever are on screen, for switching back.
    property var values: ({})
    property string server: "miniflux"

    height: form.visible ? form.height : 0

    function submit() {
        s.model.submitSettings(s.server, JSON.stringify(s.values))
    }

    // The form starts from the saved values once (the serial says when they are in); after that it keeps what
    // is typed, also when a check fails.
    function fill() {
        s.server = s.model.server
        s.values = JSON.parse(s.model.formValues)
    }
    Connections {
        target: s.model
        function onSettingsSerialChanged() { s.fill() }
    }
    Component.onCompleted: if (s.model.settingsReady) s.fill()

    Column {
        id: form
        visible: s.model.settingsReady
        width: Math.min(560, s.width)
        anchors.horizontalCenter: parent.horizontalCenter
        topPadding: s.ui.height * 0.04
        spacing: 0

        Text {
            width: parent.width
            bottomPadding: 8
            text: "Settings"
            color: s.model.fgBright
            font.family: s.ui.headingFont
            font.pixelSize: 28
            font.weight: Font.Bold
            wrapMode: Text.Wrap
        }
        Text {
            width: parent.width
            bottomPadding: 28
            text: "Changes are checked against your servers before they are saved."
            color: s.model.fgDim
            lineHeight: 1.4
            wrapMode: Text.Wrap
            font.family: s.ui.uiFont
            font.pixelSize: 14
        }

        // What was wrong with the config file, above the fields it is about.
        Notice {
            ui: s.ui
            width: parent.width
        }

        Item {
            width: parent.width
            height: find.height + 12
            Button {
                id: find
                ui: s.ui
                text: JSON.parse(s.model.firstRun).find_server
                enabled: !s.model.settingsChecking
                onClicked: s.model.findServer()
            }
        }

        ServerForm {
            ui: s.ui
            width: parent.width
            values: s.values
            server: s.server
            onChosen: name => s.server = name
            onAccepted: s.submit()
        }
        Fieldset {
            ui: s.ui
            title: "Readeck"
            note: "optional: leave empty to hide the bookmark button"
            FieldList {
                ui: s.ui
                width: parent.width
                fields: JSON.parse(s.model.readeckFields)
                values: s.values
                onAccepted: s.submit()
            }
        }
        Fieldset {
            ui: s.ui
            title: "Colours"
            note: "applied at once"
            Row {
                spacing: 8
                Button {
                    ui: s.ui
                    primary: s.model.colours === "system"
                    text: "System"
                    onClicked: s.model.chooseColours("system")
                }
                Button {
                    ui: s.ui
                    primary: s.model.colours === "onward-dark"
                    text: "Onward Dark"
                    onClicked: s.model.chooseColours("onward-dark")
                }
                Button {
                    ui: s.ui
                    primary: s.model.colours === "onward-light"
                    text: "Onward Light"
                    onClicked: s.model.chooseColours("onward-light")
                }
            }
        }

        LinkLine {
            ui: s.ui
            width: parent.width
        }

        Text {
            width: parent.width
            height: Math.max(implicitHeight, 20)
            text: s.model.settingsError
            color: s.model.red
            wrapMode: Text.Wrap
            font.family: s.ui.uiFont
            font.pixelSize: 14
        }

        Row {
            anchors.right: parent.right
            topPadding: 12
            spacing: 8
            Button {
                ui: s.ui
                text: "Cancel"
                onClicked: s.model.cancelSettings()
            }
            Button {
                ui: s.ui
                primary: true
                enabled: !s.model.settingsChecking
                text: s.model.settingsChecking ? "Checking…" : "Save"
                onClicked: s.submit()
            }
        }
    }
}
