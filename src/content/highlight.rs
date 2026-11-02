//! Code blocks: syntect in place of highlight.js, coloured from the theme palette the way
//! `src/styles/syntax.css` colours highlight.js classes. Output is HTML with inline colours, which Qt's rich text
//! understands.

use crate::theme::Palette;
use std::{str::FromStr, sync::LazyLock};
use syntect::{
    easy::HighlightLines,
    highlighting::{Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings},
    html::{IncludeBackground, styled_line_to_highlighted_html},
    parsing::{SyntaxReference, SyntaxSet},
    util::LinesWithEndings,
};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// Loads the grammars now rather than on the first code block.
pub fn warm_up() {
    LazyLock::force(&SYNTAXES);
}

fn color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    let byte = |i: usize| hex.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(0x80);
    Color { r: byte(0), g: byte(2), b: byte(4), a: 0xff }
}

/// A syntect theme that uses the palette the way syntax.css does.
pub fn theme(p: &Palette) -> Theme {
    let item = |scope: &str, hex: &str, font_style: Option<FontStyle>| ThemeItem {
        scope: ScopeSelectors::from_str(scope).unwrap_or_default(),
        style: StyleModifier { foreground: Some(color(hex)), background: None, font_style },
    };
    Theme {
        settings: ThemeSettings { foreground: Some(color(&p.fg)), ..ThemeSettings::default() },
        scopes: vec![
            item("keyword, storage, support.function.builtin, variable.language", &p.magenta, None),
            item("string, constant.character, entity.other.attribute-name, markup.inserted", &p.green, None),
            item("constant.numeric, constant.language, constant.other.symbol", &p.orange, None),
            item("entity.name.function, support.function, entity.name.section, markup.heading", &p.blue, None),
            item(
                "entity.name.type, entity.name.class, support.type, support.class, storage.type.class",
                &p.yellow,
                None,
            ),
            item("entity.name.tag, meta.preprocessor, punctuation.definition.tag, meta.annotation", &p.cyan, None),
            item("markup.deleted, invalid", &p.red, None),
            item("comment, punctuation.definition.comment", &p.fg_dim, Some(FontStyle::ITALIC)),
        ],
        ..Theme::default()
    }
}

/// The grammar for a code block, from its class names (`language-rust`, `lang-js`, `highlight-source-go`, `rust`)
/// or failing that its first line (`#!/bin/sh`). highlight.js guesses from the content; syntect can't.
pub fn find_syntax(hints: &[&str], code: &str) -> Option<&'static SyntaxReference> {
    let ss = &*SYNTAXES;
    hints
        .iter()
        .map(|hint| {
            ["language-", "lang-", "highlight-source-", "highlight-"]
                .iter()
                .find_map(|prefix| hint.strip_prefix(prefix))
                .unwrap_or(hint)
        })
        .filter(|token| !token.is_empty())
        .find_map(|token| {
            ss.find_syntax_by_token(token).or_else(|| alias(token).and_then(|t| ss.find_syntax_by_token(t)))
        })
        .or_else(|| ss.find_syntax_by_first_line(code))
        .filter(|s| s.name != "Plain Text")
}

/// Common names syntect's default set knows under another token, or not at all (then the closest relative).
fn alias(token: &str) -> Option<&'static str> {
    Some(match token.to_ascii_lowercase().as_str() {
        "ts" | "typescript" | "tsx" | "jsx" | "javascript" | "node" => "js",
        "shell" | "bash" | "zsh" | "console" | "shellsession" => "sh",
        "golang" => "go",
        "py" | "python3" => "python",
        "yml" => "yaml",
        "kotlin" | "kt" | "swift" | "dart" => "java",
        "rs" => "rust",
        "c++" | "cxx" => "cpp",
        "html" | "xml" | "svg" | "vue" => "html",
        "json5" | "jsonc" => "json",
        _ => return None,
    })
}

/// `code` as HTML spans, one `<br>`-free run of lines, for a `<pre>`. Plain (escaped) when there is no grammar.
pub fn to_html(code: &str, syntax: Option<&SyntaxReference>, theme: &Theme) -> String {
    let Some(syntax) = syntax else { return escape(code) };
    let mut h = HighlightLines::new(syntax, theme);
    let mut out = String::with_capacity(code.len() * 3);
    for line in LinesWithEndings::from(code) {
        match h
            .highlight_line(line, &SYNTAXES)
            .ok()
            .and_then(|r| styled_line_to_highlighted_html(&r, IncludeBackground::No).ok())
        {
            Some(html) => out.push_str(&html),
            None => out.push_str(&escape(line)),
        }
    }
    out
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
