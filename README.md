<p align="center">
  <img src="packaging/kletse-rss.svg" width="96" alt="Kletse RSS icon">
</p>

<h1 align="center">Kletse RSS</h1>

<p align="center">
  <b>One unread article at a time. Oldest first. Keyboard all the way.</b><br>
  A small desktop reader for <a href="https://miniflux.app">Miniflux</a>, made for <a href="https://omarchy.org">Omarchy</a>.
</p>

<p align="center">
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-backend-B7410E?logo=rust&logoColor=white">
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-UI-3178C6?logo=typescript&logoColor=white">
  <img alt="Arch package" src="https://img.shields.io/badge/Arch-PKGBUILD-1793D1?logo=archlinux&logoColor=white">
  <img alt="MIT" src="https://img.shields.io/badge/license-MIT-green">
</p>

---

## The idea

Most feed readers give you three panes and a wall of headlines. Kletse RSS shows **just the article**.

You pick a feed or category, and the app opens on the **oldest unread item**. Press **j** and it marks that item read and moves to the next one. When the list runs out, you get *“All caught up”* and you're back at the feed picker.

```
┌──────────────────────────────────────────────────────────┐
│ ☰ Tech                       3 / 27         ⤓ Readeck ▲ ▼│
├──────────────────────────────────────────────────────────┤
│                                                          │
│        LWN · Jonathan Corbet · 31 Oct 2026 · 6 min       │
│                                                          │
│        A look at the new scheduler                       │
│        lwn.net                                           │
│                                                          │
│        Lorem ipsum dolor sit amet, consectetur           │
│        adipiscing elit. Article text in a calm,          │
│        centered ~85 character column…                    │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

- **Unread only.** Read items never appear, except ones you passed during this session, so **k** can take you back to undo.
- **Miniflux does the heavy lifting.** It fetches the feeds and stores what you've read. The app is a thin client with no local database.
- **Follows your Omarchy theme live.** Colours, the UI font and window rounding update the moment you switch themes.
- **Readeck in one key.** Press **b** to send the article to [Readeck](https://readeck.org), mark it read, and move on.

## Keys

| Key | Action |
| --- | --- |
| <kbd>j</kbd> / <kbd>↓</kbd> | Next article (**marks read**) |
| <kbd>n</kbd> | Skip: next article, **keep unread** |
| <kbd>k</kbd> / <kbd>↑</kbd> | Previous article |
| <kbd>m</kbd> | Toggle read / unread |
| <kbd>Space</kbd> / <kbd>Shift</kbd>+<kbd>Space</kbd> | Scroll down / up; <kbd>Space</kbd> at the end of an article goes to the next one |
| <kbd>o</kbd> | Open the original in your browser |
| <kbd>b</kbd> | Save to Readeck, mark read, go to the next article |
| <kbd>f</kbd> | Fetch the full article (Miniflux's readability scraper) |
| <kbd>Shift</kbd>+<kbd>A</kbd> | Mark everything in the current list read (asks first) |
| <kbd>g</kbd> / <kbd>Tab</kbd> | Feeds & categories picker |
| <kbd>r</kbd> | Refresh all feeds (in the picker) |
| <kbd>Ctrl</kbd> <kbd>+</kbd> / <kbd>−</kbd> / <kbd>0</kbd> | Article font size (remembered) |
| <kbd>?</kbd> | Show the keys |
| <kbd>q</kbd> | Quit |

The picker lists **All unread**, then each category with its feeds underneath, with unread counts. Anything with nothing unread is hidden. Move with <kbd>j</kbd>/<kbd>k</kbd> and open with <kbd>Enter</kbd>.

## Install

Requirements: Arch / Omarchy with `rust` (or `rustup`), `npm`, and `webkit2gtk-4.1`.

```bash
git clone https://github.com/kletse/kletse-rss-reader.git
cd kletse-rss-reader/packaging
makepkg -si
```

This installs `/usr/bin/kletse-rss`, a launcher entry (open it with <kbd>Super</kbd>+<kbd>Space</kbd> → *Kletse RSS*) and the icon. To uninstall, run `sudo pacman -R kletse-rss`.

## Configure

On first launch the app creates `~/.config/kletse-rss/config.toml` (mode `600`):

```toml
[miniflux]
url = "https://miniflux.example.com"
# Miniflux → Settings → API Keys → Create a new API key
api_key = ""

[readeck]
url = "https://readeck.example.com"
# Readeck → Profile → API Tokens → Create a new token (needs bookmarks write)
token = ""
```

Fill in the keys and restart the app. You only need the Readeck token if you use <kbd>b</kbd>.

## Develop

```bash
npm install
npm run dev        # Tauri dev window with hot reload
npm run check      # type-check the UI
npm run build      # release binary → src-tauri/target/release/kletse-rss
```

```
src/                 TypeScript UI (no framework)
  main.ts            queue state, keyboard, reader view
  picker.ts          feeds & categories overlay
  content.ts         sanitising article HTML, embeds → links, code highlighting
  theme.ts           Omarchy palette → CSS variables, live
  api.ts             typed wrappers for the Rust commands
src-tauri/src/
  lib.rs             Tauri commands
  miniflux.rs        Miniflux API client
  readeck.rs         Readeck "save bookmark"
  theme.rs           reads ~/.local/state/omarchy/current/theme/colors.toml, watches for switches
  config.rs          ~/.config/kletse-rss/config.toml
packaging/           PKGBUILD, .desktop entry, icon
```

### How it handles articles

- Article HTML is sanitised with DOMPurify. Scripts, styles and forms are dropped, and iframes and videos become a plain *▶ Embedded media* link.
- Images load (through Miniflux's media proxy if you have it enabled).
- Every link opens in your default browser, never inside the app.
- Code blocks are highlighted with highlight.js using the theme's own colours.

## Not (yet) in scope

Managing feeds (use the Miniflux web UI), starring, browsing read items, offline reading, notifications and a bar widget.

## License

[MIT](LICENSE)
