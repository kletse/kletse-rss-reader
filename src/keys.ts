// Platform modifier and the key reference shown in the help overlay.

const MAC_PLATFORM = /^Mac/iu;

/** macOS uses ⌘ where Linux uses Ctrl for the font-size shortcuts. */
export function isMac(platform: string = navigator.platform): boolean {
  return MAC_PLATFORM.test(platform);
}
export const MOD: string = isMac() ? "⌘" : "Ctrl";

/**
 * A key as `KeyboardEvent.key` names it, the same string the keymaps in keyboard.ts bind, so keys.test.ts can
 * check the help against them. The object form is a key held with Shift or the platform modifier ({@link MOD}).
 */
export type Chord = string | { readonly key: string; readonly held: "Shift" | "Mod" };

/** Keys whose `KeyboardEvent.key` isn't what the help should print. */
const KEY_NAMES: ReadonlyMap<string, string> = new Map([
  [" ", "Space"],
  ["ArrowDown", "↓"],
  ["ArrowUp", "↑"],
  ["ArrowLeft", "←"],
  ["ArrowRight", "→"],
  ["Escape", "Esc"],
  ["-", "−"],
]);
const CAPITAL = /^[A-Z]$/u;

/** How the help (and the README) names a key: arrows as glyphs, Esc, Space, a real minus sign. */
export function keyName(key: string): string {
  return KEY_NAMES.get(key) ?? key;
}

function shift(key: string): Chord {
  return { key, held: "Shift" };
}
function mod(key: string): Chord {
  return { key, held: "Mod" };
}

/** A capital letter is bound as itself but typed with Shift, so the help says so. */
function chordLabel(chord: Chord): string {
  if (typeof chord === "string") {
    return CAPITAL.test(chord) ? chordLabel(shift(chord)) : keyName(chord);
  }
  return chord.held === "Shift" ? `Shift+${keyName(chord.key)}` : `${MOD} ${keyName(chord.key)}`;
}

function label(chords: readonly Chord[]): string {
  return chords.map(chordLabel).join(" / ");
}

/** One line of the help overlay. */
export interface HelpRow {
  /** The keys the line shows, in order. Modifier chords work in the reader. */
  readonly keys: readonly Chord[];
  readonly action: string;
  /** Where the keys work besides the reader: only in the picker, or in both. Unset means the reader. */
  readonly where?: "picker" | "both";
  /** Picker keys the line mentions in passing, e.g. "(→ / o picks a row)". */
  readonly pickerNote?: { readonly keys: readonly Chord[]; readonly text: string };
}

/** The help overlay, in reading order. keys.test.ts fails when this and the keymaps disagree. */
export const HELP: readonly HelpRow[] = [
  { keys: ["j", "ArrowDown"], action: "next (marks read)" },
  { keys: ["n"], action: "skip (keeps unread)" },
  { keys: ["k", "ArrowUp"], action: "previous" },
  { keys: ["m"], action: "toggle read / unread" },
  { keys: ["o"], action: "open original in browser" },
  { keys: ["b"], action: "save to Readeck, then next" },
  { keys: ["f"], action: "fetch full article" },
  { keys: ["A"], action: "mark whole queue read" },
  {
    keys: ["g", "p", "Tab", "Escape", "ArrowLeft"],
    action: "feeds picker",
    pickerNote: { keys: ["ArrowRight", "o"], text: "picks a row" },
  },
  { keys: ["r"], action: "refresh feeds", where: "picker" },
  { keys: ["h"], action: "history: read items, newest first (again for unread)", where: "both" },
  { keys: [" ", shift(" ")], action: "scroll; Space at the end = next" },
  { keys: [mod("+"), mod("-")], action: "font size" },
  { keys: ["[", "]"], action: "narrower / wider column" },
  { keys: [mod("0")], action: "reset font size and width" },
  { keys: [","], action: "settings", where: "both" },
  { keys: ["?"], action: "this help", where: "both" },
  { keys: ["q"], action: "quit" },
];

function actionText(row: HelpRow): string {
  const where = row.where === "picker" ? " (in picker)" : "";
  const note = row.pickerNote ? ` (${label(row.pickerNote.keys)} ${row.pickerNote.text})` : "";
  return `${row.action}${where}${note}`;
}

/** The help overlay's lines as text: the keys, then what they do. */
export const KEYS: readonly (readonly [key: string, action: string])[] = HELP.map(
  (row: HelpRow): readonly [string, string] => [label(row.keys), actionText(row)],
);
