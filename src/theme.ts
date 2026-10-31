// Maps the Omarchy colors.toml palette onto CSS variables, live.
import { listen } from "@tauri-apps/api/event";
import { api, type Theme } from "./api";

function apply(theme: Theme) {
  const c = theme.colors;
  const root = document.documentElement.style;
  const set = (name: string, value: string | undefined, fallback: string) =>
    root.setProperty(name, value ?? fallback);

  set("--bg", c.background, "#1a1b26");
  set("--bg-light", c.lighter_background, "#24283b");
  set("--bg-dark", c.dark_background, "#16161e");
  set("--fg", c.foreground, "#c0caf5");
  set("--fg-dim", c.dark_foreground, "#787c99");
  set("--fg-bright", c.bright_foreground ?? c.light_foreground, "#ffffff");
  set("--accent", c.accent, "#7aa2f7");
  set("--muted", c.muted, "#565f89");
  set("--selection", c.selection, "#33467c");
  for (const name of ["red", "yellow", "green", "cyan", "blue", "magenta", "orange"]) {
    if (c[name]) root.setProperty(`--${name}`, c[name]);
    if (c[`bright_${name}`]) root.setProperty(`--bright-${name}`, c[`bright_${name}`]);
  }
  root.setProperty("--radius", `${theme.rounding}px`);
  if (theme.font) root.setProperty("--ui-font", `"${theme.font}", monospace`);
  document.documentElement.dataset.mode = c.mode ?? "dark";
}

export async function initTheme() {
  apply(await api.theme());
  await listen<Theme>("theme-changed", (event) => apply(event.payload));
}
