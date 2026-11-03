//! Feed HTML to a list of blocks QML can lay out natively: what `src/content.ts` + DOMPurify + the article CSS
//! do in the webview, done by hand. Text inside a block stays a small HTML subset (links, bold, italics,
//! inline code), which Qt's rich-text engine renders; everything with its own box (images, code, quotes, lists,
//! tables, embeds) becomes its own block.
//!
//! Nothing here runs scripts or loads anything; only the whitelisted tags and attributes reach the output.

use super::highlight;
use crate::theme::Palette;
use base64::Engine as _;
use ego_tree::NodeRef;
use scraper::{Html, Node, node::Element};
use serde::Serialize;
use std::collections::BTreeMap;
use url::Url;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    Paragraph { html: String },
    Heading { level: u8, html: String },
    Image { src: String, alt: String, href: Option<String>, caption: Option<String> },
    Caption { html: String },
    Code { html: String, lang: Option<String> },
    Quote { blocks: Vec<Block> },
    List { ordered: bool, start: i64, items: Vec<Vec<Block>> },
    Table { html: String },
    Rule,
    Embed { href: String, label: String },
}

/// What the conversion couldn't carry over, to judge how far a native renderer gets on real feeds.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Stats {
    /// Elements the webview would show and this doesn't (inline SVG, MathML, canvas).
    pub dropped: BTreeMap<String, usize>,
    /// Tags with no rule here; their content is kept as if they weren't there.
    pub unknown: BTreeMap<String, usize>,
    /// Code blocks with a grammar / without one (shown plain).
    pub code_highlighted: usize,
    pub code_plain: usize,
    /// Language hints syntect has no grammar for.
    pub code_no_grammar: BTreeMap<String, usize>,
    pub images: usize,
    pub tables: usize,
    pub embeds: usize,
}

impl Stats {
    /// One line for `KLETSE_RSS_DEBUG=1`; empty when everything carried over.
    pub fn summary(&self) -> String {
        let list =
            |m: &BTreeMap<String, usize>| m.iter().map(|(k, v)| format!("{k}×{v}")).collect::<Vec<_>>().join(" ");
        let mut parts = Vec::new();
        if !self.dropped.is_empty() {
            parts.push(format!("dropped: {}", list(&self.dropped)));
        }
        if !self.unknown.is_empty() {
            parts.push(format!("unknown: {}", list(&self.unknown)));
        }
        if !self.code_no_grammar.is_empty() {
            parts.push(format!("no grammar: {}", list(&self.code_no_grammar)));
        }
        parts.join(" · ")
    }
}

#[derive(Debug)]
pub struct Rendered {
    pub blocks: Vec<Block>,
    pub stats: Stats,
}

/// Elements removed with everything inside them (DOMPurify's FORBID_CONTENTS plus the app's FORBID_TAGS).
const REMOVED: &[&str] = &[
    "script", "style", "noscript", "template", "select", "textarea", "title", "head", "meta", "link", "input",
    "source", "track", "param", "base",
];
/// Things the webview draws and this renderer can't.
const UNSUPPORTED: &[&str] = &["svg", "math", "canvas"];
/// Embeds become a link to their source, as `replaceEmbeds` in content.ts does.
const EMBEDS: &[&str] = &["iframe", "embed", "object", "video", "audio"];
/// Block containers with no look of their own: they only separate paragraphs.
const CONTAINERS: &[&str] = &[
    "p", "div", "section", "article", "main", "header", "footer", "aside", "nav", "center", "details", "summary",
    "address", "dl", "dd", "hgroup", "body", "html", "form", "fieldset", "legend", "li", "caption", "search",
];
/// Inline wrappers with no look of their own.
const TRANSPARENT: &[&str] = &[
    "span", "font", "time", "abbr", "acronym", "label", "bdi", "bdo", "data", "picture", "button", "big", "nobr",
    "output",
];

/// How deep into nested elements the converter follows before it only keeps the text. html5ever closes nothing
/// by itself, so a feed with thousands of unclosed tags would otherwise take the recursion, and the stack, as deep
/// as it likes; the article is converted ahead on a thread with 2 MB of it. Browsers stop at 512.
const MAX_DEPTH: usize = 256;
/// The furthest an ordered list can start from 1, either way.
const MAX_LIST_START: i64 = 1_000_000_000;
/// The most columns or rows a table cell can span; browsers stop there too.
pub const MAX_SPAN: u32 = 1000;
/// Where a link may point, an image may come from, and an embed may be from. Anything else (javascript:, file:,
/// blob:…) is left out; the backend refuses to open anything but these as well.
const LINK_SCHEMES: &[&str] = &["http", "https", "mailto"];
const IMAGE_SCHEMES: &[&str] = &["http", "https", "data"];
const EMBED_SCHEMES: &[&str] = &["http", "https"];

pub fn convert(html: &str, base_url: &str, palette: &Palette) -> Rendered {
    convert_with(html, base_url, palette, false)
}

/// An image in an article, as the image viewer steps through them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageRef {
    pub src: String,
    pub alt: String,
    /// The link the image is in, if any.
    pub href: Option<String>,
}

/// An article's images in reading order, as `convert` lays them out, without the time highlighting code takes.
pub fn images(html: &str, base_url: &str) -> Vec<ImageRef> {
    image_refs(&convert_with(html, base_url, &Palette::default(), true).blocks)
}

/// The images among `blocks`, those in quotes and lists included.
pub fn image_refs(blocks: &[Block]) -> Vec<ImageRef> {
    let mut found = Vec::new();
    collect_images(blocks, &mut found);
    found
}

fn collect_images(blocks: &[Block], found: &mut Vec<ImageRef>) {
    for block in blocks {
        match block {
            Block::Image { src, alt, href, .. } => {
                found.push(ImageRef { src: src.clone(), alt: alt.clone(), href: href.clone() });
            }
            Block::Quote { blocks } => collect_images(blocks, found),
            Block::List { items, .. } => items.iter().for_each(|item| collect_images(item, found)),
            _ => {}
        }
    }
}

/// `plain`: code isn't highlighted.
fn convert_with(html: &str, base_url: &str, palette: &Palette, plain: bool) -> Rendered {
    let doc = Html::parse_fragment(html);
    let base = Url::parse(base_url).ok();
    let mut c = Converter {
        base,
        palette,
        plain,
        syntax_theme: None,
        stats: Stats::default(),
        link: None,
        depth: 0,
        in_cell: false,
    };
    let mut blocks = Vec::new();
    let mut para = Para::default();
    for child in doc.root_element().children() {
        c.walk(child, &mut blocks, &mut para);
    }
    c.flush(&mut blocks, &mut para);
    Rendered { blocks, stats: c.stats }
}

/// The paragraph being collected: inline HTML, and whether any of it is visible.
#[derive(Default)]
struct Para {
    html: String,
    visible: bool,
}

impl Para {
    fn text(&mut self, text: &str) {
        if text.chars().any(|c| !c.is_whitespace()) {
            self.visible = true;
        }
        self.html.push_str(&highlight::escape(text));
    }

    fn take(&mut self) -> Option<String> {
        let para = std::mem::take(self);
        para.visible.then(|| para.html.trim().to_string())
    }
}

struct Converter<'a> {
    base: Option<Url>,
    palette: &'a Palette,
    /// Leave code plain: only the images are wanted.
    plain: bool,
    syntax_theme: Option<syntect::highlighting::Theme>,
    stats: Stats,
    /// The link we are inside of, so an image in a link stays clickable.
    link: Option<String>,
    /// How many elements deep `walk_children` is.
    depth: usize,
    /// Inside a table cell, where Qt can't nest a table: one in there becomes lines of text.
    in_cell: bool,
}

impl Converter<'_> {
    fn flush(&mut self, out: &mut Vec<Block>, para: &mut Para) {
        if let Some(html) = para.take() {
            out.push(Block::Paragraph { html });
        }
    }

    fn absolute(&self, url: &str) -> String {
        let url = url.trim();
        self.base.as_ref().and_then(|b| b.join(url).ok()).map_or_else(|| url.to_string(), String::from)
    }

    /// `url` made absolute, if it is one of the `schemes`. (The url crate lowercases the scheme and strips the
    /// tabs and newlines browsers ignore, so "JavaScript:" and "java\nscript:" are javascript: here too.)
    fn allowed(&self, url: &str, schemes: &[&str]) -> Option<String> {
        if url.trim().is_empty() {
            return None;
        }
        let url = self.absolute(url);
        schemes.contains(&Url::parse(&url).ok()?.scheme()).then_some(url)
    }

    fn walk_children(&mut self, node: NodeRef<'_, Node>, out: &mut Vec<Block>, para: &mut Para) {
        if self.depth >= MAX_DEPTH {
            // Too deep to follow: the text underneath, as it comes, and nothing else.
            *self.stats.dropped.entry(format!("nesting past {MAX_DEPTH}")).or_default() += 1;
            for below in node.descendants().skip(1) {
                if let Node::Text(text) = below.value() {
                    para.text(text);
                }
            }
            return;
        }
        self.depth += 1;
        for child in node.children() {
            self.walk(child, out, para);
        }
        self.depth -= 1;
    }

    /// The children of `node` as blocks of their own (a quote, a list item).
    fn blocks_of(&mut self, node: NodeRef<'_, Node>) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut para = Para::default();
        self.walk_children(node, &mut blocks, &mut para);
        self.flush(&mut blocks, &mut para);
        blocks
    }

    fn walk(&mut self, node: NodeRef<'_, Node>, out: &mut Vec<Block>, para: &mut Para) {
        match node.value() {
            Node::Text(text) => para.text(text),
            Node::Element(el) => self.element(node, el, out, para),
            _ => {}
        }
    }

    fn element(&mut self, node: NodeRef<'_, Node>, el: &Element, out: &mut Vec<Block>, para: &mut Para) {
        let name = el.name();
        match name {
            _ if REMOVED.contains(&name) => {}
            _ if UNSUPPORTED.contains(&name) => *self.stats.dropped.entry(name.into()).or_default() += 1,
            _ if EMBEDS.contains(&name) => self.embed(node, el, out, para),
            "img" => self.image(el, out, para),
            "br" => para.html.push_str("<br>"),
            "hr" => {
                self.flush(out, para);
                out.push(Block::Rule);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush(out, para);
                let mut heading = Para::default();
                self.walk_children(node, out, &mut heading);
                if let Some(html) = heading.take() {
                    let level = name[1..].parse().unwrap_or(2);
                    out.push(Block::Heading { level, html });
                }
            }
            "blockquote" => {
                self.flush(out, para);
                let blocks = self.blocks_of(node);
                if !blocks.is_empty() {
                    out.push(Block::Quote { blocks });
                }
            }
            "ul" | "ol" | "menu" => {
                self.flush(out, para);
                self.list(node, el, out);
            }
            "pre" => {
                self.flush(out, para);
                self.code(node, el, out);
            }
            "table" if self.in_cell => self.table_inline(node, out, para),
            "table" => {
                self.flush(out, para);
                self.table(node, out);
            }
            "figure" => {
                self.flush(out, para);
                self.figure(node, out);
            }
            "figcaption" => {
                // Outside a figure; inside one, figure() handles it.
                self.flush(out, para);
                let mut caption = Para::default();
                self.walk_children(node, out, &mut caption);
                if let Some(html) = caption.take() {
                    out.push(Block::Caption { html });
                }
            }
            "dt" => {
                self.flush(out, para);
                para.html.push_str("<b>");
                self.walk_children(node, out, para);
                para.html.push_str("</b>");
                self.flush(out, para);
            }
            _ if CONTAINERS.contains(&name) => {
                self.flush(out, para);
                self.walk_children(node, out, para);
                self.flush(out, para);
            }
            "a" => match el.attr("href").and_then(|h| self.allowed(h, LINK_SCHEMES)) {
                Some(href) => {
                    // Qt's rich text colours links from the palette unless the anchor says otherwise.
                    let accent = &self.palette.accent;
                    para.html.push_str(&format!("<a href=\"{}\" style=\"color:{accent}\">", highlight::escape(&href)));
                    let outer = self.link.replace(href);
                    self.walk_children(node, out, para);
                    self.link = outer;
                    para.html.push_str("</a>");
                }
                None => self.walk_children(node, out, para),
            },
            "strong" | "b" => self.wrap(node, out, para, "<b>", "</b>"),
            "em" | "i" | "cite" | "dfn" | "var" => self.wrap(node, out, para, "<i>", "</i>"),
            "u" | "ins" => self.wrap(node, out, para, "<u>", "</u>"),
            "s" | "del" | "strike" => self.wrap(node, out, para, "<s>", "</s>"),
            "sub" => self.wrap(node, out, para, "<sub>", "</sub>"),
            "sup" => self.wrap(node, out, para, "<sup>", "</sup>"),
            "small" => self.wrap(node, out, para, "<small>", "</small>"),
            "q" => self.wrap(node, out, para, "“", "”"),
            "mark" => {
                let open = format!("<span style=\"background-color:{}\">", self.palette.selection);
                self.wrap(node, out, para, &open, "</span>");
            }
            "code" | "kbd" | "samp" | "tt" => {
                // Qt's rich text has no padding or radius on inline runs; a tinted background is what's left.
                let open = format!(
                    "<code style=\"font-family:'{}'; font-size:85%; background-color:{}\">\u{a0}",
                    self.palette.ui_font, self.palette.bg_light
                );
                self.wrap(node, out, para, &open, "\u{a0}</code>");
            }
            "wbr" => {}
            _ if TRANSPARENT.contains(&name) => self.walk_children(node, out, para),
            _ => {
                *self.stats.unknown.entry(name.into()).or_default() += 1;
                self.walk_children(node, out, para);
            }
        }
    }

    fn wrap(&mut self, node: NodeRef<'_, Node>, out: &mut Vec<Block>, para: &mut Para, open: &str, close: &str) {
        para.html.push_str(open);
        self.walk_children(node, out, para);
        para.html.push_str(close);
    }

    fn image(&mut self, el: &Element, out: &mut Vec<Block>, para: &mut Para) {
        let src = el.attr("src").unwrap_or("").trim();
        if src.is_empty() || is_empty_svg_placeholder(src) {
            return;
        }
        let alt = el.attr("alt").unwrap_or("").trim().to_string();
        // WordPress swaps emoji for images of them; the character itself reads better inline.
        let is_emoji = el.classes().any(|c| c == "emoji" || c == "wp-smiley");
        if is_emoji && !alt.is_empty() {
            para.text(&alt);
            return;
        }
        let Some(src) = self.allowed(src, IMAGE_SCHEMES) else {
            *self.stats.dropped.entry("image from somewhere other than the web".into()).or_default() += 1;
            return;
        };
        self.flush(out, para);
        self.stats.images += 1;
        out.push(Block::Image { src, alt, href: self.link.clone(), caption: None });
    }

    fn embed(&mut self, node: NodeRef<'_, Node>, el: &Element, out: &mut Vec<Block>, para: &mut Para) {
        let source = || {
            node.children().find_map(|c| match c.value() {
                Node::Element(e) if e.name() == "source" => e.attr("src").map(str::to_string),
                _ => None,
            })
        };
        let src = el.attr("src").or_else(|| el.attr("data")).map(str::to_string).or_else(source);
        let Some(href) = src.and_then(|s| self.allowed(&s, EMBED_SCHEMES)) else { return };
        self.flush(out, para);
        self.stats.embeds += 1;
        let host = Url::parse(&href)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string()))
            .unwrap_or_else(|| href.clone());
        out.push(Block::Embed { label: format!("▶ Embedded media · {host}"), href });
    }

    fn list(&mut self, node: NodeRef<'_, Node>, el: &Element, out: &mut Vec<Block>) {
        let mut items = Vec::new();
        for child in node.children() {
            match child.value() {
                Node::Element(e) if e.name() == "li" => items.push(self.blocks_of(child)),
                // Stray text or elements between items: give them an item of their own rather than lose them.
                Node::Element(_) => {
                    let mut blocks = Vec::new();
                    let mut para = Para::default();
                    self.walk(child, &mut blocks, &mut para);
                    self.flush(&mut blocks, &mut para);
                    if !blocks.is_empty() {
                        items.push(blocks);
                    }
                }
                Node::Text(t) if !t.trim().is_empty() => {
                    items.push(vec![Block::Paragraph { html: highlight::escape(t.trim()) }]);
                }
                _ => {}
            }
        }
        if !items.is_empty() {
            let ordered = el.name() == "ol";
            // Clamped: the windows add the item's number to it, and Swift traps on an overflow.
            let start = el.attr("start").and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(1);
            let start = start.clamp(-MAX_LIST_START, MAX_LIST_START);
            out.push(Block::List { ordered, start, items });
        }
    }

    fn code(&mut self, node: NodeRef<'_, Node>, el: &Element, out: &mut Vec<Block>) {
        let mut text = String::new();
        collect_text(node, &mut text, 0);
        let text = text.strip_prefix('\n').unwrap_or(&text).trim_end().to_string();
        if text.is_empty() {
            return;
        }
        // Language hints: classes and data-lang on the <pre> and on a <code> inside it.
        let mut hints: Vec<String> = el.classes().map(str::to_string).collect();
        hints.extend(el.attr("data-lang").map(str::to_string));
        for d in node.descendants() {
            if let Node::Element(e) = d.value()
                && e.name() == "code"
            {
                hints.extend(e.classes().map(str::to_string));
                hints.extend(e.attr("data-lang").map(str::to_string));
            }
        }
        if self.plain {
            out.push(Block::Code { html: highlight::escape(&text), lang: None });
            return;
        }
        let hint_refs: Vec<&str> = hints.iter().map(String::as_str).collect();
        let syntax = highlight::find_syntax(&hint_refs, &text);
        let lang_hint = hint_refs
            .iter()
            .find_map(|h| h.strip_prefix("language-").or_else(|| h.strip_prefix("lang-")))
            .map(str::to_string);
        if syntax.is_some() {
            self.stats.code_highlighted += 1;
        } else {
            self.stats.code_plain += 1;
            if let Some(lang) = &lang_hint {
                *self.stats.code_no_grammar.entry(lang.clone()).or_default() += 1;
            }
        }
        let palette = self.palette;
        let theme = self.syntax_theme.get_or_insert_with(|| highlight::theme(palette));
        let html = highlight::to_html(&text, syntax, theme);
        out.push(Block::Code { html, lang: syntax.map(|s| s.name.clone()).or(lang_hint) });
    }

    /// A table as Qt rich-text table HTML; cell content is flattened to inline text (images become links).
    fn table(&mut self, node: NodeRef<'_, Node>, out: &mut Vec<Block>) {
        self.stats.tables += 1;
        let border = &self.palette.bg_light;
        let mut html = format!(
            "<table border=\"1\" cellspacing=\"0\" cellpadding=\"6\" style=\"border-collapse:collapse; border-color:{border}\">"
        );
        let mut rows = 0;
        for row in own_rows(node) {
            rows += 1;
            html.push_str("<tr>");
            for cell in cells(row) {
                let Node::Element(ce) = cell.value() else { continue };
                let span = |attr: &str| {
                    ce.attr(attr).and_then(|v| v.trim().parse::<u32>().ok()).filter(|&n| n > 1).map(|n| n.min(MAX_SPAN))
                };
                html.push('<');
                html.push_str(ce.name());
                if let Some(n) = span("colspan") {
                    html.push_str(&format!(" colspan=\"{n}\""));
                }
                if let Some(n) = span("rowspan") {
                    html.push_str(&format!(" rowspan=\"{n}\""));
                }
                html.push('>');
                let outer = std::mem::replace(&mut self.in_cell, true);
                html.push_str(&self.inline_flattened(cell));
                self.in_cell = outer;
                html.push_str("</");
                html.push_str(ce.name());
                html.push('>');
            }
            html.push_str("</tr>");
        }
        html.push_str("</table>");
        if rows > 0 {
            out.push(Block::Table { html });
        }
    }

    /// A table inside a table cell: each of its rows a line, the cells set apart with a dot. (Qt's rich text can
    /// nest tables, but the native layouts can't, and a table in a table is a layout, not data.)
    fn table_inline(&mut self, node: NodeRef<'_, Node>, out: &mut Vec<Block>, para: &mut Para) {
        self.flush(out, para);
        for row in own_rows(node) {
            let mut first = true;
            for cell in cells(row) {
                if !first {
                    para.html.push_str(" · ");
                }
                first = false;
                self.walk_children(cell, out, para);
            }
            self.flush(out, para);
        }
    }

    /// Everything under `node` as one run of inline HTML: blocks are joined with line breaks, images become
    /// links to themselves. For table cells, where Qt can't nest our blocks.
    fn inline_flattened(&mut self, node: NodeRef<'_, Node>) -> String {
        let blocks = self.blocks_of(node);
        let mut parts = Vec::new();
        for block in blocks {
            match block {
                Block::Paragraph { html } | Block::Heading { html, .. } | Block::Caption { html } => parts.push(html),
                Block::Image { src, alt, .. } => {
                    let label = if alt.is_empty() { "image".to_string() } else { alt };
                    parts.push(format!(
                        "<a href=\"{}\" style=\"color:{}\">[{}]</a>",
                        highlight::escape(&src),
                        self.palette.accent,
                        highlight::escape(&label)
                    ));
                }
                Block::Code { html, .. } => parts.push(format!("<code>{html}</code>")),
                Block::Embed { href, label } => parts.push(format!(
                    "<a href=\"{}\" style=\"color:{}\">{}</a>",
                    highlight::escape(&href),
                    self.palette.accent,
                    highlight::escape(&label)
                )),
                Block::Quote { .. } | Block::List { .. } | Block::Table { .. } | Block::Rule => {
                    *self.stats.dropped.entry("nested block in table cell".into()).or_default() += 1;
                }
            }
        }
        parts.join("<br>")
    }

    /// An image with its caption; a figure without an image keeps its parts as they come.
    fn figure(&mut self, node: NodeRef<'_, Node>, out: &mut Vec<Block>) {
        let start = out.len();
        let mut para = Para::default();
        let mut caption = None;
        for child in node.children() {
            match child.value() {
                Node::Element(e) if e.name() == "figcaption" => {
                    let mut c = Para::default();
                    self.walk_children(child, out, &mut c);
                    caption = c.take();
                }
                _ => self.walk(child, out, &mut para),
            }
        }
        self.flush(out, &mut para);
        let Some(caption) = caption else { return };
        match out[start..].iter_mut().rev().find(|b| matches!(b, Block::Image { .. })) {
            Some(Block::Image { caption: slot, .. }) => *slot = Some(caption),
            _ => out.push(Block::Caption { html: caption }),
        }
    }
}

/// The rows of `table` itself (at any depth, with or without a tbody), not those of a table in one of its cells.
fn own_rows<'a>(table: NodeRef<'a, Node>) -> impl Iterator<Item = NodeRef<'a, Node>> {
    let is = |node: &NodeRef<'_, Node>, name: &str| matches!(node.value(), Node::Element(e) if e.name() == name);
    table.descendants().filter(move |row| {
        is(row, "tr") && row.ancestors().find(|a| is(a, "table")).is_some_and(|t| t.id() == table.id())
    })
}

/// The cells of a row.
fn cells<'a>(row: NodeRef<'a, Node>) -> impl Iterator<Item = NodeRef<'a, Node>> {
    row.children().filter(|cell| matches!(cell.value(), Node::Element(e) if e.name() == "td" || e.name() == "th"))
}

/// The text of a `<pre>`, with `<br>` as a newline. Stops following elements at `MAX_DEPTH`, as `walk_children` does.
fn collect_text(node: NodeRef<'_, Node>, out: &mut String, depth: usize) {
    for child in node.children() {
        match child.value() {
            Node::Text(t) => out.push_str(t),
            Node::Element(e) if e.name() == "br" => out.push('\n'),
            Node::Element(e) if REMOVED.contains(&e.name()) => {}
            Node::Element(_) if depth < MAX_DEPTH => collect_text(child, out, depth + 1),
            Node::Element(_) => {
                for below in child.descendants().skip(1) {
                    if let Node::Text(t) = below.value() {
                        out.push_str(t);
                    }
                }
            }
            _ => {}
        }
    }
}

/// True for an inline SVG that draws nothing: the stand-in lazy-loading scripts swap for the real image.
/// Same rule as `isEmptySvgPlaceholder` in src/content.ts.
fn is_empty_svg_placeholder(src: &str) -> bool {
    let Some((header, body)) = src.split_once(',') else { return false };
    let header = header.to_ascii_lowercase();
    if !header.starts_with("data:image/svg+xml") {
        return false;
    }
    let svg = if header.ends_with(";base64") {
        base64::engine::general_purpose::STANDARD.decode(body.trim()).ok().and_then(|b| String::from_utf8(b).ok())
    } else {
        percent_encoding::percent_decode_str(body).decode_utf8().ok().map(|s| s.into_owned())
    };
    let Some(svg) = svg else { return false };
    let svg = svg.trim().to_ascii_lowercase();
    if !svg.starts_with("<svg") || svg.matches('<').count() > 2 {
        return false;
    }
    if svg.matches('<').count() == 1 {
        return svg.ends_with("/>");
    }
    svg.strip_suffix("</svg>").and_then(|s| s.split_once('>')).is_some_and(|(_, between)| between.trim().is_empty())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn blocks(html: &str) -> Vec<Block> {
        convert(html, "https://example.com/post/", &Palette::default()).blocks
    }

    fn para(html: &str) -> Block {
        Block::Paragraph { html: html.into() }
    }

    #[test]
    fn paragraphs_and_loose_text_become_paragraphs() {
        assert_eq!(
            blocks("<p>One</p>\n  <p>Two <b>bold</b></p>tail"),
            vec![para("One"), para("Two <b>bold</b>"), para("tail")]
        );
    }

    /// Runs on the test's own 2 MB stack, the size of the thread the next article is converted on.
    #[test]
    fn nesting_without_end_keeps_the_text_and_doesnt_take_the_stack_with_it() {
        let depth = 30_000;
        // (Not li: html5ever makes those siblings.)
        for tag in ["b", "blockquote", "span", "div"] {
            let html = format!("{}deep inside{}", format!("<{tag}>").repeat(depth), format!("</{tag}>").repeat(depth));
            let rendered = convert(&html, "https://example.com/", &Palette::default());
            let mut blocks = &rendered.blocks[..];
            while let [Block::Quote { blocks: inner }] = blocks {
                blocks = inner;
            }
            assert!(matches!(blocks, [Block::Paragraph { html }] if html.contains("deep inside")), "{tag}: {blocks:?}");
            assert!(rendered.stats.dropped.contains_key("nesting past 256"), "{tag}");
        }
        let pre = format!("<pre>{}code{}</pre>", "<span>".repeat(depth), "</span>".repeat(depth));
        assert_eq!(blocks(&pre), vec![Block::Code { html: "code".into(), lang: None }]);
        // Nothing of the sort happens to an ordinary article.
        let quote = blocks("<blockquote><p>One</p><blockquote><p>Two</p></blockquote></blockquote>");
        assert_eq!(quote, vec![Block::Quote { blocks: vec![para("One"), Block::Quote { blocks: vec![para("Two")] }] }]);
    }

    #[test]
    fn a_lists_start_is_taken_as_written_within_reason() {
        let start = |attr: &str| match &blocks(&format!("<ol {attr}><li>a</li><li>b</li></ol>"))[..] {
            [Block::List { ordered: true, start, .. }] => *start,
            other => panic!("{other:?}"),
        };
        assert_eq!(start("start='7'"), 7);
        assert_eq!(start("start=' -5 '"), -5);
        assert_eq!(start(""), 1);
        assert_eq!(start("start='soon'"), 1);
        assert_eq!(start("start='9223372036854775807'"), 1_000_000_000);
        assert_eq!(start("start='-9223372036854775808'"), -1_000_000_000);
        assert_eq!(start("start='99999999999999999999'"), 1); // not even an i64
    }

    #[test]
    fn a_table_in_a_table_cell_becomes_lines_in_that_cell() {
        let nested = "<table><tbody><tr><td>outer<table><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table></td>\
                      </tr><tr><td>second</td></tr></tbody></table>";
        let r = convert(nested, "https://example.com/", &Palette::default());
        let [Block::Table { html }] = &r.blocks[..] else { panic!("{:?}", r.blocks) };
        assert_eq!(html.matches("<tr>").count(), 2, "{html}");
        assert!(html.contains("<td>outer<br>a · b<br>c</td>"), "{html}");
        assert!(html.contains("<td>second</td>"), "{html}");
        assert_eq!(r.stats.tables, 1);
        assert!(r.stats.dropped.is_empty(), "{:?}", r.stats.dropped);
    }

    #[test]
    fn cell_spans_are_kept_up_to_a_thousand() {
        let b = blocks(
            "<table><tr><td colspan='2' rowspan='4000000000'>a</td><td colspan='x' rowspan='1'>b</td></tr></table>",
        );
        let [Block::Table { html }] = &b[..] else { panic!("{b:?}") };
        assert!(html.contains("<td colspan=\"2\" rowspan=\"1000\">a</td><td>b</td>"), "{html}");
    }

    #[test]
    fn an_embeds_label_is_text_wherever_it_ends_up() {
        // An address that isn't one is no embed at all, so no markup in it can reach the label.
        let hostile = "http://[<b style=\"color:red\">boo";
        assert_eq!(blocks(&format!("<iframe src='{hostile}'></iframe>")), vec![]);
        let b = blocks("<table><tr><td><iframe src='https://www.youtube.com/embed/x'></iframe></td></tr></table>");
        let [Block::Table { html }] = &b[..] else { panic!("{b:?}") };
        assert!(html.contains("<td><a href=\"https://www.youtube.com/embed/x\""), "{html}");
        assert!(html.contains(">▶ Embedded media · youtube.com</a></td>"), "{html}");
    }

    #[test]
    fn images_split_paragraphs_and_resolve_against_the_article() {
        let b = blocks("<p>Before <a href='/full.png'><img src='thumb.png' alt='A cat'></a> after</p>");
        assert_eq!(b.len(), 3);
        // The paragraph is cut inside the link; Qt's parser closes the dangling tag.
        assert!(matches!(&b[0], Block::Paragraph { html } if html.starts_with("Before")));
        assert_eq!(
            b[1],
            Block::Image {
                src: "https://example.com/post/thumb.png".into(),
                alt: "A cat".into(),
                href: Some("https://example.com/full.png".into()),
                caption: None
            }
        );
    }

    #[test]
    fn figure_captions_attach_to_their_image() {
        let b = blocks("<figure><img src='a.jpg'><figcaption>The <i>caption</i></figcaption></figure>");
        assert!(matches!(&b[..], [Block::Image { caption: Some(c), .. }] if c == "The <i>caption</i>"));
    }

    #[test]
    fn scripts_go_and_svg_counts_as_dropped() {
        let r = convert("<script>alert(1)</script><svg><circle/></svg><p>ok</p>", "", &Palette::default());
        assert_eq!(r.blocks, vec![para("ok")]);
        assert_eq!(r.stats.dropped.get("svg"), Some(&1));
    }

    #[test]
    fn embeds_become_links() {
        let b = blocks("<iframe src='https://www.youtube.com/embed/x'></iframe><video><source src='/v.mp4'></video>");
        assert_eq!(
            b,
            vec![
                Block::Embed {
                    href: "https://www.youtube.com/embed/x".into(),
                    label: "▶ Embedded media · youtube.com".into()
                },
                Block::Embed {
                    href: "https://example.com/v.mp4".into(),
                    label: "▶ Embedded media · example.com".into()
                },
            ]
        );
    }

    #[test]
    fn code_blocks_keep_their_text_and_get_highlighted() {
        let r = convert("<pre><code class='language-rust'>fn main() {}\n</code></pre>", "", &Palette::default());
        let [Block::Code { html, lang }] = &r.blocks[..] else { panic!("{:?}", r.blocks) };
        assert_eq!(lang.as_deref(), Some("Rust"));
        assert!(html.contains("<span style=\"color:"));
        assert_eq!(r.stats.code_highlighted, 1);
    }

    #[test]
    fn nested_lists_and_quotes_nest() {
        let b = blocks("<blockquote><p>q</p><ul><li>a<ol start='3'><li>b</li></ol></li></ul></blockquote>");
        let [Block::Quote { blocks: inner }] = &b[..] else { panic!("{b:?}") };
        assert_eq!(inner[0], para("q"));
        let Block::List { ordered: false, items, .. } = &inner[1] else { panic!("{inner:?}") };
        assert!(matches!(&items[0][1], Block::List { ordered: true, start: 3, .. }));
    }

    #[test]
    fn text_is_escaped() {
        assert_eq!(blocks("<p>a &lt;b&gt; &amp; c</p>"), vec![para("a &lt;b&gt; &amp; c")]);
    }

    #[test]
    fn javascript_links_lose_their_href() {
        assert_eq!(blocks("<p><a href='javascript:alert(1)'>x</a></p>"), vec![para("x")]);
        assert_eq!(blocks("<p><a href='JavaScript:alert(1)'>x</a></p>"), vec![para("x")]);
        assert_eq!(blocks("<p><a href='java\nscript:alert(1)'>x</a></p>"), vec![para("x")]);
        assert_eq!(blocks("<p><a href='file:///etc/passwd'>x</a></p>"), vec![para("x")]);
        assert_eq!(blocks("<p><a href=''>x</a></p>"), vec![para("x")]);
        let mail = blocks("<p><a href='mailto:a@example.com'>x</a></p>");
        assert!(
            matches!(&mail[..], [Block::Paragraph { html }] if html.starts_with("<a href=\"mailto:a@example.com\""))
        );
    }

    #[test]
    fn images_and_embeds_come_from_the_web_or_not_at_all() {
        let r = convert("<img src='file:///etc/passwd'><p>x</p>", "https://example.com/", &Palette::default());
        assert_eq!(r.blocks, vec![para("x")]);
        assert_eq!(r.stats.dropped.get("image from somewhere other than the web"), Some(&1));
        assert!(matches!(&blocks("<img src='data:image/png;base64,AAAA'>")[..], [Block::Image { .. }]));
        assert!(
            matches!(&blocks("<img src='//cdn.example.com/a.png'>")[..], [Block::Image { src, .. }] if src == "https://cdn.example.com/a.png")
        );
        assert_eq!(blocks("<iframe src='file:///etc/passwd'></iframe><iframe src='javascript:1'></iframe>"), vec![]);
        assert!(matches!(
            &blocks("<iframe src='https://www.youtube.com/embed/x'></iframe>")[..],
            [Block::Embed { .. }]
        ));
    }

    #[test]
    fn empty_svg_placeholders_are_dropped() {
        assert!(is_empty_svg_placeholder("data:image/svg+xml,%3Csvg%20viewBox='0%200%201750%201167'%3E%3C/svg%3E"));
        assert!(is_empty_svg_placeholder("data:image/svg+xml;base64,PHN2ZyB2aWV3Qm94PScwIDAgMSAxJy8+"));
        assert!(!is_empty_svg_placeholder("data:image/svg+xml,<svg><rect/></svg>"));
        assert!(!is_empty_svg_placeholder("https://example.com/a.svg"));
    }

    #[test]
    fn images_lists_every_image_in_reading_order_and_leaves_code_plain() {
        let html = "<p><a href='/big.png'><img src='a.png' alt='A'></a></p><blockquote><img src='b.png'></blockquote>\
                    <ul><li><img src='c.png'></li></ul><pre><code class='language-rust'>fn x() {}</code></pre>";
        let found = images(html, "https://example.com/post");
        let srcs: Vec<&str> = found.iter().map(|i| i.src.as_str()).collect();
        assert_eq!(srcs, ["https://example.com/a.png", "https://example.com/b.png", "https://example.com/c.png"]);
        assert_eq!(found[0].alt, "A");
        assert_eq!(found[0].href.as_deref(), Some("https://example.com/big.png"));
        // The same list as the converted article has.
        let rendered = convert(html, "https://example.com/post", &Palette::default());
        assert_eq!(image_refs(&rendered.blocks), found);
        assert!(images("<p>no images</p>", "").is_empty());
    }

    #[test]
    fn emoji_images_become_their_character() {
        assert_eq!(blocks("<p>hi <img class='emoji' alt='🙂' src='x.svg'></p>"), vec![para("hi 🙂")]);
    }

    // ---- The cases content.test.ts had for the webview version ----

    const BASE: &str = "https://blog.example.com/posts/hello";

    fn render(html: &str) -> Vec<Block> {
        convert(html, BASE, &Palette::default()).blocks
    }

    fn image_srcs(blocks: &[Block]) -> Vec<&str> {
        blocks.iter().filter_map(|b| if let Block::Image { src, .. } = b { Some(src.as_str()) } else { None }).collect()
    }

    #[test]
    fn keeps_ordinary_article_markup() {
        let b = render("<h2>Title</h2><p>Hi <em>there</em></p><ul><li>one</li></ul><blockquote>q</blockquote>");
        assert_eq!(b[0], Block::Heading { level: 2, html: "Title".into() });
        assert_eq!(b[1], para("Hi <i>there</i>"));
        assert!(matches!(&b[2], Block::List { items, .. } if items.len() == 1));
        assert_eq!(b[3], Block::Quote { blocks: vec![para("q")] });
    }

    #[test]
    fn drops_event_handlers_styles_forms_and_layout_attributes() {
        let b = render(
            "<p onclick=\"alert(1)\" style=\"color:red\" class=\"c\" id=\"i\">t</p><style>p{display:none}</style>\
             <form action=\"/x\"><input name=\"a\"><button>go</button></form><img src=\"a.png\" onerror=\"alert(3)\">",
        );
        let all = format!("{b:?}");
        assert!(!all.contains("alert") && !all.contains("onclick") && !all.contains("display"), "{all}");
        assert_eq!(b[0], para("t"));
        assert_eq!(b[1], para("go")); // a button's text stays, as DOMPurify kept it
    }

    #[test]
    fn turns_every_kind_of_embed_into_a_link_to_its_source() {
        let b = render(
            "<iframe src=\"https://www.youtube.com/embed/abc\"></iframe>\
             <video><source src=\"/media/clip.mp4\" type=\"video/mp4\"></video>\
             <embed src=\"https://cdn.example.net/x.swf\"><object data=\"https://cdn.example.net/y\"></object>",
        );
        let hrefs: Vec<&str> = b
            .iter()
            .filter_map(|b| if let Block::Embed { href, .. } = b { Some(href.as_str()) } else { None })
            .collect();
        assert_eq!(
            hrefs,
            [
                "https://www.youtube.com/embed/abc",
                "https://blog.example.com/media/clip.mp4",
                "https://cdn.example.net/x.swf",
                "https://cdn.example.net/y"
            ]
        );
    }

    #[test]
    fn removes_embeds_without_a_source() {
        assert_eq!(render("<p>a</p><iframe></iframe><video></video><p>b</p>"), vec![para("a"), para("b")]);
    }

    #[test]
    fn resolves_relative_image_and_link_urls_against_the_article() {
        let b =
            render("<img src=\"../img/pic.jpg\"><p><a href=\"other\">o</a><a href=\"//cdn.example.net/z\">z</a></p>");
        assert_eq!(image_srcs(&b), ["https://blog.example.com/img/pic.jpg"]);
        let Block::Paragraph { html } = &b[1] else { panic!("{b:?}") };
        assert!(html.contains("href=\"https://blog.example.com/posts/other\""), "{html}");
        assert!(html.contains("href=\"https://cdn.example.net/z\""), "{html}");
    }

    #[test]
    fn a_picture_is_its_img_without_srcset() {
        let b = render(
            "<picture><source srcset=\"small.webp 480w\" type=\"image/webp\"><img src=\"full.jpg\" srcset=\"a.jpg 1x, b.jpg 2x\"></picture>",
        );
        assert_eq!(image_srcs(&b), ["https://blog.example.com/posts/full.jpg"]);
    }

    #[test]
    fn drops_images_without_a_source_and_keeps_data_images() {
        assert_eq!(render("<p>t</p><img alt=\"tracking pixel\"><img src=\"\">"), vec![para("t")]);
        let b = render("<img src=\"data:image/gif;base64,R0lGODlhAQABAAAAACw=\">");
        assert!(image_srcs(&b)[0].starts_with("data:image/gif"));
    }

    #[test]
    fn code_keeps_its_text_escaped_and_inline_code_stays_inline() {
        let b = render("<pre><code>&lt;script&gt;alert(1)&lt;/script&gt;</code></pre><p>use <code>cargo</code></p>");
        let Block::Code { html, .. } = &b[0] else { panic!("{b:?}") };
        assert!(html.contains("&lt;script&gt;") && !html.contains("<script>"), "{html}");
        let Block::Paragraph { html } = &b[1] else { panic!("{b:?}") };
        assert!(html.starts_with("use <code") && html.contains("cargo"), "{html}");
    }

    #[test]
    fn survives_empty_and_broken_input() {
        assert_eq!(render(""), vec![]);
        assert_eq!(render("<p>unclosed <b>bold"), vec![para("unclosed <b>bold</b>")]);
        // A relative link with nothing to make it absolute goes nowhere: its text stays.
        let b = convert("<a href=\"x\">rel</a>", "not a url", &Palette::default()).blocks;
        assert_eq!(b, vec![para("rel")]);
    }
}
