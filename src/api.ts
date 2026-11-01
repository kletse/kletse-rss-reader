// Typed wrappers around the Rust commands (src-tauri/src/lib.rs).
import { invoke } from "@tauri-apps/api/core";

export interface Category {
  id: number;
  title: string;
}
export interface Feed {
  id: number;
  title: string;
  category: Category;
}
export interface FeedCount {
  feed: Feed;
  unread: number;
  read: number;
}

export type EntryStatus = "read" | "unread";

export interface Entry {
  id: number;
  title: string;
  url: string;
  author: string;
  content: string;
  published_at: string;
  status: EntryStatus;
  reading_time: number;
  feed: { id: number; title: string };
}

export interface EntryPage {
  total: number;
  entries: Entry[];
}

export type Selection = { kind: "all" } | { kind: "category"; id: number } | { kind: "feed"; id: number };

export interface Status {
  configured: boolean;
  readeck: boolean;
  demo: boolean;
}

export interface Settings {
  miniflux_url: string;
  miniflux_api_key: string;
  readeck_url: string;
  readeck_token: string;
}

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

export interface Theme {
  colors: Partial<Record<ThemeColor, string>>;
  rounding: number;
  /** Absent when the desktop has no font preference. */
  font?: string;
}

export const api = {
  status: (): Promise<Status> => invoke<Status>("app_status"),
  getSettings: (): Promise<Settings> => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings): Promise<Status> => invoke<Status>("save_settings", { settings }),
  theme: (): Promise<Theme> => invoke<Theme>("get_theme"),
  feedCounts: (): Promise<FeedCount[]> => invoke<FeedCount[]>("feed_counts"),
  unreadEntries: (selection: Selection): Promise<Entry[]> => invoke<Entry[]>("unread_entries", { selection }),
  readEntries: (selection: Selection, offset: number, limit: number): Promise<EntryPage> =>
    invoke<EntryPage>("read_entries", { selection, offset, limit }),
  setStatus: (ids: number[], status: EntryStatus): Promise<void> => invoke<void>("set_status", { ids, status }),
  fetchContent: (id: number): Promise<string> => invoke<string>("fetch_content", { id }),
  refreshFeeds: (): Promise<void> => invoke<void>("refresh_feeds"),
  saveToReadeck: (url: string): Promise<void> => invoke<void>("save_to_readeck", { url }),
  openInBrowser: (url: string): Promise<void> => invoke<void>("open_in_browser", { url }),
  quit: (): Promise<void> => invoke<void>("quit"),
};
