// Platform modifier and the key reference shown in the help overlay.

const MAC_PLATFORM = /^Mac/iu;

/** macOS uses ⌘ where Linux uses Ctrl for the font-size shortcuts. */
export function isMac(platform: string = navigator.platform): boolean {
  return MAC_PLATFORM.test(platform);
}
export const MOD: string = isMac() ? "⌘" : "Ctrl";

export const KEYS: readonly (readonly [key: string, action: string])[] = [
  ["j / ↓", "next (marks read)"],
  ["n", "skip (keeps unread)"],
  ["k / ↑", "previous"],
  ["m", "toggle read / unread"],
  ["o", "open original in browser"],
  ["b", "save to Readeck, then next"],
  ["f", "fetch full article"],
  ["Shift+A", "mark whole queue read"],
  ["g / p / Tab / Esc / ←", "feeds picker (→ / o picks a row)"],
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
