// What the backend reported about the configured servers.
import { createSignal } from "solid-js";
import type { AppStatus } from "../api";

const [readeckEnabled, setReadeckEnabled] = createSignal(false);

export { readeckEnabled };

export function applyStatus(status: AppStatus): void {
  setReadeckEnabled(status.readeck);
}
