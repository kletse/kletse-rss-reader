// The feeds/categories overlay: "All unread", categories, and their feeds with unread counts.
import { api, type FeedCount, type Selection } from "./api";
import { $, el, toast } from "./ui";

export interface PickerItem { selection: Selection; label: string; unread: number; depth: 0 | 1 }

let items: PickerItem[] = [];
let cursor = 0;
let onChoose: (item: PickerItem) => void = () => {};

export const picker = {
  get open() { return !$("picker").hidden; },

  async show(choose: (item: PickerItem) => void) {
    onChoose = choose;
    $("picker").hidden = false;
    await load();
  },

  hide() { $("picker").hidden = true; },

  /** Handles a key while the picker is open. Returns true if consumed. */
  key(e: KeyboardEvent, canClose: boolean): boolean {
    switch (e.key) {
      case "j": case "ArrowDown": move(1); return true;
      case "k": case "ArrowUp": move(-1); return true;
      case "Enter": if (items[cursor]) choose(items[cursor]); return true;
      case "r": refresh(); return true;
      case "Escape": case "g": case "p": case "Tab": if (canClose) picker.hide(); return true;
    }
    return false;
  },
};

function choose(item: PickerItem) {
  picker.hide();
  onChoose(item);
}

async function load() {
  const list = $("picker-list");
  if (items.length === 0) list.replaceChildren(el("li", { class: "empty" }, "Loading…"));
  try {
    items = build(await api.feedCounts());
  } catch (err) {
    list.replaceChildren(el("li", { class: "empty error" }, String(err)));
    return;
  }
  cursor = Math.min(cursor, Math.max(items.length - 1, 0));
  render();
}

async function refresh() {
  toast("Refreshing feeds…");
  try {
    await api.refreshFeeds();
  } catch (err) {
    toast(String(err), "error");
    return;
  }
  // Miniflux refreshes in the background; reload counts a few times while it works.
  for (const delay of [2000, 6000, 15000]) setTimeout(() => picker.open && load(), delay);
}

function build(counts: FeedCount[]): PickerItem[] {
  const byCategory = new Map<number, { title: string; feeds: FeedCount[] }>();
  for (const fc of counts) {
    if (fc.unread === 0) continue;
    const cat = byCategory.get(fc.feed.category.id) ?? { title: fc.feed.category.title, feeds: [] };
    cat.feeds.push(fc);
    byCategory.set(fc.feed.category.id, cat);
  }
  const total = counts.reduce((sum, fc) => sum + fc.unread, 0);
  const out: PickerItem[] = [];
  if (total > 0) out.push({ selection: { kind: "all" }, label: "All unread", unread: total, depth: 0 });

  const byTitle = (a: { title: string }, b: { title: string }) => a.title.localeCompare(b.title);
  for (const [id, cat] of [...byCategory].sort((a, b) => byTitle(a[1], b[1]))) {
    const unread = cat.feeds.reduce((sum, fc) => sum + fc.unread, 0);
    out.push({ selection: { kind: "category", id }, label: cat.title, unread, depth: 0 });
    for (const fc of cat.feeds.sort((a, b) => byTitle(a.feed, b.feed))) {
      out.push({ selection: { kind: "feed", id: fc.feed.id }, label: fc.feed.title, unread: fc.unread, depth: 1 });
    }
  }
  return out;
}

function render() {
  const list = $("picker-list");
  if (items.length === 0) {
    list.replaceChildren(el("li", { class: "empty" }, "Nothing unread. Press r to refresh."));
    return;
  }
  list.replaceChildren(
    ...items.map((item, i) => {
      const li = el("li", { class: `depth-${item.depth}${i === cursor ? " active" : ""}` },
        el("span", { class: "label" }, item.label),
        el("span", { class: "count" }, String(item.unread)));
      li.addEventListener("click", () => choose(item));
      return li;
    }),
  );
  list.querySelector(".active")?.scrollIntoView({ block: "nearest" });
}

function move(delta: number) {
  if (items.length === 0) return;
  cursor = (cursor + delta + items.length) % items.length;
  render();
}
