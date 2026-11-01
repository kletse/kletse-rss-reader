import type { JSX } from "solid-js";
import { advance, openPicker } from "../state/picker";
import { hasMore, index, prev, queue, queueMode, saveToReadeck, selectionLabel, total } from "../state/queue";
import { readeckEnabled } from "../state/server";
import { inSettings } from "../state/ui";

/** "3 / 120" while reading, counting the pages not loaded yet. */
function position(): string {
  const n = queue().length;
  if (n === 0) {
    return "";
  }
  return `${Math.min(index() + 1, n)} / ${hasMore() ? total() : n}`;
}

export function TopBar(): JSX.Element {
  return (
    <header id="bar" data-tauri-drag-region={true} data-settings={inSettings()}>
      <button type="button" id="picker-btn" title="Feeds (p)" onClick={(): void => openPicker()}>
        <span class="icon">☰</span> <span id="selection-label">{selectionLabel() || "Feeds"}</span>
      </button>
      <span
        id="mode-badge"
        hidden={queueMode() !== "history" || selectionLabel() === ""}
        title="Read items, newest first (h to switch)"
      >
        History
      </span>
      <span id="position" data-tauri-drag-region={true}>
        {position()}
      </span>
      <div class="bar-actions">
        <button
          type="button"
          id="readeck-btn"
          title="Save to Readeck (b)"
          hidden={!readeckEnabled()}
          onClick={(): void => void saveToReadeck()}
        >
          <svg class="btn-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M6 3h12v18l-6-4-6 4z" />
            <path d="M12 7v6M9 10h6" />
          </svg>
          Readeck
        </button>
        <button type="button" id="prev-btn" title="Previous (k)" onClick={prev}>
          ▲
        </button>
        <button type="button" id="next-btn" title="Next, mark read (j)" onClick={(): void => advance(true)}>
          ▼
        </button>
      </div>
    </header>
  );
}
