// The one keyboard handler. Whatever overlay is up gets the keys; otherwise the reader does.
import { api } from "./api";
import { isMac } from "./keys";
import {
  advance,
  choose,
  closePicker,
  movePicker,
  openPicker,
  pickerCanClose,
  refreshFeeds,
  toggleMode,
} from "./state/picker";
import { biggerFont, narrower, resetSize, smallerFont, wider } from "./state/prefs";
import {
  checking,
  current,
  fetchFull,
  markAllRead,
  openOriginal,
  prev,
  queue,
  saveToReadeck,
  toggleRead,
} from "./state/queue";
import { closeSettings, openSettings } from "./state/settings";
import { answerConfirm, atBottom, closeHelp, openHelp, overlay, scrollPage } from "./state/ui";

/** Handles a key; returns whether it was used (and the browser's default should be prevented). */
type Handler = (e: KeyboardEvent) => boolean;

/** A handler that always uses the key. */
function always(action: () => void): Handler {
  return (): boolean => {
    action();
    return true;
  };
}

function keymap(entries: [keys: string[], handler: Handler][]): ReadonlyMap<string, Handler> {
  return new Map(entries.flatMap(([keys, handler]) => keys.map((key): [string, Handler] => [key, handler])));
}

const PICKER_KEYS = keymap([
  [["j", "ArrowDown"], always(() => movePicker(1))],
  [["k", "ArrowUp"], always(() => movePicker(-1))],
  [["Enter", "ArrowRight", "o"], always(() => choose())],
  [["r"], always(() => void refreshFeeds())],
  [["h"], always(toggleMode)],
  [["?"], always(openHelp)],
  [
    ["Escape", "g", "p", "Tab"],
    always(() => {
      if (pickerCanClose()) {
        closePicker();
      }
    }),
  ],
]);

/** Ctrl (⌘ on macOS) combinations. */
const MODIFIER_KEYS = keymap([
  [["=", "+"], always(biggerFont)],
  [["-"], always(smallerFont)],
  [["0"], always(resetSize)],
]);

/** Space pages down, and at the end of an article goes to the next one; Shift+Space pages up. */
function space(e: KeyboardEvent): boolean {
  if (e.shiftKey) {
    scrollPage(-1);
  } else if (atBottom()) {
    advance(true);
  } else {
    scrollPage(1);
  }
  return true;
}

/** Enter only means something on the "all caught up" screen: back to the feeds. */
function enter(): boolean {
  const caughtUp = current() === undefined && queue().length > 0 && !checking();
  if (caughtUp) {
    openPicker();
  }
  return caughtUp;
}

const READER_KEYS = keymap([
  [["j", "ArrowDown"], always(() => advance(true))],
  [["n"], always(() => advance(false))],
  [["k", "ArrowUp"], always(prev)],
  [["Enter"], enter],
  [[" "], space],
  [["m"], always(() => void toggleRead())],
  [["o"], always(() => void openOriginal())],
  [["b"], always(() => void saveToReadeck())],
  [["f"], always(() => void fetchFull())],
  [["A"], always(() => void markAllRead())],
  [["h"], always(toggleMode)],
  [["g", "p", "Tab", "Escape", "ArrowLeft"], always(() => openPicker())],
  [["["], always(narrower)],
  [["]"], always(wider)],
  [["?"], always(openHelp)],
  [["q"], always(() => void api.quit())],
]);

const HELP_CLOSE = new Set(["Escape", "?", "q"]);

/** Overlays that take every key. Returns undefined when none of them is up. */
function modal(e: KeyboardEvent): boolean | undefined {
  switch (overlay()) {
    case "confirm": {
      const yes = e.key === "Enter" || e.key === "y";
      const no = e.key === "Escape" || e.key === "n";
      if (yes || no) {
        answerConfirm(yes);
      }
      return yes || no;
    }
    case "onboarding":
      return false; // can't be cancelled; all keys go to the form
    case "settings":
      return e.key === "Escape" && always(() => closeSettings(undefined))(e);
    case "help":
      return HELP_CLOSE.has(e.key) && always(closeHelp)(e);
    case "picker":
    case "none":
      return undefined;
  }
}

function isTyping(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest("input, textarea") !== null;
}

function run(keys: ReadonlyMap<string, Handler>, e: KeyboardEvent): boolean {
  return keys.get(e.key)?.(e) ?? false;
}

/** Keys that work in the reader and in the picker. */
const GLOBAL_KEYS = keymap([[[","], always(() => openSettings())]]);
const NO_KEYS: ReadonlyMap<string, Handler> = new Map();

/** Where the keyboard is, as far as which keys mean something. */
export type KeyMode = "reader" | "picker" | "modifier";

/** The keymaps each mode answers from (see handle() and keysFor()). */
const MODE_KEYS: Readonly<Record<KeyMode, readonly ReadonlyMap<string, Handler>[]>> = {
  reader: [GLOBAL_KEYS, READER_KEYS],
  picker: [GLOBAL_KEYS, PICKER_KEYS],
  modifier: [MODIFIER_KEYS],
};

/** Every key a mode binds, so keys.test.ts can check the help overlay (keys.ts) names each one. */
export function boundKeys(mode: KeyMode): ReadonlySet<string> {
  return new Set(MODE_KEYS[mode].flatMap((keys: ReadonlyMap<string, Handler>): string[] => [...keys.keys()]));
}

/** The keymap for where the keyboard is: the picker, a Ctrl/⌘ combination, or the reader. */
function keysFor(e: KeyboardEvent): ReadonlyMap<string, Handler> {
  if (overlay() === "picker") {
    return PICKER_KEYS;
  }
  if (isMac() ? e.metaKey : e.ctrlKey) {
    return MODIFIER_KEYS;
  }
  return e.altKey || e.metaKey ? NO_KEYS : READER_KEYS;
}

function handle(e: KeyboardEvent): boolean {
  const captured = modal(e);
  if (captured !== undefined) {
    return captured;
  }
  if (isTyping(e.target)) {
    return false; // never treat typing as shortcuts
  }
  return run(GLOBAL_KEYS, e) || run(keysFor(e), e);
}

export function onKey(e: KeyboardEvent): void {
  if (handle(e)) {
    e.preventDefault();
  }
}
