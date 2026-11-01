// Onboarding (first run) and Settings: Miniflux is required, Readeck optional.
// Saving checks both servers before anything is written.
import { type Accessor, createResource, createSignal, For, type JSX, onMount, Show } from "solid-js";
import { createStore, type SetStoreFunction } from "solid-js/store";
import { api, type Settings as Values } from "../api";
import { closeSettings } from "../state/settings";
import { errorMessage } from "../state/ui";

interface Field {
  key: keyof Values;
  label: string;
  placeholder: string;
  secret: boolean;
}

const MINIFLUX: Field[] = [
  { key: "miniflux_url", label: "Miniflux URL", placeholder: "https://miniflux.example.com", secret: false },
  {
    key: "miniflux_api_key",
    label: "API key",
    placeholder: "Settings → API Keys → Create a new API key",
    secret: true,
  },
];
const READECK: Field[] = [
  { key: "readeck_url", label: "Readeck URL", placeholder: "https://readeck.example.com", secret: false },
  {
    key: "readeck_token",
    label: "API token",
    placeholder: "Profile → API Tokens (needs “Bookmarks: write”)",
    secret: true,
  },
];

type Inputs = Partial<Record<keyof Values, HTMLInputElement>>;

function Fieldset(props: {
  title: string;
  note: string;
  fields: Field[];
  values: Values;
  setValues: SetStoreFunction<Values>;
  inputs: Inputs;
}): JSX.Element {
  return (
    <fieldset>
      <legend>
        {props.title}
        <span class="hint">{` · ${props.note}`}</span>
      </legend>
      <For each={props.fields}>
        {(f: Field): JSX.Element => (
          <label>
            <span>{f.label}</span>
            <input
              ref={(el: HTMLInputElement): void => {
                props.inputs[f.key] = el;
              }}
              type={f.secret ? "password" : "url"}
              placeholder={f.placeholder}
              spellcheck={false}
              autocomplete="off"
              value={props.values[f.key]}
              onInput={(e: InputEvent & { currentTarget: HTMLInputElement }): void =>
                props.setValues(f.key, e.currentTarget.value)
              }
            />
          </label>
        )}
      </For>
    </fieldset>
  );
}

function Form(props: { firstRun: boolean; initial: Values; problem: string | undefined }): JSX.Element {
  // The form starts from the loaded values once.
  const [values, setValues] = createStore<Values>({ ...props.initial });
  const [error, setError] = createSignal("");
  const [checking, setChecking] = createSignal(false);
  const inputs: Inputs = {};

  onMount(() => inputs[values.miniflux_url === "" ? "miniflux_url" : "miniflux_api_key"]?.focus());

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    setError("");
    setChecking(true);
    try {
      closeSettings(await api.saveSettings({ ...values }));
    } catch (err) {
      setError(errorMessage(err));
      setChecking(false);
    }
  }

  return (
    <form class="settings" onSubmit={(e: SubmitEvent): void => void submit(e)}>
      <h1>{props.firstRun ? "Welcome to Kletse RSS" : "Settings"}</h1>
      <p class="intro">
        {props.firstRun
          ? "Connect your Miniflux server to start reading. Readeck is optional: add it to save articles for later with b."
          : "Changes are checked against your servers before they are saved."}
      </p>
      <Show when={props.problem}>
        {(problem: Accessor<string>): JSX.Element => (
          <p class="form-notice" role="status">
            {problem()}
          </p>
        )}
      </Show>
      <Fieldset
        title="Miniflux"
        note="required"
        fields={MINIFLUX}
        values={values}
        setValues={setValues}
        inputs={inputs}
      />
      <Fieldset
        title="Readeck"
        note="optional: leave empty to hide the bookmark button"
        fields={READECK}
        values={values}
        setValues={setValues}
        inputs={inputs}
      />
      <p class="form-error" role="alert">
        {error()}
      </p>
      <div class="actions">
        <Show when={!props.firstRun}>
          <button type="button" onClick={(): void => closeSettings(undefined)}>
            Cancel
          </button>
        </Show>
        <button type="submit" class="primary" disabled={checking()}>
          <Show when={checking()} fallback={props.firstRun ? "Connect" : "Save"}>
            Checking…
          </Show>
        </button>
      </div>
    </form>
  );
}

/** Shown in place of the article. Esc cancels (keyboard.ts), except on first run. */
export function Settings(props: { firstRun: boolean }): JSX.Element {
  const [initial] = createResource(api.getSettings);
  const [problem] = createResource(api.configProblem);
  return (
    <Show when={initial()}>
      {(v: Accessor<Values>): JSX.Element => <Form firstRun={props.firstRun} initial={v()} problem={problem()} />}
    </Show>
  );
}
