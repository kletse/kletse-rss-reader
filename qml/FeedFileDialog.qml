// The file dialog for importing and exporting. It's a file of its own, loaded when it's first needed, so that a Qt
// without the Dialogs module loses this and not the window. It doesn't ask about a file that's there: the core does,
// in one place for every window.
import QtQuick
import QtQuick.Dialogs
import QtCore

FileDialog {
    id: dialog

    required property var model
    required property bool saving
    required property string suggested

    fileMode: saving ? FileDialog.SaveFile : FileDialog.OpenFile
    options: FileDialog.DontConfirmOverwrite
    title: saving ? "Export feeds" : "Import feeds"
    nameFilters: ["OPML files (*.opml *.xml)", "All files (*)"]
    currentFolder: StandardPaths.standardLocations(StandardPaths.HomeLocation)[0]
    // Not a binding: empty, for opening, it's a warning.
    Component.onCompleted: if (saving) selectedFile = currentFolder + "/" + suggested
    onAccepted: dialog.model.feedFile(selectedFile.toString())
}
