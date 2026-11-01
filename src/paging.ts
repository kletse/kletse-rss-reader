// Paging through a selection: where the next page starts, and what a page adds to the queue.
// Unread items come oldest first, history newest first.
import type { Entry, EntryPage, EntryStatus } from "./api";
import type { Mode } from "./picker-items";

export const PAGE = 50;
const MS_PER_SECOND = 1000;

/** The list a mode pages through. */
export function listStatus(mode: Mode): EntryStatus {
  return mode === "history" ? "read" : "unread";
}

/** `past`: unix seconds the page starts after (unread) or before (history); 0 is the start of the list. */
export interface PageRequest {
  past: number;
  limit: number;
}

function publishedMs(entry: Entry): number {
  return Date.parse(entry.published_at);
}

/**
 * Where the page after `queue` starts, and how long it is.
 *
 * Not an offset: reading moves items out of the server's unread list (j here, or a phone meanwhile) and m moves them
 * back, so a count of loaded items drifts and skips some, and bookkeeping to correct it can't see what happens
 * elsewhere. Instead the page starts at the newest loaded item's publication time (the oldest's, in history), which
 * no status change moves. Miniflux filters on whole seconds and doesn't order items published in the same second,
 * so the page starts a second early and is longer by the loaded items in that overlap; mergePage drops them again.
 * Nothing is skipped, whatever was read or marked unread in the meantime, here or elsewhere.
 */
export function nextPage(queue: readonly Entry[], mode: Mode): PageRequest {
  const times = queue.map(publishedMs).filter(Number.isFinite);
  if (times.length === 0) {
    return { past: 0, limit: PAGE };
  }
  const history = mode === "history";
  const edge = times.reduce((a, b) => (history ? Math.min(a, b) : Math.max(a, b)));
  const past = history ? Math.floor(edge / MS_PER_SECOND) + 1 : Math.ceil(edge / MS_PER_SECOND) - 1;
  const bound = past * MS_PER_SECOND;
  const overlap = times.filter((t) => (history ? t < bound : t > bound)).length;
  return { past, limit: PAGE + overlap };
}

export interface Merged {
  /** The page's items that aren't in the queue yet. */
  fresh: Entry[];
  /** Whether the server has more beyond this page. */
  more: boolean;
  /** The whole selection: the queue with `fresh`, plus what's beyond. */
  total: number;
}

export function mergePage(queue: readonly Entry[], page: EntryPage): Merged {
  // The overlap, and items marked unread elsewhere meanwhile, come back again; skip what we already have.
  const have = new Set(queue.map((e) => e.id));
  const fresh = page.entries.filter((e) => !have.has(e.id));
  const beyond = Math.max(0, page.total - page.entries.length);
  // A page with nothing new can't lead anywhere new either.
  const more = fresh.length > 0 && beyond > 0;
  return { fresh, more, total: queue.length + fresh.length + (more ? beyond : 0) };
}
