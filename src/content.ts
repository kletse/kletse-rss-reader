// Turns feed HTML into something safe and pleasant to read.
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";

const EMBEDS = "iframe, embed, object, video, audio";
const LEADING_WWW = /^www\./u;

/** Embeds become plain links to their source; embeds without one are dropped. */
function replaceEmbeds(doc: Document, baseUrl: string): void {
  const embeds = Array.from(doc.querySelectorAll(EMBEDS));
  for (const el of embeds) {
    const src = el.getAttribute("src") ?? el.getAttribute("data") ?? el.querySelector("source")?.getAttribute("src");
    if (src === undefined || src === null || src === "") {
      el.remove();
    } else {
      const a = doc.createElement("a");
      a.href = absolute(src, baseUrl);
      a.textContent = `▶ Embedded media · ${hostOf(a.href)}`;
      // The sanitizer keeps data attributes; wrapEmbeds() uses this one to find the link again.
      a.setAttribute("data-embed", "");
      el.replaceWith(a);
    }
  }
}

/** Images load lazily from absolute URLs; one without a source would load the base URL itself. */
function fixImages(root: DocumentFragment, baseUrl: string): void {
  const images = Array.from(root.querySelectorAll("img"));
  for (const img of images) {
    const src = img.getAttribute("src");
    if (src === null || src === "") {
      img.remove();
    } else {
      img.setAttribute("loading", "lazy");
      img.src = absolute(src, baseUrl);
    }
  }
}

function fixLinks(root: DocumentFragment, baseUrl: string): void {
  const links = Array.from(root.querySelectorAll("a[href]"));
  for (const a of links) {
    const href = a.getAttribute("href");
    if (href !== null) {
      a.setAttribute("href", absolute(href, baseUrl));
    }
  }
}

/** Each embed link gets its own paragraph, marked so styles/article.css can set it apart. */
function wrapEmbeds(root: DocumentFragment): void {
  const links = Array.from(root.querySelectorAll("a[data-embed]"));
  for (const a of links) {
    a.removeAttribute("data-embed");
    const p = root.ownerDocument.createElement("p");
    p.setAttribute("data-embed", "");
    a.replaceWith(p);
    p.append(a);
  }
}

/** Highlights `<pre><code>`, and bare `<pre>` blocks; inline code is left alone. */
function highlight(root: DocumentFragment): void {
  const blocks = Array.from(root.querySelectorAll("pre code, pre"));
  for (const block of blocks) {
    const wrapsCode = block.tagName === "PRE" && block.querySelector("code") !== null;
    if (!wrapsCode && block instanceof HTMLElement) {
      hljs.highlightElement(block);
    }
  }
}

/** Sanitizes article HTML: embeds become links, scripts/styles go, code gets highlighted. */
export function renderContent(html: string, baseUrl: string): DocumentFragment {
  const doc = new DOMParser().parseFromString(html, "text/html");
  replaceEmbeds(doc, baseUrl);
  const clean = DOMPurify.sanitize(doc.body.innerHTML, {
    RETURN_DOM_FRAGMENT: true,
    FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "source"],
    FORBID_ATTR: ["style", "width", "height", "class", "id", "srcset", "sizes"],
  });
  fixImages(clean, baseUrl);
  fixLinks(clean, baseUrl);
  wrapEmbeds(clean);
  highlight(clean);
  return clean;
}

function absolute(url: string, base: string): string {
  try {
    return new URL(url, base).href;
  } catch {
    return url;
  }
}

export function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(LEADING_WWW, "");
  } catch {
    return url;
  }
}
