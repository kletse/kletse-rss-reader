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
    const p = doc.createElement("p");
    p.className = "embed-link";
    const a = doc.createElement("a");
    a.href = absolute(src, baseUrl);
    a.textContent = `▶ Embedded media · ${hostOf(a.href)}`;
    p.append(a);
    el.replaceWith(p);
  }

  const clean = DOMPurify.sanitize(doc.body.innerHTML, {
    RETURN_DOM_FRAGMENT: true,
    FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select"],
    FORBID_ATTR: ["style", "width", "height", "class", "id"],
  });

  for (const img of Array.from(clean.querySelectorAll("img"))) {
    img.loading = "lazy";
    img.src = absolute(img.getAttribute("src") ?? "", baseUrl);
    img.removeAttribute("srcset");
  }
  for (const a of Array.from(clean.querySelectorAll("a[href]"))) {
    a.setAttribute("href", absolute(a.getAttribute("href")!, baseUrl));
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
