//! Onward in a terminal, filling it: the same reader, keys and servers as the window. See `tui/mod.rs`.

use std::process::ExitCode;

fn main() -> ExitCode {
    match onward::tui::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("onward-tui: {e}");
            ExitCode::FAILURE
        }
    }
}
