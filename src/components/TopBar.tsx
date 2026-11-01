import { hasMore, index, inSettings, next, openPicker, prev, queue, queueMode, readeckEnabled, saveToReadeck, selectionLabel, total } from "../state";

export function TopBar() {
  const position = () => {
    const n = queue().length;
    return n ? `${Math.min(index() + 1, n)} / ${hasMore() ? total() : n}` : "";
  };
  return (
    <header id="bar" data-tauri-drag-region classList={{ "settings-mode": inSettings() }}>
      <button id="picker-btn" title="Feeds (p)" onClick={() => openPicker()}>
        <span class="icon">☰</span> <span id="selection-label">{selectionLabel() || "Feeds"}</span>
      </button>
      <span id="mode-badge" hidden={queueMode() !== "history" || !selectionLabel()} title="Read items, newest first (h to switch)">History</span>
      <span id="position" data-tauri-drag-region>{position()}</span>
      <div class="bar-actions">
        <button id="readeck-btn" title="Save to Readeck (b)" hidden={!readeckEnabled()} onClick={() => void saveToReadeck()}>
          <svg class="btn-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 3h12v18l-6-4-6 4z" /><path d="M12 7v6M9 10h6" /></svg>
          Readeck
        </button>
        <button id="prev-btn" title="Previous (k)" onClick={prev}>▲</button>
        <button id="next-btn" title="Next, mark read (j)" onClick={() => next(true)}>▼</button>
      </div>
    </header>
  );
}
