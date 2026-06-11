use std::collections::BTreeMap;
use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};
use regex::Regex;

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
const MARK_SVG: &str = include_str!("../docs/theme/rototo-mark.svg");
const WORDMARK_SVG: &str = include_str!("../docs/theme/rototo-wordmark.svg");
pub const DEFAULT_DOCS_BASE_URL: &str = "https://docs.rototo.dev";
const HIGHLIGHT_JS_VERSION: &str = "11.9.0";

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
    let (markdown, toc_items) = prepare_markdown_for_html(page.markdown);
    let body = render_markdown(&markdown);
    let toc = render_toc(&toc_items);

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
  <a class="brand" href="index.html"><img class="brand-wordmark" src="assets/rototo-wordmark.svg" alt="rototo"></a>
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
  <aside class="tree sidenav" aria-label="Documentation">
{nav}
  </aside>
  <main class="doc">
    <div class="crumb">{section}</div>
{body}{page_nav}  </main>
{toc}
</div>
{language_script}
{highlight_script}
</body>
</html>
"#,
        title = escape_html(page.title),
        fonts = GOOGLE_FONTS_HREF,
        topnav = render_topnav(page.id),
        section = section,
        nav = nav,
        body = body,
        page_nav = render_page_nav(page.id),
        toc = toc,
        language_script = render_sdk_language_script(),
        highlight_script = render_syntax_highlight_script(),
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
        ("rototo-mark.svg", MARK_SVG),
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
    render_package_readme_with_base_url(sdk, DEFAULT_DOCS_BASE_URL)
}

pub fn render_package_readme_with_base_url(sdk: &str, docs_base_url: &str) -> Result<String> {
    let docs_base_url = normalize_docs_base_url(docs_base_url)?;
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
    markdown = rewrite_package_readme_doc_links(&markdown, docs_base_url);
    Ok(format!(
        "<!-- Generated from docs/src/{page_id}.md by `rototo docs --package-readme {sdk} --out sdks/{sdk}/README.md`. Do not edit directly. -->\n\n{markdown}",
        page_id = page.id,
    ))
}

fn normalize_docs_base_url(docs_base_url: &str) -> Result<&str> {
    let docs_base_url = docs_base_url.trim().trim_end_matches('/');
    if docs_base_url.is_empty() {
        return Err(RototoError::new("docs base URL must not be blank"));
    }
    Ok(docs_base_url)
}

fn rewrite_package_readme_doc_links(markdown: &str, docs_base_url: &str) -> String {
    let link =
        Regex::new(r"\[([^\]\n]+)\]\(([^)\s]+)\)").expect("documentation link regex is valid");
    link.replace_all(markdown, |captures: &regex::Captures<'_>| {
        let text = captures.get(1).expect("capture exists").as_str();
        let target = captures.get(2).expect("capture exists").as_str();
        if let Some((page_id, anchor)) = internal_doc_link_target(target) {
            format!(
                "[{text}]({docs_base_url}/{}{})",
                page_href(page_id),
                anchor.unwrap_or("")
            )
        } else {
            captures.get(0).expect("capture exists").as_str().to_owned()
        }
    })
    .into_owned()
}

fn internal_doc_link_target(target: &str) -> Option<(&'static str, Option<&str>)> {
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with('#')
    {
        return None;
    }
    let (target, anchor) = match target.find('#') {
        Some(index) => (&target[..index], Some(&target[index..])),
        None => (target, None),
    };
    let file_name = Path::new(target).file_name()?.to_str()?;
    let id = file_name
        .strip_suffix(".md")
        .or_else(|| file_name.strip_suffix(".html"))?;
    DOCS.iter()
        .find(|page| page.id == id)
        .map(|page| (page.id, anchor))
}

#[derive(Debug)]
struct TocItem {
    level: u8,
    id: String,
    title: String,
}

fn prepare_markdown_for_html(markdown: &str) -> (String, Vec<TocItem>) {
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

fn render_toc(items: &[TocItem]) -> String {
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

fn render_sdk_language_options() -> String {
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
    options
}

fn render_sdk_language_picker() -> String {
    format!(
        r#"<label class="sdk-language-picker sdk-snippet-picker"><span>SDK</span><select class="sdk-language-select" aria-label="SDK language for this code sample">{options}</select></label>"#,
        options = render_sdk_language_options(),
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
  var selects = Array.prototype.slice.call(document.querySelectorAll(".sdk-language-select"));
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
    selects.forEach(function(select) {{
      select.value = language;
    }});
  }}
  setLanguage(preferred());
  selects.forEach(function(select) {{
    select.addEventListener("change", function(event) {{
      setLanguage(event.target.value);
    }});
  }});
}})();
</script>"#
    )
}

fn render_syntax_highlight_script() -> String {
    format!(
        r#"<script src="https://unpkg.com/@highlightjs/cdn-assets@{version}/highlight.min.js"></script>
<script src="https://unpkg.com/@highlightjs/cdn-assets@{version}/languages/bash.min.js"></script>
<script src="https://unpkg.com/@highlightjs/cdn-assets@{version}/languages/gradle.min.js"></script>
<script>
(function() {{
  if (!window.hljs) {{
    return;
  }}
  if (window.hljs.getLanguage("bash")) {{
    window.hljs.registerAliases(["sh", "shell"], {{ languageName: "bash" }});
  }}
  if (window.hljs.getLanguage("ini")) {{
    window.hljs.registerAliases(["toml"], {{ languageName: "ini" }});
  }}
  window.hljs.configure({{ ignoreUnescapedHTML: true }});
  window.hljs.highlightAll();
}})();
</script>"#,
        version = HIGHLIGHT_JS_VERSION,
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
