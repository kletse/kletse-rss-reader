// App state and actions. Components read these signals; changing them updates the UI.
import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { api, type Entry, type Status } from "./api";
import { build, type PickerItem } from "./picker-items";
import { initTheme } from "./theme";

// ---- Queue ---------------------------------------------------------------
// The queue is fixed when a selection is opened; going back shows items read this session.
const [store, setStore] = createStore({ queue: [] as Entry[] });
export const queue = () => store.queue;
const [index, setIndex] = createSignal(0); // index === queue.length means "all caught up"
const [selectionLabel, setSelectionLabel] = createSignal("");
const [loading, setLoading] = createSignal(false);
const [readeckEnabled, setReadeckEnabled] = createSignal(false);
export { index, selectionLabel, loading, readeckEnabled };
let busy = false;

export const current = (): Entry | undefined => store.queue[index()];

function resetQueue(entries: Entry[] = [], label = selectionLabel()) {
  setStore("queue", entries);
  setIndex(0);
  setSelectionLabel(label);
}

// ---- Overlays ------------------------------------------------------------
// Only one thing has the keyboard at a time; the settings form replaces the article.
export type Overlay = "none" | "picker" | "help" | "confirm" | "settings" | "onboarding";
const [overlay, setOverlay] = createSignal<Overlay>("none");
export { overlay, setOverlay };
export const inSettings = () => overlay() === "settings" || overlay() === "onboarding";

// ---- Toast ---------------------------------------------------------------
export interface Toast { message: string; kind: "info" | "error" }
const [toastMessage, setToastMessage] = createSignal<Toast | null>(null);
export { toastMessage };
let toastTimer: number | undefined;
export function toast(message: string, kind: Toast["kind"] = "info") {
  setToastMessage({ message, kind });
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => setToastMessage(null), kind === "error" ? 6000 : 2200);
}

// ---- Confirm -------------------------------------------------------------
const [confirmText, setConfirmText] = createSignal("");
export { confirmText };
let confirmResolve: ((yes: boolean) => void) | undefined;
let beforeConfirm: Overlay = "none";

/** Shows a yes/no overlay; resolves true on Enter/y. */
export function confirmDialog(text: string): Promise<boolean> {
  setConfirmText(text);
  beforeConfirm = overlay();
  setOverlay("confirm");
  return new Promise((resolve) => (confirmResolve = resolve));
}

export function answerConfirm(yes: boolean) {
  setOverlay(beforeConfirm);
  confirmResolve?.(yes);
  confirmResolve = undefined;
}

// ---- Reader element (scrolling) ------------------------------------------
let reader: HTMLElement | undefined;
export const setReader = (el: HTMLElement) => (reader = el);

export function scrollPage(direction: 1 | -1) {
  reader?.scrollBy({ top: direction * reader.clientHeight * 0.85, behavior: "smooth" });
}

export const atBottom = () => !reader || reader.scrollTop + reader.clientHeight >= reader.scrollHeight - 4;

export function scrollToTop() {
  if (reader) reader.scrollTop = 0;
}

// ---- Font size and column width (width in multiples of the font size) ----
const FONT_KEY = "kletse-rss:font-size";
const WIDTH_KEY = "kletse-rss:width-factor";
export const DEFAULT_FONT_SIZE = 18;
export const DEFAULT_WIDTH = 40;

function stored(key: string, fallback: number): number {
  try { return Number(localStorage.getItem(key)) || fallback; } catch { return fallback; } // storage unavailable
}
function save(key: string, value: number) {
  try { localStorage.setItem(key, String(value)); } catch { /* ignore */ }
}

export let fontSize = stored(FONT_KEY, DEFAULT_FONT_SIZE);
export let widthFactor = stored(WIDTH_KEY, DEFAULT_WIDTH);

export function setFontSize(px: number) {
  fontSize = Math.min(32, Math.max(12, px));
  document.documentElement.style.setProperty("--article-size", `${fontSize}px`);
  save(FONT_KEY, fontSize);
}

export function setWidth(factor: number) {
  widthFactor = Math.min(80, Math.max(20, factor));
  document.documentElement.style.setProperty("--width-factor", String(widthFactor));
  save(WIDTH_KEY, widthFactor);
}

// ---- Actions -------------------------------------------------------------
async function setStatus(i: number, status: Entry["status"]) {
  const entry = store.queue[i];
  if (!entry || entry.status === status) return;
  const { id, status: before } = entry;
  setStore("queue", i, "status", status); // optimistic
  try {
    await api.setStatus([id], status);
  } catch (err) {
    // Roll back, unless another selection was opened meanwhile.
    if (store.queue[i]?.id === id) setStore("queue", i, "status", before);
    toast(String(err), "error");
  }
}

export function next(markRead: boolean) {
  if (!current()) { openPicker(); return; }
  if (markRead) void setStatus(index(), "read");
  setIndex(index() + 1);
}

export function prev() {
  if (index() > 0) setIndex(index() - 1);
}

export async function toggleRead() {
  const entry = current();
  if (!entry) return;
  await setStatus(index(), entry.status === "read" ? "unread" : "read");
  toast(`Marked ${entry.status}`);
}

/** Opens a link in the default browser; shows a toast if that fails. */
export async function openLink(url: string): Promise<boolean> {
  try {
    await api.openInBrowser(url);
    return true;
  } catch (err) {
    toast(String(err), "error");
    return false;
  }
}

export async function openOriginal() {
  const entry = current();
  if (!entry) return;
  const i = index();
  if (!(await openLink(entry.url))) return;
  toast("Opened in browser");
  await setStatus(i, "read");
}

export async function saveToReadeck() {
  const entry = current();
  if (!entry || busy) return;
  if (!readeckEnabled()) {
    toast("Readeck isn't set up. Add it in Settings (press ,).");
    return;
  }
  busy = true;
  try {
    await api.saveToReadeck(entry.url);
    toast("Saved to Readeck");
    next(true);
  } catch (err) {
    toast(String(err), "error");
  } finally {
    busy = false;
  }
}

export async function fetchFull() {
  const entry = current();
  if (!entry || busy) return;
  const { id } = entry;
  const i = index();
  busy = true;
  toast("Fetching full article…");
  try {
    const content = await api.fetchContent(id);
    if (store.queue[i]?.id === id) setStore("queue", i, "content", content);
  } catch (err) {
    toast(String(err), "error");
  } finally {
    busy = false;
  }
}

export async function markAllRead() {
  const unread = store.queue.filter((e) => e.status === "unread");
  if (!unread.length) return;
  if (!(await confirmDialog(`Mark all ${unread.length} unread items in “${selectionLabel()}” as read?`))) return;
  try {
    await api.setStatus(unread.map((e) => e.id), "read");
    setStore("queue", (e) => e.status === "unread", "status", "read");
    setIndex(store.queue.length);
  } catch (err) {
    toast(String(err), "error");
  }
}

let loadId = 0; // picking again while a selection loads makes the older load a no-op
export async function openSelection(item: PickerItem) {
  const id = ++loadId;
  resetQueue([], item.label);
  setLoading(true);
  let entries: Entry[] = [];
  try {
    entries = await api.unreadEntries(item.selection);
  } catch (err) {
    if (id === loadId) toast(String(err), "error");
  }
  if (id !== loadId) return;
  setLoading(false);
  resetQueue(entries);
}

// ---- Picker --------------------------------------------------------------
export type PickerList = { kind: "loading" } | { kind: "error"; message: string } | { kind: "items"; items: PickerItem[] };
const [pickerList, setPickerList] = createSignal<PickerList>({ kind: "loading" });
const [pickerCursor, setPickerCursor] = createSignal(0);
export { pickerList, pickerCursor };
const pickerItems = () => { const l = pickerList(); return l.kind === "items" ? l.items : []; };

export function openPicker() {
  setOverlay("picker");
  void loadPicker();
}

export function closePicker() {
  if (overlay() === "picker") setOverlay("none");
}

/** The picker can only be dismissed once there is something to go back to. */
export const pickerCanClose = () => store.queue.length > 0 || selectionLabel() !== "";

async function loadPicker() {
  if (pickerList().kind !== "items") setPickerList({ kind: "loading" });
  try {
    const items = build(await api.feedCounts());
    setPickerList({ kind: "items", items });
    setPickerCursor((c) => Math.min(c, Math.max(items.length - 1, 0)));
  } catch (err) {
    setPickerList({ kind: "error", message: String(err) });
  }
}

export async function refreshFeeds() {
  toast("Refreshing feeds…");
  try {
    await api.refreshFeeds();
  } catch (err) {
    toast(String(err), "error");
    return;
  }
  // Miniflux refreshes in the background; reload counts a few times while it works.
  for (const delay of [2000, 6000, 15000]) setTimeout(() => overlay() === "picker" && loadPicker(), delay);
}

export function movePicker(delta: number) {
  const n = pickerItems().length;
  if (n) setPickerCursor((c) => (c + delta + n) % n);
}

export function choose(item: PickerItem | undefined = pickerItems()[pickerCursor()]) {
  if (!item) return;
  closePicker();
  void openSelection(item);
}

// ---- Settings / onboarding ------------------------------------------------
function applyStatus(status: Status) {
  setReadeckEnabled(status.readeck);
}

/** Opens the settings form. On first run (onboarding) it can't be cancelled. */
export function openSettings(firstRun = false) {
  if (!inSettings()) setOverlay(firstRun ? "onboarding" : "settings");
}

/** Called with the new status after a successful save, or null when cancelled. */
export function closeSettings(status: Status | null) {
  const firstRun = overlay() === "onboarding";
  if (status) {
    applyStatus(status);
    if (!firstRun) toast("Settings saved");
    // The server may have changed: start from the picker with a fresh queue.
    resetQueue([], "");
    openPicker();
  } else if (!firstRun) {
    setOverlay("none");
    if (!store.queue.length) openPicker();
  }
}

// ---- Start ---------------------------------------------------------------
export async function start() {
  setFontSize(fontSize);
  setWidth(widthFactor);
  await initTheme();
  const status = await api.status();
  if (!status.configured) {
    openSettings(true);
    return;
  }
  applyStatus(status);
  openPicker();
}
