//! Generates the C++ side of the Qt bridge (src/bridge.rs), compiles the QML ahead of time and embeds it, with
//! the fonts, in the binary.
use cxx_qt_build::{CxxQtBuilder, QmlModule};

const QML: &[&str] = &[
    "qml/Main.qml",
    "qml/TopBar.qml",
    "qml/Reader.qml",
    "qml/Article.qml",
    "qml/Blocks.qml",
    "qml/RichText.qml",
    "qml/Message.qml",
    "qml/Picker.qml",
    "qml/Help.qml",
    "qml/Confirm.qml",
    "qml/Toast.qml",
    "qml/Settings.qml",
    "qml/Field.qml",
    "qml/Panel.qml",
    "qml/Button.qml",
];

const FONTS: &[&str] = &[
    "assets/fonts/literata-latin-wght-normal.woff2",
    "assets/fonts/literata-latin-wght-italic.woff2",
    "assets/fonts/inter-latin-wght-normal.woff2",
    "assets/fonts/inter-latin-wght-italic.woff2",
];

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("be.kletse.rss").qml_files(QML.iter().copied()))
        .qt_module("Network")
        .qt_module("Quick")
        .qrc_resources(FONTS.iter().copied())
        .files(["src/bridge.rs"])
        .build();
}
