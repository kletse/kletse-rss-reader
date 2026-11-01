// Platform modifier and the key reference shown in the help overlay.

/** macOS uses ⌘ where Linux uses Ctrl for the font-size shortcuts. */
export const isMac = (platform: string = navigator.platform): boolean => /^Mac/i.test(platform);
export const MOD = isMac() ? "⌘" : "Ctrl";

export const KEYS: [string, string][] = [
  ["j / ↓", "next (marks read)"],
  ["n", "skip (keeps unread)"],
  ["k / ↑", "previous"],
  ["m", "toggle read / unread"],
  ["o", "open original in browser"],
  ["b", "save to Readeck, then next"],
  ["f", "fetch full article"],
  ["Shift+A", "mark whole queue read"],
  ["g / p / Tab / Esc", "feeds picker"],
  ["r", "refresh feeds (in picker)"],
  ["h", "history: read items, newest first (again for unread)"],
  ["Space / Shift+Space", "scroll; Space at the end = next"],
  [`${MOD} + / ${MOD} −`, "font size"],
  ["[ / ]", "narrower / wider column"],
  [`${MOD} 0`, "reset font size and width"],
  [",", "settings"],
  ["?", "this help"],
  ["q", "quit"],
];
