// Turns feed HTML into something safe and pleasant to read.
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";

const EMBEDS = "iframe, embed, object, video, audio";

/** Sanitizes article HTML: embeds become links, scripts/styles go, code gets highlighted. */
export function renderContent(html: string, baseUrl: string): DocumentFragment {
  const doc = new DOMParser().parseFromString(html, "text/html");

  for (const el of Array.from(doc.querySelectorAll(EMBEDS))) {
    const src = el.getAttribute("src") ?? el.getAttribute("data") ?? el.querySelector("source")?.getAttribute("src");
    if (!src) { el.remove(); continue; }
    const a = doc.createElement("a");
    a.href = absolute(src, baseUrl);
    a.textContent = `▶ Embedded media · ${hostOf(a.href)}`;
    // Classes are stripped by the sanitizer below; the data attribute survives and is turned into one after.
    a.dataset.embed = "";
    el.replaceWith(a);
  }

  const clean = DOMPurify.sanitize(doc.body.innerHTML, {
    RETURN_DOM_FRAGMENT: true,
    FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "source"],
    FORBID_ATTR: ["style", "width", "height", "class", "id", "srcset", "sizes"],
  });

  for (const img of Array.from(clean.querySelectorAll("img"))) {
    const src = img.getAttribute("src");
    // Without a source the browser would try to load the base URL itself as an image.
    if (!src) { img.remove(); continue; }
    img.setAttribute("loading", "lazy");
    img.src = absolute(src, baseUrl);
  }
  for (const a of Array.from(clean.querySelectorAll("a[href]"))) {
    a.setAttribute("href", absolute(a.getAttribute("href")!, baseUrl));
  }
  for (const a of Array.from(clean.querySelectorAll("a[data-embed]"))) {
    a.removeAttribute("data-embed");
    const p = clean.ownerDocument.createElement("p");
    p.className = "embed-link";
    a.replaceWith(p);
    p.append(a);
  }
  for (const code of Array.from(clean.querySelectorAll("pre code, pre"))) {
    if (code.tagName === "PRE" && code.querySelector("code")) continue;
    hljs.highlightElement(code as HTMLElement);
  }
  return clean;
}

function absolute(url: string, base: string): string {
  try { return new URL(url, base).href; } catch { return url; }
}

export function hostOf(url: string): string {
  try { return new URL(url).hostname.replace(/^www\./, ""); } catch { return url; }
}
