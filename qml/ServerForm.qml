// Which kind of server, and its fields, links and help: all from the table in src/providers.rs. Settings and first
// run both hold one of these, and keep `values` themselves.
import QtQuick

Column {
    id: sf

    required property var ui
    readonly property var model: ui.model
    readonly property var providers: JSON.parse(sf.model.providers)
    readonly property var provider: providers.find(p => p.id === server) ?? providers[0]
    property var values: ({})
    property string server: "miniflux"
    /// A kind of server was clicked.
    signal chosen(string name)
    signal accepted()

    function focusFirst() {
        fieldList.focusFirst()
    }

    // After the fields of the new kind are made.
    onServerChanged: Qt.callLater(focusFirst)
    onValuesChanged: Qt.callLater(focusFirst)
    Component.onCompleted: focusFirst()

    Fieldset {
        ui: sf.ui
        title: "Server"
        note: "where your feeds are"
        Flow {
            width: parent.width
            spacing: 8
            Repeater {
                model: sf.providers
                delegate: Button {
                    required property var modelData
                    ui: sf.ui
                    primary: sf.server === modelData.id
                    text: modelData.label
                    width: Math.min(implicitWidth, parent.width)
                    onClicked: sf.chosen(modelData.id)
                }
            }
        }
    }
    Fieldset {
        ui: sf.ui
        title: sf.provider.short_label
        note: "required"
        FieldList {
            id: fieldList
            ui: sf.ui
            width: parent.width
            fields: sf.provider.fields
            values: sf.values
            onAccepted: sf.accepted()
        }
        Flow {
            width: parent.width
            spacing: 8
            Repeater {
                model: sf.provider.links
                delegate: Button {
                    required property var modelData
                    ui: sf.ui
                    text: modelData.label
                    tip: modelData.url
                    onClicked: sf.model.openLink(modelData.url)
                }
            }
        }
    }
}
