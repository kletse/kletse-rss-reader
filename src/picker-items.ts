// Turns feed counts into the rows of the feeds picker.
import type { FeedCount, Selection } from "./api";

/** "unread": read the unread items, oldest first. "history": browse read items, newest first. */
export type Mode = "unread" | "history";

export interface PickerItem { selection: Selection; label: string; count: number; depth: 0 | 1 }

/**
 * Turns feed counts into the picker rows: "All unread" (or "All read" in history mode),
 * then each category (sorted) with its feeds. Anything with nothing to show is left out.
 */
export function build(counts: FeedCount[], mode: Mode = "unread"): PickerItem[] {
  const countOf = (fc: FeedCount) => (mode === "history" ? fc.read : fc.unread);
  const byCategory = new Map<number, { title: string; feeds: FeedCount[] }>();
  for (const fc of counts) {
    if (countOf(fc) === 0) continue;
    const cat = byCategory.get(fc.feed.category.id) ?? { title: fc.feed.category.title, feeds: [] };
    cat.feeds.push(fc);
    byCategory.set(fc.feed.category.id, cat);
  }
  const total = counts.reduce((sum, fc) => sum + countOf(fc), 0);
  const out: PickerItem[] = [];
  const allLabel = mode === "history" ? "All read" : "All unread";
  if (total > 0) out.push({ selection: { kind: "all" }, label: allLabel, count: total, depth: 0 });

  const byTitle = (a: { title: string }, b: { title: string }) => a.title.localeCompare(b.title);
  for (const [id, cat] of [...byCategory].sort((a, b) => byTitle(a[1], b[1]))) {
    const count = cat.feeds.reduce((sum, fc) => sum + countOf(fc), 0);
    out.push({ selection: { kind: "category", id }, label: cat.title, count, depth: 0 });
    for (const fc of [...cat.feeds].sort((a, b) => byTitle(a.feed, b.feed))) {
      out.push({ selection: { kind: "feed", id: fc.feed.id }, label: fc.feed.title, count: countOf(fc), depth: 1 });
    }
  }
  return out;
}
