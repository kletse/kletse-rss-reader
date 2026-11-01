import { cleanup, fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from "vitest";
import { App } from "./App";
import type { Entry, EntryPage, EntryStatus, FeedCount, api as realApi, Selection, Status } from "./api";
import { HISTORY_PAGE, hasMore, queue } from "./state/queue";
import { closeSettings, start } from "./state/settings";
import { setOverlay } from "./state/ui";

type Api = typeof realApi;
type MockedApi = { [K in keyof Api]: Mock<Api[K]> };

const api = vi.hoisted(
  (): MockedApi => ({
    status: vi.fn(),
    getSettings: vi.fn(),
    saveSettings: vi.fn(),
    theme: vi.fn(),
    feedCounts: vi.fn(),
    unreadEntries: vi.fn(),
    readEntries: vi.fn(),
    setStatus: vi.fn(),
    fetchContent: vi.fn(),
    refreshFeeds: vi.fn(),
    saveToReadeck: vi.fn(),
    openInBrowser: vi.fn(),
    quit: vi.fn(),
  }),
);
vi.mock("./api", () => ({ api }));
vi.mock("./theme", () => ({ initTheme: (): Promise<void> => Promise.resolve() }));

function noLayout(): void {
  // jsdom does no layout, so it has no scrolling methods.
}
Element.prototype.scrollIntoView = noLayout;
Element.prototype.scrollBy = noLayout;

function entry(id: number, title: string, status: EntryStatus = "unread"): Entry {
  return {
    id,
    title,
    url: `https://example.com/${id}`,
    author: "",
    content: `<p>${title} body</p>`,
    published_at: "2026-10-01T10:00:00Z",
    status,
    reading_time: 1,
    feed: { id: 1, title: "Feed" },
  };
}
const counts: FeedCount[] = [{ feed: { id: 1, title: "Feed", category: { id: 1, title: "Dev" } }, unread: 2, read: 3 }];
const configured: Status = { configured: true, readeck: false, demo: true };

/** Key events bubble from the body to the window, where the app listens. */
function key(k: string, opts: KeyboardEventInit = {}): void {
  fireEvent.keyDown(document.body, { key: k, ...opts });
}

function element(selector: string): HTMLElement | null {
  return document.querySelector<HTMLElement>(selector);
}

beforeEach(() => {
  vi.clearAllMocks();
  api.status.mockResolvedValue(configured);
  api.feedCounts.mockResolvedValue(counts);
  api.unreadEntries.mockResolvedValue([entry(1, "First"), entry(2, "Second")]);
  api.setStatus.mockResolvedValue();
  api.getSettings.mockResolvedValue({
    miniflux_url: "https://mf",
    miniflux_api_key: "k",
    readeck_url: "",
    readeck_token: "",
  });
  setOverlay("none");
});
afterEach(cleanup);

/** Waits until the picker is open with its rows loaded. */
async function pickerLoaded(): Promise<void> {
  await waitFor(() => expect(element("#picker-list button[data-depth='0']")).not.toBeNull());
}

/** Starts the app and opens "All unread" from the picker. */
async function openQueue(): Promise<void> {
  render(() => <App />);
  await start();
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
    expect(element(".status")?.textContent).toBe("read");
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
    expect(element(".status")?.textContent).toBe("unread");
  });

  it("← opens the picker, and → opens the chosen row", async () => {
    await openQueue();
    key("ArrowLeft");
    await pickerLoaded();
    key("ArrowRight");
    await waitFor(() => expect(element("#picker")).toBeNull());
    expect(await screen.findByText("First")).toBeTruthy();
  });

  it("esc opens the picker, and Esc closes it again once there is a queue", async () => {
    await openQueue();
    key("Escape");
    await pickerLoaded();
    key("Escape");
    await waitFor(() => expect(element("#picker")).toBeNull());
  });
});

describe("history", () => {
  /** Read entries newest first, `total` of them, served a page at a time. */
  function serveHistory(total: number): void {
    api.readEntries.mockImplementation(
      (_selection: Selection, offset: number, limit: number): Promise<EntryPage> =>
        Promise.resolve({
          total,
          entries: Array.from({ length: Math.max(0, Math.min(limit, total - offset)) }, (_, i) =>
            entry(total - offset - i, `Read ${total - offset - i}`, "read"),
          ),
        }),
    );
  }

  async function openHistory(): Promise<void> {
    render(() => <App />);
    await start();
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
    expect(api.readEntries).toHaveBeenCalledWith({ kind: "all" }, 0, HISTORY_PAGE);
    expect(element("#mode-badge")?.hidden).toBe(false);
    key("j");
    await screen.findByText("Read 2");
    expect(api.setStatus).not.toHaveBeenCalled(); // already read
    key("m");
    await waitFor(() => expect(api.setStatus).toHaveBeenCalledWith([2], "unread"));
    expect(element(".status")?.textContent).toBe("unread");
    key("k");
    await screen.findByText("Read 3");
    key("n");
    key("n");
    key("n");
    await screen.findByText("End of history");
  });

  it("loads the next page as you get near the end", async () => {
    serveHistory(HISTORY_PAGE + 5);
    await openHistory();
    await screen.findByText(`Read ${HISTORY_PAGE + 5}`);
    expect(screen.getByText(`1 / ${HISTORY_PAGE + 5}`)).toBeTruthy();
    for (let i = 0; i < HISTORY_PAGE - 10; i += 1) {
      key("n");
    }
    await waitFor(() => expect(api.readEntries).toHaveBeenCalledWith({ kind: "all" }, HISTORY_PAGE, HISTORY_PAGE));
    await waitFor(() => expect(queue()).toHaveLength(HISTORY_PAGE + 5));
    expect(hasMore()).toBe(false);
  });

  it("h in the reader opens the picker in the other mode; Esc goes back to the open queue", async () => {
    await openQueue();
    key("h");
    await screen.findByText("History", { selector: ".panel-title span" });
    key("Escape");
    await waitFor(() => expect(element("#picker")).toBeNull());
    expect(screen.getByText("First")).toBeTruthy();
    expect(element("#mode-badge")?.hidden).toBe(true);
  });
});

describe("overlays", () => {
  it("the picker can't be dismissed before anything was opened", async () => {
    render(() => <App />);
    await start();
    closeSettings({ ...configured }); // resets the queue and selection
    await pickerLoaded();
    key("Escape");
    expect(element("#picker")).not.toBeNull();
  });

  it("the confirm dialog swallows reader keys", async () => {
    await openQueue();
    key("A", { shiftKey: true });
    await screen.findByText(/Mark all 2 unread items/u);
    key("j");
    expect(screen.getByText("First")).toBeTruthy();
    key("n");
    await waitFor(() => expect(element("#confirm")).toBeNull());
    expect(api.setStatus).not.toHaveBeenCalled();
  });

  it("? in the picker shows the keys, and closing them goes back to the picker", async () => {
    render(() => <App />);
    await start();
    await pickerLoaded();
    expect(screen.queryByText(/r refresh/u)).toBeNull(); // the picker itself only shows "?"
    key("?");
    await screen.findByText("Keys");
    key("Escape");
    await waitFor(() => expect(screen.queryByText("Keys")).toBeNull());
    expect(element("#picker")).not.toBeNull();
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
  it("esc cancels settings", async () => {
    await openQueue();
    key(",");
    await screen.findByText("Settings");
    key("Escape");
    expect(await screen.findByText("First")).toBeTruthy();
  });

  it("onboarding can't be cancelled and connects on submit", async () => {
    api.status.mockResolvedValue({ configured: false, readeck: false, demo: false });
    api.saveSettings.mockResolvedValue({ ...configured, readeck: true });
    render(() => <App />);
    await start();
    await screen.findByText("Welcome to Kletse RSS");
    key("Escape");
    expect(screen.getByText("Welcome to Kletse RSS")).toBeTruthy();
    fireEvent.click(screen.getByText("Connect"));
    await pickerLoaded();
    expect(element("#readeck-btn")?.hidden).toBe(false);
  });
});
