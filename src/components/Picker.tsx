import { type Accessor, createEffect, For, type JSX, Match, Switch } from "solid-js";
import type { PickerItem } from "../picker-items";
import {
  choose,
  closePicker,
  pickerCanClose,
  pickerCursor,
  pickerItems,
  pickerList,
  pickerMode,
  toggleMode,
} from "../state/picker";
import { openSettings } from "../state/settings";
import { openHelp } from "../state/ui";

function Row(props: { item: PickerItem; active: boolean }): JSX.Element {
  return (
    <li>
      <button
        type="button"
        aria-current={props.active}
        data-depth={props.item.depth}
        tabindex="-1"
        onClick={(): void => choose(props.item)}
      >
        <span class="label">{props.item.label}</span>
        <span class="count">{props.item.count}</span>
      </button>
    </li>
  );
}

function Rows(props: { history: boolean }): JSX.Element {
  return (
    <For
      each={pickerItems()}
      fallback={
        <li class="empty">
          {props.history
            ? "Nothing read yet. Press h for unread items."
            : "Nothing unread. Press r to refresh, or h for history."}
        </li>
      }
    >
      {(item: PickerItem, i: Accessor<number>): JSX.Element => <Row item={item} active={i() === pickerCursor()} />}
    </For>
  );
}

/**
 * The feeds/categories overlay: "All unread", categories, and their feeds with unread counts.
 * In history mode the same, with read counts.
 */
export function Picker(): JSX.Element {
  let list: HTMLUListElement | undefined;
  const history = (): boolean => pickerMode() === "history";
  const error = (): string | undefined => {
    const l = pickerList();
    return l.kind === "error" ? l.message : undefined;
  };
  createEffect(() => {
    pickerCursor();
    pickerList();
    list?.querySelector("[aria-current='true']")?.scrollIntoView({ block: "nearest" });
  });
  const closeOnBackdrop = (e: MouseEvent): void => {
    if (e.target === e.currentTarget && pickerCanClose()) {
      closePicker();
    }
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: a backdrop click closes the picker for the mouse; Esc does it for the keyboard (keyboard.ts)
    <div id="picker" class="overlay" role="presentation" onClick={closeOnBackdrop}>
      <div class="panel" role="dialog" aria-label={history() ? "History" : "Feeds"}>
        <div class="panel-title">
          <span>{history() ? "History" : "Feeds"}</span>
          <button type="button" id="help-btn" title="Keys (?)" onClick={openHelp}>
            ?
          </button>
          <button
            type="button"
            id="mode-btn"
            title={history() ? "Unread items (h)" : "Read items, newest first (h)"}
            onClick={toggleMode}
          >
            {history() ? "Unread" : "History"}
          </button>
          <button type="button" id="settings-btn" title="Settings (,)" onClick={(): void => openSettings()}>
            <svg class="btn-icon" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
              <circle cx="12" cy="12" r="3" />
            </svg>
            Settings
          </button>
        </div>
        <ul
          id="picker-list"
          ref={(el: HTMLUListElement): void => {
            list = el;
          }}
        >
          <Switch>
            <Match when={pickerList().kind === "loading"}>
              <li class="empty">Loading…</li>
            </Match>
            <Match when={error()}>
              {(message: Accessor<string>): JSX.Element => <li class="empty error">{message()}</li>}
            </Match>
            <Match when={pickerList().kind === "ready"}>
              <Rows history={history()} />
            </Match>
          </Switch>
        </ul>
      </div>
    </div>
  );
}
