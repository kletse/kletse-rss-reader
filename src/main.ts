// Kletse RSS: one unread article at a time, oldest first.
import "@fontsource-variable/inter";
import "./styles.css";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type Entry } from "./api";
import { renderContent, hostOf } from "./content";
import { picker, type PickerItem } from "./picker";
import { initTheme } from "./theme";
import { $, el, toast, confirmDialog } from "./ui";

// ---- State ---------------------------------------------------------------
// The queue is fixed when a selection is opened; going back shows items read this session.
let queue: Entry[] = [];
let index = 0; // index === queue.length means "all caught up"
let selectionLabel = "";
let busy = false;

const current = (): Entry | undefined => queue[index];

// ---- Font size -----------------------------------------------------------
const FONT_KEY = "kletse-rss:font-size";
let fontSize = 18;
try { fontSize = Number(localStorage.getItem(FONT_KEY)) || 18; } catch { /* storage unavailable */ }
function setFontSize(px: number) {
  fontSize = Math.min(32, Math.max(12, px));
  document.documentElement.style.setProperty("--article-size", `${fontSize}px`);
  try { localStorage.setItem(FONT_KEY, String(fontSize)); } catch { /* ignore */ }
}

// ---- Column width (in multiples of the font size, so it scales with it) --
const WIDTH_KEY = "kletse-rss:width-factor";
const DEFAULT_WIDTH = 40;
let widthFactor = DEFAULT_WIDTH;
try { widthFactor = Number(localStorage.getItem(WIDTH_KEY)) || DEFAULT_WIDTH; } catch { /* storage unavailable */ }
function setWidth(factor: number) {
  widthFactor = Math.min(80, Math.max(20, factor));
  document.documentElement.style.setProperty("--width-factor", String(widthFactor));
  try { localStorage.setItem(WIDTH_KEY, String(widthFactor)); } catch { /* ignore */ }
}

// ---- Rendering -----------------------------------------------------------
const reader = $("reader");

function render() {
  $("selection-label").textContent = selectionLabel || "Feeds";
  const entry = current();
  $("position").textContent = queue.length ? `${Math.min(index + 1, queue.length)} / ${queue.length}` : "";
  reader.scrollTop = 0;

  if (!entry) {
    reader.replaceChildren(queue.length
      ? el("div", { class: "message" }, el("h1", {}, "All caught up"),
          el("p", {}, "Press j or Enter for the feeds, k to go back."))
      : el("div", { class: "message" }));
    return;
  }

  const date = new Date(entry.published_at);
  const meta = [entry.feed.title, entry.author, formatDate(date),
    entry.reading_time ? `${entry.reading_time} min` : ""].filter(Boolean).join(" · ");

  const title = el("a", { href: entry.url, class: "title-link" }, entry.title || "(untitled)");
  const article = el("article", {},
    el("div", { class: "meta" }, el("span", {}, meta),
      el("span", { class: `status ${entry.status}` }, entry.status === "read" ? "read" : "unread")),
    el("h1", {}, title),
    el("div", { class: "source" }, hostOf(entry.url)),
    el("div", { class: "content" }),
  );
  article.querySelector(".content")!.append(renderContent(entry.content, entry.url));
  reader.replaceChildren(article);
}

function formatDate(d: Date): string {
  return d.toLocaleString(undefined, { day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" });
}

// ---- Actions -------------------------------------------------------------
async function setStatus(entry: Entry, status: Entry["status"]) {
  if (entry.status === status) return;
  const before = entry.status;
  entry.status = status; // optimistic
  try {
    await api.setStatus([entry.id], status);
  } catch (err) {
    entry.status = before;
    toast(String(err), "error");
    if (entry === current()) render();
  }
}

function next(markRead: boolean) {
  const entry = current();
  if (!entry) { openPicker(); return; }
  if (markRead) void setStatus(entry, "read");
  index++;
  render();
}

function prev() {
  if (index === 0) return;
  index--;
  render();
}

async function toggleRead() {
  const entry = current();
  if (!entry) return;
  await setStatus(entry, entry.status === "read" ? "unread" : "read");
  render();
  toast(`Marked ${entry.status}`);
}

async function openOriginal() {
  const entry = current();
  if (!entry) return;
  await openUrl(entry.url);
  void setStatus(entry, "read");
  render();
}

async function saveToReadeck() {
  const entry = current();
  if (!entry || busy) return;
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

async function fetchFull() {
  const entry = current();
  if (!entry || busy) return;
  busy = true;
  toast("Fetching full article…");
  try {
    entry.content = await api.fetchContent(entry.id);
    if (entry === current()) render();
  } catch (err) {
    toast(String(err), "error");
  } finally {
    busy = false;
  }
}

async function markAllRead() {
  const unread = queue.filter((e) => e.status === "unread");
  if (!unread.length) return;
  if (!(await confirmDialog(`Mark all ${unread.length} unread items in “${selectionLabel}” as read?`))) return;
  try {
    await api.setStatus(unread.map((e) => e.id), "read");
    unread.forEach((e) => (e.status = "read"));
    index = queue.length;
    render();
  } catch (err) {
    toast(String(err), "error");
  }
}

async function openSelection(item: PickerItem) {
  selectionLabel = item.label;
  queue = [];
  index = 0;
  reader.replaceChildren(el("div", { class: "message" }, el("p", {}, "Loading…")));
  try {
    queue = await api.unreadEntries(item.selection);
  } catch (err) {
    toast(String(err), "error");
  }
  render();
}

function openPicker() {
  void picker.show(openSelection);
}

function scrollPage(direction: 1 | -1) {
  reader.scrollBy({ top: direction * reader.clientHeight * 0.85, behavior: "smooth" });
}

const atBottom = () => reader.scrollTop + reader.clientHeight >= reader.scrollHeight - 4;

// ---- Help ----------------------------------------------------------------
const KEYS: [string, string][] = [
  ["j / ↓", "next (marks read)"],
  ["n", "skip (keeps unread)"],
  ["k / ↑", "previous"],
  ["m", "toggle read / unread"],
  ["o", "open original in browser"],
  ["b", "save to Readeck, then next"],
  ["f", "fetch full article"],
  ["Shift+A", "mark whole queue read"],
  ["g / p / Tab", "feeds picker"],
  ["r", "refresh feeds (in picker)"],
  ["Space / Shift+Space", "scroll; Space at the end = next"],
  ["Ctrl + / Ctrl −", "font size"],
  ["[ / ]", "narrower / wider column"],
  ["Ctrl 0", "reset font size and width"],
  ["?", "this help"],
  ["q", "quit"],
];

function toggleHelp() {
  const help = $("help");
  if (help.hidden) {
    $("help-panel").replaceChildren(el("div", { class: "panel-title" }, "Keys"),
      el("dl", {}, ...KEYS.flatMap(([k, v]) => [el("dt", {}, k), el("dd", {}, v)])));
  }
  help.hidden = !help.hidden;
}

// ---- Keyboard ------------------------------------------------------------
window.addEventListener("keydown", (e) => {
  if (!$("confirm").hidden) return;

  if (!$("help").hidden) {
    if (["Escape", "?", "q"].includes(e.key)) { e.preventDefault(); toggleHelp(); }
    return;
  }

  if (picker.open) {
    if (picker.key(e, queue.length > 0 || selectionLabel !== "")) e.preventDefault();
    return;
  }

  if (e.ctrlKey) {
    if (e.key === "=" || e.key === "+") setFontSize(fontSize + 1);
    else if (e.key === "-") setFontSize(fontSize - 1);
    else if (e.key === "0") { setFontSize(18); setWidth(DEFAULT_WIDTH); }
    else return;
    e.preventDefault();
    return;
  }
  if (e.altKey || e.metaKey) return;

  const caughtUp = !current() && queue.length > 0;
  switch (e.key) {
    case "j": case "ArrowDown": next(true); break;
    case "n": next(false); break;
    case "k": case "ArrowUp": prev(); break;
    case "Enter": if (caughtUp) openPicker(); else return; break;
    case " ":
      if (e.shiftKey) scrollPage(-1);
      else if (atBottom()) next(true);
      else scrollPage(1);
      break;
    case "m": void toggleRead(); break;
    case "o": void openOriginal(); break;
    case "b": void saveToReadeck(); break;
    case "f": void fetchFull(); break;
    case "A": void markAllRead(); break;
    case "g": case "p": case "Tab": openPicker(); break;
    case "[": setWidth(widthFactor - 4); break;
    case "]": setWidth(widthFactor + 4); break;
    case "?": toggleHelp(); break;
    case "q": window.close(); break;
    default: return;
  }
  e.preventDefault();
});

// Links inside articles always open in the browser.
document.addEventListener("click", (e) => {
  const a = (e.target as HTMLElement).closest("a[href]") as HTMLAnchorElement | null;
  if (!a) return;
  e.preventDefault();
  void openUrl(a.href);
});

$("picker-btn").addEventListener("click", openPicker);
$("prev-btn").addEventListener("click", prev);
$("next-btn").addEventListener("click", () => next(true));
$("readeck-btn").addEventListener("click", () => void saveToReadeck());
$("picker").addEventListener("click", (e) => {
  if (e.target === $("picker") && queue.length) picker.hide();
});

// ---- Start ---------------------------------------------------------------
async function start() {
  setFontSize(fontSize);
  setWidth(widthFactor);
  await initTheme();
  const setupError = await api.setupError();
  if (setupError) {
    reader.replaceChildren(el("div", { class: "message" }, el("h1", {}, "Setup needed"),
      el("p", {}, setupError), el("p", { class: "hint" }, "Restart the app after editing the config.")));
    return;
  }
  render();
  openPicker();
}
void start();
