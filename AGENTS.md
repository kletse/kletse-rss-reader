# Working on Kletse RSS

Notes for coding agents (and people) picking this up in a new session. Practical notes are here; what the app does
and how to build it is in the [README](README.md).

## The shape of it

- **Rust + Qt Quick via [cxx-qt](https://github.com/KDAB/cxx-qt) 0.10.** No web view, no Node. One crate at the
  repo root.
- **All behaviour is in `src/app.rs`** and is tested without Qt: an action returns `Effect`s (server calls,
  scrolling, saving prefs), and their answers come back as `Outcome`s through `App::apply`. Add behaviour there,
  with a test in `src/app_tests.rs` (key presses against a fake Miniflux). Don't put logic in QML.
- **`src/bridge.rs` is the only QObject.** It runs effects on a tokio runtime, queues the outcomes back onto the
  Qt thread (`qt_thread().queue`), and after every change `sync()` copies state into properties. Setters only
  notify on a real change, so syncing everything each time is fine. The article is re-rendered only when its id,
  text or the theme changes (`Rendered`), and `articleSerial` only changes for a new article or new text, which
  is what resets the scroll position.
- **QML (`qml/`) draws and forwards.** Keys go to `model.key(name, …)` named as the web's `KeyboardEvent.key`
  names them ("j", "A", "ArrowDown", " ", "Escape"); `Main.qml` `keyName()` translates. Keymaps and the help text
  live in `src/keys.rs`, and a test keeps them in step with each other and with the README's key table.
- **Articles:** `src/content/convert.rs` turns feed HTML into JSON blocks that `qml/Blocks.qml` lays out. Text
  inside a block is a small HTML subset rendered by Qt's rich text (`qml/RichText.qml`, a read-only `TextEdit`).
  Qt's rich text knows only a little CSS: colours, font, `line-height` (its 100% is the font's natural spacing,
  not 1em), and table borders. There's no padding or radius on inline runs. Colours are baked into the HTML from
  the palette, which is why a theme change re-renders.
- **Scrolling is hand-made** in `qml/Reader.qml`, to feel like the web view did. Flickable is not `interactive`,
  and a `WheelHandler` applies WebKitGTK's rules: touchpad and TrackPoint deltas ×4 (GTK 3 takes a Wayland
  pixel as 1/10 of a scroll unit, WebKit scrolls 40 px per unit), a kinetic glide after the fingers lift
  (velocity × e^-4t), and wheel clicks of height^(2/3) px, animated. Qt's own Flickable wheel handling (1:1, no
  momentum, 72 px per click) felt about 4× too slow under Omarchy's `touchpad:scroll_factor`. macOS takes deltas
  1:1, since the system adds its own momentum.
- **The shared core** (`miniflux.rs`, `readeck.rs`, `config.rs`, `error.rs`, `demo.rs`) came over from the Tauri
  version unchanged apart from dropping the IPC bits. Its tests use wiremock.

## Commands

```bash
cargo test                                   # ~140 tests, no window
cargo clippy --all-targets -- -D warnings    # CI is this strict
cargo fmt
KLETSE_RSS_DEMO=1 cargo run                  # sample data, no server, nothing marked read
```

Qt is found through `qmake6`/`qmake` on the `PATH`. A first build compiles the C++ side of cxx-qt and takes a few
minutes. After that, editing QML or Rust rebuilds in well under a minute.

## Seeing the QML's errors (read this first)

QML is only checked when it runs: a type that doesn't resolve, a missing property or a binding loop compiles fine.
`qmlcachegen` catches syntax errors only.

- **Qt logs to journald, not stderr, when stderr isn't a terminal.** An agent's shell is not a terminal. A
  broken QML file then looks like an app that hangs with no output, because the window never appears and the
  event loop keeps running. Always set `QT_FORCE_STDERR_LOGGING=1`, or read `journalctl --user -n 50 -o cat`.
- Set `LANG=C.UTF-8` as well, or Qt complains about the C locale on every start.
- `QT_FATAL_WARNINGS=1` turns any warning into a crash with a non-zero exit, so a run that exits 0 had no warnings.
  CI's smoke test relies on this.

## Seeing the window without a window

There's no way to click around a live window from an agent session, so the app can screenshot itself:

```bash
LANG=C.UTF-8 QT_FORCE_STDERR_LOGGING=1 QT_QPA_PLATFORM=offscreen KLETSE_RSS_DEMO=1 \
KLETSE_RSS_SNAPSHOT=/tmp/shot.png KLETSE_RSS_SNAPSHOT_KEYS="Enter n n" \
  timeout 30 target/debug/kletse-rss
```

Once the feeds load, it presses the keys one per 400 ms (default `Enter`, which opens the first row; write a
space as `Space`). It then waits 3 s for images, saves the window to the PNG and quits. Useful key sequences:
`" "` (the picker itself), `"Enter ?"` (help), `"Enter ,"` (settings), `"Enter A"` (confirm), `"h Enter"`
(history), `"Enter n n n n n n n n"` (all caught up; the demo has 8 unread). Read the PNG to look at it. The
offscreen platform renders like the real one, including the app's fonts.

Note: the settings form shows the **real** config of the machine it runs on (demo mode only affects server calls),
so keep that in mind before sharing a screenshot.

`KLETSE_RSS_DEBUG=1` prints, per article, what the converter dropped (inline SVG, MathML) or didn't recognise.

## Traps found so far

- **Names in this QML module shadow Qt's.** `qml/Settings.qml` hid QtCore's `Settings` until `Main.qml` imported
  QtCore `as Core`. Before naming a new QML file, check that Qt Quick, Controls and QtCore have no type by that name.
- **Don't redeclare properties Qt types already have.** `property string icon` on a `Button` fails, because
  `AbstractButton.icon` is FINAL. The same goes for `width` on `TextMetrics` and the like.
- **Size loops end up as size zero.** A control whose content width depends on its own width, such as a label
  `width: Math.min(implicitWidth, availableWidth)` inside a Button's `contentItem`, quietly collapses to nothing.
  Compute `implicitWidth` from children's implicit widths only. Binding loops on `height` show up as warnings, so
  watch for them.
- **`grabToImage` doesn't work on `window.contentItem`** ("no QML engine"). Grab an item you declared instead
  (`Main.qml` grabs its root `FocusScope`, which paints the window colour).
- **Components loaded with `sourceComponent`** are created in the declaring file's context. Inside them, `parent`
  is the `Loader` (null for a moment at creation), hence the `parent?.modelData` in `Blocks.qml`. Recursive
  layouts (quotes, lists) load `Blocks.qml` again with `Loader.setSource(url, props)`, because a QML type can't
  contain itself.
- **Key text with Ctrl held is a control character.** `keyName()` falls back to the key code for letters, digits
  and `=`/`+`/`-`. On macOS, Qt reports ⌘ as `ControlModifier` and the Control key as `MetaModifier`.
- **Test-only accessors** in `app.rs` are `#[cfg(test)]`. Clippy runs with `-D warnings`, so an unused one fails
  CI.

## cxx-qt 0.10 notes

- Bridge syntax: `#[cxx_qt::bridge] mod qobject { extern "RustQt" { #[qobject] #[qml_element]
  #[qproperty(T, name)] type Model = super::ModelRust; #[qinvokable] fn f(self: Pin<&mut Model>); #[qsignal] … } }`.
  `#[auto_cxx_name]` on the `extern "RustQt"` block makes `snake_case` names `camelCase` in QML.
- Use `//` comments, not `///`, between attributes inside the bridge. Lints such as `too_many_arguments` on
  generated code need an inner `#![allow]` on the module.
- `self.rust_mut()` needs `use cxx_qt::CxxQtType`, and `qt_thread()` needs `use cxx_qt::Threading` plus
  `impl cxx_qt::Threading for Model {}` in the bridge.
- The build (`build.rs`) uses `CxxQtBuilder::new_qml_module(QmlModule::new("be.kletse.rss").qml_files([...]))`.
  **Every new QML file must be added to the `QML` list in `build.rs`**, and every embedded file (fonts) to
  `qrc_resources`. They end up under `qrc:/qt/qml/be/kletse/rss/<path>`.
- When unsure of an API, read the crate source in `~/.cargo/registry/src/*/cxx-qt*-0.10.*`. It is newer than
  most training data.

## Platforms

- **Omarchy/Arch** is the main target. The theme comes from `~/.local/state/omarchy/current/theme/colors.toml`,
  watched with `notify`, plus `hyprctl` for rounding and `omarchy font current`.
- **macOS** is build-from-source (`packaging/macos/build-app.sh`, Homebrew Qt). macOS-only Rust
  (`src/theme/macos.rs`) can't be compiled on Linux, so the macOS CI job is its only check. It also builds the
  app bundle. On macOS the theme is re-checked when the window gains focus and when the system colour scheme
  changes (`model.recheckTheme()`).
