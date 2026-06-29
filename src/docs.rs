use std::path::Path;

use crate::error::{Result, RototoError};

mod markdown;
mod readme;

use markdown::{prepare_markdown_for_html, render_code_block, render_markdown, render_toc};
pub use readme::{render_package_readme, render_package_readme_with_base_url};

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
        id: "motivation",
        title: "Introducing Rototo",
        markdown: include_str!("../docs/src/motivation.md"),
    },
    DocPage {
        id: "concepts",
        title: "Rototo Concepts",
        markdown: include_str!("../docs/src/concepts.md"),
    },
    DocPage {
        id: "development-workflow",
        title: "Development Workflow",
        markdown: include_str!("../docs/src/development-workflow.md"),
    },
    DocPage {
        id: "production-workflow",
        title: "Production Workflow",
        markdown: include_str!("../docs/src/production-workflow.md"),
    },
];

pub const DOC_NAV_SECTIONS: &[DocNavSection] = &[
    DocNavSection {
        title: "Start",
        pages: &["motivation"],
    },
    DocNavSection {
        title: "Learn",
        pages: &["concepts", "development-workflow", "production-workflow"],
    },
];

/// Design system stylesheet and brand assets vendored under `docs/theme/`.
const DOCS_CSS: &str = include_str!("../docs/theme/rototo-docs.css");
const FAVICON_SVG: &str = include_str!("../docs/theme/favicon.svg");
const MARK_SVG: &str = include_str!("../docs/theme/rototo-mark.svg");
const WORDMARK_SVG: &str = include_str!("../docs/theme/rototo-wordmark.svg");
pub const DEFAULT_DOCS_BASE_URL: &str = "https://docs.rototo.dev";
const HIGHLIGHT_JS_VERSION: &str = "11.9.0";
const DOCS_ENTRY_PAGE: &str = "motivation";

/// Brand fonts referenced by the stylesheet: Manrope for display headings,
/// Hanken Grotesk for body text, and JetBrains Mono for code and labels.
const GOOGLE_FONTS_HREF: &str = "https://fonts.googleapis.com/css2?family=Hanken+Grotesk:ital,wght@0,400..700;1,400..700&family=JetBrains+Mono:ital,wght@0,400..700;1,400..700&family=Manrope:wght@400..800&display=swap";

/// Top navigation bar entries as (label, href relative to the docs pages).
/// The homepage lives one level above the docs directory.
const TOPNAV_LINKS: &[(&str, &str)] = &[("Home", "../index.html"), ("Docs", "motivation.html")];

/// The rototo GitHub repository, linked from the public site.
const GITHUB_URL: &str = "https://github.com/manasgarg/rototo";

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
  <a class="brand" href="../index.html"><img class="brand-wordmark" src="assets/rototo-wordmark.svg" alt="rototo"></a>
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

/// Exports the public site: the homepage at the root and the documentation
/// under docs/, each with its own copy of the shared assets so every page
/// keeps relative links and the export stays browsable from file://.
pub async fn export_html(out: &Path) -> Result<()> {
    let docs_dir = out.join("docs");
    for assets in [out.join("assets"), docs_dir.join("assets")] {
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
    }
    for page in DOCS {
        tokio::fs::write(docs_dir.join(page_href(page.id)), render_page_html(page))
            .await
            .map_err(|err| {
                RototoError::new(format!(
                    "failed to write documentation page {}: {err}",
                    page.id
                ))
            })?;
    }
    tokio::fs::write(out.join("index.html"), render_homepage_html())
        .await
        .map_err(|err| RototoError::new(format!("failed to write homepage: {err}")))?;
    tokio::fs::write(out.join("_redirects"), render_redirects())
        .await
        .map_err(|err| RototoError::new(format!("failed to write redirects: {err}")))?;
    Ok(())
}

/// Cloudflare Pages redirects: documentation used to live at the site root,
/// so every old page URL forwards to its docs/ location.
fn render_redirects() -> String {
    let mut redirects = String::new();
    for page in DOCS {
        if page.id == "index" {
            continue;
        }
        redirects.push_str(&format!(
            "/{href} /docs/{href} 301\n",
            href = page_href(page.id)
        ));
    }
    redirects
}

/// The rototo.dev homepage. Copy here is an initial draft pending the
/// product content outline; structure (hero, model, snippet, SDKs, console)
/// is the stable part.
pub fn render_homepage_html() -> String {
    let snippet = render_code_block(
        "toml",
        r#"# variables/checkout-redesign.toml
schema_version = 1
type = "string"

[resolve]
default = "classic"

[[resolve.rule]]
when = 'env.qualifier["premium-users"]'
value = "redesign"
"#,
    );
    let resolve_snippet = render_code_block(
        "sh",
        r#"rototo resolve git+https://github.com/acme/config#main \
  --variable checkout-redesign \
  --context user.tier=premium
"#,
    );

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>rototo — Git-backed runtime configuration</title>
<meta name="description" content="rototo keeps runtime configuration in a Git package: linted, reviewed in pull requests, and resolved at runtime with typed values.">
<link rel="icon" href="assets/favicon.svg" type="image/svg+xml">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="{fonts}" rel="stylesheet">
<link rel="stylesheet" href="assets/rototo-docs.css">
<style>
.home {{ max-width: 64rem; margin: 0 auto; padding: 0 1.5rem 4rem; }}
.home-hero {{ padding: 4.5rem 0 3rem; }}
.home-hero h1 {{ font-size: clamp(2rem, 5vw, 3.1rem); line-height: 1.1; margin: 0 0 1rem; max-width: 36rem; }}
.home-hero p {{ font-size: 1.1rem; max-width: 40rem; }}
.home-cta {{ display: flex; gap: 0.75rem; flex-wrap: wrap; margin-top: 1.75rem; }}
.home-cta a {{ display: inline-block; padding: 0.6rem 1.1rem; border-radius: 8px; text-decoration: none; font-weight: 600; }}
.home-cta .primary {{ background: var(--ink, #16242c); color: var(--paper, #fdfbf7); }}
.home-cta .secondary {{ border: 1px solid currentColor; }}
.home-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(15rem, 1fr)); gap: 1.25rem; margin: 2.5rem 0; }}
.home-card {{ border: 1px solid rgba(22, 36, 44, 0.14); border-radius: 12px; padding: 1.25rem; }}
.home-card h3 {{ margin-top: 0; }}
.home-split {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(20rem, 1fr)); gap: 1.25rem; align-items: start; }}
.home h2 {{ margin-top: 3rem; }}
.home-sdks {{ display: flex; gap: 0.6rem; flex-wrap: wrap; margin-top: 1rem; }}
.home-sdks span {{ border: 1px solid rgba(22, 36, 44, 0.2); border-radius: 999px; padding: 0.35rem 0.9rem; font-weight: 600; }}
.home-footer {{ margin-top: 4rem; padding-top: 1.5rem; border-top: 1px solid rgba(22, 36, 44, 0.14); display: flex; gap: 1.25rem; flex-wrap: wrap; }}
</style>
</head>
<body>
<header class="topbar">
  <a class="brand" href="index.html"><img class="brand-wordmark" src="assets/rototo-wordmark.svg" alt="rototo"></a>
  <nav class="topnav" aria-label="Primary">
    <a href="docs/motivation.html">Docs</a>
    <a href="{github}">GitHub</a>
  </nav>
</header>
<main class="home">
  <section class="home-hero">
    <h1>Runtime configuration, reviewed like code.</h1>
    <p>
      rototo keeps your application's runtime configuration in a Git package:
      validated by lint, changed through pull requests, and resolved at runtime
      into typed values your services can trust. No config database, no side
      channel around review — the repository is the control plane.
    </p>
    <div class="home-cta">
      <a class="primary" href="docs/motivation.html">Read the docs</a>
      <a class="secondary" href="docs/concepts.html">Concepts</a>
    </div>
  </section>

  <section>
    <h2>One package, three guarantees</h2>
    <div class="home-grid">
      <div class="home-card">
        <h3>Declared</h3>
        <p>
          Variables, qualifiers, and JSON Schemas live as files under
          <code>rototo-package.toml</code>. Every change has an author, a
          diff, and a history.
        </p>
      </div>
      <div class="home-card">
        <h3>Validated</h3>
        <p>
          Lint understands the package semantically: unknown qualifiers,
          values that break their schema, and rules that can never match are
          caught before merge, not in production.
        </p>
      </div>
      <div class="home-card">
        <h3>Resolved</h3>
        <p>
          Applications load the package by source URI and resolve named
          variables with runtime context. Long-running services refresh from
          the same source and keep last-known-good state when a fetch fails.
        </p>
      </div>
    </div>
  </section>

  <section>
    <h2>What it looks like</h2>
    <div class="home-split">
      <div>{snippet}</div>
      <div>{resolve_snippet}</div>
    </div>
  </section>

  <section>
    <h2>SDKs that share one engine</h2>
    <p>
      The Rust core owns loading, lint, evaluation, and refresh; every SDK is a
      thin binding over it, so resolution behaves identically in every
      language.
    </p>
    <div class="home-sdks">
      <span>Rust</span>
      <span>Python</span>
      <span>TypeScript</span>
      <span>Java</span>
      <span>Go</span>
    </div>
  </section>

  <section>
    <h2>Operate it from the console</h2>
    <p>
      <code>rototo console</code> serves a web console from the same binary as
      the CLI: browse packages, trace how a variable resolves against saved
      contexts, edit review branches, and publish pull requests. Run it
      on your laptop with your own GitHub token, or behind your proxy with
      GitHub OAuth for the whole team.
    </p>
    <p><a href="docs/concepts.html">Read the concepts →</a></p>
  </section>

  <footer class="home-footer">
    <a href="docs/motivation.html">Documentation</a>
    <a href="{github}">GitHub</a>
    <span>MIT or Apache-2.0</span>
  </footer>
</main>
</body>
</html>
"#,
        fonts = GOOGLE_FONTS_HREF,
        github = GITHUB_URL,
    )
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
    for (label, href) in TOPNAV_LINKS {
        let current_attr = if *label == "Docs" && current == DOCS_ENTRY_PAGE {
            r#" aria-current="page""#
        } else {
            ""
        };
        nav.push_str(&format!(
            "    <a href=\"{href}\"{current_attr}>{label}</a>\n"
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
        "" | "/" | "index" | "index.html" => DOCS_ENTRY_PAGE,
        _ => id.strip_suffix(".html").unwrap_or(id),
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
