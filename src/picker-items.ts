// Turns feed counts into the rows of the feeds picker.
import type { FeedCount, Selection } from "./api";

/** "unread": read the unread items, oldest first. "history": browse read items, newest first. */
export type Mode = "unread" | "history";

export interface PickerItem {
  selection: Selection;
  label: string;
  count: number;
  depth: 0 | 1;
}

interface CategoryGroup {
  title: string;
  feeds: FeedCount[];
}

function byTitle(a: { title: string }, b: { title: string }): number {
  return a.title.localeCompare(b.title);
}

/** Feeds with something to show, grouped by category id. */
function groupByCategory(counts: FeedCount[], countOf: (fc: FeedCount) => number): Map<number, CategoryGroup> {
  const groups = new Map<number, CategoryGroup>();
  for (const fc of counts) {
    if (countOf(fc) > 0) {
      const group = groups.get(fc.feed.category.id) ?? { title: fc.feed.category.title, feeds: [] };
      group.feeds.push(fc);
      groups.set(fc.feed.category.id, group);
    }
  }
  return groups;
}

/** A category row followed by a row per feed, feeds sorted by title. */
function categoryRows(id: number, group: CategoryGroup, countOf: (fc: FeedCount) => number): PickerItem[] {
  const count = group.feeds.reduce((sum, fc) => sum + countOf(fc), 0);
  const feeds = [...group.feeds].sort((a, b) => byTitle(a.feed, b.feed));
  return [
    { selection: { kind: "category", id }, label: group.title, count, depth: 0 },
    ...feeds.map(
      (fc): PickerItem => ({
        selection: { kind: "feed", id: fc.feed.id },
        label: fc.feed.title,
        count: countOf(fc),
        depth: 1,
      }),
    ),
  ];
}

/**
 * Turns feed counts into the picker rows: "All unread" (or "All read" in history mode),
 * then each category (sorted) with its feeds. Anything with nothing to show is left out.
 */
export function build(counts: FeedCount[], mode: Mode = "unread"): PickerItem[] {
  const countOf = (fc: FeedCount): number => (mode === "history" ? fc.read : fc.unread);
  const total = counts.reduce((sum, fc) => sum + countOf(fc), 0);
  const groups = [...groupByCategory(counts, countOf)].sort((a, b) => byTitle(a[1], b[1]));
  const categories = groups.flatMap(([id, group]) => categoryRows(id, group, countOf));
  if (total === 0) {
    return categories;
  }
  const label = mode === "history" ? "All read" : "All unread";
  return [{ selection: { kind: "all" }, label, count: total, depth: 0 }, ...categories];
}
