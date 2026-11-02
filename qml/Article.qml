// The article: its own header (feed, author, date, read status, title, site), then the converted body.
import QtQuick

Item {
    id: a

    required property var ui
    readonly property var model: ui.model

    height: column.height

    function host(url) {
        const m = /^[a-z][a-z0-9+.-]*:\/\/(?:[^@\/]*@)?([^:\/?#]+)/i.exec(url)
        return m ? m[1].replace(/^www\./, "") : url
    }

    function published(iso) {
        const d = new Date(iso)
        return isNaN(d.getTime()) ? "" : d.toLocaleString(Qt.locale(), "d MMM yyyy, hh:mm")
    }

    Column {
        id: column
        width: Math.min(a.width, a.ui.measure)
        anchors.horizontalCenter: parent.horizontalCenter

        Item {
            width: parent.width
            height: Math.max(meta.implicitHeight, status.implicitHeight)

            Text {
                id: meta
                width: parent.width - status.width - 12
                text: {
                    const parts = [a.model.feedTitle, a.model.author, a.published(a.model.publishedAt)]
                    if (a.model.readingTime > 0)
                        parts.push(a.model.readingTime + " min")
                    return parts.filter(p => p !== "").join(" · ")
                }
                color: a.model.fgDim
                font.family: a.ui.uiFont
                font.pixelSize: 13
                wrapMode: Text.Wrap
            }
            Text {
                id: status
                anchors.right: parent.right
                anchors.baseline: meta.baseline
                text: a.model.status.toUpperCase()
                color: a.model.status === "unread" ? a.model.accent : a.model.muted
                font.family: a.ui.uiFont
                font.pixelSize: 11
                font.letterSpacing: 0.9
            }
        }

        Text {
            width: parent.width
            topPadding: 12
            bottomPadding: 4
            text: a.model.title
            color: titleHover.hovered ? a.model.accent : a.model.fgBright
            wrapMode: Text.Wrap
            font.family: a.ui.headingFont
            font.weight: Font.Bold
            font.pixelSize: a.ui.articleSize * 1.66
            font.letterSpacing: -0.015 * a.ui.articleSize * 1.66
            lineHeight: 1.2
            HoverHandler { id: titleHover; cursorShape: Qt.PointingHandCursor }
            TapHandler { onTapped: a.model.openLink(a.model.url) }
        }

        Text {
            width: parent.width
            bottomPadding: 32
            text: a.host(a.model.url)
            color: a.model.muted
            font.family: a.ui.uiFont
            font.pixelSize: 13
            elide: Text.ElideRight
        }

        Blocks {
            width: parent.width
            ui: a.ui
            blocks: {
                try {
                    return JSON.parse(a.model.blocks)
                } catch (e) {
                    return []
                }
            }
        }
    }
}
