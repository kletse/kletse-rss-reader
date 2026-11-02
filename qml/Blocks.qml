// A list of converted blocks (src/content/convert.rs), laid out top to bottom. Quotes and list items hold blocks
// of their own and load this file again for them.
import QtQuick

Column {
    id: root

    required property var ui
    property var blocks: []
    property color textColor: ui.model.fg
    property real gap: ui.articleSize * 1.1

    spacing: gap

    function open(url) {
        if (url)
            root.ui.model.openLink(url)
    }

    function nested(loader, blocks, extra) {
        const props = { ui: root.ui, blocks: blocks, textColor: root.textColor, gap: root.gap * 0.6 }
        for (const k in extra)
            props[k] = extra[k]
        loader.setSource("Blocks.qml", props)
    }

    Repeater {
        model: root.blocks

        delegate: Loader {
            required property var modelData
            width: root.width
            sourceComponent: {
                switch (modelData.type) {
                case "paragraph": return paragraph
                case "heading": return heading
                case "image": return image
                case "caption": return caption
                case "code": return code
                case "quote": return quote
                case "list": return list
                case "table": return table
                case "rule": return rule
                case "embed": return embed
                }
                return null
            }
        }
    }

    // Components are created in this file's scope; `parent` is the Loader, which holds the block.

    Component {
        id: paragraph
        RichText {
            ui: root.ui
            html: parent?.modelData?.html ?? ""
            color: root.textColor
        }
    }

    Component {
        id: heading
        RichText {
            readonly property int level: parent?.modelData?.level ?? 2
            ui: root.ui
            html: parent?.modelData?.html ?? ""
            color: ui.model.fgBright
            topPadding: ui.articleSize * 0.7
            lineHeight: 115
            font.family: ui.headingFont
            font.weight: Font.Bold
            font.letterSpacing: -0.2
            font.pixelSize: ui.articleSize * [1.5, 1.5, 1.3, 1.15, 1.05, 1, 1][level]
        }
    }

    Component {
        id: caption
        RichText {
            ui: root.ui
            html: parent?.modelData?.html ?? ""
            color: ui.model.fgDim
            horizontalAlignment: TextEdit.AlignHCenter
            font.pixelSize: ui.articleSize * 0.8
        }
    }

    Component {
        id: image
        Column {
            id: fig
            readonly property var b: parent?.modelData ?? ({})
            width: parent?.width ?? 0
            spacing: 8

            Image {
                id: img
                readonly property bool sized: implicitWidth > 0
                anchors.horizontalCenter: parent.horizontalCenter
                source: fig.b.src
                asynchronous: true
                cache: true
                fillMode: Image.PreserveAspectFit
                smooth: true
                mipmap: true
                // Natural size, never wider than the column (CSS: max-inline-size: 100%; block-size: auto).
                width: sized ? Math.min(fig.width, implicitWidth) : fig.width
                height: sized ? width * implicitHeight / implicitWidth : (status === Image.Loading ? 160 : 0)

                Rectangle {
                    anchors.fill: parent
                    visible: img.status === Image.Loading
                    color: root.ui.model.bgLight
                    radius: root.ui.radius
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.open(fig.b.href || fig.b.src)
                }
            }

            Text {
                visible: img.status === Image.Error
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                text: "image didn't load: " + (fig.b.alt || fig.b.src)
                color: root.ui.model.muted
                font.family: root.ui.uiFont
                font.pixelSize: 12
                elide: Text.ElideMiddle
            }

            RichText {
                visible: !!fig.b.caption
                ui: root.ui
                html: fig.b.caption || ""
                color: ui.model.fgDim
                horizontalAlignment: TextEdit.AlignHCenter
                font.pixelSize: ui.articleSize * 0.8
            }
        }
    }

    Component {
        id: code
        Rectangle {
            id: codeBox
            readonly property var b: parent?.modelData ?? ({})
            width: parent?.width ?? 0
            height: codeText.implicitHeight + 28
            color: root.ui.model.bgDark
            radius: root.ui.radius

            Flickable {
                id: codeFlick
                anchors.fill: parent
                anchors.margins: 14
                anchors.leftMargin: 16
                anchors.rightMargin: 16
                contentWidth: codeText.implicitWidth
                contentHeight: codeText.implicitHeight
                flickableDirection: Flickable.HorizontalFlick
                interactive: contentWidth > width
                boundsBehavior: Flickable.StopAtBounds
                clip: true

                TextEdit {
                    id: codeText
                    textFormat: TextEdit.RichText
                    text: "<pre style=\"font-family:'" + root.ui.uiFont + "'\">" + (codeBox.b.html ?? "") + "</pre>"
                    readOnly: true
                    selectByMouse: true
                    activeFocusOnPress: false
                    color: root.ui.model.fg
                    selectionColor: root.ui.model.selection
                    font.family: root.ui.uiFont
                    font.pixelSize: root.ui.articleSize * 0.95 * 0.8
                }
            }
        }
    }

    Component {
        id: quote
        Item {
            id: quoteBox
            readonly property var b: parent?.modelData ?? ({})
            width: parent?.width ?? 0
            height: quoteBlocks.height

            Rectangle {
                width: 3
                height: parent.height
                color: root.ui.model.accent
            }

            Loader {
                id: quoteBlocks
                x: root.ui.articleSize
                width: parent.width - x
                Component.onCompleted: root.nested(this, quoteBox.b.blocks ?? [], { textColor: root.ui.model.fgDim })
            }
        }
    }

    Component {
        id: list
        Column {
            id: listCol
            readonly property var b: parent?.modelData ?? ({ items: [] })
            width: parent?.width ?? 0
            spacing: root.gap * 0.4

            Repeater {
                model: listCol.b.items

                delegate: Item {
                    id: item
                    required property var modelData
                    required property int index
                    width: listCol.width
                    height: Math.max(marker.implicitHeight, itemBlocks.height)

                    // Laid out by the same rich-text engine as the item's first line, so they share a baseline.
                    RichText {
                        id: marker
                        width: root.ui.articleSize * 1.4
                        ui: root.ui
                        horizontalAlignment: TextEdit.AlignRight
                        html: listCol.b.ordered ? (listCol.b.start + item.index) + "." : "•"
                        color: root.ui.model.fgDim
                        selectByMouse: false
                    }

                    Loader {
                        id: itemBlocks
                        x: root.ui.articleSize * 2
                        width: parent.width - x
                        Component.onCompleted: root.nested(this, item.modelData, {})
                    }
                }
            }
        }
    }

    Component {
        id: table
        Flickable {
            id: tableBox
            readonly property var b: parent?.modelData ?? ({})
            width: parent?.width ?? 0
            height: tableText.implicitHeight
            contentWidth: Math.max(width, tableText.implicitWidth)
            contentHeight: tableText.implicitHeight
            flickableDirection: Flickable.HorizontalFlick
            interactive: contentWidth > width
            boundsBehavior: Flickable.StopAtBounds
            clip: true

            RichText {
                id: tableText
                width: undefined
                ui: root.ui
                html: tableBox.b.html ?? ""
                wrapMode: TextEdit.NoWrap
                lineHeight: 120
                font.pixelSize: ui.articleSize * 0.85
            }
        }
    }

    Component {
        id: rule
        Item {
            width: parent?.width ?? 0
            height: root.ui.articleSize * 2
            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width
                height: 1
                color: root.ui.model.bgLight
            }
        }
    }

    Component {
        id: embed
        Item {
            id: embedItem
            readonly property var b: parent?.modelData ?? ({})
            width: parent?.width ?? 0
            height: embedBox.height

            Rectangle {
                id: embedBox
                width: embedLabel.implicitWidth + 24
                height: embedLabel.implicitHeight + 16
                color: "transparent"
                border.color: root.ui.model.bgLight
                radius: root.ui.radius

                Text {
                    id: embedLabel
                    anchors.centerIn: parent
                    text: embedItem.b.label ?? ""
                    color: root.ui.model.accent
                    font.family: root.ui.uiFont
                    font.pixelSize: root.ui.articleSize * 0.8
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.open(embedItem.b.href)
                }
            }
        }
    }
}
