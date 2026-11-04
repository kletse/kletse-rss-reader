// The window: a top bar, the reader (an article, a message or the settings form), and the overlays over it.
// Everything it shows comes from `model` (src/bridge.rs); keys and clicks go back to it.
pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtCore as Core
import be.kletse.rss

ApplicationWindow {
    id: win

    readonly property alias model: model
    readonly property int articleSize: model.articleSize
    /// The column scales with the font size, as in CSS: `--article-size * --width-factor`.
    readonly property real measure: model.articleSize * model.widthFactor
    readonly property string articleFont: literata.status === FontLoader.Ready ? literata.name : "serif"
    readonly property string headingFont: inter.status === FontLoader.Ready ? inter.name : "sans-serif"
    readonly property string uiFont: model.uiFont !== "" ? model.uiFont : "monospace"
    readonly property real radius: model.rounding
    /// The rich-text block that holds the selection, for Ctrl+C (⌘C).
    property Item selectionHolder: null

    function holdSelection(item) {
        if (selectionHolder && selectionHolder !== item)
            selectionHolder.deselect()
        selectionHolder = item
    }

    width: 900
    height: 1000
    minimumWidth: 360
    minimumHeight: 300
    visible: true
    title: model.windowTitle
    color: model.bg !== "" ? model.bg : "#1a1b26"
    font.family: uiFont
    font.pixelSize: 14

    Model {
        id: model
        Component.onCompleted: start()
        onScrollPage: up => reader.scrollPage(up ? -1 : 1)
        onQuitRequested: Qt.quit()
        // The search box takes the keyboard itself (SearchPanel.qml).
        onOverlayChanged: if (overlay !== "search") keys.forceActiveFocus()
    }

    // macOS: follow the system appearance and accent colour (it has no event for the accent, so on focus too).
    onActiveChanged: if (active) model.recheckTheme()
    Connections {
        target: Application.styleHints
        function onColorSchemeChanged() { model.recheckTheme() }
    }

    FontLoader { id: literata; source: "qrc:/qt/qml/be/kletse/rss/assets/fonts/literata-latin-wght-normal.woff2" }
    FontLoader { source: "qrc:/qt/qml/be/kletse/rss/assets/fonts/literata-latin-wght-italic.woff2" }
    FontLoader { id: inter; source: "qrc:/qt/qml/be/kletse/rss/assets/fonts/inter-latin-wght-normal.woff2" }
    FontLoader { source: "qrc:/qt/qml/be/kletse/rss/assets/fonts/inter-latin-wght-italic.woff2" }

    // Mac apps reopen where they were left; under a tiling compositor the window manager decides.
    Core.Settings {
        id: geometry
        category: "window"
        property int x: -1
        property int y: -1
        property int width: 900
        property int height: 1000
    }
    Component.onCompleted: {
        if (model.mac && geometry.x >= 0) {
            win.x = geometry.x
            win.y = geometry.y
            win.width = geometry.width
            win.height = geometry.height
        }
    }
    onClosing: {
        if (model.mac) {
            geometry.x = win.x
            geometry.y = win.y
            geometry.width = win.width
            geometry.height = win.height
        }
    }

    Shortcut {
        sequences: [StandardKey.Copy]
        onActivated: if (win.selectionHolder) win.selectionHolder.copy()
    }
    // The usual Mac window keys; ⌘Q comes with the application menu Qt makes.
    Shortcut {
        enabled: model.mac
        sequences: [StandardKey.Close]
        onActivated: win.close()
    }
    Shortcut {
        enabled: model.mac
        sequence: "Ctrl+M"
        onActivated: win.showMinimized()
    }

    // Every key goes through here to the app (keys.rs has the keymaps). Typing in a settings field reaches here
    // only for keys the field didn't use, such as Esc.
    FocusScope {
        id: keys
        anchors.fill: parent
        focus: true

        // The window colour, so a snapshot has it too.
        Rectangle {
            anchors.fill: parent
            color: win.color
        }

        Keys.onPressed: event => {
            const name = win.keyName(event)
            if (name === "")
                return
            const typing = win.activeFocusItem !== null && win.activeFocusItem.hasOwnProperty("echoMode")
            event.accepted = model.key(name,
                                       (event.modifiers & Qt.ShiftModifier) !== 0,
                                       (event.modifiers & Qt.ControlModifier) !== 0,
                                       (event.modifiers & Qt.AltModifier) !== 0,
                                       (event.modifiers & Qt.MetaModifier) !== 0,
                                       typing, reader.atBottom)
        }

        TopBar {
            id: topBar
            ui: win
            width: parent.width
        }

        Reader {
            id: reader
            ui: win
            anchors.top: topBar.bottom
            anchors.bottom: parent.bottom
            width: parent.width
        }

        // The next article's images, loading while this one is read (bridge.rs `prefetch`). Same source and fill
        // mode as in Blocks.qml, so the article finds them in Qt's image cache.
        Repeater {
            model: {
                try {
                    return JSON.parse(model.prefetchImages)
                } catch (e) {
                    return []
                }
            }
            delegate: Image {
                required property string modelData
                visible: false
                source: modelData
                asynchronous: true
                cache: true
                fillMode: Image.PreserveAspectFit
            }
        }

        Lightbox { ui: win; anchors.fill: parent; visible: model.overlay === "image" }
        Picker { ui: win; anchors.fill: parent; visible: model.pickerShown }
        Loader {
            anchors.fill: parent
            active: model.overlay === "search"
            sourceComponent: SearchPanel { ui: win }
        }
        Help { ui: win; anchors.fill: parent; visible: model.overlay === "help" }
        Confirm { ui: win; anchors.fill: parent; visible: model.overlay === "confirm" }
        Toast { ui: win; anchors.fill: parent }
    }

    /// The key as the web's `KeyboardEvent.key` names it, which is what keys.rs binds.
    function keyName(event) {
        switch (event.key) {
        case Qt.Key_Down: return "ArrowDown"
        case Qt.Key_Up: return "ArrowUp"
        case Qt.Key_Left: return "ArrowLeft"
        case Qt.Key_Right: return "ArrowRight"
        case Qt.Key_Escape: return "Escape"
        case Qt.Key_Return:
        case Qt.Key_Enter: return "Enter"
        case Qt.Key_Tab:
        case Qt.Key_Backtab: return "Tab"
        case Qt.Key_Space: return " "
        case Qt.Key_Shift:
        case Qt.Key_Control:
        case Qt.Key_Alt:
        case Qt.Key_Meta:
        case Qt.Key_AltGr:
        case Qt.Key_CapsLock: return ""
        }
        // With Ctrl held the text is a control character; the key code still says which key it was.
        if (event.text.length === 1 && event.text.charCodeAt(0) >= 32)
            return event.text
        if (event.key === Qt.Key_Plus) return "+"
        if (event.key === Qt.Key_Equal) return "="
        if (event.key === Qt.Key_Minus) return "-"
        if (event.key >= Qt.Key_0 && event.key <= Qt.Key_9) return String.fromCharCode(event.key)
        if (event.key >= Qt.Key_A && event.key <= Qt.Key_Z) {
            const letter = String.fromCharCode(event.key)
            return (event.modifiers & Qt.ShiftModifier) ? letter : letter.toLowerCase()
        }
        return ""
    }

    // KLETSE_RSS_SNAPSHOT=/path.png: once the feeds are in (or the first-run form is), press KLETSE_RSS_SNAPSHOT_KEYS (Enter: open the first
    // row; "Space" for a space, "Ctrl+" and "Shift+" before a key to hold them, as in "Ctrl+Tab"), give images a
    // moment, render the window to a PNG and quit. Screenshots, and CI's check that the QML loads and draws.
    Timer {
        id: snapshotKeys
        property var pending: null
        interval: 400
        repeat: true
        // The first run has no feeds to wait for, as the Mac's snapshot knows too.
        running: model.snapshotPath !== "" && pending === null
                 && (model.pickerState === "ready" || (model.overlay === "onboarding" && model.settingsReady))
        onTriggered: {
            // One key per tick, so what a key starts (opening a selection) is done before the next one.
            if (pending === null)
                pending = model.snapshotKeys.split(" ").filter(k => k !== "")
            let name = pending.shift()
            if (name !== undefined) {
                let ctrl = false
                let shift = false
                while (name.length > 1 && /^(Ctrl|Shift)\+/.test(name)) {
                    if (name.startsWith("Ctrl+"))
                        ctrl = true
                    else
                        shift = true
                    name = name.slice(name.indexOf("+") + 1)
                }
                model.key(name === "Space" ? " " : name, shift, ctrl, false, false, false, false)
            }
            if (pending.length === 0) {
                stop()
                snapshotShot.start()
            } else if (!running) {
                start()
            }
        }
    }
    Timer {
        id: snapshotShot
        interval: 3000
        onTriggered: keys.grabToImage(result => {
            result.saveToFile(model.snapshotPath)
            Qt.quit()
        })
    }
}
