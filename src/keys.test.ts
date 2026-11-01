// Keeps the help overlay (keys.ts), the keymaps (keyboard.ts) and the README's key table from drifting apart.
import { describe, expect, it } from "vitest";
import readme from "../README.md?raw";
import { boundKeys, type KeyMode } from "./keyboard";
import { type Chord, HELP, type HelpRow, KEYS, keyName } from "./keys";

/**
 * Bound keys the help leaves out on purpose. A key bound without a help line fails the test until it's added to
 * the help or here.
 */
const UNLISTED: Readonly<Record<KeyMode, readonly string[]>> = {
  // Only acts on the "all caught up" screen, which says so itself.
  reader: ["Enter"],
  // Moving works as in the reader, Enter picks as everywhere, and the keys that open the picker also close it.
  picker: ["j", "ArrowDown", "k", "ArrowUp", "Enter", "Escape", "g", "p", "Tab"],
  // = is + without Shift.
  modifier: ["="],
};

function chordKey(chord: Chord): string {
  return typeof chord === "string" ? chord : chord.key;
}

function isMod(chord: Chord): boolean {
  return typeof chord !== "string" && chord.held === "Mod";
}

function worksIn(mode: "reader" | "picker", row: HelpRow): boolean {
  return mode === "reader" ? row.where !== "picker" : row.where !== undefined;
}

/** The keys the help says work in a mode. */
function documented(mode: KeyMode): Set<string> {
  if (mode === "modifier") {
    return new Set(HELP.flatMap((row: HelpRow): Chord[] => row.keys.filter(isMod)).map(chordKey));
  }
  const rows = HELP.filter((row: HelpRow): boolean => worksIn(mode, row));
  const plain = rows.flatMap((row: HelpRow): Chord[] => row.keys.filter((chord: Chord): boolean => !isMod(chord)));
  const notes = mode === "picker" ? HELP.flatMap((row: HelpRow): readonly Chord[] => row.pickerNote?.keys ?? []) : [];
  return new Set([...plain, ...notes].map(chordKey));
}

const MODES: readonly KeyMode[] = ["reader", "picker", "modifier"];

describe("help overlay", () => {
  it.each(MODES)("names every key the %s binds", (mode: KeyMode) => {
    const named = new Set([...documented(mode), ...UNLISTED[mode]]);
    expect([...boundKeys(mode)].filter((key: string): boolean => !named.has(key))).toEqual([]);
  });

  it.each(MODES)("names no key the %s doesn't bind", (mode: KeyMode) => {
    const bound = boundKeys(mode);
    expect([...documented(mode)].filter((key: string): boolean => !bound.has(key))).toEqual([]);
  });

  it("reads as before", () => {
    expect(KEYS).toEqual([
      ["j / ↓", "next (marks read)"],
      ["n", "skip (keeps unread)"],
      ["k / ↑", "previous"],
      ["m", "toggle read / unread"],
      ["o", "open original in browser"],
      ["b", "save to Readeck, then next"],
      ["f", "fetch full article (again for feed version)"],
      ["Shift+A", "mark whole queue read"],
      ["g / p / Tab / Esc / ←", "feeds picker (→ / o picks a row)"],
      ["r", "refresh feeds (in picker)"],
      ["h", "history: read items, newest first (again for unread)"],
      ["Space / Shift+Space", "scroll; Space at the end = next"],
      ["Ctrl + / Ctrl −", "font size"],
      ["[ / ]", "narrower / wider column"],
      ["Ctrl 0", "reset font size and width"],
      [",", "settings"],
      ["?", "this help"],
      ["q", "quit"],
    ]);
  });
});

/** The README writes Ctrl and the help ⌘ on a Mac, and Shift is part of the key, so rows compare without them. */
const MODIFIERS = new Set(["Ctrl", "⌘", "Shift"]);

function sorted(items: readonly string[]): string[] {
  return [...items].sort((a: string, b: string): number => a.localeCompare(b));
}

function rowKeys(names: readonly string[]): string {
  return sorted([...new Set(names.filter((name: string): boolean => !MODIFIERS.has(name)))]).join(" ");
}

/**
 * The <kbd> names in the first cell of each row of the README's key table. Only the table itself counts: a row that
 * strays below it renders as a stray line of text, not as part of the table.
 */
function readmeRows(): string[] {
  const section = readme.split("\n## Keys\n")[1]?.split("\n## ")[0] ?? "";
  const table =
    section
      .trim()
      .split("\n\n")
      .find((block: string): boolean => block.startsWith("| Key")) ?? "";
  return table
    .split("\n")
    .filter((line: string): boolean => line.startsWith("| <kbd>"))
    .map((line: string): string => {
      const cell = line.split("|")[1] ?? "";
      return rowKeys(
        cell
          .split("<kbd>")
          .slice(1)
          .map((part: string): string => part.split("</kbd>")[0] ?? ""),
      );
    });
}

describe("the README's key table", () => {
  it("has a row for each help line, with the same keys", () => {
    const help = HELP.map((row: HelpRow): string => rowKeys(row.keys.map(chordKey).map(keyName)));
    expect(sorted(readmeRows())).toEqual(sorted(help));
  });
});
