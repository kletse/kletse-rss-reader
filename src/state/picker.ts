// The feeds picker. It has its own mode: switching it doesn't touch the open queue until you choose something.
import { createSignal } from "solid-js";
import { api, type FeedCount } from "../api";
import { build, type Mode, type PickerItem } from "../picker-items";
import { next, openSelection, queue, queueMode, selectionLabel } from "./queue";
import { errorMessage, overlay, setOverlay, toast, toastError } from "./ui";

export type PickerList =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; counts: FeedCount[] };
const [pickerList, setPickerList] = createSignal<PickerList>({ kind: "loading" });
const [pickerCursor, setPickerCursor] = createSignal(0);
const [pickerMode, setPickerModeSignal] = createSignal<Mode>("unread");

export { pickerCursor, pickerList, pickerMode };

// Miniflux refreshes feeds in the background; the counts are reloaded a few times while it works.
const RELOAD_SOON_MS = 2000;
const RELOAD_LATER_MS = 6000;
const RELOAD_LAST_MS = 15_000;
const REFRESH_RELOAD_MS = [RELOAD_SOON_MS, RELOAD_LATER_MS, RELOAD_LAST_MS] as const;

export function pickerItems(): PickerItem[] {
  const list = pickerList();
  return list.kind === "ready" ? build(list.counts, pickerMode()) : [];
}

function setPickerMode(mode: Mode): void {
  if (mode !== pickerMode()) {
    setPickerModeSignal(mode);
    setPickerCursor(0);
  }
}

async function loadPicker(): Promise<void> {
  if (pickerList().kind !== "ready") {
    setPickerList({ kind: "loading" });
  }
  try {
    setPickerList({ kind: "ready", counts: await api.feedCounts() });
    const n = pickerItems().length;
    setPickerCursor((c) => Math.min(c, Math.max(n - 1, 0)));
  } catch (err) {
    setPickerList({ kind: "error", message: errorMessage(err) });
  }
}

/** Opens the picker, by default in the mode of the open queue. */
export function openPicker(mode: Mode = queueMode()): void {
  setPickerMode(mode);
  setOverlay("picker");
  void loadPicker();
}

export function closePicker(): void {
  if (overlay() === "picker") {
    setOverlay("none");
  }
}

/** h: switch between unread and history (read items, newest first). */
export function toggleMode(): void {
  const mode = overlay() === "picker" ? pickerMode() : queueMode();
  openPicker(mode === "history" ? "unread" : "history");
}

/** The picker can only be dismissed once there is something to go back to. */
export function pickerCanClose(): boolean {
  return queue().length > 0 || selectionLabel() !== "";
}

export async function refreshFeeds(): Promise<void> {
  toast("Refreshing feeds…");
  try {
    await api.refreshFeeds();
  } catch (err) {
    toastError(err);
    return;
  }
  for (const delay of REFRESH_RELOAD_MS) {
    setTimeout(() => {
      if (overlay() === "picker") {
        void loadPicker();
      }
    }, delay);
  }
}

export function movePicker(delta: number): void {
  const n = pickerItems().length;
  if (n > 0) {
    setPickerCursor((c) => (c + delta + n) % n);
  }
}

export function choose(item: PickerItem | undefined = pickerItems()[pickerCursor()]): void {
  if (item !== undefined) {
    closePicker();
    void openSelection(item, pickerMode());
  }
}

/** j, Space at the end, the ▼ button: the next article, or the picker once the queue is done. */
export function advance(markRead: boolean): void {
  if (!next(markRead)) {
    openPicker();
  }
}
