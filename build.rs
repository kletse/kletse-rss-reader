//! Generates the C++ side of the Qt bridge (src/bridge.rs), compiles the QML ahead of time and embeds it, with
//! the fonts, in the binary. Without the `qt` feature (the macOS app's static library) there is nothing to do.
#[cfg(feature = "qt")]
use cxx_qt_build::{CxxQtBuilder, QmlModule};

#[cfg(feature = "qt")]
const QML: &[&str] = &[
    "qml/Main.qml",
    "qml/Transfer.qml",
    "qml/FeedManager.qml",
    "qml/FeedFileDialog.qml",
    "qml/TopBar.qml",
    "qml/Reader.qml",
    "qml/Article.qml",
    "qml/Blocks.qml",
    "qml/RichText.qml",
    "qml/Message.qml",
    "qml/Logo.qml",
    "qml/Picker.qml",
    "qml/Help.qml",
    "qml/Confirm.qml",
    "qml/Toast.qml",
    "qml/Settings.qml",
    "qml/Onboarding.qml",
    "qml/ServiceRow.qml",
    "qml/ServiceIcon.qml",
    "qml/ServerForm.qml",
    "qml/FieldList.qml",
    "qml/Fieldset.qml",
    "qml/ReadeckGrant.qml",
    "qml/Notice.qml",
    "qml/LinkLine.qml",
    "qml/Field.qml",
    "qml/Panel.qml",
    "qml/Button.qml",
    "qml/SearchPanel.qml",
    "qml/Lightbox.qml",
];

#[cfg(feature = "qt")]
const FONTS: &[&str] = &[
    "assets/fonts/literata-latin-wght-normal.woff2",
    "assets/fonts/literata-latin-wght-italic.woff2",
    "assets/fonts/inter-latin-wght-normal.woff2",
    "assets/fonts/inter-latin-wght-italic.woff2",
];

#[cfg(not(feature = "qt"))]
fn main() {}

#[cfg(feature = "qt")]
fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("be.kletse.onward").qml_files(QML.iter().copied()))
        .qt_module("Network")
        .qt_module("Quick")
        .qrc_resources(FONTS.iter().copied())
        .files(["src/bridge.rs"])
        .build();
}
