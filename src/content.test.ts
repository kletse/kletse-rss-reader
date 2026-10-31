import { describe, expect, it } from "vitest";
import { hostOf, renderContent } from "./content";

const BASE = "https://blog.example.com/posts/hello";

function render(html: string): HTMLElement {
  const div = document.createElement("div");
  div.append(renderContent(html, BASE));
  return div;
}

describe("renderContent", () => {
  it("keeps ordinary article markup", () => {
    const out = render("<h2>Title</h2><p>Hi <em>there</em></p><ul><li>one</li></ul><blockquote>q</blockquote>");
    expect(out.querySelector("h2")?.textContent).toBe("Title");
    expect(out.querySelector("em")?.textContent).toBe("there");
    expect(out.querySelectorAll("li")).toHaveLength(1);
    expect(out.querySelector("blockquote")?.textContent).toBe("q");
  });

  it("drops scripts, event handlers and javascript: links", () => {
    const out = render(
      `<p onclick="alert(1)">x</p><script>alert(2)</script><img src="a.png" onerror="alert(3)">` +
      `<a href="javascript:alert(4)">j</a><svg onload="alert(5)"></svg>`,
    );
    expect(out.innerHTML).not.toMatch(/alert|script|onerror|onclick|onload/);
    expect(out.querySelector("a")?.hasAttribute("href")).toBe(false);
  });

  it("drops styles, forms, inline style and layout attributes", () => {
    const out = render(
      `<style>p{display:none}</style><form action="/x"><input name="a"><button>go</button></form>` +
      `<p style="color:red" class="c" id="i" width="1" height="2">t</p>`,
    );
    expect(out.querySelector("style, form, input, button")).toBeNull();
    const p = out.querySelector("p")!;
    expect(p.attributes).toHaveLength(0);
  });

  it("turns iframes and videos into a link to the source", () => {
    const out = render(
      `<iframe src="https://www.youtube.com/embed/abc"></iframe>` +
      `<video><source src="/media/clip.mp4" type="video/mp4"></video>` +
      `<embed src="https://cdn.example.net/x.swf"><object data="https://cdn.example.net/y"></object>`,
    );
    expect(out.querySelector("iframe, video, embed, object, source")).toBeNull();
    const links = Array.from(out.querySelectorAll("p.embed-link a")).map((a) => a.getAttribute("href"));
    expect(links).toEqual([
      "https://www.youtube.com/embed/abc",
      "https://blog.example.com/media/clip.mp4",
      "https://cdn.example.net/x.swf",
      "https://cdn.example.net/y",
    ]);
    expect(out.querySelector("a")?.textContent).toBe("▶ Embedded media · youtube.com");
  });

  it("removes embeds without a source", () => {
    const out = render(`<p>a</p><iframe></iframe><video></video><p>b</p>`);
    expect(out.querySelectorAll("p")).toHaveLength(2);
    expect(out.querySelector("iframe, video, a")).toBeNull();
  });

  it("resolves relative image and link URLs against the article", () => {
    const out = render(`<img src="../img/pic.jpg"><a href="other">o</a><a href="//cdn.example.net/z">z</a>`);
    expect(out.querySelector("img")?.getAttribute("src")).toBe("https://blog.example.com/img/pic.jpg");
    const hrefs = Array.from(out.querySelectorAll("a")).map((a) => a.getAttribute("href"));
    expect(hrefs).toEqual(["https://blog.example.com/posts/other", "https://cdn.example.net/z"]);
  });

  it("lazy-loads images and strips srcset, sizes and picture sources", () => {
    const out = render(
      `<picture><source srcset="small.webp 480w" type="image/webp"><img src="full.jpg" srcset="a.jpg 1x, b.jpg 2x" sizes="100vw"></picture>`,
    );
    expect(out.querySelector("source")).toBeNull();
    const img = out.querySelector("img")!;
    expect(img.getAttribute("loading")).toBe("lazy");
    expect(img.getAttribute("src")).toBe("https://blog.example.com/posts/full.jpg");
    expect(img.hasAttribute("srcset")).toBe(false);
    expect(img.hasAttribute("sizes")).toBe(false);
  });

  it("drops images without a source instead of loading the article URL", () => {
    const out = render(`<p>t</p><img alt="tracking pixel"><img src="">`);
    expect(out.querySelector("img")).toBeNull();
  });

  it("keeps data: images (feeds inline small graphics)", () => {
    const out = render(`<img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=">`);
    expect(out.querySelector("img")?.getAttribute("src")).toMatch(/^data:image\/gif/);
  });

  it("highlights code blocks and leaves inline code alone", () => {
    const out = render(
      `<pre><code class="language-rust">fn main() { let x = 1; }</code></pre><p>use <code>cargo</code></p>`,
    );
    expect(out.querySelectorAll("pre code .hljs-keyword").length).toBeGreaterThan(0);
    expect(out.querySelector("pre code")?.textContent).toBe("fn main() { let x = 1; }");
    expect(out.querySelector("p code")?.innerHTML).toBe("cargo");
  });

  it("escapes HTML inside highlighted code", () => {
    const out = render(`<pre><code>&lt;script&gt;alert(1)&lt;/script&gt;</code></pre>`);
    expect(out.querySelector("script")).toBeNull();
    expect(out.querySelector("pre code")?.textContent).toBe("<script>alert(1)</script>");
  });

  it("survives empty and broken input", () => {
    expect(render("").childNodes).toHaveLength(0);
    expect(render("<p>unclosed <b>bold").textContent).toBe("unclosed bold");
    const div = document.createElement("div");
    div.append(renderContent(`<a href="x">rel</a>`, "not a url"));
    expect(div.querySelector("a")?.getAttribute("href")).toBe("x");
  });
});

describe("hostOf", () => {
  it("returns the hostname without www", () => {
    expect(hostOf("https://www.example.com/a/b?c")).toBe("example.com");
    expect(hostOf("http://sub.example.org:8080/")).toBe("sub.example.org");
  });

  it("falls back to the input when it is not a URL", () => {
    expect(hostOf("nope")).toBe("nope");
    expect(hostOf("")).toBe("");
  });
});
