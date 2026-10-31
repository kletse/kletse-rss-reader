import { createEffect, For, Match, Switch } from "solid-js";
import { choose, closePicker, openSettings, pickerCursor, pickerList, queue } from "../state";

/** The feeds/categories overlay: "All unread", categories, and their feeds with unread counts. */
export function Picker() {
  let list!: HTMLUListElement;
  const error = () => { const l = pickerList(); return l.kind === "error" ? l.message : undefined; };
  const items = () => { const l = pickerList(); return l.kind === "items" ? l.items : undefined; };
  createEffect(() => {
    pickerCursor();
    pickerList();
    list.querySelector(".active")?.scrollIntoView({ block: "nearest" });
  });

  return (
    <div id="picker" class="overlay" onClick={(e) => e.target === e.currentTarget && queue().length && closePicker()}>
      <div class="panel">
        <div class="panel-title">
          <span>Feeds</span>
          <span class="hint">j/k · Enter or → · r refresh · Esc</span>
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
                <For each={items()} fallback={<li class="empty">Nothing unread. Press r to refresh.</li>}>
                  {(item, i) => (
                    <li class={`depth-${item.depth}`} classList={{ active: i() === pickerCursor() }} onClick={() => choose(item)}>
                      <span class="label">{item.label}</span>
                      <span class="count">{item.unread}</span>
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
