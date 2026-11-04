// Onboarding (first run) and Settings: a feed server is required (Miniflux, or a Google Reader API server such as
// FreshRSS), Readeck optional. Saving checks both servers before anything is written. Shown in place of the article; Esc cancels (src/app.rs), except on first run.
// One tab at a time (Server, Readeck): the core lists them and says which is open, and Ctrl+Tab reaches it through
// `model.key()`. A hidden tab's fields keep what is typed, and every field is sent.
import QtQuick

Item {
    id: s

    required property var ui
    readonly property var model: ui.model
    readonly property bool firstRun: model.overlay === "onboarding"
    // "miniflux" or "greader": which fields show. The other kind's are kept, for switching back.
    property string server: "miniflux"
    readonly property bool greader: server === "greader"
    // The tab on screen, and every tab: [{ name, title, note }].
    readonly property string section: model.settingsSection
    readonly property var sections: {
        try {
            return JSON.parse(s.model.settingsSections)
        } catch (e) {
            return []
        }
    }

    function sectionInfo(name) {
        return s.sections.find(t => t.name === name) ?? { title: "", note: "" }
    }

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
    // The tab's first empty field, or its last: on the Server tab, the chosen server's.
    function focusFirst() {
        const fields = s.section === "readeck" ? [readeckUrl, readeckToken]
            : s.greader ? [greaderUrl, greaderUser, greaderPassword] : [minifluxUrl, minifluxKey]
        const first = fields.find(f => f.text === "") ?? fields[fields.length - 1]
        first.input.forceActiveFocus()
    }
    Connections {
        target: s.model
        function onSettingsSerialChanged() { s.fill() }
        function onSettingsSectionChanged() { if (s.model.settingsReady) s.focusFirst() }
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
                ? "Connect your Miniflux or Google Reader API server to start reading. Readeck is optional: add it on its tab to save articles for later with b."
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

        // The tabs, with the keys that switch them.
        Row {
            width: parent.width
            spacing: 8
            Repeater {
                model: s.sections
                delegate: Button {
                    required property var modelData
                    ui: s.ui
                    primary: modelData.name === s.section
                    text: modelData.title
                    onClicked: s.model.showSettingsSection(modelData.name)
                }
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                leftPadding: 8
                text: s.model.settingsTabsHint
                color: s.model.fgDim
                font.family: s.ui.uiFont
                font.pixelSize: 12
            }
        }
        Item { width: 1; height: 20 }

        // Under the tabs, whichever is open; a failed save opens the one it is about.
        Text {
            visible: s.model.settingsError !== ""
            width: parent.width
            bottomPadding: 16
            text: s.model.settingsError
            color: s.model.red
            wrapMode: Text.Wrap
            font.family: s.ui.uiFont
            font.pixelSize: 14
        }

        Fieldset {
            visible: s.section === "server"
            title: s.sectionInfo("server").title
            note: s.sectionInfo("server").note
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
            visible: s.section === "server" && !s.greader
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
            visible: s.section === "server" && s.greader
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
            visible: s.section === "readeck"
            title: s.sectionInfo("readeck").title
            note: s.sectionInfo("readeck").note
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
            Text {
                width: parent.width
                text: "Leave both empty to hide the bookmark button."
                color: s.model.fgDim
                lineHeight: 1.3
                wrapMode: Text.Wrap
                font.family: s.ui.uiFont
                font.pixelSize: 12
            }
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
