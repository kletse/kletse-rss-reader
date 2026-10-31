import { describe, expect, it } from "vitest";
import type { FeedCount } from "./api";
import { build } from "./picker-items";

const fc = (id: number, title: string, category: [number, string], unread: number): FeedCount => ({
  feed: { id, title, category: { id: category[0], title: category[1] } },
  unread,
});

describe("picker build", () => {
  it("is empty when nothing is unread", () => {
    expect(build([])).toEqual([]);
    expect(build([fc(1, "A", [1, "Dev"], 0)])).toEqual([]);
  });

  it("lists all unread, then categories with their feeds, hiding read-out feeds", () => {
    const items = build([
      fc(1, "Zed", [1, "Dev"], 2),
      fc(2, "Alpha", [1, "Dev"], 3),
      fc(3, "Quiet", [2, "Blogs"], 0),
      fc(4, "Loud", [2, "Blogs"], 1),
      fc(5, "Nothing", [3, "Empty"], 0),
    ]);
    expect(items.map((i) => [i.label, i.unread, i.depth])).toEqual([
      ["All unread", 6, 0],
      ["Blogs", 1, 0],
      ["Loud", 1, 1],
      ["Dev", 5, 0],
      ["Alpha", 3, 1],
      ["Zed", 2, 1],
    ]);
    expect(items[0].selection).toEqual({ kind: "all" });
    expect(items[1].selection).toEqual({ kind: "category", id: 2 });
    expect(items[2].selection).toEqual({ kind: "feed", id: 4 });
  });

  it("does not mutate its input", () => {
    const input = [fc(1, "B", [1, "C"], 1), fc(2, "A", [1, "C"], 1)];
    build(input);
    expect(input.map((f) => f.feed.title)).toEqual(["B", "A"]);
  });
});
