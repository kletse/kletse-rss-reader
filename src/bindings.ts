// Generated from the Rust types and commands by src-tauri/src/bindings.rs. Do not edit:
// run `npm run bindings` after changing them. src/api.ts is the hand-written facade over this file.

/**
 * What the UI needs to know at startup and after saving settings.
 */
export type AppStatus = {
/**
 * Miniflux URL and key are present; otherwise show onboarding.
 */
configured: boolean,
/**
 * Readeck is set up; otherwise hide the bookmark button.
 */
readeck: boolean, };

/**
 * The settings form, flat. Empty Readeck fields mean "not set up".
 * No `Debug` on purpose: it holds the API key and token.
 */
export type Settings = { miniflux_url: string, miniflux_api_key: string, readeck_url: string, readeck_token: string, };

export type Theme = {
/**
 * Omarchy `colors.toml` keys: `background`, `foreground`, `accent`, `mode`, …
 */
colors: { [key in string]: string },
/**
 * Corner radius in px.
 */
rounding: number,
/**
 * UI (monospace) font family, if the desktop has a preference. Left out of the JSON when there's none.
 */
font?: string, };

export type Category = { id: number, title: string, };

export type Feed = { id: number, title: string, category: Category, };

/**
 * What the picker shows: every feed with its category and its unread and read counts.
 */
export type FeedCount = { feed: Feed, unread: number, read: number, };

/**
 * Read state of an entry, as Miniflux spells it. (Miniflux also has `removed`, which we never see.)
 */
export type EntryStatus = "read" | "unread";

export type EntryFeed = { id: number, title: string, };

export type Entry = { id: number, title: string, url: string, author: string, content: string, published_at: string, status: EntryStatus, reading_time: number, feed: EntryFeed, };

/**
 * One page of entries, plus how many the server has in total for the query.
 */
export type EntryPage = { total: number, entries: Array<Entry>, };

/**
 * Which set of entries to read.
 */
export type Selection = { "kind": "all" } | { "kind": "category", id: number, } | { "kind": "feed", id: number, };

/** Every command in `generate_handler!`: the arguments `invoke` takes (camelCase, as Tauri expects) and what it resolves to. */
export type Commands = {
  app_status: { args: Record<string, never>; returns: AppStatus };
  get_settings: { args: Record<string, never>; returns: Settings };
  save_settings: { args: { settings: Settings }; returns: AppStatus };
  get_theme: { args: Record<string, never>; returns: Theme };
  feed_counts: { args: Record<string, never>; returns: FeedCount[] };
  entries: { args: { selection: Selection; status: EntryStatus; past: number; limit: number }; returns: EntryPage };
  newest_unread_id: { args: { selection: Selection }; returns: number };
  unread_count: { args: { selection: Selection; upTo: number }; returns: number };
  mark_read_up_to: { args: { selection: Selection; upTo: number }; returns: number };
  set_status: { args: { ids: number[]; status: EntryStatus }; returns: void };
  fetch_content: { args: { id: number }; returns: string };
  refresh_feeds: { args: Record<string, never>; returns: void };
  save_to_readeck: { args: { url: string }; returns: void };
  open_in_browser: { args: { url: string }; returns: void };
  quit: { args: Record<string, never>; returns: void };
};
