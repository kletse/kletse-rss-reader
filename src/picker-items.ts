// Turns feed counts into the rows of the feeds picker.
import type { FeedCount, Selection } from "./api";

export interface PickerItem { selection: Selection; label: string; unread: number; depth: 0 | 1 }

/** Turns feed counts into the picker rows: "All unread", then each category (sorted) with its feeds. */
export function build(counts: FeedCount[]): PickerItem[] {
  const byCategory = new Map<number, { title: string; feeds: FeedCount[] }>();
  for (const fc of counts) {
    if (fc.unread === 0) continue;
    const cat = byCategory.get(fc.feed.category.id) ?? { title: fc.feed.category.title, feeds: [] };
    cat.feeds.push(fc);
    byCategory.set(fc.feed.category.id, cat);
  }
  const total = counts.reduce((sum, fc) => sum + fc.unread, 0);
  const out: PickerItem[] = [];
  if (total > 0) out.push({ selection: { kind: "all" }, label: "All unread", unread: total, depth: 0 });

  const byTitle = (a: { title: string }, b: { title: string }) => a.title.localeCompare(b.title);
  for (const [id, cat] of [...byCategory].sort((a, b) => byTitle(a[1], b[1]))) {
    const unread = cat.feeds.reduce((sum, fc) => sum + fc.unread, 0);
    out.push({ selection: { kind: "category", id }, label: cat.title, unread, depth: 0 });
    for (const fc of [...cat.feeds].sort((a, b) => byTitle(a.feed, b.feed))) {
      out.push({ selection: { kind: "feed", id: fc.feed.id }, label: fc.feed.title, unread: fc.unread, depth: 1 });
    }
  }
  return out;
}
