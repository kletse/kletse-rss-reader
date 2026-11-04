//! The keymaps and the key reference the help overlay shows. Keys are named as the web's `KeyboardEvent.key`
//! names them ("j", "A", "ArrowDown", " ", "Escape"); the QML side translates Qt's key events into these names.
//! A test keeps the help and the keymaps from drifting apart.

/// What a key does. Some depend on where you are (`Space`, `Enter`); the app decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// j / ↓: next article, marking this one read. n: next, keeping it unread.
    Next {
        mark_read: bool,
    },
    Prev,
    /// Only on the "all caught up" screen: back to the feeds.
    Enter,
    /// Scroll a page; at the end of an article, the next one. Shift+Space scrolls up.
    Space {
        up: bool,
    },
    ToggleRead,
    OpenOriginal,
    SaveToReadeck,
    FetchFull,
    /// Shift+A: the whole list in the reader, the row under the cursor in the picker.
    MarkAllRead,
    /// `/`: the search box.
    Search,
    /// `s`: unread items oldest or newest first.
    FlipOrder,
    /// `v`: the article's images, one at a time.
    ViewImages,
    ToggleMode,
    OpenPicker,
    Narrower,
    Wider,
    Help,
    Quit,
    Settings,
    PickerMove(i32),
    PickerChoose,
    PickerRefresh,
    PickerClose,
    BiggerFont,
    SmallerFont,
    ResetSize,
}

type Keymap = &'static [(&'static [&'static str], Action)];

pub const READER_KEYS: Keymap = &[
    (&["j", "ArrowDown"], Action::Next { mark_read: true }),
    (&["n"], Action::Next { mark_read: false }),
    (&["k", "ArrowUp"], Action::Prev),
    (&["Enter"], Action::Enter),
    (&[" "], Action::Space { up: false }),
    (&["m"], Action::ToggleRead),
    (&["o"], Action::OpenOriginal),
    (&["b"], Action::SaveToReadeck),
    (&["f"], Action::FetchFull),
    (&["A"], Action::MarkAllRead),
    (&["v"], Action::ViewImages),
    (&["/"], Action::Search),
    (&["s"], Action::FlipOrder),
    (&["h"], Action::ToggleMode),
    (&["g", "p", "Tab", "Escape", "ArrowLeft"], Action::OpenPicker),
    (&["["], Action::Narrower),
    (&["]"], Action::Wider),
    (&["?"], Action::Help),
    (&["q"], Action::Quit),
];

pub const PICKER_KEYS: Keymap = &[
    (&["j", "ArrowDown"], Action::PickerMove(1)),
    (&["k", "ArrowUp"], Action::PickerMove(-1)),
    (&["Enter", "ArrowRight", "o"], Action::PickerChoose),
    (&["r"], Action::PickerRefresh),
    (&["A"], Action::MarkAllRead),
    (&["/"], Action::Search),
    (&["s"], Action::FlipOrder),
    (&["h"], Action::ToggleMode),
    (&["?"], Action::Help),
    (&["Escape", "g", "p", "Tab"], Action::PickerClose),
    (&["q"], Action::Quit),
];

/// Ctrl combinations (⌘ on macOS: Qt reports ⌘ as Ctrl there).
pub const MODIFIER_KEYS: Keymap =
    &[(&["=", "+"], Action::BiggerFont), (&["-"], Action::SmallerFont), (&["0"], Action::ResetSize)];

/// Keys that work in the reader and in the picker.
pub const GLOBAL_KEYS: Keymap = &[(&[","], Action::Settings)];

/// What a button in the window does, by the name the window gives it.
pub fn button_action(button: &str) -> Option<Action> {
    Some(match button {
        "next" => Action::Next { mark_read: true },
        "prev" => Action::Prev,
        "picker" => Action::OpenPicker,
        "readeck" => Action::SaveToReadeck,
        "mode" => Action::ToggleMode,
        "settings" => Action::Settings,
        "help" => Action::Help,
        "search" => Action::Search,
        "order" => Action::FlipOrder,
        _ => return None,
    })
}

pub fn lookup(keymap: Keymap, key: &str) -> Option<Action> {
    keymap.iter().find(|(keys, _)| keys.contains(&key)).map(|&(_, action)| action)
}

/// A key as the help prints it, maybe held with Shift or the platform modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    Plain(&'static str),
    Shift(&'static str),
    Mod(&'static str),
}

impl Chord {
    #[cfg(test)]
    fn key(self) -> &'static str {
        match self {
            Chord::Plain(k) | Chord::Shift(k) | Chord::Mod(k) => k,
        }
    }
}

/// Where a help line's keys work besides the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    Reader,
    Picker,
    Both,
}

#[derive(Debug)]
pub struct HelpRow {
    pub keys: &'static [Chord],
    pub action: &'static str,
    pub place: Where,
    /// Picker keys the line mentions in passing, e.g. "(→ / o picks a row)".
    pub picker_note: Option<(&'static [Chord], &'static str)>,
}

const fn row(keys: &'static [Chord], action: &'static str) -> HelpRow {
    HelpRow { keys, action, place: Where::Reader, picker_note: None }
}

use Chord::{Mod, Plain, Shift};

/// The help overlay, in reading order.
pub const HELP: &[HelpRow] = &[
    row(&[Plain("j"), Plain("ArrowDown")], "next (marks read)"),
    row(&[Plain("n")], "skip (keeps unread)"),
    row(&[Plain("k"), Plain("ArrowUp")], "previous"),
    row(&[Plain("m")], "toggle read / unread"),
    row(&[Plain("o")], "open original in browser"),
    row(&[Plain("b")], "save to Readeck, then next"),
    row(&[Plain("f")], "fetch full article (again for feed version)"),
    HelpRow {
        keys: &[Plain("A")],
        action: "mark whole list read (in picker: the selected row)",
        place: Where::Both,
        picker_note: None,
    },
    row(&[Plain("v")], "view the article's images (← / → between them)"),
    HelpRow { keys: &[Plain("/")], action: "search all feeds, read and unread", place: Where::Both, picker_note: None },
    HelpRow {
        keys: &[Plain("s")],
        action: "unread oldest / newest first (remembered)",
        place: Where::Both,
        picker_note: None,
    },
    HelpRow {
        keys: &[Plain("g"), Plain("p"), Plain("Tab"), Plain("Escape"), Plain("ArrowLeft")],
        action: "feeds picker",
        place: Where::Reader,
        picker_note: Some((&[Plain("ArrowRight"), Plain("o")], "picks a row")),
    },
    HelpRow { keys: &[Plain("r")], action: "refresh feeds", place: Where::Picker, picker_note: None },
    HelpRow {
        keys: &[Plain("h")],
        action: "history: read items, newest first (again for unread)",
        place: Where::Both,
        picker_note: None,
    },
    row(&[Plain(" "), Shift(" ")], "scroll; Space at the end = next"),
    row(&[Mod("+"), Mod("-")], "font size"),
    row(&[Plain("["), Plain("]")], "narrower / wider column"),
    row(&[Mod("0")], "reset font size and width"),
    HelpRow { keys: &[Plain(",")], action: "settings", place: Where::Both, picker_note: None },
    HelpRow { keys: &[Plain("?")], action: "this help", place: Where::Both, picker_note: None },
    HelpRow { keys: &[Plain("q")], action: "quit", place: Where::Both, picker_note: None },
];

/// How the help (and the README) names a key: arrows as glyphs, Esc, Space, a real minus sign.
pub fn key_name(key: &str) -> &str {
    match key {
        " " => "Space",
        "ArrowDown" => "↓",
        "ArrowUp" => "↑",
        "ArrowLeft" => "←",
        "ArrowRight" => "→",
        "Escape" => "Esc",
        "-" => "−",
        other => other,
    }
}

fn chord_label(chord: Chord, mac: bool) -> String {
    match chord {
        // A capital letter is bound as itself but typed with Shift, so the help says so.
        Plain(k) if k.len() == 1 && k.chars().all(|c| c.is_ascii_uppercase()) => chord_label(Shift(k), mac),
        Plain(k) => key_name(k).to_string(),
        Shift(k) => format!("Shift+{}", key_name(k)),
        Mod(k) => format!("{} {}", if mac { "⌘" } else { "Ctrl" }, key_name(k)),
    }
}

fn label(chords: &[Chord], mac: bool) -> String {
    chords.iter().map(|&c| chord_label(c, mac)).collect::<Vec<_>>().join(" / ")
}

/// The help overlay's lines: the keys, then what they do.
pub fn help_lines(mac: bool) -> Vec<(String, String)> {
    HELP.iter()
        .map(|row| {
            let place = if row.place == Where::Picker { " (in picker)" } else { "" };
            let note =
                row.picker_note.map(|(keys, text)| format!(" ({} {text})", label(keys, mac))).unwrap_or_default();
            (label(row.keys, mac), format!("{}{place}{note}", row.action))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[derive(Clone, Copy)]
    enum KeyMode {
        Reader,
        Picker,
        Modifier,
    }

    fn bound(mode: KeyMode) -> BTreeSet<&'static str> {
        let maps: &[Keymap] = match mode {
            KeyMode::Reader => &[GLOBAL_KEYS, READER_KEYS],
            KeyMode::Picker => &[GLOBAL_KEYS, PICKER_KEYS],
            KeyMode::Modifier => &[MODIFIER_KEYS],
        };
        maps.iter().flat_map(|m| m.iter().flat_map(|(keys, _)| keys.iter().copied())).collect()
    }

    /// Bound keys the help leaves out on purpose.
    fn unlisted(mode: KeyMode) -> &'static [&'static str] {
        match mode {
            // Only acts on the "all caught up" screen, which says so itself.
            KeyMode::Reader => &["Enter"],
            // Moving works as in the reader, Enter picks as everywhere, and the keys that open the picker close it.
            KeyMode::Picker => &["j", "ArrowDown", "k", "ArrowUp", "Enter", "Escape", "g", "p", "Tab"],
            // = is + without Shift.
            KeyMode::Modifier => &["="],
        }
    }

    fn documented(mode: KeyMode) -> BTreeSet<&'static str> {
        let is_mod = |c: &Chord| matches!(c, Mod(_));
        match mode {
            KeyMode::Modifier => {
                HELP.iter().flat_map(|r| r.keys.iter().filter(|c| is_mod(c))).map(|c| c.key()).collect()
            }
            KeyMode::Reader | KeyMode::Picker => {
                let picker = matches!(mode, KeyMode::Picker);
                let works = |r: &&HelpRow| if picker { r.place != Where::Reader } else { r.place != Where::Picker };
                let plain = HELP.iter().filter(works).flat_map(|r| r.keys.iter().filter(|c| !is_mod(c)));
                let notes = HELP.iter().filter(|_| picker).flat_map(|r| r.picker_note.map_or(&[][..], |n| n.0));
                plain.chain(notes).map(|c| c.key()).collect()
            }
        }
    }

    #[test]
    fn the_help_names_every_bound_key_and_no_other() {
        for mode in [KeyMode::Reader, KeyMode::Picker, KeyMode::Modifier] {
            let mut named = documented(mode);
            named.extend(unlisted(mode));
            let missing: Vec<_> = bound(mode).difference(&named).copied().collect();
            assert!(missing.is_empty(), "bound but not in the help: {missing:?}");
            let extra: Vec<_> = documented(mode).difference(&bound(mode)).copied().collect();
            assert!(extra.is_empty(), "in the help but not bound: {extra:?}");
        }
    }

    #[test]
    fn reads_as_before() {
        let lines = help_lines(false);
        let first: Vec<(&str, &str)> = lines.iter().take(3).map(|(k, a)| (k.as_str(), a.as_str())).collect();
        assert_eq!(first, [("j / ↓", "next (marks read)"), ("n", "skip (keeps unread)"), ("k / ↑", "previous")]);
        assert!(lines.contains(&("Shift+A".into(), "mark whole list read (in picker: the selected row)".into())));
        assert!(lines.contains(&("/".into(), "search all feeds, read and unread".into())));
        assert!(lines.contains(&("g / p / Tab / Esc / ←".into(), "feeds picker (→ / o picks a row)".into())));
        assert!(lines.contains(&("r".into(), "refresh feeds (in picker)".into())));
        assert!(lines.contains(&("Space / Shift+Space".into(), "scroll; Space at the end = next".into())));
        assert!(lines.contains(&("Ctrl + / Ctrl −".into(), "font size".into())));
        assert!(help_lines(true).contains(&("⌘ 0".into(), "reset font size and width".into())));
    }

    #[test]
    fn the_readme_lists_every_help_line() {
        let readme = include_str!("../README.md");
        for row in HELP {
            for chord in row.keys {
                let shown = match chord {
                    Plain(k) | Shift(k) | Mod(k) => key_name(k),
                };
                assert!(readme.contains(&format!("<kbd>{shown}</kbd>")), "README doesn't mention {shown}");
            }
        }
    }
}
