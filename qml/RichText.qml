// One run of rich text (a paragraph, heading, caption): Qt's own HTML-subset renderer, selectable, links open
// in the browser. Keyboard focus stays with the window so j/k keep working.
import QtQuick

TextEdit {
    id: t

    required property var ui
    property string html
    // Qt's 100% is the font's natural line spacing (about 1.5em for Literata), not 1em as in CSS, and Qt adds
    // the extra below the line rather than half above, half below. 125% comes out at the app's 1.9em.
    property int lineHeight: 125

    width: parent ? parent.width : 0
    textFormat: TextEdit.RichText
    text: "<div style='line-height:" + lineHeight + "%'>" + html + "</div>"
    wrapMode: TextEdit.Wrap
    readOnly: true
    selectByMouse: true
    activeFocusOnPress: false
    persistentSelection: false
    color: ui.model.fg
    selectionColor: ui.model.selection
    selectedTextColor: ui.model.fgBright
    font.family: ui.articleFont
    font.pixelSize: ui.articleSize * 0.95
    font.weight: 350

    onLinkActivated: link => ui.model.openLink(link)
    // Ctrl+C copies the one block that holds a selection.
    onSelectedTextChanged: if (selectedText !== "") ui.holdSelection(t)

    HoverHandler {
        cursorShape: t.hoveredLink !== "" ? Qt.PointingHandCursor : Qt.IBeamCursor
    }
}
