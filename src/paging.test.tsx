import { cleanup, render, screen, waitFor } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Entry } from "./api";
import { mergePage, nextPage, PAGE } from "./paging";
import { current, hasMore, loading, queue } from "./state/queue";
import { start } from "./state/settings";
import { setOverlay } from "./state/ui";
import { api, entry, fakeMiniflux, isUnread, items, key, miniflux, pickerLoaded, range, serve } from "./testing";

vi.mock("./api", () => import("./testing"));
vi.mock("./theme", () => ({ initTheme: (): Promise<void> => Promise.resolve() }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: (): { setTitle: () => Promise<void> } => ({ setTitle: (): Promise<void> => Promise.resolve() }),
}));

function noLayout(): void {
  // jsdom does no layout, so it has no scrolling methods.
}
Element.prototype.scrollIntoView = noLayout;
Element.prototype.scrollBy = noLayout;

beforeEach(() => {
  vi.clearAllMocks();
  api.status.mockResolvedValue({ configured: true, readeck: false });
  api.feedCounts.mockResolvedValue([
    { feed: { id: 1, title: "Feed", category: { id: 1, title: "Dev" } }, unread: 1, read: 1 },
  ]);
  fakeMiniflux();
  setOverlay("none");
});
afterEach(cleanup);

/** Starts the app and opens "All unread" (or "All read", in history) from the picker. */
async function open(history = false): Promise<void> {
  render(() => <App />);
  await start();
  await pickerLoaded();
  if (history) {
    key("h");
    await screen.findByText("All read", { selector: "#picker-list .label" });
  }
  key("Enter");
  await waitFor(() => expect(current()).toBeDefined());
}

/** The article on screen once any page it waits for has loaded; undefined at the true end. */
async function shown(): Promise<Entry | undefined> {
  await waitFor(() => expect(current() !== undefined || !(hasMore() || loading())).toBe(true));
  return current();
}

/** Presses `keys` on every article until the end; returns the ids it showed, in order. */
async function pressThrough(...keys: string[]): Promise<number[]> {
  const article = await shown();
  if (article === undefined) {
    return [];
  }
  for (const k of keys) {
    key(k);
  }
  return [article.id, ...(await pressThrough(...keys))];
}

function markElsewhere(ids: number[]): void {
  for (const e of miniflux.entries) {
    if (ids.includes(e.id)) {
      e.status = "read";
    }
  }
}

describe("where a page starts", () => {
  const second = (e: Entry): number => Date.parse(e.published_at) / 1000;

  it("starts at the beginning of the list", () => {
    expect(nextPage([], "unread")).toEqual({ past: 0, limit: PAGE });
  });

  it("starts a second before the newest unread item, and is longer by what that second overlaps", () => {
    const tied = [entry(1, "A"), entry(2, "B", "read", 5), entry(3, "C", "unread", 5)];
    expect(nextPage(tied, "unread")).toEqual({ past: second(tied[2]!) - 1, limit: PAGE + 2 });
  });

  it("in history, starts a second after the oldest item", () => {
    const read = [entry(9, "A", "read"), entry(8, "B", "read")];
    expect(nextPage(read, "history")).toEqual({ past: second(read[1]!) + 1, limit: PAGE + 1 });
  });

  it("drops what's loaded already, and stops at a page with nothing new", () => {
    const loaded = [entry(1, "A"), entry(2, "B")];
    expect(mergePage(loaded, { total: 10, entries: [entry(2, "B"), entry(3, "C")] })).toEqual({
      fresh: [entry(3, "C")],
      more: true,
      total: 11,
    });
    expect(mergePage(loaded, { total: 10, entries: loaded })).toEqual({ fresh: [], more: false, total: 2 });
  });
});

describe("unread paging", () => {
  it("loads a page at a time, and j marking items read doesn't make it skip any", async () => {
    serve(items(120));
    await open();
    expect(api.entries).toHaveBeenCalledWith({ kind: "all" }, "unread", 0, PAGE);
    expect(queue()).toHaveLength(PAGE);
    expect(screen.getByText("1 / 120")).toBeTruthy(); // the whole selection
    for (let i = 0; i < PAGE - 10; i += 1) {
      key("j");
    }
    await waitFor(() => expect(queue()).toHaveLength(2 * PAGE));
    expect(screen.getByText(`${PAGE - 9} / 120`)).toBeTruthy();
    expect(await pressThrough("j")).toEqual(range(PAGE - 9, 120));
    expect(queue().map((e) => e.id)).toEqual(range(1, 120));
    await screen.findByText("All caught up");
    expect(miniflux.entries.filter(isUnread)).toHaveLength(0);
    key("j");
    await pickerLoaded(); // the true end goes back to the feeds
  });

  it("shows everything once, with ties in publication time and items read elsewhere meanwhile", async () => {
    // Items 30–110 were all published in the same second, which Miniflux returns in any order.
    serve(items(130).map((e) => (e.id >= 30 && e.id <= 110 ? entry(e.id, e.title, "unread", 30) : e)));
    await open();
    const loaded = new Set(queue().map((e) => e.id));
    const notLoaded = [
      ...range(30, 110)
        .filter((id) => !loaded.has(id))
        .slice(0, 6),
      125,
      130,
    ];
    markElsewhere([...range(5, 20), ...notLoaded]);
    const ids = await pressThrough("n"); // skipping keeps them unread, so they stay in the server's list
    expect(new Set(ids).size).toBe(ids.length);
    expect([...ids].sort((a, b) => a - b)).toEqual(range(1, 130).filter((id) => !notLoaded.includes(id)));
    await screen.findByText("All caught up");
  });

  it("marking all read marks what was unread when the selection was opened, not what arrived since", async () => {
    serve(items(120));
    await open();
    key("j");
    key("j");
    await screen.findByText("Item 3");
    miniflux.entries.push(entry(121, "Arrived later"));
    key("A", { shiftKey: true });
    await screen.findByText("Mark all 118 unread items in “All unread” as read?");
    key("y");
    await screen.findByText("All caught up");
    expect(api.markReadUpTo).toHaveBeenCalledWith({ kind: "all" }, 120);
    expect(miniflux.entries.filter(isUnread).map((e) => e.id)).toEqual([121]);
    expect(queue().every((e) => e.status === "read")).toBe(true);
    expect(hasMore()).toBe(false);
  });

  it("marking all read also marks a later item that a page did load", async () => {
    serve(items(PAGE + 5));
    await open();
    miniflux.entries.push(entry(PAGE + 6, "Arrived later"));
    for (let i = 0; i < PAGE - 10; i += 1) {
      key("n");
    }
    await waitFor(() => expect(queue()).toHaveLength(PAGE + 6));
    key("A", { shiftKey: true });
    await screen.findByText(`Mark all ${PAGE + 6} unread items in “All unread” as read?`);
    key("y");
    await screen.findByText("All caught up");
    expect(api.setStatus).toHaveBeenCalledWith([PAGE + 6], "read");
    expect(miniflux.entries.filter(isUnread)).toHaveLength(0);
  });
});

describe("history paging", () => {
  /** Read entries, `total` of them, titled "Read 1" … "Read total", oldest first. */
  function serveHistory(total: number): void {
    serve(range(1, total).map((id) => entry(id, `Read ${id}`, "read")));
  }

  it("loads the next page as you get near the end", async () => {
    serveHistory(PAGE + 5);
    await open(true);
    expect(screen.getByText(`Read ${PAGE + 5}`)).toBeTruthy();
    expect(screen.getByText(`1 / ${PAGE + 5}`)).toBeTruthy();
    for (let i = 0; i < PAGE - 10; i += 1) {
      key("n");
    }
    // Page two starts before the oldest item loaded: Read 6.
    const past = Date.parse(entry(6, "").published_at) / 1000 + 1;
    await waitFor(() => expect(api.entries).toHaveBeenCalledWith({ kind: "all" }, "read", past, PAGE + 1));
    await waitFor(() => expect(queue()).toHaveLength(PAGE + 5));
    expect(hasMore()).toBe(false);
  });

  it("m marking items unread doesn't make it skip any", async () => {
    serveHistory(2 * PAGE);
    await open(true);
    expect(await pressThrough("m", "n")).toEqual(range(1, 2 * PAGE).reverse());
    expect(miniflux.entries.filter(isUnread)).toHaveLength(2 * PAGE);
    await screen.findByText("End of history");
  });

  it("marking all read marks what was marked unread here", async () => {
    serveHistory(2 * PAGE);
    await open(true);
    key("m");
    key("n");
    key("m");
    key("A", { shiftKey: true });
    await screen.findByText("Mark all 2 unread items in “All read” as read?");
    key("y");
    await waitFor(() => expect(api.setStatus).toHaveBeenLastCalledWith([2 * PAGE, 2 * PAGE - 1], "read"));
    expect(miniflux.entries.filter(isUnread)).toHaveLength(0);
  });
});
