use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};

use crate::error::{Result, RototoError};

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct DocPage {
    pub id: &'static str,
    pub title: &'static str,
    pub markdown: &'static str,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct DocNavSection {
    pub title: &'static str,
    pub pages: &'static [&'static str],
}

pub const DOCS: &[DocPage] = &[DocPage {
    id: "index",
    title: "rototo docs revamp",
    markdown: include_str!("../docs/src/index.md"),
}];

pub const DOC_NAV_SECTIONS: &[DocNavSection] = &[DocNavSection {
    title: "Start",
    pages: &["index"],
}];

/// Design system stylesheet and brand assets vendored under `docs/theme/`.
const DOCS_CSS: &str = include_str!("../docs/theme/rototo-docs.css");
const FAVICON_SVG: &str = include_str!("../docs/theme/favicon.svg");
const WORDMARK_SVG: &str = include_str!("../docs/theme/rototo-wordmark.svg");

/// Brand fonts referenced by the stylesheet: Manrope for display headings,
/// Hanken Grotesk for body text, and JetBrains Mono for code and labels.
const GOOGLE_FONTS_HREF: &str = "https://fonts.googleapis.com/css2?family=Hanken+Grotesk:ital,wght@0,400;0,600;0,700;1,400&family=JetBrains+Mono:ital,wght@0,400;0,600;0,700;1,400&family=Manrope:wght@600;700;800&display=swap";

/// Top navigation bar entries as (label, page id).
const TOPNAV_PAGES: &[(&str, &str)] = &[("Docs", "index")];

pub fn get_page(id: &str) -> Result<&'static DocPage> {
    let id = normalize_page_id(id);
    DOCS.iter()
        .find(|page| page.id == id)
        .ok_or_else(|| RototoError::new(format!("unknown documentation page: {id}")))
}

pub fn render_page_html(page: &DocPage) -> String {
    let nav = render_nav(page.id);
    let section = escape_html(section_title_for(page.id));

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="icon" href="assets/favicon.svg" type="image/svg+xml">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="{fonts}" rel="stylesheet">
<link rel="stylesheet" href="assets/rototo-docs.css">
</head>
<body>
<header class="topbar">
  <a class="brand" href="index.html"><img src="assets/rototo-wordmark.svg" alt="rototo"></a>
  <nav class="topnav" aria-label="Primary">
{topnav}  </nav>
</header>
<div class="layout">
  <details class="mobile-nav">
    <summary><span>Docs</span><strong>{section}</strong></summary>
    <nav class="mobile-nav-panel" aria-label="Documentation">
{nav}
    </nav>
  </details>
  <aside class="sidenav" aria-label="Documentation">
{nav}
  </aside>
  <main class="doc">
    <div class="crumb">{section}</div>
{body}{page_nav}  </main>
</div>
</body>
</html>
"#,
        title = escape_html(page.title),
        fonts = GOOGLE_FONTS_HREF,
        topnav = render_topnav(page.id),
        section = section,
        nav = nav,
        body = render_markdown(page.markdown),
        page_nav = render_page_nav(page.id),
    )
}

pub async fn export_html(out: &Path) -> Result<()> {
    let assets = out.join("assets");
    tokio::fs::create_dir_all(&assets).await.map_err(|err| {
        RototoError::new(format!(
            "failed to create documentation directory {}: {err}",
            assets.display()
        ))
    })?;
    let asset_files = [
        ("rototo-docs.css", DOCS_CSS),
        ("favicon.svg", FAVICON_SVG),
        ("rototo-wordmark.svg", WORDMARK_SVG),
    ];
    for (name, contents) in asset_files {
        tokio::fs::write(assets.join(name), contents)
            .await
            .map_err(|err| {
                RototoError::new(format!("failed to write documentation asset {name}: {err}"))
            })?;
    }
    for page in DOCS {
        tokio::fs::write(out.join(page_href(page.id)), render_page_html(page))
            .await
            .map_err(|err| {
                RototoError::new(format!(
                    "failed to write documentation page {}: {err}",
                    page.id
                ))
            })?;
    }
    Ok(())
}

fn render_nav(current: &str) -> String {
    let mut nav = String::new();
    for section in DOC_NAV_SECTIONS {
        nav.push_str(&format!(
            "    <div class=\"nav-section\">\n      <div class=\"nav-section-title\">{title}</div>\n",
            title = escape_html(section.title),
        ));
        for page_id in section.pages {
            let page = nav_page(page_id);
            let current_attr = if page.id == current {
                r#" aria-current="page""#
            } else {
                ""
            };
            nav.push_str(&format!(
                "      <a href=\"{href}\"{current_attr}>{title}</a>\n",
                href = page_href(page.id),
                title = escape_html(page.title),
            ));
        }
        nav.push_str("    </div>\n");
    }
    nav
}

fn render_topnav(current: &str) -> String {
    let mut nav = String::new();
    for (label, page_id) in TOPNAV_PAGES {
        let current_attr = if *page_id == current {
            r#" aria-current="page""#
        } else {
            ""
        };
        nav.push_str(&format!(
            "    <a href=\"{href}\"{current_attr}>{label}</a>\n",
            href = page_href(page_id),
        ));
    }
    nav
}

fn render_page_nav(current: &str) -> String {
    let pages: Vec<&str> = DOC_NAV_SECTIONS
        .iter()
        .flat_map(|section| section.pages.iter().copied())
        .collect();
    let Some(position) = pages.iter().position(|id| *id == current) else {
        return String::new();
    };

    let mut links = String::new();
    let mut push_link = |label: &str, page: &DocPage| {
        links.push_str(&format!(
            r#"<a href="{href}"><span>{label}</span><strong>{title}</strong></a>"#,
            href = page_href(page.id),
            title = escape_html(page.title),
        ));
    };
    if position > 0 {
        push_link("Previous", nav_page(pages[position - 1]));
    }
    if position + 1 < pages.len() {
        push_link("Next", nav_page(pages[position + 1]));
    }
    if links.is_empty() {
        return String::new();
    }
    format!("<nav class=\"page-nav\" aria-label=\"Page\">{links}</nav>\n")
}

fn section_title_for(page_id: &str) -> &'static str {
    DOC_NAV_SECTIONS
        .iter()
        .find(|section| section.pages.contains(&page_id))
        .map(|section| section.title)
        .unwrap_or("Docs")
}

fn nav_page(id: &str) -> &'static DocPage {
    DOCS.iter()
        .find(|page| page.id == id)
        .expect("documentation navigation references an unknown page")
}

fn page_href(id: &str) -> String {
    if id == "index" {
        "index.html".to_owned()
    } else {
        format!("{id}.html")
    }
}

fn normalize_page_id(id: &str) -> &str {
    match id {
        "" | "/" | "index.html" => "index",
        _ => id.strip_suffix(".html").unwrap_or(id),
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Render page markdown to HTML, replacing fenced code blocks with
/// syntax-highlighted `<pre class="code-block language-*">` blocks.
fn render_markdown(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let mut events = Vec::new();
    let mut code_block: Option<(String, String)> = None;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match &kind {
                    CodeBlockKind::Fenced(info) => code_block_language(info),
                    CodeBlockKind::Indented => "text".to_owned(),
                };
                code_block = Some((language, String::new()));
            }
            Event::Text(text) => match code_block.as_mut() {
                Some((_, code)) => code.push_str(&text),
                None => events.push(Event::Text(text)),
            },
            Event::End(TagEnd::CodeBlock) => {
                let (language, code) = code_block
                    .take()
                    .expect("code block end event without matching start");
                events.push(Event::Html(render_code_block(&language, &code).into()));
            }
            other => events.push(other),
        }
    }

    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    body
}

fn code_block_language(info: &str) -> String {
    let language: String = info
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    if language.is_empty() {
        "text".to_owned()
    } else {
        language
    }
}

fn render_code_block(language: &str, code: &str) -> String {
    let highlighted = match language {
        "toml" => highlight_toml(code),
        "json" => highlight_json(code),
        "sh" => highlight_sh(code),
        _ => escape_html(code),
    };
    format!(
        "<pre class=\"code-block language-{language}\"><code class=\"language-{language}\">{highlighted}</code></pre>\n"
    )
}

fn push_span(out: &mut String, class: &str, text: &str) {
    out.push_str("<span class=\"sx-");
    out.push_str(class);
    out.push_str("\">");
    out.push_str(&escape_html(text));
    out.push_str("</span>");
}

fn highlight_toml(code: &str) -> String {
    let mut out = String::new();
    for line in code.lines() {
        let trimmed = line.trim_start();
        out.push_str(&line[..line.len() - trimmed.len()]);
        if trimmed.starts_with('#') {
            push_span(&mut out, "comment", trimmed);
        } else if trimmed.starts_with('[') && trimmed.trim_end().ends_with(']') {
            push_span(&mut out, "section", trimmed);
        } else if let Some((key, rest)) = split_toml_key(trimmed) {
            push_span(&mut out, "key", key);
            let equals = rest.find('=').expect("toml key line contains `=`");
            out.push_str(&rest[..equals]);
            push_span(&mut out, "punct", "=");
            highlight_toml_value(&mut out, &rest[equals + 1..]);
        } else {
            highlight_toml_value(&mut out, trimmed);
        }
        out.push('\n');
    }
    out
}

/// Split a `key = value` TOML line into the key and the remainder starting
/// with the whitespace before `=`. Returns `None` for non-assignment lines.
fn split_toml_key(line: &str) -> Option<(&str, &str)> {
    let key_end = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let (key, rest) = line.split_at(key_end);
    let is_key_char =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '"' | '\'');
    if key.is_empty() || !rest.trim_start().starts_with('=') || !key.chars().all(is_key_char) {
        return None;
    }
    Some((key, rest))
}

fn highlight_toml_value(out: &mut String, value: &str) {
    let mut rest = value;
    while let Some(c) = rest.chars().next() {
        if c == '#' {
            push_span(out, "comment", rest);
            return;
        } else if c == '"' || c == '\'' {
            let len = quoted_len(rest, c);
            push_span(out, "string", &rest[..len]);
            rest = &rest[len..];
        } else if c.is_ascii_digit()
            || (c == '-' && rest[1..].starts_with(|d: char| d.is_ascii_digit()))
        {
            let len = rest
                .find(|d: char| {
                    !(d.is_ascii_alphanumeric() || matches!(d, '.' | '_' | '-' | '+' | ':'))
                })
                .unwrap_or(rest.len());
            push_span(out, "number", &rest[..len]);
            rest = &rest[len..];
        } else if c.is_ascii_alphabetic() {
            let len = rest
                .find(|d: char| !(d.is_ascii_alphanumeric() || matches!(d, '_' | '-')))
                .unwrap_or(rest.len());
            let word = &rest[..len];
            if word == "true" || word == "false" {
                push_span(out, "literal", word);
            } else {
                out.push_str(&escape_html(word));
            }
            rest = &rest[len..];
        } else if matches!(c, '[' | ']' | '{' | '}' | ',' | '=') {
            push_span(out, "punct", &rest[..c.len_utf8()]);
            rest = &rest[c.len_utf8()..];
        } else {
            let (chunk, remainder) = rest.split_at(c.len_utf8());
            out.push_str(&escape_html(chunk));
            rest = remainder;
        }
    }
}

fn highlight_json(code: &str) -> String {
    let mut out = String::new();
    let mut rest = code;
    while let Some(c) = rest.chars().next() {
        if c == '"' {
            let len = quoted_len(rest, '"');
            let class = if rest[len..].trim_start().starts_with(':') {
                "key"
            } else {
                "string"
            };
            push_span(&mut out, class, &rest[..len]);
            rest = &rest[len..];
        } else if c.is_ascii_digit() || c == '-' {
            let len = rest
                .find(|d: char| !(d.is_ascii_digit() || matches!(d, '.' | '-' | '+' | 'e' | 'E')))
                .unwrap_or(rest.len());
            push_span(&mut out, "number", &rest[..len]);
            rest = &rest[len..];
        } else if c.is_ascii_alphabetic() {
            let len = rest
                .find(|d: char| !d.is_ascii_alphabetic())
                .unwrap_or(rest.len());
            let word = &rest[..len];
            if matches!(word, "true" | "false" | "null") {
                push_span(&mut out, "literal", word);
            } else {
                out.push_str(&escape_html(word));
            }
            rest = &rest[len..];
        } else if matches!(c, '{' | '}' | '[' | ']' | ':' | ',') {
            push_span(&mut out, "punct", &rest[..c.len_utf8()]);
            rest = &rest[c.len_utf8()..];
        } else {
            let (chunk, remainder) = rest.split_at(c.len_utf8());
            out.push_str(&escape_html(chunk));
            rest = remainder;
        }
    }
    out
}

fn highlight_sh(code: &str) -> String {
    let mut out = String::new();
    for line in code.lines() {
        let mut rest = line;
        let mut at_word_start = true;
        while let Some(c) = rest.chars().next() {
            if c == '#' {
                push_span(&mut out, "comment", rest);
                break;
            } else if c == '\'' || c == '"' {
                let len = quoted_len(rest, c);
                push_span(&mut out, "string", &rest[..len]);
                rest = &rest[len..];
                at_word_start = false;
            } else if c == '-' && at_word_start {
                let len = rest
                    .find(|d: char| !(d.is_ascii_alphanumeric() || matches!(d, '-' | '_')))
                    .unwrap_or(rest.len());
                push_span(&mut out, "flag", &rest[..len]);
                rest = &rest[len..];
                at_word_start = false;
            } else if is_sh_punct(c) {
                push_span(&mut out, "punct", &rest[..c.len_utf8()]);
                rest = &rest[c.len_utf8()..];
                at_word_start = false;
            } else if c.is_whitespace() {
                out.push(c);
                rest = &rest[c.len_utf8()..];
                at_word_start = true;
            } else {
                let len = rest
                    .find(|d: char| {
                        d.is_whitespace() || is_sh_punct(d) || matches!(d, '#' | '\'' | '"')
                    })
                    .unwrap_or(rest.len());
                out.push_str(&escape_html(&rest[..len]));
                rest = &rest[len..];
                at_word_start = false;
            }
        }
        out.push('\n');
    }
    out
}

fn is_sh_punct(c: char) -> bool {
    matches!(
        c,
        '.' | '/'
            | '='
            | '+'
            | ':'
            | ','
            | ';'
            | '|'
            | '&'
            | '<'
            | '>'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
    )
}

/// Byte length of the quoted string starting at the opening `quote`,
/// including both quotes. Stops at the line/text end if unterminated.
fn quoted_len(text: &str, quote: char) -> usize {
    let mut escaped = false;
    for (idx, c) in text.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return idx + quote.len_utf8();
        }
    }
    text.len()
}
