//! Onward without a window: what the app does (app.rs), the servers it talks to and how articles are
//! converted. The Qt window (main.rs, bridge.rs), the native macOS app (macos/, through ffi.rs) and the terminal
//! version (tui_main.rs, tui/) all drive it.

pub mod app;
pub mod backend;
// The one QObject the Qt window sees. It lives here rather than next to main.rs because cxx-qt links its C++
// side into every target of the crate, the library's tests included.
#[cfg(feature = "qt")]
mod bridge;
pub mod config;
pub mod content;
pub mod dates;
pub mod demo;
pub mod detect;
pub mod error;
pub mod feeds;
#[cfg(all(feature = "ffi", target_os = "macos"))]
pub mod ffi;
pub mod greader;
pub mod keys;
pub mod manage;
pub mod miniflux;
mod moved;
pub mod opml;
pub mod paging;
pub mod picker;
pub mod prefs;
pub mod providers;
pub mod readeck;
pub mod server;
pub mod theme;
#[cfg(feature = "tui")]
pub mod tui;
