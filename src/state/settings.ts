// Settings and onboarding, and starting the app.
import { type AppStatus, api } from "../api";
import { initTheme } from "../theme";
import { openPicker } from "./picker";
import { applyPrefs } from "./prefs";
import { clearQueue, queue } from "./queue";
import { applyStatus } from "./server";
import { inSettings, overlay, setOverlay, toast } from "./ui";

/** Opens the settings form. On first run (onboarding) it can't be cancelled. */
export function openSettings(firstRun = false): void {
  if (!inSettings()) {
    setOverlay(firstRun ? "onboarding" : "settings");
  }
}

/** Called with the new status after a successful save, or undefined when cancelled. */
export function closeSettings(status: AppStatus | undefined): void {
  const firstRun = overlay() === "onboarding";
  if (status !== undefined) {
    applyStatus(status);
    if (!firstRun) {
      toast("Settings saved");
    }
    // The server may have changed: start from the picker with a fresh queue.
    clearQueue();
    openPicker();
  } else if (!firstRun) {
    setOverlay("none");
    if (queue().length === 0) {
      openPicker();
    }
  }
}

export async function start(): Promise<void> {
  applyPrefs();
  await initTheme();
  const status = await api.status();
  if (status.configured) {
    applyStatus(status);
    openPicker("unread");
  } else {
    openSettings(true);
  }
}
