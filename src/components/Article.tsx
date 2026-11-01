import { createMemo, type JSX } from "solid-js";
import type { Entry } from "../api";
import { displayTitle, hostOf, renderContent } from "../content";

function formatDate(d: Date): string {
  return d.toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Feed · author · date · reading time, leaving out what the entry doesn't have. */
function metaLine(e: Entry): string {
  const parts = [e.feed.title, e.author, formatDate(new Date(e.published_at))];
  if (e.reading_time > 0) {
    parts.push(`${e.reading_time} min`);
  }
  return parts.filter((part) => part !== "").join(" · ");
}

export function Article(props: { entry: Entry }): JSX.Element {
  // Sanitized article DOM; rebuilt only when the content changes (e.g. after "fetch full article").
  const content = createMemo(() => Array.from(renderContent(props.entry.content, props.entry.url).childNodes));

  return (
    <article>
      <div class="meta">
        <span>{metaLine(props.entry)}</span>
        <span class="status" data-status={props.entry.status}>
          {props.entry.status}
        </span>
      </div>
      <h1>
        <a href={props.entry.url} class="title-link">
          {displayTitle(props.entry.title)}
        </a>
      </h1>
      <div class="source">{hostOf(props.entry.url)}</div>
      <div class="content">{content()}</div>
    </article>
  );
}
