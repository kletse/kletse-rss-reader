import { createEffect, For, Match, Switch } from "solid-js";
import { choose, closePicker, openSettings, pickerCursor, pickerItems, pickerList, pickerMode, queue, toggleMode } from "../state";

/**
 * The feeds/categories overlay: "All unread", categories, and their feeds with unread counts.
 * In history mode the same, with read counts.
 */
export function Picker() {
  let list!: HTMLUListElement;
  const error = () => { const l = pickerList(); return l.kind === "error" ? l.message : undefined; };
  const items = () => (pickerList().kind === "ready" ? pickerItems() : undefined);
  const history = () => pickerMode() === "history";
  createEffect(() => {
    pickerCursor();
    pickerList();
    list.querySelector(".active")?.scrollIntoView({ block: "nearest" });
  });

  return (
    <div id="picker" class="overlay" onClick={(e) => e.target === e.currentTarget && queue().length && closePicker()}>
      <div class="panel">
        <div class="panel-title">
          <span>{history() ? "History" : "Feeds"}</span>
          <span class="hint">j/k · Enter · r refresh · h {history() ? "unread" : "history"} · Esc</span>
          <button id="mode-btn" title={history() ? "Unread items (h)" : "Read items, newest first (h)"} onClick={toggleMode}>
            {history() ? "Unread" : "History"}
          </button>
          <button id="settings-btn" title="Settings (,)" onClick={() => openSettings()}>
            <svg class="btn-icon" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3" /><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9 7 7M17 17l2.1 2.1M4.9 19.1 7 17M17 7l2.1-2.1" /></svg>
            Settings
          </button>
        </div>
        <ul id="picker-list" ref={list}>
          <Switch>
            <Match when={pickerList().kind === "loading"}><li class="empty">Loading…</li></Match>
            <Match when={error() !== undefined}>
              <li class="empty error">{error()}</li>
            </Match>
            <Match when={items()}>
              {(items) => (
                <For each={items()} fallback={<li class="empty">{history() ? "Nothing read yet. Press h for unread items." : "Nothing unread. Press r to refresh, or h for history."}</li>}>
                  {(item, i) => (
                    <li class={`depth-${item.depth}`} classList={{ active: i() === pickerCursor() }} onClick={() => choose(item)}>
                      <span class="label">{item.label}</span>
                      <span class="count">{item.count}</span>
                    </li>
                  )}
                </For>
              )}
            </Match>
          </Switch>
        </ul>
      </div>
    </div>
  );
}
