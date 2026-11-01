// Shared by the UI tests: the Rust commands as mocks, served by a fake Miniflux, plus entries and key presses.
// A test file mocks the commands with `vi.mock("./api", () => import("./testing"))`.
import { fireEvent, waitFor } from "@solidjs/testing-library";
import { expect, type Mock, vi } from "vitest";
import type { Entry, EntryPage, EntryStatus, api as realApi, Selection } from "./api";

type Api = typeof realApi;
type MockedApi = { [K in keyof Api]: Mock<Api[K]> };

export const api: MockedApi = {
  status: vi.fn(),
  getSettings: vi.fn(),
  configProblem: vi.fn(),
  saveSettings: vi.fn(),
  theme: vi.fn(),
  feedCounts: vi.fn(),
  entries: vi.fn(),
  newestUnreadId: vi.fn(),
  unreadCount: vi.fn(),
  markReadUpTo: vi.fn(),
  setStatus: vi.fn(),
  fetchContent: vi.fn(),
  refreshFeeds: vi.fn(),
  saveToReadeck: vi.fn(),
  openInBrowser: vi.fn(),
  quit: vi.fn(),
};

const MINUTE_MS = 60_000;

/** An entry published `minute` (by default `id`) minutes after the start of October. */
export function entry(id: number, title: string, status: EntryStatus = "unread", minute = id): Entry {
  return {
    id,
    title,
    url: `https://example.com/${id}`,
    author: "",
    content: `<p>${title} body</p>`,
    published_at: new Date(Date.UTC(2026, 9, 1) + minute * MINUTE_MS).toISOString(),
    status,
    reading_time: 1,
    feed: { id: 1, title: "Feed" },
  };
}

export function range(from: number, to: number): number[] {
  return Array.from({ length: to - from + 1 }, (_, i) => from + i);
}

/** `n` unread entries titled "Item 1" … "Item n", oldest first. */
export function items(n: number): Entry[] {
  return range(1, n).map((id) => entry(id, `Item ${id}`));
}

export function isUnread(e: Entry): boolean {
  return e.status === "unread";
}

/** What the fake Miniflux holds (every selection gets all of it). Tests change statuses here as "elsewhere". */
export const miniflux: { entries: Entry[]; requests: number } = { entries: [], requests: 0 };

export function serve(entries: Entry[]): void {
  miniflux.entries = entries;
}

function seconds(e: Entry): number {
  return Date.parse(e.published_at) / 1000;
}

/**
 * Pages like Miniflux: `past` is a strict bound in whole seconds, and items published in the same second come in
 * a different order on every request, as Postgres may return them.
 */
function page(status: EntryStatus, past: number, limit: number): EntryPage {
  miniflux.requests += 1;
  const salt = miniflux.requests;
  const dir = status === "unread" ? 1 : -1;
  const list = miniflux.entries
    .filter((e) => e.status === status && (past === 0 || dir * (seconds(e) - past) > 0))
    .sort((a, b) => dir * (seconds(a) - seconds(b)) || ((a.id * salt) % 17) - ((b.id * salt) % 17) || a.id - b.id);
  return { total: list.length, entries: list.slice(0, limit).map((e) => ({ ...e })) };
}

function mark(ids: number[], status: EntryStatus): void {
  for (const e of miniflux.entries) {
    if (ids.includes(e.id)) {
      e.status = status;
    }
  }
}

function unreadUpTo(upTo: number): number[] {
  return miniflux.entries.filter((e) => isUnread(e) && e.id <= upTo).map((e) => e.id);
}

/** Lets the entry commands answer from `miniflux`. */
export function fakeMiniflux(): void {
  api.entries.mockImplementation((_: Selection, status: EntryStatus, past: number, limit: number) =>
    Promise.resolve(page(status, past, limit)),
  );
  api.setStatus.mockImplementation((ids: number[], status: EntryStatus) => {
    mark(ids, status);
    return Promise.resolve();
  });
  api.newestUnreadId.mockImplementation(() =>
    Promise.resolve(Math.max(0, ...miniflux.entries.filter(isUnread).map((e) => e.id))),
  );
  api.unreadCount.mockImplementation((_: Selection, upTo: number) => Promise.resolve(unreadUpTo(upTo).length));
  api.markReadUpTo.mockImplementation((_: Selection, upTo: number) => {
    const ids = unreadUpTo(upTo);
    mark(ids, "read");
    return Promise.resolve(ids.length);
  });
}

/** Key events bubble from the body to the window, where the app listens. */
export function key(k: string, opts: KeyboardEventInit = {}): void {
  fireEvent.keyDown(document.body, { key: k, ...opts });
}

export function element(selector: string): HTMLElement | null {
  return document.querySelector<HTMLElement>(selector);
}

/** Waits until the picker is open with its rows loaded. */
export async function pickerLoaded(): Promise<void> {
  await waitFor(() => expect(element("#picker-list button[data-depth='0']")).not.toBeNull());
}
