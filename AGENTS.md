# Working on Kletse RSS

Notes for coding agents (and people) picking this up in a new session. Practical notes are here; what the app does
and how to build it is in the [README](README.md).

## The shape of it

- **Rust + Qt Quick via [cxx-qt](https://github.com/KDAB/cxx-qt) 0.10** on Linux, **SwiftUI** on macOS (`macos/`),
  and **ratatui** in a terminal on both (`src/tui/`). No web view, no Node. One crate at the repo root: a library
  (`src/lib.rs`, everything but the window) and two binaries, Qt (`src/main.rs`) and terminal (`src/tui_main.rs`).
  The `qt` feature (default) brings in cxx-qt and `bridge.rs`, which lives in the library because cxx-qt links its
  C++ into every target of the crate; the `ffi` feature builds the C interface the Mac app links (`src/ffi.rs`); the
  `tui` feature builds the terminal version. Build `tui` with `--no-default-features`, or with `cargo tui`: with
  `qt` on as well, the terminal binary links Qt for nothing.
- **All behaviour is in `src/app.rs`** and is tested without Qt: an action returns `Effect`s (server calls,
  scrolling, saving prefs), and their answers come back as `Outcome`s through `App::apply`. Add behaviour there,
  with a test in `src/app_tests.rs` (key presses against a fake Miniflux). Don't put logic in QML.
- **`src/bridge.rs` is the only QObject.** It runs effects on a tokio runtime, queues the outcomes back onto the
  Qt thread (`qt_thread().queue`), and after every change `sync()` copies state into properties. Setters only
  notify on a real change, so syncing everything each time is fine. The article is re-rendered only when its id,
  text or the theme changes (`Rendered`), and `articleSerial` only changes for a new article or new text, which
  is what resets the scroll position.
- **The next article is got ready ahead** (`prefetch` in bridge.rs and ffi.rs): converted on the runtime, and
  its image URLs handed to the window. In Qt a hidden `Repeater` of `Image`s in `Main.qml` loads them; they only
  share Qt's image cache with the article's `Image`s if the cache key matches, which includes the `fillMode`.
  Keep `source`, `fillMode` and `sourceSize` the same in `Main.qml`, `Blocks.qml` and `Lightbox.qml`.
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
  version unchanged apart from dropping the IPC bits. Its tests use wiremock. `greader.rs` is the Google Reader API
  client (FreshRSS, Miniflux's integration, Inoreader…), giving the same `Entry`/`FeedCount`/`Selection` as
  `miniflux.rs`; `server.rs` is the enum over the two that `backend.rs` calls, which keeps one client per config
  (the Google Reader one remembers its login and the stream each hashed feed id stands for). Paging goes by
  `Entry::order_millis()`, the server's own order time (FreshRSS: arrival), not always `published_at`. What the
  protocol can't do comes back as `Error::Refused`, and `Status::fetches` tells the windows to leave out `r`/`f`
  hints. To try it against a real server, FreshRSS runs in Docker; on macOS point the app at a throwaway config
  with `HOME=<dir>` (`dirs` ignores `XDG_CONFIG_HOME` there).

## Commands

```bash
cargo test                                   # ~170 tests, no window
cargo clippy --all-targets -- -D warnings    # CI is this strict
cargo fmt
KLETSE_RSS_DEMO=1 cargo run                  # sample data, no server, nothing marked read

KLETSE_RSS_DEMO=1 cargo tui                  # the terminal version
cargo test --no-default-features --features tui
cargo clippy --all-targets --no-default-features --features tui -- -D warnings
```

Plain `cargo test` and `cargo clippy` don't compile `src/tui/`: run the two lines above when you change it, or
anything it uses. CI does.

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
space as `Space`, and `Ctrl+` or `Shift+` before a key holds them, as in `Ctrl+Tab`). It then waits 3 s for images, saves the window to the PNG and quits. Useful key sequences:
`" "` (the picker itself), `"Enter ?"` (help), `"Enter ,"` (settings), `"Enter A"` (confirm), `"h Enter"`
(history), `"Enter n n n n n n n n"` (all caught up; the demo has 8 unread), `"Enter /"` (the search box),
`"Enter , Ctrl+Tab"` (the settings' Readeck tab),
`"Enter v"` (the image viewer; the first demo article has an image), `"Enter n p A"` (marking a picker row read), `"j j Enter j"` (the end of a feed, with where j goes next).
For the first-run form, leave out `KLETSE_RSS_DEMO` (the demo counts as set up) and point `HOME` at an empty
directory (`dirs` ignores `XDG_CONFIG_HOME` on macOS): with no feeds to wait for, it starts once the form is in.
Read the PNG to look at it. The offscreen platform renders like the real one, including the app's fonts.

Note: the settings form shows the **real** config of the machine it runs on (demo mode only affects server calls),
so keep that in mind before sharing a screenshot. Prefs (font size, width, order) are read from the real
`prefs.toml` but never written in demo mode.

`KLETSE_RSS_DEBUG=1` prints, per article, what the converter dropped (inline SVG, MathML) or didn't recognise.

## Traps found so far

- **Names in this QML module shadow Qt's.** `qml/Settings.qml` hid QtCore's `Settings` until `Main.qml` imported
  QtCore `as Core`. Before naming a new QML file, check that Qt Quick, Controls and QtCore have no type by that name
  (newer Controls have a `SearchField`, hence `SearchPanel.qml`).
- **A text field in an overlay must be created when it opens.** `Main.qml` gives the keyboard back to its
  `FocusScope` whenever the overlay changes; the search box is loaded by a `Loader` only while it's open and
  takes the focus itself on creation, so a hidden field never keeps it.
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
- **Linking needs lld.** When `ld` is GNU bfd, cxx-qt's build helper picks `lld` if it's installed and `ld.gold`
  otherwise. Gold fails with `undefined reference to 'cxx_qt_init_crate_kletse_rss'` (and the other
  `cxx_qt_init_*`) since `bridge.rs` moved into the library. Arch's `rust` package pulls in `lld`; with rustup,
  install it (`sudo pacman -S lld`). The PKGBUILD lists it in `makedepends`.
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
- **macOS** is build-from-source and native: `packaging/macos/build-app.sh` builds the library as a static lib
  (`cargo rustc --lib --no-default-features --features ffi --crate-type staticlib`) and the Swift package in
  `macos/` around it, with `MACOSX_DEPLOYMENT_TARGET` matching the package's macOS 15. macOS-only Rust
  (`src/theme/macos.rs`, `src/ffi.rs`) can't be compiled on Linux, so the macOS CI job is its only check. The Qt
  window still builds and runs on a Mac (`cargo run`, with Homebrew Qt), which is handy for comparing.

## The terminal version (`src/tui/`)

- **`tui/ui.rs` is to the terminal what `bridge.rs` is to QML**, and the same rule holds: it shows state and forwards
  input, and what a key means stays in `app.rs`. What's here is only what a terminal needs and a window doesn't:
  text fields (`editor.rs`), scrolling, hit-testing clicks, and fetching images (`images.rs`), which Qt's `Image`
  does by itself. `mod.rs` runs the effects: the main thread waits on one channel, fed by a thread reading the
  terminal and by the tokio runtime, and draws after each batch. `Ui::prepare` runs before each draw and can ask for
  jobs and a repaint, so anything it decides is acted on in the same pass.
- **Articles are laid out by `tui/article.rs`** from the same `NativeBlock`s Swift gets, into `Row`s of styled cells
  that know which columns are clickable. It's our own word wrap because ratatui's can't report row counts or where
  a link landed.
- **Pictures in an article** go in steps, in `Ui::prepare`: `Job::Fetch` decodes one, which gives its size; the layout
  then sets rows aside (`Slice`), where before there was a `▣` line; `Job::Slice` encodes it for those rows; `draw`
  puts it there with ratatui-image's `SlicedImage`, which can cut it at the top and bottom of the screen. The article
  moves when a picture arrives, so `lay_out` keeps your place by counting text rows, leaving out `Row::image` ones.
  Only the pictures of this article, the next and the last are kept.
- **Colours are the terminal's ANSI palette**, never truecolor: that's what follows the theme, and not every
  terminal has truecolor. `article::palette()` gives each ANSI colour a fake hex code for the converter, and `ink()` turns
  those back. Dim text is `Modifier::DIM`, not a grey.
- **Ctrl never reaches `App::key`.** Ctrl + and − would set the window's font size in the shared `prefs.toml`.
  `Ui::help_lines` takes those rows out, and swaps the Shift+Space row for the keys that work in a terminal.
- **To see it**, there's no snapshot mode: tmux is one. Run it in a detached session and read the pane:

  ```bash
  tmux new-session -d -s k -x 100 -y 32 "KLETSE_RSS_DEMO=1 LANG=C.UTF-8 target/debug/kletse-rss-tui"
  tmux send-keys -t k Enter; sleep 0.3; tmux capture-pane -p -t k; tmux kill-session -t k
  ```

  `tui/tests.rs` does the same on ratatui's `TestBackend`, with real key and mouse events against the demo. It
  records browser links rather than opening them; `with_content` swaps in an article with a real picture, because
  the demo's only image is an SVG, which the `image` crate can't decode.
- **To see real pictures**, tmux won't do (half-blocks). Run foot itself and capture its window: `hyprctl dispatch`
  an `exec_cmd` with `{ float = true, no_initial_focus = true, size = "1000 620" }`, then `grim -T <stableId from
  hyprctl clients -j>`. It must be on a visible workspace: foot doesn't repaint on a hidden one, so a capture there
  shows an empty window that says nothing. Point it at a fake Miniflux with `XDG_CONFIG_HOME` and `XDG_STATE_HOME`
  set, so real feeds and prefs stay out of it.

Traps here:

- **Don't call `Terminal::clear`.** It asks the terminal for the cursor position, and the answer can't get past the
  thread blocked in `event::read`, which holds crossterm's lock: a 2 s stall, then an error that ends the app. Use
  `repaint` in `mod.rs`. `Terminal::resize` is fine.
- **Mouse reporting is what stops the wheel marking articles read.** Without it terminals send the wheel as ↑ and ↓ on
  the alternate screen, and ↓ is "next, mark read". A test holds this in place.
- **`Picker::from_query_stdio` must run before the reader thread, and not under tmux.** Under tmux it runs
  `tmux set -p allow-passthrough on` behind your back, and if nothing answers it leaves a thread reading stdin.
- **Sixel and iTerm2 pictures linger.** They're pixels over the text, and ratatui's diff doesn't know: rows it leaves
  alone keep them. So `sync_stale` sets `repaint` whenever a picture moves or something goes over it, and the loop
  wraps that in a synchronized update to hide the clear. Kitty's are cells and half-blocks are text, so these don't.
- **Blanks over a sixel must be written.** After a repaint ratatui leaves out blank cells, so a picture shows through
  a panel's spaces. `solid` in `draw.rs` marks them; call it at the end of anything drawn over the article.
- **A sixel must not reach the screen's last row.** The library places it by moving the cursor down and back up, the
  terminal stops the cursor at the bottom, and the picture lands a row too high. `reader` keeps a row free.

## The macOS app (`macos/`)

- **`src/ffi.rs` is to Swift what `bridge.rs` is to QML.** Three C functions (`macos/Sources/CKletse/kletse.h`):
  `kr_new`, `kr_send` (a JSON command in, a JSON reply out: whether a key was used, events such as
  `scroll_down` and `quit`, and the whole state) and `kr_free_string`. Effects run on tokio; when an answer is
  in, the core calls `notify` from that thread, and Swift sends `pump` on the main thread, which applies it. The
  app never changes off the main thread. The article's blocks go out only when converted again (as `Rendered`
  in bridge.rs), and `article.serial` changes only for a new article or new text.
- **Articles reach Swift as styled runs, not HTML:** `src/content/native.rs` turns the inline HTML convert.rs
  leaves in each block into runs (bold, link, code, colour…), and tables into rows of cells. Swift builds
  `AttributedString`s from them (`BlocksView.swift`). Code colours still come from the palette, which Swift
  feeds with the real appearance and accent colour (`theme` command), so a theme change re-renders.
- **SwiftUI only draws and forwards**, like the QML: `Model.swift` holds the last reply. Every key goes through
  an `NSEvent` local monitor (`Keyboard.swift`) to the core first, named as `keyName()` in Main.qml names them,
  with ⌘ as `ctrl` and Control as `meta`. Keys the core doesn't use carry on (⌘Q, typing in a field). Overlays
  map onto native presentation: the picker and help are sheets, confirm an alert. Their bindings only
  ask the core to close them.
- **The toolbar mirrors the Qt top bar:** the feeds button on the left (a clock icon in history mode), where you
  are in the middle ("History · 8 / 88"), the buttons on the right. The window title is set but kept out of the
  toolbar (`.toolbar(removing: .title)`); the article's heading is right below.
- **A field editor doesn't mean typing.** A click on `.textSelection(.enabled)` text makes an `NSTextView` field
  editor the first responder, a read-only one. `Keyboard.typing` also asks `isEditable`, or every key stops working
  until something else takes the focus.
- **Don't give menu items single-key shortcuts.** AppKit matches menu key equivalents before the text field
  sees the key, so typing `j` in the settings would go to the next article.
- **A C callback must not be main-actor code.** A closure written inside a `@MainActor` type is inferred as
  isolated to it and traps when tokio calls it; `wake` in `Core.swift` is a top-level function for that reason.
- **The window must be told to open at launch** (`.defaultLaunchBehavior(.presented)`): macOS remembers a
  `Window` that was closed at the last quit, per bundle id, and otherwise starts with no window at all.
- **Snapshots work as for Qt:** `KLETSE_RSS_DEMO=1 KLETSE_RSS_SNAPSHOT=/tmp/shot.png KLETSE_RSS_SNAPSHOT_KEYS="Enter ?"
  "packaging/Kletse RSS.app/Contents/MacOS/kletse-rss"`. The window really opens on screen. An open sheet is drawn
  over the window, at 1200×860 as the README's shots are; alerts don't draw into a bitmap.
