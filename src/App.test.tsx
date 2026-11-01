import { cleanup, fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { AppStatus, FeedCount } from "./api";
import { PAGE } from "./paging";
import { clearQueue, queue } from "./state/queue";
import { closeSettings, start } from "./state/settings";
import { setOverlay } from "./state/ui";
import { api, element, entry, fakeMiniflux, key, pickerLoaded, serve } from "./testing";

vi.mock("./api", () => import("./testing"));
vi.mock("./theme", () => ({ initTheme: (): Promise<void> => Promise.resolve() }));
const setTitle = vi.hoisted(() => vi.fn((_title: string): Promise<void> => Promise.resolve()));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: (): { setTitle: typeof setTitle } => ({ setTitle }) }));

function noLayout(): void {
  // jsdom does no layout, so it has no scrolling methods.
}
Element.prototype.scrollIntoView = noLayout;
Element.prototype.scrollBy = noLayout;

const counts: FeedCount[] = [{ feed: { id: 1, title: "Feed", category: { id: 1, title: "Dev" } }, unread: 2, read: 3 }];
const configured: AppStatus = { configured: true, readeck: false };

beforeEach(() => {
  vi.clearAllMocks();
  api.status.mockResolvedValue(configured);
  api.feedCounts.mockResolvedValue(counts);
  fakeMiniflux();
  serve([entry(1, "First"), entry(2, "Second")]);
  api.getSettings.mockResolvedValue({
    miniflux_url: "https://mf",
    miniflux_api_key: "k",
    readeck_url: "",
    readeck_token: "",
  });
  setOverlay("none");
  clearQueue();
});
afterEach(cleanup);

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
    api.setStatus.mockRejectedValueOnce({ kind: "unreachable", message: "offline" });
    await openQueue();
    key("m");
    await screen.findByText("offline");
    expect(element(".status")?.textContent).toBe("unread");
  });

  it("f fetches the full article once, then switches between it and the feed's version", async () => {
    api.fetchContent.mockResolvedValue("<p>Full text</p>");
    await openQueue();
    key("f");
    await screen.findByText("Full text");
    expect(api.fetchContent).toHaveBeenCalledWith(1);
    expect(screen.getByText("Full article")).toBeTruthy();
    key("f");
    await screen.findByText("First body");
    expect(screen.getByText("Feed version")).toBeTruthy();
    key("f");
    await screen.findByText("Full text");
    expect(screen.getByText("Full article")).toBeTruthy();
    expect(api.fetchContent).toHaveBeenCalledTimes(1);
  });

  it("f finishing after moving on leaves the article on screen alone", async () => {
    let finish: (content: string) => void = noLayout;
    api.fetchContent.mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    await openQueue();
    key("f");
    key("n");
    await screen.findByText("Second");
    finish("<p>Full text</p>");
    await waitFor(() => expect(queue()[0]?.content).toBe("<p>Full text</p>"));
    expect(screen.queryByText("Full article")).toBeNull();
    key("k");
    await screen.findByText("Full text");
    key("f");
    await screen.findByText("First body");
    expect(api.fetchContent).toHaveBeenCalledTimes(1);
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

  it("the window title shows the article, and just the app name elsewhere", async () => {
    const windowTitle = (): string | undefined => setTitle.mock.lastCall?.[0];
    await openQueue();
    expect(windowTitle()).toBe("First — Kletse RSS");
    key("Escape");
    await pickerLoaded();
    expect(windowTitle()).toBe("Kletse RSS");
    key("Escape");
    await waitFor(() => expect(windowTitle()).toBe("First — Kletse RSS"));
    key("n");
    await screen.findByText("Second");
    expect(windowTitle()).toBe("Second — Kletse RSS");
    key("n");
    await screen.findByText("All caught up");
    expect(windowTitle()).toBe("Kletse RSS");
  });

  it("an untitled article is (untitled) in the window title too", async () => {
    serve([entry(1, "")]);
    render(() => <App />);
    await start();
    await pickerLoaded();
    key("Enter");
    await screen.findByText("(untitled)");
    expect(setTitle.mock.lastCall?.[0]).toBe("(untitled) — Kletse RSS");
  });
});

describe("history", () => {
  /** Read entries, `total` of them, titled "Read 1" … "Read total", oldest first. */
  function serveHistory(total: number): void {
    serve(Array.from({ length: total }, (_, i) => entry(i + 1, `Read ${i + 1}`, "read")));
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
    expect(api.entries).toHaveBeenCalledWith({ kind: "all" }, "read", 0, PAGE);
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
    api.status.mockResolvedValue({ configured: false, readeck: false });
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
