// Onboarding (first run) and Settings: a feed server is required (Miniflux, or a Google Reader API server such as
// FreshRSS), Readeck optional. Saving checks both servers before anything is written. Shown in place of the article; Esc cancels (src/app.rs), except on first run.
import QtQuick

Item {
    id: s

    required property var ui
    readonly property var model: ui.model
    readonly property bool firstRun: model.overlay === "onboarding"
    // "miniflux" or "greader": which fields show. The other kind's are kept, for switching back.
    property string server: "miniflux"
    readonly property bool greader: server === "greader"

    height: form.visible ? form.height : 0

    function submit() {
        s.model.submitSettings(s.server, minifluxUrl.text, minifluxKey.text, greaderUrl.text, greaderUser.text,
                               greaderPassword.text, readeckUrl.text, readeckToken.text)
    }

    // The form starts from the saved values once (the serial says when they are in); after that it keeps what
    // is typed, also when a check fails.
    function fill() {
        s.server = s.model.server === "greader" ? "greader" : "miniflux"
        minifluxUrl.text = s.model.minifluxUrl
        minifluxKey.text = s.model.minifluxApiKey
        greaderUrl.text = s.model.greaderUrl
        greaderUser.text = s.model.greaderUsername
        greaderPassword.text = s.model.greaderPassword
        readeckUrl.text = s.model.readeckUrl
        readeckToken.text = s.model.readeckToken
        s.focusFirst()
    }
    // The chosen server's first empty field, or its last.
    function focusFirst() {
        const fields = s.greader ? [greaderUrl, greaderUser, greaderPassword] : [minifluxUrl, minifluxKey]
        const first = fields.find(f => f.text === "") ?? fields[fields.length - 1]
        first.input.forceActiveFocus()
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
            text: s.firstRun ? "Welcome to Kletse RSS" : "Settings"
            color: s.model.fgBright
            font.family: s.ui.headingFont
            font.pixelSize: 28
            font.weight: Font.Bold
            wrapMode: Text.Wrap
        }
        Text {
            width: parent.width
            bottomPadding: 28
            text: s.firstRun
                ? "Connect your Miniflux or Google Reader API server to start reading. Readeck is optional: add it to save articles for later with b."
                : "Changes are checked against your servers before they are saved."
            color: s.model.fgDim
            lineHeight: 1.4
            wrapMode: Text.Wrap
            font.family: s.ui.uiFont
            font.pixelSize: 14
        }

        // What was wrong with the config file, above the fields it is about.
        Item {
            visible: s.model.settingsProblem !== ""
            width: parent.width
            height: visible ? notice.height + 22 : 0
            Rectangle {
                width: 3
                height: notice.height
                color: s.model.yellow
            }
            Text {
                id: notice
                x: 15
                width: parent.width - 15
                topPadding: 2
                bottomPadding: 2
                text: s.model.settingsProblem
                color: s.model.fg
                lineHeight: 1.4
                wrapMode: Text.WrapAnywhere
                font.family: s.ui.uiFont
                font.pixelSize: 14
            }
        }

        Fieldset {
            title: "Server"
            note: "where your feeds are"
            Row {
                spacing: 8
                Button {
                    ui: s.ui
                    primary: !s.greader
                    text: "Miniflux"
                    onClicked: { s.server = "miniflux"; s.focusFirst() }
                }
                Button {
                    ui: s.ui
                    primary: s.greader
                    text: "Google Reader API (FreshRSS, …)"
                    onClicked: { s.server = "greader"; s.focusFirst() }
                }
            }
        }
        Fieldset {
            visible: !s.greader
            title: "Miniflux"
            note: "required"
            Field {
                id: minifluxUrl
                ui: s.ui
                width: parent.width
                label: "Miniflux URL"
                placeholder: "https://miniflux.example.com"
                onAccepted: s.submit()
            }
            Field {
                id: minifluxKey
                ui: s.ui
                width: parent.width
                label: "API key"
                placeholder: "Settings → API Keys → Create a new API key"
                secret: true
                onAccepted: s.submit()
            }
        }
        Fieldset {
            visible: s.greader
            title: "Google Reader API"
            note: "required"
            Field {
                id: greaderUrl
                ui: s.ui
                width: parent.width
                label: "Server URL"
                placeholder: "https://freshrss.example.com/api/greader.php"
                onAccepted: s.submit()
            }
            Text {
                width: parent.width
                text: "FreshRSS: https://your-freshrss/api/greader.php. Miniflux: the server URL, with the Google Reader integration enabled."
                color: s.model.fgDim
                lineHeight: 1.3
                wrapMode: Text.Wrap
                font.family: s.ui.uiFont
                font.pixelSize: 12
            }
            Field {
                id: greaderUser
                ui: s.ui
                width: parent.width
                label: "Username"
                onAccepted: s.submit()
            }
            Field {
                id: greaderPassword
                ui: s.ui
                width: parent.width
                label: "Password"
                placeholder: "FreshRSS: the API password in your profile"
                secret: true
                onAccepted: s.submit()
            }
        }
        Fieldset {
            title: "Readeck"
            note: "optional: leave empty to hide the bookmark button"
            Field {
                id: readeckUrl
                ui: s.ui
                width: parent.width
                label: "Readeck URL"
                placeholder: "https://readeck.example.com"
                onAccepted: s.submit()
            }
            Field {
                id: readeckToken
                ui: s.ui
                width: parent.width
                label: "API token"
                placeholder: "Profile → API Tokens (needs “Bookmarks: write”)"
                secret: true
                onAccepted: s.submit()
            }
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
                visible: !s.firstRun
                text: "Cancel"
                onClicked: s.model.cancelSettings()
            }
            Button {
                ui: s.ui
                primary: true
                enabled: !s.model.settingsChecking
                text: s.model.settingsChecking ? "Checking…" : (s.firstRun ? "Connect" : "Save")
                onClicked: s.submit()
            }
        }
    }

    component Fieldset: Item {
        id: fs
        property string title
        property string note
        default property alias fields: fieldColumn.data

        width: form.width
        height: box.height + legend.height / 2 + 18

        Rectangle {
            id: box
            y: legend.height / 2
            width: parent.width
            height: fieldColumn.height + legend.height / 2 + 18
            color: "transparent"
            border.color: s.model.bgLight
            radius: s.ui.radius
        }
        // The legend sits on the border, as a fieldset's does.
        Rectangle {
            x: 10
            width: legend.width + 12
            height: legend.height
            color: s.model.bg
            Text {
                id: legend
                x: 6
                text: fs.title + "<font color='" + s.model.fgDim + "'> · " + fs.note + "</font>"
                textFormat: Text.StyledText
                color: s.model.accent
                font.family: s.ui.uiFont
                font.pixelSize: 14
            }
        }
        Column {
            id: fieldColumn
            x: 16
            y: legend.height + 4
            width: parent.width - 32
            spacing: 10
        }
    }
}
