// The reading queue and everything you can do to the current article.
// Unread mode: unread items, oldest first; going back shows items read this session.
// History mode: read items, newest first.
// Both load a page at a time as you get near the end. At the end of the unread list, it looks once more for items that
// arrived meanwhile.
import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { api, type Entry, type EntryPage, type EntryStatus, type Selection } from "../api";
import { listStatus, mergePage, nextPage } from "../paging";
import type { Mode, PickerItem } from "../picker-items";
import { readeckEnabled } from "./server";
import { confirmDialog, scrollToTop, toast, toastError } from "./ui";

const [store, setStore] = createStore<{ queue: Entry[] }>({ queue: [] });
const [index, setIndex] = createSignal(0); // index === queue.length means "all caught up"
const [selectionLabel, setSelectionLabel] = createSignal("");
const [loading, setLoading] = createSignal(false);
const [queueMode, setQueueMode] = createSignal<Mode>("unread");
const [total, setTotal] = createSignal(0); // the whole selection, including pages not loaded yet
const [hasMore, setHasMore] = createSignal(false); // more pages on the server
const [checking, setChecking] = createSignal(false); // past the end of the unread list, looking for new arrivals

export { checking, hasMore, index, loading, queueMode, selectionLabel, total };

export function queue(): Entry[] {
  return store.queue;
}

export function current(): Entry | undefined {
  return store.queue[index()];
}

/**
 * Opening another selection (or Shift+A emptying this one) makes the loads still in flight no-ops:
 * each load remembers this id and checks it when it's done.
 */
const loads = { latest: 0 };
let openedSelection: Selection | undefined;
/** The newest unread item's id when the selection was opened (0 in history). Shift+A marks up to there. */
let newestAtOpen = 0;
/** Saving to Readeck or fetching the full article: one at a time. */
let busy = false;

function resetQueue(label: string): void {
  setStore("queue", []);
  setIndex(0);
  setSelectionLabel(label);
  setTotal(0);
  setHasMore(false);
  setChecking(false);
}

/** Empties the queue and cancels a selection that is still loading (after the server changed). */
export function clearQueue(): void {
  loads.latest += 1;
  versions.clear();
  setLoading(false);
  openedSelection = undefined;
  resetQueue("");
}

// ---- Paging (see ../paging.ts) -------------------------------------------
/** Load the next page when this few loaded items are left. */
const PREFETCH = 10;
let loadingMore = false;

function appendPage(page: EntryPage): void {
  const { fresh, more, total: size } = mergePage(store.queue, page);
  setStore("queue", (q) => [...q, ...fresh]);
  setHasMore(more);
  setTotal(size);
}

/** A load's error handler: shows the error, unless another selection was opened since. */
function failedFor(load: number): (err: unknown) => undefined {
  return (err: unknown): undefined => {
    if (load === loads.latest) {
      toastError(err);
    }
  };
}

/**
 * The page after what's loaded; at the end of the list, that's whatever arrived since.
 * Undefined if it failed, or if another selection was opened meanwhile.
 */
async function fetchPage(selection: Selection, mode: Mode, load: number): Promise<EntryPage | undefined> {
  const { past, limit } = nextPage(store.queue, mode);
  const page = await api.entries(selection, listStatus(mode), past, limit).catch(failedFor(load));
  return load === loads.latest ? page : undefined;
}

/** Appends the page after what's loaded; one at a time. */
async function loadPage(): Promise<void> {
  const selection = openedSelection;
  if (loadingMore || selection === undefined) {
    return;
  }
  loadingMore = true;
  const page = await fetchPage(selection, queueMode(), loads.latest);
  loadingMore = false;
  if (page !== undefined) {
    appendPage(page);
  }
}

async function loadMore(): Promise<void> {
  if (hasMore()) {
    await loadPage();
  }
}

/**
 * Just past the last unread item: the page after it is whatever arrived since, so those follow on as if they'd been
 * there all along, instead of "All caught up". (It starts at the newest loaded item's publication time, so an item a
 * feed backdates before that shows up only when the selection is opened again.)
 */
async function checkForArrivals(): Promise<void> {
  setChecking(true);
  await loadPage();
  setChecking(false);
}

function prefetch(): void {
  if (hasMore() && index() >= store.queue.length - PREFETCH) {
    void loadMore();
  }
}

// ---- Opening a selection -------------------------------------------------
/** Starts loading a selection; returns its load id. */
function beginLoad(item: PickerItem, mode: Mode): number {
  loads.latest += 1;
  setQueueMode(mode);
  openedSelection = item.selection;
  newestAtOpen = 0;
  resetQueue(item.label);
  setLoading(true);
  return loads.latest;
}

export async function openSelection(item: PickerItem, mode: Mode): Promise<void> {
  const id = beginLoad(item, mode);
  const newest = mode === "unread" ? api.newestUnreadId(item.selection) : Promise.resolve(0);
  const first = await Promise.all([fetchPage(item.selection, mode, id), newest]).catch(failedFor(id));
  if (id !== loads.latest) {
    return;
  }
  setLoading(false);
  const [page, newestId] = first ?? [];
  if (page !== undefined && newestId !== undefined) {
    newestAtOpen = newestId;
    appendPage(page);
  }
}

// ---- Actions -------------------------------------------------------------
async function setStatus(i: number, status: EntryStatus): Promise<void> {
  const entry = store.queue[i];
  if (entry === undefined || entry.status === status) {
    return;
  }
  const { id, status: before } = entry;
  setStore("queue", i, "status", status); // optimistic
  try {
    await api.setStatus([id], status);
  } catch (err) {
    toastError(err);
    if (store.queue[i]?.id === id) {
      setStore("queue", i, "status", before); // roll back, unless another selection was opened meanwhile
    }
  }
}

/** After moving on: the next page when it's near, or, just past the end of the unread list, new arrivals. */
function loadAhead(): void {
  if (current() === undefined && !hasMore() && queueMode() === "unread") {
    void checkForArrivals();
  } else {
    prefetch();
  }
}

/**
 * Moves past the current article, marking it read if asked.
 * Returns false at the end of the queue, when there is nothing more to load.
 */
export function next(markRead: boolean): boolean {
  if (current() === undefined) {
    if (hasMore()) {
      void loadMore(); // the next page is still loading, or failed: try (again)
    }
    return hasMore() || checking();
  }
  if (markRead) {
    void setStatus(index(), "read");
  }
  setIndex(index() + 1);
  loadAhead();
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

/**
 * Both versions of each article whose full text was fetched this session, by entry id, so f can switch back.
 * Kept across selections: the server stores the fetched text, so a reloaded entry no longer has the feed's.
 */
const versions = new Map<number, { feed: string; full: string }>();

/** Swaps an article's text; if it's the one on screen, starts it from the top and says which version it is. */
function showContent(i: number, content: string, version: string): void {
  setStore("queue", i, "content", content);
  if (index() === i) {
    scrollToTop();
    toast(version);
  }
}

/** Fetches the full article and shows it, unless another selection was opened meanwhile. */
async function fetchVersions(i: number, id: number, feed: string): Promise<void> {
  toast("Fetching full article…");
  const full = await api.fetchContent(id);
  if (store.queue[i]?.id === id) {
    versions.set(id, { feed, full });
    showContent(i, full, "Full article");
  }
}

/** Fetches the full article the first time, then switches between it and the feed's version. */
export async function fetchFull(): Promise<void> {
  const entry = current();
  if (entry === undefined) {
    return;
  }
  const { id, content } = entry;
  const i = index();
  const known = versions.get(id);
  if (known === undefined) {
    await exclusive(() => fetchVersions(i, id, content));
  } else if (content === known.full) {
    showContent(i, known.feed, "Feed version");
  } else {
    showContent(i, known.full, "Full article");
  }
}

/** After the server marked everything read: show it, and go to the end of the queue. */
function markedAllRead(): void {
  setStore("queue", (e) => e.status === "unread", "status", "read");
  setIndex(store.queue.length);
  if (queueMode() === "history") {
    prefetch();
    return;
  }
  // Nothing unread is left up to where the selection was opened. Drop a page still loading: it would bring back
  // items just marked read.
  loads.latest += 1;
  setHasMore(false);
  setTotal(store.queue.length);
}

/**
 * Shift+A. Most of the unread list isn't loaded, so Miniflux marks what it has unread up to the newest item there
 * was when the selection was opened: nothing that arrived since gets marked read unseen, as its "mark all as read"
 * endpoints would. Later items that a page did load are marked by id. (In history newestAtOpen is 0, so that's just
 * the items marked unread here.) The server counts, so the question stays true even after reading elsewhere.
 */
async function confirmMarkAll(selection: Selection, load: number): Promise<void> {
  const upTo = newestAtOpen;
  const later = store.queue.filter((e) => e.status === "unread" && e.id > upTo).map((e) => e.id);
  const count = (await api.unreadCount(selection, upTo)) + later.length;
  const question = `Mark all ${count} unread items in “${selectionLabel()}” as read?`;
  if (count === 0 || load !== loads.latest || !(await confirmDialog(question))) {
    return;
  }
  await Promise.all([api.markReadUpTo(selection, upTo), api.setStatus(later, "read")]);
  if (load === loads.latest) {
    markedAllRead();
  }
}

export async function markAllRead(): Promise<void> {
  const selection = openedSelection;
  if (selection !== undefined && !loading()) {
    const load = loads.latest;
    await confirmMarkAll(selection, load).catch(failedFor(load));
  }
}
