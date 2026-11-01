// Maps the theme palette (Omarchy colors.toml on Linux, system appearance on macOS) onto CSS variables, live.
import { listen } from "@tauri-apps/api/event";
import { api, HUES, type Theme } from "./api";

/** CSS generic families (`ui-monospace`, `system-ui`, …) are a single lowercase word, maybe with dashes. */
const GENERIC_FAMILY = /^[a-z-]+$/u;

/** The base palette: CSS variable, the theme's value, and a Tokyo Night fallback. */
function palette(c: Theme["colors"]): [string, string | undefined, string][] {
  return [
    ["--bg", c.background, "#1a1b26"],
    ["--bg-light", c.lighter_background, "#24283b"],
    ["--bg-dark", c.dark_background, "#16161e"],
    ["--fg", c.foreground, "#c0caf5"],
    ["--fg-dim", c.dark_foreground, "#787c99"],
    ["--fg-bright", c.bright_foreground ?? c.light_foreground, "#ffffff"],
    ["--accent", c.accent, "#7aa2f7"],
    ["--muted", c.muted, "#565f89"],
    ["--selection", c.selection, "#33467c"],
  ];
}

/** Hues the theme doesn't define keep the defaults from styles/tokens.css. */
function hues(c: Theme["colors"]): [string, string][] {
  return HUES.flatMap((hue): [string, string | undefined][] => [
    [`--${hue}`, c[hue]],
    [`--bright-${hue}`, c[`bright_${hue}`]],
  ]).filter((pair): pair is [string, string] => pair[1] !== undefined);
}

function apply(theme: Theme): void {
  const root = document.documentElement.style;
  for (const [name, value, fallback] of palette(theme.colors)) {
    root.setProperty(name, value ?? fallback);
  }
  for (const [name, value] of hues(theme.colors)) {
    root.setProperty(name, value);
  }
  root.setProperty("--radius", `${theme.rounding}px`);
  if (theme.font !== undefined && theme.font !== "") {
    root.setProperty("--ui-font", `${fontFamily(theme.font)}, monospace`);
  }
  document.documentElement.setAttribute("data-mode", theme.colors.mode ?? "dark");
}

/** CSS generic families (`ui-monospace`, `system-ui`, …) go unquoted; real names get quotes. */
export function fontFamily(name: string): string {
  return GENERIC_FAMILY.test(name) ? name : `"${name}"`;
}

export async function initTheme(): Promise<void> {
  apply(await api.theme());
  await listen<Theme>("theme-changed", (event) => apply(event.payload));
}
