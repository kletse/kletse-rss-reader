// The Rust commands (src-tauri/src/lib.rs) as typed functions. The types, command names and argument names come
// from src/bindings.ts, generated from the Rust side (`npm run bindings`); this file narrows a few types and gives
// the commands the names the rest of the UI uses.
import { invoke } from "@tauri-apps/api/core";
import type {
  AppStatus,
  Commands,
  Entry,
  EntryPage,
  EntryStatus,
  FeedCount,
  Theme as RustTheme,
  Selection,
  Settings,
} from "./bindings";

export type {
  AppStatus,
  Category,
  Entry,
  EntryFeed,
  EntryPage,
  EntryStatus,
  Feed,
  FeedCount,
  Selection,
  Settings,
} from "./bindings";

/** The accent hues a theme may define, plain and `bright_` (Omarchy colors.toml). */
export const HUES = ["red", "yellow", "green", "cyan", "blue", "magenta", "orange"] as const;
export type Hue = (typeof HUES)[number];

/** Omarchy colors.toml keys the UI uses; any of them may be missing. */
export type ThemeColor =
  | "background"
  | "lighter_background"
  | "dark_background"
  | "foreground"
  | "dark_foreground"
  | "bright_foreground"
  | "light_foreground"
  | "accent"
  | "muted"
  | "selection"
  | "mode"
  | Hue
  | `bright_${Hue}`;

/** Rust sends any colors.toml key; the UI only looks up the ones it knows. */
export type Theme = Omit<RustTheme, "colors"> & { colors: Partial<Record<ThemeColor, string>> };

type Command = keyof Commands;

/** `invoke`, checked against the generated command table. */
function call<C extends Command>(command: C, args: Commands[C]["args"]): Promise<Commands[C]["returns"]> {
  return invoke<Commands[C]["returns"]>(command, args);
}

export const api = {
  status: (): Promise<AppStatus> => call("app_status", {}),
  getSettings: (): Promise<Settings> => call("get_settings", {}),
  saveSettings: (settings: Settings): Promise<AppStatus> => call("save_settings", { settings }),
  theme: (): Promise<Theme> => call("get_theme", {}),
  feedCounts: (): Promise<FeedCount[]> => call("feed_counts", {}),
  unreadEntries: (selection: Selection): Promise<Entry[]> => call("unread_entries", { selection }),
  readEntries: (selection: Selection, offset: number, limit: number): Promise<EntryPage> =>
    call("read_entries", { selection, offset, limit }),
  setStatus: (ids: number[], status: EntryStatus): Promise<void> => call("set_status", { ids, status }),
  fetchContent: (id: number): Promise<string> => call("fetch_content", { id }),
  refreshFeeds: (): Promise<void> => call("refresh_feeds", {}),
  saveToReadeck: (url: string): Promise<void> => call("save_to_readeck", { url }),
  openInBrowser: (url: string): Promise<void> => call("open_in_browser", { url }),
  quit: (): Promise<void> => call("quit", {}),
};
