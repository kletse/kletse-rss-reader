// Kletse RSS: one unread article at a time, oldest first.
import { createEffect, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import { Article } from "./components/Article";
import { Confirm, Help, Toast } from "./components/Overlays";
import { Picker } from "./components/Picker";
import { Settings } from "./components/Settings";
import { TopBar } from "./components/TopBar";
import { api } from "./api";
import { isMac } from "./keys";
import * as s from "./state";

/** The one keyboard handler. Whatever overlay is up gets the keys; otherwise the reader does. */
export function onKey(e: KeyboardEvent) {
  const consumed = handle(e);
  if (consumed) e.preventDefault();
}

function handle(e: KeyboardEvent): boolean {
  switch (s.overlay()) {
    case "confirm": {
      const yes = e.key === "Enter" || e.key === "y";
      const no = e.key === "Escape" || e.key === "n";
      if (yes || no) s.answerConfirm(yes);
      return yes || no;
    }
    case "onboarding":
      return false; // can't be cancelled; all keys go to the form
    case "settings":
      if (e.key !== "Escape") return false;
      s.closeSettings(null);
      return true;
    case "help":
      if (!["Escape", "?", "q"].includes(e.key)) return false;
      s.setOverlay("none");
      return true;
  }
  // Never treat typing as shortcuts.
  if ((e.target as HTMLElement).closest?.("input, textarea")) return false;

  if (e.key === ",") { s.openSettings(); return true; }

  if (s.overlay() === "picker") {
    switch (e.key) {
      case "j": case "ArrowDown": s.movePicker(1); return true;
      case "k": case "ArrowUp": s.movePicker(-1); return true;
      case "Enter": case "ArrowRight": s.choose(); return true;
      case "r": void s.refreshFeeds(); return true;
      case "Escape": case "g": case "p": case "Tab": if (s.pickerCanClose()) s.closePicker(); return true;
    }
    return false;
  }

  if (isMac() ? e.metaKey : e.ctrlKey) {
    if (e.key === "=" || e.key === "+") s.setFontSize(s.fontSize + 1);
    else if (e.key === "-") s.setFontSize(s.fontSize - 1);
    else if (e.key === "0") { s.setFontSize(s.DEFAULT_FONT_SIZE); s.setWidth(s.DEFAULT_WIDTH); }
    else return false;
    return true;
  }
  if (e.altKey || e.metaKey) return false;

  const caughtUp = !s.current() && s.queue().length > 0;
  switch (e.key) {
    case "j": case "ArrowDown": s.next(true); break;
    case "n": s.next(false); break;
    case "k": case "ArrowUp": s.prev(); break;
    case "Enter": if (caughtUp) s.openPicker(); else return false; break;
    case " ":
      if (e.shiftKey) s.scrollPage(-1);
      else if (s.atBottom()) s.next(true);
      else s.scrollPage(1);
      break;
    case "m": void s.toggleRead(); break;
    case "o": void s.openOriginal(); break;
    case "b": void s.saveToReadeck(); break;
    case "f": void s.fetchFull(); break;
    case "A": void s.markAllRead(); break;
    case "g": case "p": case "Tab": case "Escape": s.openPicker(); break;
    case "[": s.setWidth(s.widthFactor - 4); break;
    case "]": s.setWidth(s.widthFactor + 4); break;
    case "?": s.setOverlay("help"); break;
    case "q": void api.quit(); break;
    default: return false;
  }
  return true;
}

/** Links (in articles and titles) always open in the browser. */
function onClick(e: MouseEvent) {
  const a = (e.target as HTMLElement).closest?.("a[href]") as HTMLAnchorElement | null;
  if (!a) return;
  e.preventDefault();
  void s.openLink(a.href);
}

export function App() {
  let reader!: HTMLElement;
  onMount(() => {
    s.setReader(reader);
    window.addEventListener("keydown", onKey);
    document.addEventListener("click", onClick);
  });
  onCleanup(() => {
    window.removeEventListener("keydown", onKey);
    document.removeEventListener("click", onClick);
  });
  // A new article (or the settings form) starts at the top.
  createEffect(on([s.queue, s.index, s.inSettings], s.scrollToTop));

  return (
    <>
      <TopBar />
      <main id="reader" tabindex="-1" ref={reader}>
        <Switch>
          <Match when={s.inSettings()}><Settings firstRun={s.overlay() === "onboarding"} /></Match>
          <Match when={s.loading()}><div class="message"><p>Loading…</p></div></Match>
          <Match when={s.current()} keyed>{(entry) => <Article entry={entry} />}</Match>
          <Match when={s.queue().length > 0}>
            <div class="message">
              <h1>All caught up</h1>
              <p>Press j or Enter for the feeds, k to go back.</p>
            </div>
          </Match>
        </Switch>
      </main>
      <Show when={s.overlay() === "picker"}><Picker /></Show>
      <Show when={s.overlay() === "help"}><Help /></Show>
      <Show when={s.overlay() === "confirm"}><Confirm /></Show>
      <Toast />
    </>
  );
}
