// What the backend reported about the configured servers.
import { createSignal } from "solid-js";
import type { Status } from "../api";

const [readeckEnabled, setReadeckEnabled] = createSignal(false);

export { readeckEnabled };

export function applyStatus(status: Status): void {
  setReadeckEnabled(status.readeck);
}
