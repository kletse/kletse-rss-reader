// The small overlays: key help, yes/no confirmation, and the toast.
import { type Accessor, For, type JSX, Show } from "solid-js";
import { KEYS } from "../keys";
import { confirmText, type Toast as ToastMessage, toastMessage } from "../state/ui";

export function Help(): JSX.Element {
  return (
    <div id="help" class="overlay">
      <div class="panel" id="help-panel">
        <div class="panel-title">Keys</div>
        <dl>
          <For each={KEYS}>
            {([key, action]: readonly [string, string]): JSX.Element => (
              <>
                <dt>{key}</dt>
                <dd>{action}</dd>
              </>
            )}
          </For>
        </dl>
      </div>
    </div>
  );
}

export function Confirm(): JSX.Element {
  return (
    <div id="confirm" class="overlay">
      <div class="panel small">
        <p id="confirm-text">{confirmText()}</p>
        <p class="hint">Enter / y to confirm · Esc / n to cancel</p>
      </div>
    </div>
  );
}

export function Toast(): JSX.Element {
  return (
    <Show when={toastMessage()}>
      {(t: Accessor<ToastMessage>): JSX.Element => (
        <div id="toast" role="status" data-kind={t().kind}>
          {t().message}
        </div>
      )}
    </Show>
  );
}
