// Onboarding (first run) and Settings: Miniflux is required, Readeck optional.
// Saving checks both servers before anything is written.
import { createResource, createSignal, For, onMount, Show } from "solid-js";
import { createStore } from "solid-js/store";
import { api, type Settings as Values } from "../api";
import { closeSettings } from "../state";

interface Field { key: keyof Values; label: string; placeholder: string; secret?: boolean }

const MINIFLUX: Field[] = [
  { key: "miniflux_url", label: "Miniflux URL", placeholder: "https://miniflux.example.com" },
  { key: "miniflux_api_key", label: "API key", placeholder: "Settings → API Keys → Create a new API key", secret: true },
];
const READECK: Field[] = [
  { key: "readeck_url", label: "Readeck URL", placeholder: "https://readeck.example.com" },
  { key: "readeck_token", label: "API token", placeholder: "Profile → API Tokens (needs “Bookmarks: write”)", secret: true },
];

/** Shown in place of the article. Esc cancels (handled in App), except on first run. */
export function Settings(props: { firstRun: boolean }) {
  const [initial] = createResource(api.getSettings);
  return <Show when={initial()}>{(v) => <Form firstRun={props.firstRun} initial={v()} />}</Show>;
}

function Form(props: { firstRun: boolean; initial: Values }) {
  // The form starts from the loaded values once.
  const [values, setValues] = createStore<Values>({ ...props.initial });
  const [error, setError] = createSignal("");
  const [checking, setChecking] = createSignal(false);
  const inputs: Partial<Record<keyof Values, HTMLInputElement>> = {};
  const label = () => (props.firstRun ? "Connect" : "Save");

  onMount(() => inputs[values.miniflux_url ? "miniflux_api_key" : "miniflux_url"]?.focus());

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    setError("");
    setChecking(true);
    try {
      closeSettings(await api.saveSettings({ ...values }));
    } catch (err) {
      setError(String(err));
      setChecking(false);
    }
  }

  const fieldset = (title: string, note: string, fields: Field[]) => (
    <fieldset>
      <legend>{title}<span class="hint">{` · ${note}`}</span></legend>
      <For each={fields}>
        {(f) => (
          <label>
            <span>{f.label}</span>
            <input
              ref={(el) => (inputs[f.key] = el)}
              type={f.secret ? "password" : "url"}
              placeholder={f.placeholder}
              spellcheck={false}
              autocomplete="off"
              value={values[f.key]}
              onInput={(e) => setValues(f.key, e.currentTarget.value)}
            />
          </label>
        )}
      </For>
    </fieldset>
  );

  return (
    <form class="settings" onSubmit={submit}>
      <h1>{props.firstRun ? "Welcome to Kletse RSS" : "Settings"}</h1>
      <p class="intro">
        {props.firstRun
          ? "Connect your Miniflux server to start reading. Readeck is optional: add it to save articles for later with b."
          : "Changes are checked against your servers before they are saved."}
      </p>
      {fieldset("Miniflux", "required", MINIFLUX)}
      {fieldset("Readeck", "optional: leave empty to hide the bookmark button", READECK)}
      <p class="form-error" role="alert">{error()}</p>
      <div class="actions">
        <Show when={!props.firstRun}>
          <button type="button" onClick={() => closeSettings(null)}>Cancel</button>
        </Show>
        <button type="submit" class="primary" disabled={checking()}>{checking() ? "Checking…" : label()}</button>
      </div>
    </form>
  );
}
