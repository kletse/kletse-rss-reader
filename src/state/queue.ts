// The reading queue and everything you can do to the current article.
// Unread mode: the queue is fixed when a selection is opened; going back shows items read this session.
// History mode: read items, newest first, loaded a page at a time as you get near the end.
import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { api, type Entry, type EntryPage, type EntryStatus, type Selection } from "../api";
import type { Mode, PickerItem } from "../picker-items";
import { readeckEnabled } from "./server";
import { confirmDialog, toast, toastError } from "./ui";

const [store, setStore] = createStore<{ queue: Entry[] }>({ queue: [] });
const [index, setIndex] = createSignal(0); // index === queue.length means "all caught up"
const [selectionLabel, setSelectionLabel] = createSignal("");
const [loading, setLoading] = createSignal(false);
const [queueMode, setQueueMode] = createSignal<Mode>("unread");
const [total, setTotal] = createSignal(0); // the whole selection, including pages not loaded yet
const [hasMore, setHasMore] = createSignal(false); // history only: more pages on the server

export { hasMore, index, loading, queueMode, selectionLabel, total };

export function queue(): Entry[] {
  return store.queue;
}

export function current(): Entry | undefined {
  return store.queue[index()];
}

/** Picking again while a selection loads makes the older load a no-op: each load gets the next id. */
const loads = { latest: 0 };
/** Saving to Readeck or fetching the full article: one at a time. */
let busy = false;

interface QueueContents {
  entries: Entry[];
  label: string;
  /** The whole selection, including pages not loaded yet. */
  size: number;
  more: boolean;
}

function resetQueue(contents: QueueContents): void {
  setStore("queue", contents.entries);
  setIndex(0);
  setSelectionLabel(contents.label);
  setTotal(contents.size);
  setHasMore(contents.more);
}

/** Empties the queue and cancels a selection that is still loading (after the server changed). */
export function clearQueue(): void {
  loads.latest += 1;
  setLoading(false);
  resetQueue({ entries: [], label: "", size: 0, more: false });
}

// ---- History paging ------------------------------------------------------
export const HISTORY_PAGE = 50;
/** Load the next page when this few loaded items are left. */
const PREFETCH = 10;
let historySelection: Selection | undefined;
/** Position in the server's read list. Items marked unread here drop out of that list, hence not queue.length. */
let historyOffset = 0;
let loadingMore = false;

function adjustHistoryOffset(delta: number): void {
  if (queueMode() === "history") {
    historyOffset += delta;
  }
}

function appendPage(page: EntryPage): void {
  historyOffset += page.entries.length;
  // Items read elsewhere meanwhile shift the list; skip what we already have.
  const have = new Set(store.queue.map((e) => e.id));
  const fresh = page.entries.filter((e) => !have.has(e.id));
  setStore("queue", (q) => [...q, ...fresh]);
  const more = page.entries.length > 0 && historyOffset < page.total;
  setHasMore(more);
  setTotal(more ? Math.max(page.total, store.queue.length) : store.queue.length);
}

/** A load's error handler: shows the error, unless another selection was opened since. */
function failedFor(load: number): (err: unknown) => undefined {
  return (err: unknown): undefined => {
    if (load === loads.latest) {
      toastError(err);
    }
  };
}

async function loadMore(): Promise<void> {
  const selection = historySelection;
  if (!hasMore() || loadingMore || selection === undefined) {
    return;
  }
  const id = loads.latest;
  loadingMore = true;
  const page = await api.readEntries(selection, historyOffset, HISTORY_PAGE).catch(failedFor(id));
  loadingMore = false;
  if (page !== undefined && id === loads.latest) {
    appendPage(page);
  }
}

function prefetch(): void {
  if (hasMore() && index() >= store.queue.length - PREFETCH) {
    void loadMore();
  }
}

// ---- Opening a selection -------------------------------------------------
/** Unread mode loads the whole selection; history mode its first page. */
async function fetchFirst(selection: Selection, mode: Mode): Promise<EntryPage> {
  if (mode === "history") {
    return await api.readEntries(selection, 0, HISTORY_PAGE);
  }
  const entries = await api.unreadEntries(selection);
  return { entries, total: entries.length };
}

/** Starts loading a selection; returns its load id. */
function beginLoad(item: PickerItem, mode: Mode): number {
  loads.latest += 1;
  setQueueMode(mode);
  historySelection = mode === "history" ? item.selection : undefined;
  resetQueue({ entries: [], label: item.label, size: 0, more: false });
  setLoading(true);
  return loads.latest;
}

function showFirst(label: string, mode: Mode, first: EntryPage): void {
  setLoading(false);
  historyOffset = first.entries.length;
  const more = mode === "history" && first.entries.length > 0 && first.entries.length < first.total;
  resetQueue({ entries: first.entries, label, size: Math.max(first.total, first.entries.length), more });
}

export async function openSelection(item: PickerItem, mode: Mode): Promise<void> {
  const id = beginLoad(item, mode);
  const first = await fetchFirst(item.selection, mode).catch(failedFor(id));
  if (id === loads.latest) {
    showFirst(item.label, mode, first ?? { entries: [], total: 0 });
  }
}

// ---- Actions -------------------------------------------------------------
async function setStatus(i: number, status: EntryStatus): Promise<void> {
  const entry = store.queue[i];
  if (entry === undefined || entry.status === status) {
    return;
  }
  const { id, status: before } = entry;
  const load = loads.latest;
  setStore("queue", i, "status", status); // optimistic
  const saved = await api.setStatus([id], status).then(
    () => true,
    (err: unknown) => {
      toastError(err);
      return false;
    },
  );
  if (saved && load === loads.latest) {
    adjustHistoryOffset(status === "read" ? 1 : -1);
  } else if (!saved && store.queue[i]?.id === id) {
    setStore("queue", i, "status", before); // roll back, unless another selection was opened meanwhile
  }
}

/**
 * Moves past the current article, marking it read if asked.
 * Returns false at the end of the queue, when there is nothing more to load.
 */
export function next(markRead: boolean): boolean {
  if (current() === undefined) {
    if (hasMore()) {
      void loadMore(); // the next page of history is still loading, or failed: try (again)
    }
    return hasMore();
  }
  if (markRead) {
    void setStatus(index(), "read");
  }
  setIndex(index() + 1);
  prefetch();
  return true;
}

export function prev(): void {
  if (index() > 0) {
    setIndex(index() - 1);
  }
}

export async function toggleRead(): Promise<void> {
  const entry = current();
  if (entry === undefined) {
    return;
  }
  await setStatus(index(), entry.status === "read" ? "unread" : "read");
  toast(`Marked ${entry.status}`);
}

/** Opens a link in the default browser; shows a toast if that fails. */
export async function openLink(url: string): Promise<boolean> {
  try {
    await api.openInBrowser(url);
    return true;
  } catch (err) {
    toastError(err);
    return false;
  }
}

export async function openOriginal(): Promise<void> {
  const entry = current();
  if (entry === undefined) {
    return;
  }
  const i = index();
  if (await openLink(entry.url)) {
    toast("Opened in browser");
    await setStatus(i, "read");
  }
}

/** Runs one Readeck save or full-article fetch at a time; a failure becomes a toast. */
async function exclusive(task: () => Promise<void>): Promise<void> {
  if (busy) {
    return;
  }
  busy = true;
  try {
    await task();
  } catch (err) {
    toastError(err);
  } finally {
    busy = false;
  }
}

export async function saveToReadeck(): Promise<void> {
  const entry = current();
  if (entry === undefined) {
    return;
  }
  if (!readeckEnabled()) {
    toast("Readeck isn't set up. Add it in Settings (press ,).");
    return;
  }
  await exclusive(async () => {
    await api.saveToReadeck(entry.url);
    toast("Saved to Readeck");
    next(true);
  });
}

export async function fetchFull(): Promise<void> {
  const entry = current();
  if (entry === undefined) {
    return;
  }
  const { id } = entry;
  const i = index();
  await exclusive(async () => {
    toast("Fetching full article…");
    const content = await api.fetchContent(id);
    if (store.queue[i]?.id === id) {
      setStore("queue", i, "content", content);
    }
  });
}

/** After the server marked `count` items read: show it, and go to the end of the queue. */
function markedAllRead(count: number): void {
  adjustHistoryOffset(count);
  setStore("queue", (e) => e.status === "unread", "status", "read");
  setIndex(store.queue.length);
  prefetch();
}

export async function markAllRead(): Promise<void> {
  const unread = store.queue.filter((e) => e.status === "unread");
  const question = `Mark all ${unread.length} unread items in “${selectionLabel()}” as read?`;
  if (unread.length === 0 || !(await confirmDialog(question))) {
    return;
  }
  const load = loads.latest;
  try {
    await api.setStatus(
      unread.map((e) => e.id),
      "read",
    );
    if (load === loads.latest) {
      markedAllRead(unread.length);
    }
  } catch (err) {
    toastError(err);
  }
}
