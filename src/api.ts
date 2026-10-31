// Typed wrappers around the Rust commands (src-tauri/src/lib.rs).
import { invoke } from "@tauri-apps/api/core";

export interface Category { id: number; title: string }
export interface Feed { id: number; title: string; category: Category }
export interface FeedCount { feed: Feed; unread: number }

export interface Entry {
  id: number;
  title: string;
  url: string;
  author: string;
  content: string;
  published_at: string;
  status: "read" | "unread";
  reading_time: number;
  feed: { id: number; title: string };
}

export type Selection =
  | { kind: "all" }
  | { kind: "category"; id: number }
  | { kind: "feed"; id: number };

export interface Theme {
  colors: Record<string, string>;
  rounding: number;
  font: string | null;
}

export const api = {
  setupError: () => invoke<string | null>("setup_error"),
  theme: () => invoke<Theme>("get_theme"),
  feedCounts: () => invoke<FeedCount[]>("feed_counts"),
  unreadEntries: (selection: Selection) => invoke<Entry[]>("unread_entries", { selection }),
  setStatus: (ids: number[], status: "read" | "unread") => invoke<void>("set_status", { ids, status }),
  fetchContent: (id: number) => invoke<string>("fetch_content", { id }),
  refreshFeeds: () => invoke<void>("refresh_feeds"),
  saveToReadeck: (url: string) => invoke<void>("save_to_readeck", { url }),
  openInBrowser: (url: string) => invoke<void>("open_in_browser", { url }),
};
