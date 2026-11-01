// The native window title follows what you're reading, so window switchers show it.
// Tauri doesn't pass document.title on to the window, so this sets it through the window API.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createEffect, createMemo } from "solid-js";
import { displayTitle } from "./content";
import { current, loading } from "./state/queue";
import { inSettings, overlay } from "./state/ui";

export const APP_NAME = "Kletse RSS";

/** "Article title — Kletse RSS" while an article is on screen; just the app name otherwise. */
export function windowTitle(): string {
  const entry = current();
  if (entry === undefined || loading() || inSettings() || overlay() === "picker") {
    return APP_NAME;
  }
  return `${displayTitle(entry.title)} — ${APP_NAME}`;
}

/** Keeps the window title in step with windowTitle(). Call it inside a component. */
export function followWindowTitle(): void {
  const title = createMemo(windowTitle);
  createEffect(() => {
    getCurrentWindow()
      .setTitle(title())
      .catch(() => {
        // Only the title in the window switcher is stale; not worth a toast.
      });
  });
}
