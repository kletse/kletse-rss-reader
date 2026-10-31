// The small overlays: key help, yes/no confirmation, and the toast.
import { For, Show } from "solid-js";
import { KEYS } from "../keys";
import { confirmText, toastMessage } from "../state";

export function Help() {
  return (
    <div id="help" class="overlay">
      <div class="panel" id="help-panel">
        <div class="panel-title">Keys</div>
        <dl>
          <For each={KEYS}>{([k, v]) => <><dt>{k}</dt><dd>{v}</dd></>}</For>
        </dl>
      </div>
    </div>
  );
}

export function Confirm() {
  return (
    <div id="confirm" class="overlay">
      <div class="panel small">
        <p id="confirm-text">{confirmText()}</p>
        <p class="hint">Enter / y to confirm · Esc / n to cancel</p>
      </div>
    </div>
  );
}

export function Toast() {
  return (
    <Show when={toastMessage()}>
      {(t) => <div id="toast" class={t().kind}>{t().message}</div>}
    </Show>
  );
}
