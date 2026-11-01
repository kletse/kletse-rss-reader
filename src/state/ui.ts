// UI state with no server behind it: which overlay has the keyboard, the toast, the yes/no dialog, scrolling.
import { createSignal } from "solid-js";

// ---- Overlays ------------------------------------------------------------
// Only one thing has the keyboard at a time; the settings form replaces the article.
export type Overlay = "none" | "picker" | "help" | "confirm" | "settings" | "onboarding";
const [overlay, setOverlay] = createSignal<Overlay>("none");

export { overlay, setOverlay };

let beforeHelp: Overlay = "none";

/** ?: the key reference, over whatever was open. */
export function openHelp(): void {
  beforeHelp = overlay();
  setOverlay("help");
}

/** Back to where ? was pressed: the reader or the picker. */
export function closeHelp(): void {
  setOverlay(beforeHelp);
}

export function inSettings(): boolean {
  return overlay() === "settings" || overlay() === "onboarding";
}

// ---- Toast ---------------------------------------------------------------
export interface Toast {
  message: string;
  kind: "info" | "error";
}
/** How long a toast stays up; errors get time to be read. */
const TOAST_MS: Record<Toast["kind"], number> = { info: 2200, error: 6000 };
const [toastMessage, setToastMessage] = createSignal<Toast | undefined>();

export { toastMessage };

let toastTimer: ReturnType<typeof setTimeout> | undefined;

export function toast(message: string, kind: Toast["kind"] = "info"): void {
  setToastMessage({ message, kind });
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => setToastMessage(undefined), TOAST_MS[kind]);
}

/** The Rust commands reject with a plain string; anything else is unexpected but still shown. */
export function errorMessage(err: unknown): string {
  if (typeof err === "string") {
    return err;
  }
  if (err instanceof Error) {
    return err.message;
  }
  return JSON.stringify(err);
}

export function toastError(err: unknown): void {
  toast(errorMessage(err), "error");
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
  return new Promise((resolve) => {
    confirmResolve = resolve;
  });
}

export function answerConfirm(yes: boolean): void {
  setOverlay(beforeConfirm);
  confirmResolve?.(yes);
  confirmResolve = undefined;
}

// ---- Reader element (scrolling) ------------------------------------------
/** Space scrolls by this much of the visible height, keeping a little context. */
const SCROLL_PAGE_FRACTION = 0.85;
/** Rounding slack when deciding the reader is scrolled to the end. */
const BOTTOM_SLACK_PX = 4;
let reader: HTMLElement | undefined;

export function setReader(el: HTMLElement): void {
  reader = el;
}

export function scrollPage(direction: 1 | -1): void {
  reader?.scrollBy({ top: direction * reader.clientHeight * SCROLL_PAGE_FRACTION, behavior: "smooth" });
}

export function atBottom(): boolean {
  return reader === undefined || reader.scrollTop + reader.clientHeight >= reader.scrollHeight - BOTTOM_SLACK_PX;
}

export function scrollToTop(): void {
  if (reader !== undefined) {
    reader.scrollTop = 0;
  }
}
