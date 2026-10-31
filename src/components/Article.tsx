import { createMemo } from "solid-js";
import type { Entry } from "../api";
import { hostOf, renderContent } from "../content";

function formatDate(d: Date): string {
  return d.toLocaleString(undefined, { day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" });
}

export function Article(props: { entry: Entry }) {
  const meta = () => {
    const e = props.entry;
    return [e.feed.title, e.author, formatDate(new Date(e.published_at)),
      e.reading_time ? `${e.reading_time} min` : ""].filter(Boolean).join(" · ");
  };
  // Sanitized article DOM; rebuilt only when the content changes (e.g. after "fetch full article").
  const content = createMemo(() => Array.from(renderContent(props.entry.content, props.entry.url).childNodes));

  return (
    <article>
      <div class="meta">
        <span>{meta()}</span>
        <span class={`status ${props.entry.status}`}>{props.entry.status === "read" ? "read" : "unread"}</span>
      </div>
      <h1><a href={props.entry.url} class="title-link">{props.entry.title || "(untitled)"}</a></h1>
      <div class="source">{hostOf(props.entry.url)}</div>
      <div class="content">{content()}</div>
    </article>
  );
}
