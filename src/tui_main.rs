//! Kletse RSS in a terminal, filling it: the same reader, keys and servers as the window. See `tui/mod.rs`.

use std::process::ExitCode;

fn main() -> ExitCode {
    match kletse_rss::tui::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kletse-rss-tui: {e}");
            ExitCode::FAILURE
        }
    }
}
