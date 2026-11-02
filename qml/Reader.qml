// Everything under the top bar: the article, a message, or the settings form, in one scrolling area.
import QtQuick
import QtQuick.Controls.Basic as T

Flickable {
    id: reader

    required property var ui
    readonly property var model: ui.model
    /// Scrolled to the end: Space moves on to the next article.
    readonly property bool atBottom: contentY + height >= contentHeight - 4
    readonly property real gutter: width < 560 ? 16 : 24

    contentWidth: width
    contentHeight: page.height
    boundsBehavior: Flickable.StopAtBounds
    clip: true

    T.ScrollBar.vertical: T.ScrollBar {
        id: bar
        width: 8
        policy: T.ScrollBar.AsNeeded
        contentItem: Rectangle {
            implicitWidth: 8
            radius: reader.ui.radius
            color: reader.model.bgLight
            opacity: bar.active || bar.hovered ? 1 : 0.6
        }
    }

    /// Space pages by most of the visible height, keeping a little context.
    function scrollPage(direction) {
        const target = (scroll.running ? scroll.to : contentY) + direction * height * 0.85
        scroll.stop()
        scroll.from = contentY
        scroll.to = Math.max(0, Math.min(contentHeight - height, target))
        scroll.start()
    }

    NumberAnimation {
        id: scroll
        target: reader
        property: "contentY"
        duration: 180
        easing.type: Easing.OutCubic
    }

    // A new article, its full text, or another screen: from the top.
    Connections {
        target: reader.model
        function onArticleSerialChanged() { reader.toTop() }
        function onScreenChanged() { reader.toTop() }
    }
    function toTop() {
        scroll.stop()
        contentY = 0
    }

    Item {
        id: page
        width: reader.width
        // Room to scroll the last lines up to the middle, as the web version had.
        height: content.y + content.height + reader.height * 0.3

        Loader {
            id: content
            x: reader.gutter
            y: reader.width < 560 ? 24 : 40
            width: page.width - 2 * reader.gutter
            sourceComponent: {
                switch (reader.model.screen) {
                case "settings": return settings
                case "loading": return loading
                case "article": return article
                case "end": return end
                }
                return null
            }
        }
    }

    Component {
        id: article
        Article { ui: reader.ui }
    }
    Component {
        id: settings
        Settings { ui: reader.ui }
    }
    Component {
        id: loading
        Message { ui: reader.ui; text: "Loading…" }
    }
    Component {
        id: end
        Message {
            ui: reader.ui
            title: reader.model.endTitle
            text: "Press j or Enter for the feeds, k to go back."
        }
    }
}
