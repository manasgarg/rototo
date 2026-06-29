use std::collections::BTreeMap;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};

use super::{SDK_LANGUAGES, escape_html, render_sdk_language_picker};

#[derive(Debug)]
pub(super) struct TocItem {
    level: u8,
    id: String,
    title: String,
}

pub(super) fn prepare_markdown_for_html(markdown: &str) -> (String, Vec<TocItem>) {
    let mut out = String::new();
    let mut toc = Vec::new();
    let mut slugs = BTreeMap::new();
    let mut in_fenced_code = false;

    for line in markdown.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            in_fenced_code = !in_fenced_code;
            out.push_str(line);
            continue;
        }
        if in_fenced_code {
            out.push_str(line);
            continue;
        }

        let Some(heading) = parse_markdown_heading(line) else {
            out.push_str(line);
            continue;
        };
        let base_slug = slugify_heading(&heading.title);
        let count = slugs.entry(base_slug.clone()).or_insert(0usize);
        *count += 1;
        let id = if *count == 1 {
            base_slug
        } else {
            format!("{base_slug}-{count}")
        };

        if heading.level == 2 || heading.level == 3 {
            toc.push(TocItem {
                level: heading.level,
                id: id.clone(),
                title: heading.title.clone(),
            });
        }
        out.push_str(&format!(
            "{prefix}{markers} {title} {{#{id}}}{newline}",
            prefix = heading.prefix,
            markers = "#".repeat(heading.level as usize),
            title = heading.title,
            newline = heading.newline,
        ));
    }

    (out, toc)
}

#[derive(Debug)]
struct MarkdownHeading {
    level: u8,
    prefix: String,
    title: String,
    newline: String,
}

fn parse_markdown_heading(line: &str) -> Option<MarkdownHeading> {
    let content = line.trim_end_matches(['\r', '\n']);
    let newline = &line[content.len()..];
    let prefix_len = content.len() - content.trim_start_matches(' ').len();
    if prefix_len > 3 {
        return None;
    }
    let prefix = &content[..prefix_len];
    let trimmed = &content[prefix_len..];
    let level = trimmed.chars().take_while(|char| *char == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let title = rest.trim().trim_end_matches('#').trim_end().trim();
    if title.is_empty() || title.contains("{#") {
        return None;
    }
    Some(MarkdownHeading {
        level: level as u8,
        prefix: prefix.to_owned(),
        title: title.to_owned(),
        newline: newline.to_owned(),
    })
}

fn slugify_heading(title: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for char in title.chars() {
        if char.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(char.to_ascii_lowercase());
            pending_dash = false;
        } else if char.is_whitespace() || matches!(char, '-' | '_' | '/' | ':' | '.') {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        "section".to_owned()
    } else {
        slug
    }
}

pub(super) fn render_toc(items: &[TocItem]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut toc = String::from(
        "  <aside class=\"toc\" aria-label=\"On this page\">\n    <div class=\"toc-title\">On this page</div>\n",
    );
    for item in items {
        let class = if item.level == 3 {
            r#" class="sub""#
        } else {
            ""
        };
        toc.push_str(&format!(
            "    <a href=\"#{id}\"{class}>{title}</a>\n",
            id = escape_html(&item.id),
            title = escape_html(&plain_heading_title(&item.title)),
        ));
    }
    toc.push_str("  </aside>\n");
    toc
}

fn plain_heading_title(title: &str) -> String {
    title
        .chars()
        .filter(|char| !matches!(char, '`' | '*' | '[' | ']' | '(' | ')' | '<' | '>'))
        .collect()
}

/// Render page markdown to HTML, replacing fenced code blocks with
/// syntax-highlighted `<pre class="code-block language-*">` blocks.
pub(super) fn render_markdown(markdown: &str) -> String {
    let markdown = expand_sdk_snippet_groups(markdown);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);

    let mut events = Vec::new();
    let mut code_block: Option<(String, String)> = None;
    for event in Parser::new_ext(&markdown, options) {
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
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = match strip_md_link_extension(&dest_url) {
                    Some(rewritten) => rewritten.into(),
                    None => dest_url,
                };
                events.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }));
            }
            other => events.push(other),
        }
    }

    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    body
}

/// Internal doc links are authored as `./concepts.md`, but the rendered HTML
/// site serves pages without the `.md` source extension. Strip `.md` from
/// relative links (keeping any `#anchor`) so cross-page links resolve. External
/// links are left untouched.
fn strip_md_link_extension(dest: &str) -> Option<String> {
    if dest.starts_with("http://") || dest.starts_with("https://") || dest.starts_with("mailto:") {
        return None;
    }
    let (path, anchor) = match dest.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor)),
        None => (dest, None),
    };
    let stripped = path.strip_suffix(".md")?;
    Some(match anchor {
        Some(anchor) => format!("{stripped}#{anchor}"),
        None => stripped.to_owned(),
    })
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

pub(super) fn render_code_block(language: &str, code: &str) -> String {
    render_code_block_with_attrs(language, code, "", "")
}

fn render_code_block_with_attrs(
    language: &str,
    code: &str,
    extra_class: &str,
    extra_attrs: &str,
) -> String {
    let code_language = highlight_js_language(language);
    let code = escape_html(code);
    let class_suffix = if extra_class.is_empty() {
        String::new()
    } else {
        format!(" {extra_class}")
    };
    format!(
        "<pre class=\"code-block language-{language}{class_suffix}\"{extra_attrs}><code class=\"language-{code_language}\">{code}</code></pre>\n"
    )
}

fn highlight_js_language(language: &str) -> &str {
    match language {
        "sh" => "bash",
        "text" => "plaintext",
        "toml" => "ini",
        other => other,
    }
}

fn expand_sdk_snippet_groups(markdown: &str) -> String {
    let mut out = String::new();
    let mut lines = markdown.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let Some(id) = sdk_snippet_start(line) else {
            out.push_str(line);
            continue;
        };

        let mut group = String::new();
        let mut closed = false;
        for group_line in lines.by_ref() {
            if group_line.trim() == ":::" {
                closed = true;
                break;
            }
            group.push_str(group_line);
        }
        assert!(closed, "sdk-snippet group `{id}` is missing closing :::");
        out.push_str(&render_sdk_snippet_group(id, &group));
    }
    out
}

fn sdk_snippet_start(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix(":::sdk-snippet ")
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// Pull the code block for one `language` out of the named `:::sdk-snippet`
/// group in `markdown`. SDK package READMEs reuse these canonical per-language
/// snippets instead of keeping their own copies. The returned code keeps its
/// trailing newline.
pub(super) fn sdk_snippet_code(markdown: &str, group_id: &str, language: &str) -> Option<String> {
    let mut lines = markdown.split_inclusive('\n');
    while let Some(line) = lines.next() {
        if sdk_snippet_start(line) != Some(group_id) {
            continue;
        }
        let mut group = String::new();
        for group_line in lines.by_ref() {
            if group_line.trim() == ":::" {
                break;
            }
            group.push_str(group_line);
        }
        return parse_sdk_snippet_blocks(group_id, &group)
            .into_iter()
            .find(|(snippet_language, _)| snippet_language == language)
            .map(|(_, code)| code);
    }
    None
}

fn render_sdk_snippet_group(id: &str, markdown: &str) -> String {
    let snippets = parse_sdk_snippet_blocks(id, markdown);
    let mut out = format!(
        "<div class=\"sdk-snippet-group\" data-snippet-id=\"{}\">\n",
        escape_html(id)
    );
    out.push_str("  <div class=\"sdk-snippet-toolbar\">");
    out.push_str(&render_sdk_language_picker());
    out.push_str("</div>\n");
    for language in SDK_LANGUAGES {
        let code = snippets
            .iter()
            .find(|(snippet_language, _)| snippet_language == language.id)
            .map(|(_, code)| code.as_str())
            .unwrap_or_else(|| panic!("sdk-snippet group `{id}` is missing `{}`", language.id));
        out.push_str(&render_code_block_with_attrs(
            language.id,
            code,
            "sdk-snippet",
            &format!(
                r#" data-sdk-lang="{}" aria-label="{} SDK snippet""#,
                escape_html(language.id),
                escape_html(language.label),
            ),
        ));
    }
    out.push_str("</div>\n");
    out
}

fn parse_sdk_snippet_blocks(id: &str, markdown: &str) -> Vec<(String, String)> {
    let mut snippets = Vec::new();
    let mut lines = markdown.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        let Some(language) = fenced_code_language(line) else {
            panic!("sdk-snippet group `{id}` contains non-code content: {line}");
        };
        assert!(
            SDK_LANGUAGES
                .iter()
                .any(|sdk_language| sdk_language.id == language),
            "sdk-snippet group `{id}` uses unsupported language `{language}`"
        );
        assert!(
            !snippets
                .iter()
                .any(|(existing_language, _)| existing_language == &language),
            "sdk-snippet group `{id}` repeats language `{language}`"
        );

        let mut code = String::new();
        let mut closed = false;
        for code_line in lines.by_ref() {
            if code_line.trim() == "```" {
                closed = true;
                break;
            }
            code.push_str(code_line);
        }
        assert!(
            closed,
            "sdk-snippet group `{id}` language `{language}` is missing closing fence"
        );
        snippets.push((language, code));
    }
    assert_eq!(
        snippets.len(),
        SDK_LANGUAGES.len(),
        "sdk-snippet group `{id}` should include every supported SDK language"
    );
    snippets
}

fn fenced_code_language(line: &str) -> Option<String> {
    let trimmed = line.trim();
    trimmed
        .strip_prefix("```")
        .map(code_block_language)
        .filter(|language| language != "text")
}
