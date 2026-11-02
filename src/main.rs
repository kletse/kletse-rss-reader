//! Kletse RSS: one unread article at a time, oldest first. Or, in history mode, read ones, last read first.
//!
//! The window is Qt Quick (qml/), driven from Rust through one bridge object (bridge.rs). All behaviour lives in
//! app.rs, which never waits on anything and is tested without Qt; backend.rs talks to the servers.

mod app;
mod backend;
mod bridge;
mod config;
mod content;
mod dates;
mod demo;
mod error;
mod keys;
mod miniflux;
mod paging;
mod picker;
mod prefs;
mod readeck;
mod theme;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

fn main() {
    let mut app = QGuiApplication::new();
    if let Some(mut app) = app.as_mut() {
        app.as_mut().set_application_name(&QString::from("kletse-rss"));
        app.as_mut().set_application_display_name(&QString::from(app::APP_NAME));
        app.as_mut().set_organization_name(&QString::from("kletse"));
        app.as_mut().set_organization_domain(&QString::from("kletse.be"));
        app.as_mut().set_application_version(&QString::from(env!("CARGO_PKG_VERSION")));
        // The Wayland app id, so the compositor matches the window to kletse-rss.desktop (icon, window rules).
        QGuiApplication::set_desktop_file_name(&QString::from("kletse-rss"));
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/be/kletse/rss/qml/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        std::process::exit(app.exec());
    }
}
