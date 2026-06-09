use std::path::Path;
use std::sync::LazyLock;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

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

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct SdkLanguage {
    pub id: &'static str,
    pub label: &'static str,
}

pub const SDK_LANGUAGES: &[SdkLanguage] = &[
    SdkLanguage {
        id: "rust",
        label: "Rust",
    },
    SdkLanguage {
        id: "python",
        label: "Python",
    },
    SdkLanguage {
        id: "typescript",
        label: "TypeScript",
    },
    SdkLanguage {
        id: "java",
        label: "Java",
    },
    SdkLanguage {
        id: "go",
        label: "Go",
    },
];

pub const DOCS: &[DocPage] = &[
    DocPage {
        id: "index",
        title: "rototo",
        markdown: include_str!("../docs/src/index.md"),
    },
    DocPage {
        id: "getting-started",
        title: "Getting Started",
        markdown: include_str!("../docs/src/getting-started.md"),
    },
    DocPage {
        id: "operational-switches",
        title: "Operational Switches",
        markdown: include_str!("../docs/src/operational-switches.md"),
    },
    DocPage {
        id: "incident-banner",
        title: "Incident Banner",
        markdown: include_str!("../docs/src/incident-banner.md"),
    },
    DocPage {
        id: "onboarding-checklist",
        title: "Onboarding Checklist",
        markdown: include_str!("../docs/src/onboarding-checklist.md"),
    },
    DocPage {
        id: "bucketed-rollout",
        title: "Bucketed Rollout",
        markdown: include_str!("../docs/src/bucketed-rollout.md"),
    },
    DocPage {
        id: "notification-delivery-policy",
        title: "Notification Delivery Policy",
        markdown: include_str!("../docs/src/notification-delivery-policy.md"),
    },
    DocPage {
        id: "service-degradation-policy",
        title: "Service Degradation Policy",
        markdown: include_str!("../docs/src/service-degradation-policy.md"),
    },
    DocPage {
        id: "workspace-layering",
        title: "Workspace Layering",
        markdown: include_str!("../docs/src/workspace-layering.md"),
    },
    DocPage {
        id: "modeling-runtime-configuration",
        title: "Modeling Runtime Configuration",
        markdown: include_str!("../docs/src/modeling-runtime-configuration.md"),
    },
    DocPage {
        id: "application-integration",
        title: "Application Integration",
        markdown: include_str!("../docs/src/application-integration.md"),
    },
    DocPage {
        id: "testing-runtime-configuration",
        title: "Testing Runtime Configuration",
        markdown: include_str!("../docs/src/testing-runtime-configuration.md"),
    },
    DocPage {
        id: "operating-runtime-configuration",
        title: "Operating Runtime Configuration",
        markdown: include_str!("../docs/src/operating-runtime-configuration.md"),
    },
    DocPage {
        id: "production-workflow",
        title: "Production Workflow",
        markdown: include_str!("../docs/src/production-workflow.md"),
    },
    DocPage {
        id: "reference-workspace-manifest",
        title: "Workspace Manifest",
        markdown: include_str!("../docs/src/reference-workspace-manifest.md"),
    },
    DocPage {
        id: "reference-workspace-layout",
        title: "Workspace Layout",
        markdown: include_str!("../docs/src/reference-workspace-layout.md"),
    },
    DocPage {
        id: "reference-workspace-sources",
        title: "Workspace Sources",
        markdown: include_str!("../docs/src/reference-workspace-sources.md"),
    },
    DocPage {
        id: "reference-workspace-layering",
        title: "Workspace Layering",
        markdown: include_str!("../docs/src/reference-workspace-layering.md"),
    },
    DocPage {
        id: "reference-context",
        title: "Resolve Context",
        markdown: include_str!("../docs/src/reference-context.md"),
    },
    DocPage {
        id: "reference-qualifiers",
        title: "Qualifiers",
        markdown: include_str!("../docs/src/reference-qualifiers.md"),
    },
    DocPage {
        id: "reference-predicate-operators",
        title: "Predicate Operators",
        markdown: include_str!("../docs/src/reference-predicate-operators.md"),
    },
    DocPage {
        id: "reference-variables",
        title: "Variables",
        markdown: include_str!("../docs/src/reference-variables.md"),
    },
    DocPage {
        id: "reference-variable-values",
        title: "Variable Values",
        markdown: include_str!("../docs/src/reference-variable-values.md"),
    },
    DocPage {
        id: "reference-resources",
        title: "Resources",
        markdown: include_str!("../docs/src/reference-resources.md"),
    },
    DocPage {
        id: "reference-qualifier-resolution",
        title: "Qualifier Resolution",
        markdown: include_str!("../docs/src/reference-qualifier-resolution.md"),
    },
    DocPage {
        id: "reference-variable-resolution",
        title: "Variable Resolution",
        markdown: include_str!("../docs/src/reference-variable-resolution.md"),
    },
    DocPage {
        id: "reference-resolution-output",
        title: "Resolution Output",
        markdown: include_str!("../docs/src/reference-resolution-output.md"),
    },
    DocPage {
        id: "reference-cli-overview",
        title: "CLI Overview",
        markdown: include_str!("../docs/src/reference-cli-overview.md"),
    },
    DocPage {
        id: "reference-cli-commands",
        title: "CLI Commands",
        markdown: include_str!("../docs/src/reference-cli-commands.md"),
    },
    DocPage {
        id: "reference-sdk-loading",
        title: "SDK Loading",
        markdown: include_str!("../docs/src/reference-sdk-loading.md"),
    },
    DocPage {
        id: "reference-sdk-resolution",
        title: "SDK Resolution",
        markdown: include_str!("../docs/src/reference-sdk-resolution.md"),
    },
    DocPage {
        id: "reference-sdk-refresh",
        title: "SDK Refresh",
        markdown: include_str!("../docs/src/reference-sdk-refresh.md"),
    },
    DocPage {
        id: "reference-sdk-rust",
        title: "Rust SDK",
        markdown: include_str!("../docs/src/reference-sdk-rust.md"),
    },
    DocPage {
        id: "reference-sdk-python",
        title: "Python SDK",
        markdown: include_str!("../docs/src/reference-sdk-python.md"),
    },
    DocPage {
        id: "reference-sdk-typescript",
        title: "TypeScript SDK",
        markdown: include_str!("../docs/src/reference-sdk-typescript.md"),
    },
    DocPage {
        id: "reference-sdk-java",
        title: "Java SDK",
        markdown: include_str!("../docs/src/reference-sdk-java.md"),
    },
    DocPage {
        id: "reference-sdk-go",
        title: "Go SDK",
        markdown: include_str!("../docs/src/reference-sdk-go.md"),
    },
    DocPage {
        id: "reference-lint-overview",
        title: "Lint",
        markdown: include_str!("../docs/src/reference-lint-overview.md"),
    },
    DocPage {
        id: "reference-diagnostics",
        title: "Diagnostics",
        markdown: include_str!("../docs/src/reference-diagnostics.md"),
    },
    DocPage {
        id: "reference-custom-lua-lint",
        title: "Custom Lua Lint",
        markdown: include_str!("../docs/src/reference-custom-lua-lint.md"),
    },
    DocPage {
        id: "reference-json-output",
        title: "JSON Output",
        markdown: include_str!("../docs/src/reference-json-output.md"),
    },
];

pub const DOC_NAV_SECTIONS: &[DocNavSection] = &[
    DocNavSection {
        title: "Start",
        pages: &["index"],
    },
    DocNavSection {
        title: "Learn",
        pages: &[
            "getting-started",
            "operational-switches",
            "incident-banner",
            "onboarding-checklist",
            "bucketed-rollout",
            "notification-delivery-policy",
            "service-degradation-policy",
            "workspace-layering",
        ],
    },
    DocNavSection {
        title: "Adopt",
        pages: &[
            "modeling-runtime-configuration",
            "application-integration",
            "testing-runtime-configuration",
            "operating-runtime-configuration",
            "production-workflow",
        ],
    },
    DocNavSection {
        title: "Reference",
        pages: &[
            "reference-workspace-manifest",
            "reference-workspace-layout",
            "reference-workspace-sources",
            "reference-workspace-layering",
            "reference-context",
            "reference-qualifiers",
            "reference-predicate-operators",
            "reference-variables",
            "reference-variable-values",
            "reference-resources",
            "reference-qualifier-resolution",
            "reference-variable-resolution",
            "reference-resolution-output",
            "reference-cli-overview",
            "reference-cli-commands",
            "reference-sdk-loading",
            "reference-sdk-resolution",
            "reference-sdk-refresh",
            "reference-sdk-rust",
            "reference-sdk-python",
            "reference-sdk-typescript",
            "reference-sdk-java",
            "reference-sdk-go",
            "reference-lint-overview",
            "reference-diagnostics",
            "reference-custom-lua-lint",
            "reference-json-output",
        ],
    },
];

/// Design system stylesheet and brand assets vendored under `docs/theme/`.
const DOCS_CSS: &str = include_str!("../docs/theme/rototo-docs.css");
const FAVICON_SVG: &str = include_str!("../docs/theme/favicon.svg");
const WORDMARK_SVG: &str = include_str!("../docs/theme/rototo-wordmark.svg");
static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// Brand fonts referenced by the stylesheet: Manrope for display headings,
/// Hanken Grotesk for body text, and JetBrains Mono for code and labels.
const GOOGLE_FONTS_HREF: &str = "https://fonts.googleapis.com/css2?family=Hanken+Grotesk:ital,wght@0,400..700;1,400..700&family=JetBrains+Mono:ital,wght@0,400..700;1,400..700&family=Manrope:wght@400..800&display=swap";

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
<html lang="en" data-sdk-lang="rust">
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
  {language_picker}
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
{language_script}
</body>
</html>
"#,
        title = escape_html(page.title),
        fonts = GOOGLE_FONTS_HREF,
        topnav = render_topnav(page.id),
        language_picker = render_sdk_language_picker(),
        section = section,
        nav = nav,
        body = render_markdown(page.markdown),
        page_nav = render_page_nav(page.id),
        language_script = render_sdk_language_script(),
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

pub fn render_package_readme(sdk: &str) -> Result<String> {
    let page = match sdk {
        "python" => get_page("reference-sdk-python")?,
        "typescript" => get_page("reference-sdk-typescript")?,
        "java" => get_page("reference-sdk-java")?,
        "go" => get_page("reference-sdk-go")?,
        other => {
            return Err(RototoError::new(format!(
                "unsupported package README SDK: {other}"
            )));
        }
    };
    let mut markdown = page.markdown.to_owned();
    let readme_title = match sdk {
        "python" => "rototo Python SDK",
        "typescript" => "rototo TypeScript SDK",
        "java" => "rototo Java SDK",
        "go" => "rototo Go SDK",
        _ => unreachable!("unsupported SDK was rejected above"),
    };
    if let Some(rest) = markdown.strip_prefix(&format!("# {} Reference\n", page.title)) {
        markdown = format!("# {readme_title}\n{rest}");
    } else if let Some(rest) = markdown.strip_prefix(&format!("# {}\n", page.title)) {
        markdown = format!("# {readme_title}\n{rest}");
    }
    Ok(format!(
        "<!-- Generated from docs/src/{page_id}.md by `rototo docs --package-readme {sdk} --out sdks/{sdk}/README.md`. Do not edit directly. -->\n\n{markdown}",
        page_id = page.id,
    ))
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

fn render_sdk_language_picker() -> String {
    let mut options = String::new();
    for language in SDK_LANGUAGES {
        let selected = if language.id == "rust" {
            " selected"
        } else {
            ""
        };
        options.push_str(&format!(
            r#"<option value="{id}"{selected}>{label}</option>"#,
            id = escape_html(language.id),
            label = escape_html(language.label),
        ));
    }
    format!(
        r#"<label class="sdk-language-picker"><span>SDK</span><select id="sdk-language" aria-label="SDK language">{options}</select></label>"#
    )
}

fn render_sdk_language_script() -> String {
    let supported = SDK_LANGUAGES
        .iter()
        .map(|language| format!("\"{}\"", language.id))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"<script>
(function() {{
  var supported = [{supported}];
  var key = "rototo.sdkLanguage";
  var select = document.getElementById("sdk-language");
  function stored() {{
    try {{
      return window.localStorage.getItem(key);
    }} catch (_) {{
      return null;
    }}
  }}
  function remember(language) {{
    try {{
      window.localStorage.setItem(key, language);
    }} catch (_) {{}}
  }}
  function preferred() {{
    var params = new URLSearchParams(window.location.search);
    return params.get("sdk") || stored() || "rust";
  }}
  function setLanguage(value) {{
    var language = supported.indexOf(value) >= 0 ? value : "rust";
    document.documentElement.setAttribute("data-sdk-lang", language);
    remember(language);
    if (select) {{
      select.value = language;
    }}
  }}
  setLanguage(preferred());
  if (select) {{
    select.addEventListener("change", function(event) {{
      setLanguage(event.target.value);
    }});
  }}
}})();
</script>"#
    )
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
    let markdown = expand_sdk_snippet_groups(markdown);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

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
    render_code_block_with_attrs(language, code, "", "")
}

fn render_code_block_with_attrs(
    language: &str,
    code: &str,
    extra_class: &str,
    extra_attrs: &str,
) -> String {
    let highlighted = highlight_code(language, code);
    let class_suffix = if extra_class.is_empty() {
        String::new()
    } else {
        format!(" {extra_class}")
    };
    format!(
        "<pre class=\"code-block language-{language}{class_suffix}\"{extra_attrs}><code class=\"language-{language}\">{highlighted}</code></pre>\n"
    )
}

fn highlight_code(language: &str, code: &str) -> String {
    let syntax_set = &*SYNTAX_SET;
    let Some(syntax) = syntax_for_language(syntax_set, language) else {
        return fallback_highlight_code(language, code);
    };
    let mut generator = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        syntax_set,
        ClassStyle::SpacedPrefixed { prefix: "sx-" },
    );
    for line in LinesWithEndings::from(code) {
        if generator
            .parse_html_for_line_which_includes_newline(line)
            .is_err()
        {
            return fallback_highlight_code(language, code);
        }
    }
    generator.finalize()
}

fn syntax_for_language<'a>(
    syntax_set: &'a SyntaxSet,
    language: &str,
) -> Option<&'a syntect::parsing::SyntaxReference> {
    let token = syntax_token(language);
    syntax_set
        .find_syntax_by_token(token)
        .or_else(|| syntax_set.find_syntax_by_extension(token))
        .or_else(|| syntax_set.find_syntax_by_token(language))
        .or_else(|| syntax_set.find_syntax_by_extension(language))
}

fn syntax_token(language: &str) -> &str {
    match language {
        "sh" => "bash",
        "typescript" => "ts",
        other => other,
    }
}

fn fallback_highlight_code(language: &str, code: &str) -> String {
    match language {
        "toml" => highlight_toml(code),
        "json" => highlight_json(code),
        "python" => highlight_python(code),
        "java" => highlight_java(code),
        "rust" => highlight_rust(code),
        "sh" => highlight_sh(code),
        "typescript" => highlight_typescript(code),
        _ => escape_html(code),
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

fn render_sdk_snippet_group(id: &str, markdown: &str) -> String {
    let snippets = parse_sdk_snippet_blocks(id, markdown);
    let mut out = format!(
        "<div class=\"sdk-snippet-group\" data-snippet-id=\"{}\">\n",
        escape_html(id)
    );
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

fn highlight_python(code: &str) -> String {
    let mut out = String::new();
    let mut rest = code;
    while let Some(c) = rest.chars().next() {
        if c == '#' {
            let len = rest.find('\n').unwrap_or(rest.len());
            push_span(&mut out, "comment", &rest[..len]);
            rest = &rest[len..];
        } else if rest.starts_with("'''") || rest.starts_with("\"\"\"") {
            let quote = &rest[..3];
            let len = rest[3..]
                .find(quote)
                .map(|index| index + 6)
                .unwrap_or(rest.len());
            push_span(&mut out, "string", &rest[..len]);
            rest = &rest[len..];
        } else if c == '\'' || c == '"' {
            let len = quoted_len(rest, c);
            push_span(&mut out, "string", &rest[..len]);
            rest = &rest[len..];
        } else if c.is_ascii_digit() {
            let len = rest
                .find(|d: char| !(d.is_ascii_digit() || matches!(d, '.' | '_' | 'e' | 'E')))
                .unwrap_or(rest.len());
            push_span(&mut out, "number", &rest[..len]);
            rest = &rest[len..];
        } else if is_python_ident_start(c) {
            let len = rest
                .find(|d: char| !is_python_ident_continue(d))
                .unwrap_or(rest.len());
            let word = &rest[..len];
            if is_python_keyword(word) {
                push_span(&mut out, "keyword", word);
            } else if is_python_literal(word) {
                push_span(&mut out, "literal", word);
            } else {
                out.push_str(&escape_html(word));
            }
            rest = &rest[len..];
        } else if is_python_punct(c) {
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

fn is_python_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_python_ident_continue(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn is_python_keyword(word: &str) -> bool {
    matches!(
        word,
        "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "class"
            | "continue"
            | "def"
            | "elif"
            | "else"
            | "except"
            | "finally"
            | "for"
            | "from"
            | "if"
            | "import"
            | "in"
            | "is"
            | "lambda"
            | "not"
            | "or"
            | "pass"
            | "raise"
            | "return"
            | "try"
            | "with"
            | "while"
    )
}

fn is_python_literal(word: &str) -> bool {
    matches!(word, "False" | "None" | "True")
}

fn is_python_punct(c: char) -> bool {
    matches!(
        c,
        '.' | '/'
            | '='
            | '+'
            | '-'
            | '*'
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

fn highlight_java(code: &str) -> String {
    let mut out = String::new();
    let mut rest = code;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with("//") {
            let len = rest.find('\n').unwrap_or(rest.len());
            push_span(&mut out, "comment", &rest[..len]);
            rest = &rest[len..];
        } else if rest.starts_with("/*") {
            let len = rest[2..]
                .find("*/")
                .map(|index| index + 4)
                .unwrap_or(rest.len());
            push_span(&mut out, "comment", &rest[..len]);
            rest = &rest[len..];
        } else if c == '\'' || c == '"' {
            let len = quoted_len(rest, c);
            push_span(&mut out, "string", &rest[..len]);
            rest = &rest[len..];
        } else if c.is_ascii_digit() {
            let len = rest
                .find(|d: char| !(d.is_ascii_digit() || matches!(d, '.' | '_' | 'e' | 'E' | 'L')))
                .unwrap_or(rest.len());
            push_span(&mut out, "number", &rest[..len]);
            rest = &rest[len..];
        } else if is_java_ident_start(c) {
            let len = rest
                .find(|d: char| !is_java_ident_continue(d))
                .unwrap_or(rest.len());
            let word = &rest[..len];
            if is_java_keyword(word) {
                push_span(&mut out, "keyword", word);
            } else if is_java_literal(word) {
                push_span(&mut out, "literal", word);
            } else {
                out.push_str(&escape_html(word));
            }
            rest = &rest[len..];
        } else if is_java_punct(c) {
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

fn is_java_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_java_ident_continue(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn is_java_keyword(word: &str) -> bool {
    matches!(
        word,
        "abstract"
            | "assert"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "continue"
            | "default"
            | "do"
            | "else"
            | "enum"
            | "extends"
            | "final"
            | "finally"
            | "for"
            | "if"
            | "implements"
            | "import"
            | "instanceof"
            | "interface"
            | "new"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "static"
            | "switch"
            | "throw"
            | "throws"
            | "try"
            | "var"
            | "void"
            | "while"
    )
}

fn is_java_literal(word: &str) -> bool {
    matches!(word, "false" | "null" | "true")
}

fn is_java_punct(c: char) -> bool {
    matches!(
        c,
        '.' | '/'
            | '='
            | '+'
            | '-'
            | '*'
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

fn highlight_typescript(code: &str) -> String {
    let mut out = String::new();
    let mut rest = code;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with("//") {
            let len = rest.find('\n').unwrap_or(rest.len());
            push_span(&mut out, "comment", &rest[..len]);
            rest = &rest[len..];
        } else if rest.starts_with("/*") {
            let len = rest[2..]
                .find("*/")
                .map(|index| index + 4)
                .unwrap_or(rest.len());
            push_span(&mut out, "comment", &rest[..len]);
            rest = &rest[len..];
        } else if c == '\'' || c == '"' || c == '`' {
            let len = quoted_len(rest, c);
            push_span(&mut out, "string", &rest[..len]);
            rest = &rest[len..];
        } else if c.is_ascii_digit() {
            let len = rest
                .find(|d: char| !(d.is_ascii_digit() || matches!(d, '.' | '_' | 'e' | 'E')))
                .unwrap_or(rest.len());
            push_span(&mut out, "number", &rest[..len]);
            rest = &rest[len..];
        } else if is_typescript_ident_start(c) {
            let len = rest
                .find(|d: char| !is_typescript_ident_continue(d))
                .unwrap_or(rest.len());
            let word = &rest[..len];
            if is_typescript_keyword(word) {
                push_span(&mut out, "keyword", word);
            } else if is_typescript_literal(word) {
                push_span(&mut out, "literal", word);
            } else if is_typescript_builtin_type(word) {
                push_span(&mut out, "key", word);
            } else {
                out.push_str(&escape_html(word));
            }
            rest = &rest[len..];
        } else if is_typescript_punct(c) {
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

fn is_typescript_ident_start(c: char) -> bool {
    c == '_' || c == '$' || c.is_ascii_alphabetic()
}

fn is_typescript_ident_continue(c: char) -> bool {
    c == '_' || c == '$' || c.is_ascii_alphanumeric()
}

fn is_typescript_keyword(word: &str) -> bool {
    matches!(
        word,
        "as" | "async"
            | "await"
            | "break"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "else"
            | "export"
            | "extends"
            | "finally"
            | "for"
            | "from"
            | "function"
            | "if"
            | "import"
            | "interface"
            | "let"
            | "new"
            | "return"
            | "throw"
            | "try"
            | "type"
            | "typeof"
            | "while"
    )
}

fn is_typescript_literal(word: &str) -> bool {
    matches!(word, "false" | "null" | "true" | "undefined")
}

fn is_typescript_builtin_type(word: &str) -> bool {
    matches!(
        word,
        "Array"
            | "Error"
            | "Promise"
            | "Record"
            | "boolean"
            | "number"
            | "object"
            | "string"
            | "void"
    )
}

fn is_typescript_punct(c: char) -> bool {
    matches!(
        c,
        '.' | '/'
            | '='
            | '+'
            | '-'
            | '*'
            | ':'
            | ','
            | ';'
            | '|'
            | '&'
            | '<'
            | '>'
            | '?'
            | '!'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
    )
}

fn highlight_rust(code: &str) -> String {
    let mut out = String::new();
    let mut rest = code;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with("//") {
            let len = rest.find('\n').unwrap_or(rest.len());
            push_span(&mut out, "comment", &rest[..len]);
            rest = &rest[len..];
        } else if let Some(len) = rust_raw_string_len(rest) {
            push_span(&mut out, "string", &rest[..len]);
            rest = &rest[len..];
        } else if c == '"' {
            let len = quoted_len(rest, '"');
            push_span(&mut out, "string", &rest[..len]);
            rest = &rest[len..];
        } else if c.is_ascii_digit() {
            let len = rust_number_len(rest);
            push_span(&mut out, "number", &rest[..len]);
            rest = &rest[len..];
        } else if is_rust_ident_start(c) {
            let len = rest
                .find(|d: char| !is_rust_ident_continue(d))
                .unwrap_or(rest.len());
            let word = &rest[..len];
            if is_rust_keyword(word) {
                push_span(&mut out, "keyword", word);
            } else if is_rust_literal(word) {
                push_span(&mut out, "literal", word);
            } else if is_rust_builtin_type(word) {
                push_span(&mut out, "key", word);
            } else {
                out.push_str(&escape_html(word));
            }
            rest = &rest[len..];
        } else if is_rust_punct(c) {
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

fn rust_raw_string_len(text: &str) -> Option<usize> {
    let mut chars = text.char_indices();
    if chars.next()?.1 != 'r' {
        return None;
    }

    let mut hashes = 0;
    let mut opening_quote = None;
    for (idx, c) in text.char_indices().skip(1) {
        if c == '#' {
            hashes += 1;
        } else if c == '"' {
            opening_quote = Some(idx);
            break;
        } else {
            return None;
        }
    }

    let opening_quote = opening_quote?;
    let closing = format!("\"{}", "#".repeat(hashes));
    text[opening_quote + 1..]
        .find(&closing)
        .map(|idx| opening_quote + 1 + idx + closing.len())
        .or(Some(text.len()))
}

fn rust_number_len(text: &str) -> usize {
    text.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.')))
        .unwrap_or(text.len())
}

fn is_rust_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_rust_ident_continue(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn is_rust_keyword(word: &str) -> bool {
    matches!(
        word,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}

fn is_rust_literal(word: &str) -> bool {
    matches!(word, "true" | "false" | "None" | "Some" | "Ok" | "Err")
}

fn is_rust_builtin_type(word: &str) -> bool {
    matches!(
        word,
        "bool"
            | "str"
            | "char"
            | "f32"
            | "f64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "Box"
            | "Duration"
            | "Error"
            | "Option"
            | "Result"
            | "String"
            | "Vec"
    )
}

fn is_rust_punct(c: char) -> bool {
    matches!(
        c,
        ':' | ';'
            | ','
            | '.'
            | '!'
            | '?'
            | '='
            | '+'
            | '-'
            | '*'
            | '/'
            | '&'
            | '|'
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
