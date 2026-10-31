// Onboarding (first run) and Settings: Miniflux is required, Readeck optional.
// Saving checks both servers before anything is written.
import { api, type Settings, type Status } from "./api";
import { $, el } from "./ui";

interface Field { key: keyof Settings; label: string; placeholder: string; secret?: boolean; help?: string }

const MINIFLUX: Field[] = [
  { key: "miniflux_url", label: "Miniflux URL", placeholder: "https://miniflux.example.com" },
  { key: "miniflux_api_key", label: "API key", placeholder: "Settings → API Keys → Create a new API key", secret: true },
];
const READECK: Field[] = [
  { key: "readeck_url", label: "Readeck URL", placeholder: "https://readeck.example.com" },
  { key: "readeck_token", label: "API token", placeholder: "Profile → API Tokens (needs “Bookmarks: write”)", secret: true },
];

/**
 * Shows the form in place of the article. Resolves with the new status after a successful
 * save, or null when cancelled (Esc / Cancel; only possible when already configured).
 */
export async function showSettings(firstRun: boolean): Promise<Status | null> {
  const values = await api.getSettings();
  const reader = $("reader");
  const inputs = new Map<keyof Settings, HTMLInputElement>();

  const fieldset = (title: string, note: string, fields: Field[]) =>
    el("fieldset", {},
      el("legend", {}, title, el("span", { class: "hint" }, ` · ${note}`)),
      ...fields.map((f) => {
        const input = el("input", {
          type: f.secret ? "password" : "url",
          placeholder: f.placeholder,
          spellcheck: "false",
          autocomplete: "off",
        });
        input.value = values[f.key];
        inputs.set(f.key, input);
        return el("label", {}, el("span", {}, f.label), input);
      }));

  const error = el("p", { class: "form-error", role: "alert" });
  const submit = el("button", { type: "submit", class: "primary" }, firstRun ? "Connect" : "Save");
  const cancel = el("button", { type: "button" }, "Cancel");
  const form = el("form", { class: "settings" },
    el("h1", {}, firstRun ? "Welcome to Kletse RSS" : "Settings"),
    el("p", { class: "intro" }, firstRun
      ? "Connect your Miniflux server to start reading. Readeck is optional: add it to save articles for later with b."
      : "Changes are checked against your servers before they are saved."),
    fieldset("Miniflux", "required", MINIFLUX),
    fieldset("Readeck", "optional: leave empty to hide the bookmark button", READECK),
    error,
    el("div", { class: "actions" }, ...(firstRun ? [] : [cancel]), submit),
  );
  reader.replaceChildren(form);
  reader.scrollTop = 0;
  inputs.get(values.miniflux_url ? "miniflux_api_key" : "miniflux_url")!.focus();

  return new Promise((resolve) => {
    const done = (result: Status | null) => {
      window.removeEventListener("keydown", onKey, true);
      resolve(result);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !firstRun) { e.preventDefault(); done(null); }
    };
    window.addEventListener("keydown", onKey, true);
    cancel.addEventListener("click", () => done(null));

    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      const settings = Object.fromEntries([...inputs].map(([k, input]) => [k, input.value])) as unknown as Settings;
      error.textContent = "";
      submit.disabled = true;
      submit.textContent = "Checking…";
      try {
        done(await api.saveSettings(settings));
      } catch (err) {
        error.textContent = String(err);
        submit.disabled = false;
        submit.textContent = firstRun ? "Connect" : "Save";
      }
    });
  });
}
