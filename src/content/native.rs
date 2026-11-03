//! Converted blocks (convert.rs) for the native macOS app: the small HTML subset left inside a block becomes runs
//! of styled text, so Swift builds attributed strings from data and never parses HTML. Tables become rows of
//! cells. Whitespace collapses as it does in HTML, except in code.

use super::convert::Block;
use ego_tree::NodeRef;
use scraper::{Html, Node};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum NativeBlock {
    Paragraph { runs: Vec<Run> },
    Heading { level: u8, runs: Vec<Run> },
    Image { src: String, alt: String, href: Option<String>, caption: Option<Vec<Run>> },
    Caption { runs: Vec<Run> },
    Code { runs: Vec<Run>, lang: Option<String> },
    Quote { blocks: Vec<NativeBlock> },
    List { ordered: bool, start: i64, items: Vec<Vec<NativeBlock>> },
    Table { rows: Vec<Vec<Cell>> },
    Rule,
    Embed { href: String, label: String },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Cell {
    pub header: bool,
    pub colspan: u32,
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    #[serde(flatten)]
    pub style: Style,
}

/// How a run looks. Only what is set goes into the JSON.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Style {
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub strike: bool,
    /// Inline code: the monospace font on a tinted background.
    #[serde(skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub small: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// `#rrggbb`, from the palette (links, highlighted code).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Script {
    Sup,
    Sub,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde hands it over by reference
fn is_false(b: &bool) -> bool {
    !b
}

pub fn native(blocks: &[Block]) -> Vec<NativeBlock> {
    blocks.iter().map(block).collect()
}

fn block(b: &Block) -> NativeBlock {
    match b {
        Block::Paragraph { html } => NativeBlock::Paragraph { runs: runs(html) },
        Block::Heading { level, html } => NativeBlock::Heading { level: *level, runs: runs(html) },
        Block::Image { src, alt, href, caption } => NativeBlock::Image {
            src: src.clone(),
            alt: alt.clone(),
            href: href.clone(),
            caption: caption.as_deref().map(runs),
        },
        Block::Caption { html } => NativeBlock::Caption { runs: runs(html) },
        Block::Code { html, lang } => NativeBlock::Code { runs: code_runs(html), lang: lang.clone() },
        Block::Quote { blocks } => NativeBlock::Quote { blocks: native(blocks) },
        Block::List { ordered, start, items } => {
            NativeBlock::List { ordered: *ordered, start: *start, items: items.iter().map(|i| native(i)).collect() }
        }
        Block::Table { html } => NativeBlock::Table { rows: table(html) },
        Block::Rule => NativeBlock::Rule,
        Block::Embed { href, label } => NativeBlock::Embed { href: href.clone(), label: label.clone() },
    }
}

/// Inline HTML as runs, whitespace collapsed.
pub fn runs(html: &str) -> Vec<Run> {
    let doc = Html::parse_fragment(html);
    let mut out = Runs::new(false);
    walk(*doc.root_element(), &Style::default(), &mut out);
    out.runs
}

/// A code block's highlighted HTML as runs, every space and line break kept.
fn code_runs(html: &str) -> Vec<Run> {
    let doc = Html::parse_fragment(html);
    let mut out = Runs::new(true);
    walk(*doc.root_element(), &Style::default(), &mut out);
    out.runs
}

fn table(html: &str) -> Vec<Vec<Cell>> {
    let doc = Html::parse_fragment(html);
    let mut rows = Vec::new();
    for node in doc.root_element().descendants() {
        let Node::Element(e) = node.value() else { continue };
        if e.name() != "tr" {
            continue;
        }
        let mut cells = Vec::new();
        for cell in node.children() {
            let Node::Element(ce) = cell.value() else { continue };
            if ce.name() != "td" && ce.name() != "th" {
                continue;
            }
            let colspan = ce.attr("colspan").and_then(|v| v.parse().ok()).unwrap_or(1);
            let mut out = Runs::new(false);
            walk(cell, &Style::default(), &mut out);
            cells.push(Cell { header: ce.name() == "th", colspan, runs: out.runs });
        }
        rows.push(cells);
    }
    rows
}

fn walk(node: NodeRef<'_, Node>, style: &Style, out: &mut Runs) {
    for child in node.children() {
        match child.value() {
            Node::Text(text) => out.text(text, style),
            Node::Element(e) => {
                let mut s = style.clone();
                match e.name() {
                    "br" => {
                        out.line_break(style);
                        continue;
                    }
                    "b" | "strong" => s.bold = true,
                    "i" | "em" => s.italic = true,
                    "u" => s.underline = true,
                    "s" => s.strike = true,
                    "sub" => s.script = Some(Script::Sub),
                    "sup" => s.script = Some(Script::Sup),
                    "small" => s.small = true,
                    "code" => s.code = true,
                    "a" => s.link = e.attr("href").map(str::to_string),
                    _ => {}
                }
                if let Some(css) = e.attr("style") {
                    apply_css(&mut s, css);
                }
                walk(child, &s, out);
            }
            _ => {}
        }
    }
}

/// The few declarations convert.rs and syntect write.
fn apply_css(s: &mut Style, css: &str) {
    for decl in css.split(';') {
        let Some((name, value)) = decl.split_once(':') else { continue };
        let value = value.trim();
        match name.trim() {
            "color" => s.color = Some(value.to_string()),
            "background-color" => s.background = Some(value.to_string()),
            "font-weight" => s.bold = value == "bold" || value.parse::<u32>().is_ok_and(|w| w >= 600),
            "font-style" => s.italic = value == "italic",
            "text-decoration" => {
                s.underline |= value.contains("underline");
                s.strike |= value.contains("line-through");
            }
            _ => {}
        }
    }
}

/// Runs being collected; next to each other, runs of the same style merge.
struct Runs {
    runs: Vec<Run>,
    preformatted: bool,
    /// Whitespace seen since the last visible character, with the style it had: one space before the next one.
    space: Option<Style>,
    /// Nothing visible yet on this line: leading whitespace is dropped.
    line_start: bool,
}

impl Runs {
    fn new(preformatted: bool) -> Self {
        Self { runs: Vec::new(), preformatted, space: None, line_start: true }
    }

    fn push(&mut self, c: char, style: &Style) {
        match self.runs.last_mut() {
            Some(run) if run.style == *style => run.text.push(c),
            _ => self.runs.push(Run { text: c.to_string(), style: style.clone() }),
        }
    }

    fn text(&mut self, text: &str, style: &Style) {
        for c in text.chars() {
            if self.preformatted {
                self.push(c, style);
            } else if c.is_ascii_whitespace() {
                if !self.line_start && self.space.is_none() {
                    self.space = Some(style.clone());
                }
            } else {
                if let Some(space) = self.space.take() {
                    self.push(' ', &space);
                }
                self.push(c, style);
                self.line_start = false;
            }
        }
    }

    fn line_break(&mut self, style: &Style) {
        self.push('\n', style);
        self.space = None;
        self.line_start = true;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Run {
        Run { text: text.into(), style: Style::default() }
    }

    #[test]
    fn whitespace_collapses_and_styles_nest() {
        let r = runs("  Hello\n   <b>big <i>bold</i></b>  world&amp;co ");
        assert_eq!(r[0], plain("Hello "));
        assert_eq!(r[1], Run { text: "big ".into(), style: Style { bold: true, ..Style::default() } });
        assert_eq!(r[2].text, "bold");
        assert!(r[2].style.bold && r[2].style.italic);
        // Each space keeps the style it was typed in; the trailing one is dropped.
        assert_eq!(r[3], plain(" world&co"));
    }

    #[test]
    fn line_breaks_drop_the_spaces_around_them() {
        let r = runs("one <br> two");
        assert_eq!(r, vec![plain("one\ntwo")]);
    }

    #[test]
    fn links_and_inline_code_keep_what_the_converter_wrote() {
        let r = runs(
            "see <a href=\"https://x.org/a\" style=\"color:#123456\">this</a> and \
             <code style=\"font-family:'Menlo'; font-size:85%; background-color:#eeeeee\">\u{a0}f()\u{a0}</code>",
        );
        assert_eq!(r[1].text, "this");
        assert_eq!(r[1].style.link.as_deref(), Some("https://x.org/a"));
        assert_eq!(r[1].style.color.as_deref(), Some("#123456"));
        let code = r.iter().find(|r| r.style.code).unwrap();
        assert_eq!(code.text, "\u{a0}f()\u{a0}");
        assert_eq!(code.style.background.as_deref(), Some("#eeeeee"));
    }

    #[test]
    fn code_keeps_its_spaces_and_colours() {
        let html = "<span style=\"color:#ff0000;\">fn</span><span style=\"color:#00ff00;\">  main</span>()\n  x";
        let r = code_runs(html);
        assert_eq!(r[0].text, "fn");
        assert_eq!(r[0].style.color.as_deref(), Some("#ff0000"));
        assert_eq!(r[1].text, "  main");
        assert_eq!(r[2], plain("()\n  x"));
    }

    #[test]
    fn tables_become_rows_of_cells() {
        let html =
            "<table border=\"1\"><tr><th colspan=\"2\">Head</th></tr><tr><td>a <b>b</b></td><td>c</td></tr></table>";
        let rows = table(html);
        assert_eq!(rows.len(), 2);
        assert!(rows[0][0].header);
        assert_eq!(rows[0][0].colspan, 2);
        assert_eq!(rows[1][0].runs.len(), 2);
        assert_eq!(rows[1][1].runs, vec![plain("c")]);
    }

    #[test]
    fn json_leaves_out_what_is_not_set() {
        let json = serde_json::to_string(&native(&[Block::Paragraph { html: "<i>x</i>".into() }])).unwrap();
        assert_eq!(json, r#"[{"type":"paragraph","runs":[{"text":"x","italic":true}]}]"#);
    }
}
