import { cleanup, fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Entry, FeedCount, Status } from "./api";

const entry = (id: number, title: string, status: Entry["status"] = "unread"): Entry => ({
  id, title, url: `https://example.com/${id}`, author: "", content: `<p>${title} body</p>`,
  published_at: "2026-10-01T10:00:00Z", status, reading_time: 1, feed: { id: 1, title: "Feed" },
});
const counts: FeedCount[] = [{ feed: { id: 1, title: "Feed", category: { id: 1, title: "Dev" } }, unread: 2, read: 3 }];
const configured: Status = { configured: true, readeck: false, demo: true };

const api = vi.hoisted(() => ({
  status: vi.fn(), getSettings: vi.fn(), saveSettings: vi.fn(), theme: vi.fn(), feedCounts: vi.fn(),
  unreadEntries: vi.fn(), readEntries: vi.fn(), setStatus: vi.fn(), fetchContent: vi.fn(), refreshFeeds: vi.fn(),
  saveToReadeck: vi.fn(), openInBrowser: vi.fn(), quit: vi.fn(),
}));
vi.mock("./api", () => ({ api }));
vi.mock("./theme", () => ({ initTheme: async () => {} }));
Element.prototype.scrollIntoView = () => {}; // not in jsdom
Element.prototype.scrollBy = () => {};

const { App } = await import("./App");
const s = await import("./state");

const key = (k: string, opts: KeyboardEventInit = {}) => fireEvent.keyDown(window, { key: k, ...opts });

beforeEach(() => {
  vi.clearAllMocks();
  api.status.mockResolvedValue(configured);
  api.feedCounts.mockResolvedValue(counts);
  api.unreadEntries.mockImplementation(async () => [entry(1, "First"), entry(2, "Second")]);
  api.setStatus.mockResolvedValue(undefined);
  api.getSettings.mockResolvedValue({ miniflux_url: "https://mf", miniflux_api_key: "k", readeck_url: "", readeck_token: "" });
  s.setOverlay("none");
});
afterEach(cleanup);

/** Waits until the picker is open with its rows loaded. */
const pickerLoaded = () => waitFor(() => expect(document.querySelector("#picker-list li.depth-0")).not.toBeNull());

/** Starts the app and opens "All unread" from the picker. */
async function openQueue() {
  render(() => <App />);
  await s.start();
  await pickerLoaded();
  key("Enter");
  await screen.findByText("First");
}

describe("reader", () => {
  it("j marks the article read and shows the next; k goes back", async () => {
    await openQueue();
    expect(screen.getByText("1 / 2")).toBeTruthy();
    key("j");
    await screen.findByText("Second");
    expect(api.setStatus).toHaveBeenCalledWith([1], "read");
    key("k");
    await screen.findByText("First");
    expect(document.querySelector(".status")?.textContent).toBe("read");
  });

  it("n skips without marking read, and the end says all caught up", async () => {
    await openQueue();
    key("n");
    key("n");
    await screen.findByText("All caught up");
    expect(api.setStatus).not.toHaveBeenCalled();
  });

  it("rolls the status back when the server fails", async () => {
    api.setStatus.mockRejectedValueOnce("offline");
    await openQueue();
    key("m");
    await screen.findByText("offline");
    expect(document.querySelector(".status")?.textContent).toBe("unread");
  });

  it("Esc opens the picker, and Esc closes it again once there is a queue", async () => {
    await openQueue();
    key("Escape");
    await pickerLoaded();
    key("Escape");
    await waitFor(() => expect(document.getElementById("picker")).toBeNull());
  });
});

describe("history", () => {
  /** Read entries newest first, `total` of them, served a page at a time. */
  function serveHistory(total: number) {
    api.readEntries.mockImplementation(async (_sel, offset: number, limit: number) => ({
      total,
      entries: Array.from({ length: Math.max(0, Math.min(limit, total - offset)) }, (_, i) =>
        entry(total - offset - i, `Read ${total - offset - i}`, "read")),
    }));
  }

  async function openHistory() {
    render(() => <App />);
    await s.start();
    await pickerLoaded();
    key("h");
    await screen.findByText("History", { selector: ".panel-title span" });
    expect(screen.getByText("All read", { selector: "#picker-list .label" })).toBeTruthy();
    key("Enter");
  }

  it("h in the picker switches to read items, newest first, with the same keys", async () => {
    serveHistory(3);
    await openHistory();
    await screen.findByText("Read 3");
    expect(api.readEntries).toHaveBeenCalledWith({ kind: "all" }, 0, s.HISTORY_PAGE);
    expect(document.getElementById("mode-badge")?.hidden).toBe(false);
    key("j");
    await screen.findByText("Read 2");
    expect(api.setStatus).not.toHaveBeenCalled(); // already read
    key("m");
    await waitFor(() => expect(api.setStatus).toHaveBeenCalledWith([2], "unread"));
    expect(document.querySelector(".status")?.textContent).toBe("unread");
    key("k");
    await screen.findByText("Read 3");
    key("n"); key("n"); key("n");
    await screen.findByText("End of history");
  });

  it("loads the next page as you get near the end", async () => {
    serveHistory(s.HISTORY_PAGE + 5);
    await openHistory();
    await screen.findByText(`Read ${s.HISTORY_PAGE + 5}`);
    expect(screen.getByText(`1 / ${s.HISTORY_PAGE + 5}`)).toBeTruthy();
    for (let i = 0; i < s.HISTORY_PAGE - 10; i++) key("n");
    await waitFor(() => expect(api.readEntries).toHaveBeenCalledWith({ kind: "all" }, s.HISTORY_PAGE, s.HISTORY_PAGE));
    await waitFor(() => expect(s.queue().length).toBe(s.HISTORY_PAGE + 5));
    expect(s.hasMore()).toBe(false);
  });

  it("h in the reader opens the picker in the other mode; Esc goes back to the open queue", async () => {
    await openQueue();
    key("h");
    await screen.findByText("History", { selector: ".panel-title span" });
    key("Escape");
    await waitFor(() => expect(document.getElementById("picker")).toBeNull());
    expect(screen.getByText("First")).toBeTruthy();
    expect(document.getElementById("mode-badge")?.hidden).toBe(true);
  });
});

describe("overlays", () => {
  it("the picker can't be dismissed before anything was opened", async () => {
    render(() => <App />);
    await s.start();
    s.closeSettings({ ...configured }); // resets the queue and selection
    await pickerLoaded();
    key("Escape");
    expect(document.getElementById("picker")).not.toBeNull();
  });

  it("the confirm dialog swallows reader keys", async () => {
    await openQueue();
    key("A", { shiftKey: true });
    await screen.findByText(/Mark all 2 unread items/);
    key("j");
    expect(screen.getByText("First")).toBeTruthy();
    key("n");
    await waitFor(() => expect(document.getElementById("confirm")).toBeNull());
    expect(api.setStatus).not.toHaveBeenCalled();
  });

  it("help opens with ? and closes with Esc", async () => {
    await openQueue();
    key("?");
    await screen.findByText("Keys");
    key("Escape");
    await waitFor(() => expect(screen.queryByText("Keys")).toBeNull());
  });
});

describe("settings", () => {
  it("Esc cancels settings", async () => {
    await openQueue();
    key(",");
    await screen.findByText("Settings");
    key("Escape");
    await screen.findByText("First");
  });

  it("onboarding can't be cancelled and connects on submit", async () => {
    api.status.mockResolvedValue({ configured: false, readeck: false, demo: false });
    api.saveSettings.mockResolvedValue({ ...configured, readeck: true });
    render(() => <App />);
    await s.start();
    await screen.findByText("Welcome to Kletse RSS");
    key("Escape");
    expect(screen.getByText("Welcome to Kletse RSS")).toBeTruthy();
    fireEvent.click(screen.getByText("Connect"));
    await pickerLoaded();
    expect(document.getElementById("readeck-btn")?.hidden).toBe(false);
  });
});
