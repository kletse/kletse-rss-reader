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
    readonly property real topMargin: width < 560 ? 24 : 40
    /// An article that fits is centred, with nothing to scroll.
    readonly property bool centred: model.screen === "article" && content.height + 2 * topMargin <= height

    contentWidth: width
    contentHeight: page.height
    boundsBehavior: Flickable.StopAtBounds
    clip: true
    // Scrolling works as it did in the web view (WebKitGTK), below; no dragging the page, which got in the way of
    // selecting text.
    interactive: false

    // ---- Scrolling the way the web view did --------------------------------------------------------------
    // Qt Quick takes touchpad deltas 1:1 and stops dead when the fingers lift, and a wheel click is 72 px. GTK 3
    // counts a Wayland pixel delta as a tenth of a scroll unit and WebKit scrolls 40 px per unit: 4× the
    // distance, then a kinetic glide (ScrollAnimationKinetic: velocity decays as e^-4t). A wheel click scrolls
    // height^(2/3) px (Scrollbar::pixelsPerLineStep), smoothly. macOS gives deltas with the system's own
    // acceleration and momentum, which WebKit there takes as they come.

    readonly property real preciseFactor: model.mac ? 1 : 4
    readonly property real friction: 4
    property var samples: []        // [ms, contentY delta] of the touchpad gesture going on
    property real glideFrom: 0      // contentY where the glide started
    property real glideVelocity: 0  // px/s at its start (kept after it stops: a quick next swipe adds to it)
    property real glideStart: 0     // ms

    function clampY(y) {
        return Math.max(0, Math.min(contentHeight - height, y))
    }

    function stopScrolling() {
        scroll.stop()
        glide.stop()
    }

    function scrollBy(dy) {
        stopScrolling()
        contentY = clampY(contentY + dy)
    }

    /// A wheel click: animated, adding up when they come quickly.
    function smoothScrollBy(dy) {
        const from = scroll.running ? scroll.to : contentY
        stopScrolling()
        scroll.from = contentY
        scroll.to = clampY(from + dy)
        scroll.duration = 150
        scroll.start()
    }

    /// After the fingers lift: the speed of the last 100 ms, decaying. A swipe while one still glides adds to it,
    /// as WebKit's velocity accumulation does.
    function startGlide() {
        const now = Date.now()
        const recent = samples.filter(s => now - s[0] <= 100)
        samples = []
        if (recent.length < 2)
            return
        const span = (recent[recent.length - 1][0] - recent[0][0]) / 1000
        if (span <= 0)
            return
        let v = recent.reduce((sum, s) => sum + s[1], 0) / span
        const previous = glideVelocity * Math.exp(-friction * (now - glideStart) / 1000)
        if (previous !== 0 && Math.sign(previous) === Math.sign(v) && Math.abs(v) >= Math.abs(previous * 0.33)) {
            const multiplier = (v - previous * 0.33) / (previous - previous * 0.33)
            v += previous * Math.min(multiplier, 6)
        }
        if (Math.abs(v) < 1)
            return
        glideFrom = contentY
        glideVelocity = v
        glideStart = now
        glide.start()
    }

    function wheel(event) {
        const dx = event.angleDelta.y
        const px = event.pixelDelta.y
        // Touchpads come in phases; a TrackPoint has none but, like them, gives angleDelta = 12 × pixelDelta.
        // A wheel gives 120 per click.
        const precise = event.phase !== Qt.NoScrollPhase
            || (px !== 0 && Math.abs(Math.abs(dx) - 12 * Math.abs(px)) <= 12)
        if (event.phase === Qt.ScrollBegin) {
            // Fingers down stop a glide. Its velocity stays, still decaying, for a swipe to add to.
            stopScrolling()
            samples = []
            return
        }
        if (event.phase === Qt.ScrollEnd) {
            if (!model.mac)
                startGlide()
            return
        }
        if (precise || (model.mac && px !== 0)) {
            // angleDelta is 12 × the exact (unrounded) pixel delta on Wayland; use it for sub-pixel precision.
            const delta = model.mac ? -px : -dx / 12 * preciseFactor
            scrollBy(delta)
            if (event.phase === Qt.ScrollUpdate)
                samples = samples.concat([[Date.now(), delta]]).filter(s => Date.now() - s[0] <= 300)
        } else if (dx !== 0) {
            smoothScrollBy(-dx / 120 * Math.pow(height, 2 / 3))
        }
    }

    WheelHandler {
        target: null
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        onWheel: event => reader.wheel(event)
    }

    FrameAnimation {
        id: glide
        onTriggered: {
            const t = (Date.now() - reader.glideStart) / 1000
            const decay = Math.exp(-reader.friction * t)
            const y = reader.clampY(reader.glideFrom + reader.glideVelocity / reader.friction * (1 - decay))
            const stuck = y === reader.contentY && t > 0.05
            reader.contentY = y
            if (Math.abs(reader.glideVelocity * decay) < 1 || stuck)
                stop()
        }
    }

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
        stopScrolling()
        scroll.from = contentY
        scroll.to = clampY(target)
        scroll.duration = 180
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
        stopScrolling()
        contentY = 0
    }
    onCentredChanged: if (centred) toTop()

    Item {
        id: page
        width: reader.width
        // Room to scroll the last lines up to the middle, as the web version had.
        height: reader.centred ? reader.height : content.y + content.height + reader.height * 0.3

        Loader {
            id: content
            x: reader.gutter
            y: reader.centred ? (reader.height - height) / 2 : reader.topMargin
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
            text: reader.model.endHint
        }
    }
}
