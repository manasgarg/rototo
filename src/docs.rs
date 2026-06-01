use std::net::SocketAddr;
use std::path::Path;

use pulldown_cmark::{Options, Parser, html};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

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

pub const DOCS: &[DocPage] = &[
    DocPage {
        id: "index",
        title: "rototo documentation",
        markdown: include_str!("../docs/src/index.md"),
    },
    DocPage {
        id: "why-rototo",
        title: "Why rototo",
        markdown: include_str!("../docs/src/concepts/why-rototo.md"),
    },
    DocPage {
        id: "model",
        title: "The rototo Model",
        markdown: include_str!("../docs/src/concepts/model.md"),
    },
    DocPage {
        id: "quickstart",
        title: "Quickstart",
        markdown: include_str!("../docs/src/tutorials/quickstart.md"),
    },
    DocPage {
        id: "production-workflow",
        title: "Production Workflow",
        markdown: include_str!("../docs/src/tutorials/production-workflow.md"),
    },
    DocPage {
        id: "how-to-add-a-new-runtime-config-value",
        title: "How to Add a New Runtime Config Value",
        markdown: include_str!("../docs/src/how-to/how-to-add-a-new-runtime-config-value.md"),
    },
    DocPage {
        id: "how-to-change-a-config-value-for-one-environment",
        title: "How to Change a Config Value for One Environment",
        markdown: include_str!(
            "../docs/src/how-to/how-to-change-a-config-value-for-one-environment.md"
        ),
    },
    DocPage {
        id: "how-to-add-a-new-context-field",
        title: "How to Add a New Context Field",
        markdown: include_str!("../docs/src/how-to/how-to-add-a-new-context-field.md"),
    },
    DocPage {
        id: "how-to-select-a-value-for-a-runtime-condition",
        title: "How to Select a Value for a Runtime Condition",
        markdown: include_str!(
            "../docs/src/how-to/how-to-select-a-value-for-a-runtime-condition.md"
        ),
    },
    DocPage {
        id: "how-to-move-large-values-out-of-toml",
        title: "How to Move Large Values Into Resources",
        markdown: include_str!("../docs/src/how-to/how-to-move-large-values-out-of-toml.md"),
    },
    DocPage {
        id: "how-to-test-a-config-change-before-merge",
        title: "How to Test a Config Change Before Merge",
        markdown: include_str!("../docs/src/how-to/how-to-test-a-config-change-before-merge.md"),
    },
    DocPage {
        id: "how-to-enforce-a-config-policy",
        title: "How to Enforce a Config Policy",
        markdown: include_str!("../docs/src/how-to/how-to-enforce-a-config-policy.md"),
    },
    DocPage {
        id: "how-to-load-config-from-a-git-repo-in-an-app",
        title: "How to Load Config from a Git Repo in an App",
        markdown: include_str!(
            "../docs/src/how-to/how-to-load-config-from-a-git-repo-in-an-app.md"
        ),
    },
    DocPage {
        id: "how-to-keep-config-fresh-in-a-running-app",
        title: "How to Keep Config Fresh in a Running App",
        markdown: include_str!("../docs/src/how-to/how-to-keep-config-fresh-in-a-running-app.md"),
    },
    DocPage {
        id: "how-to-investigate-why-a-value-was-selected",
        title: "How to Investigate Why a Value Was Selected",
        markdown: include_str!("../docs/src/how-to/how-to-investigate-why-a-value-was-selected.md"),
    },
    DocPage {
        id: "how-to-diagnose-a-failing-workspace",
        title: "How to Diagnose a Failing Workspace",
        markdown: include_str!("../docs/src/how-to/how-to-diagnose-a-failing-workspace.md"),
    },
    DocPage {
        id: "example-environment-specific-limits",
        title: "Example: Keep Deployment-Lane Limits Out of Application Code",
        markdown: include_str!("../docs/src/examples/example-environment-specific-limits.md"),
    },
    DocPage {
        id: "example-reviewed-account-class",
        title: "Example: Select Behavior for a Reviewed Account Class",
        markdown: include_str!("../docs/src/examples/example-reviewed-account-class.md"),
    },
    DocPage {
        id: "example-llm-agent-configuration",
        title: "Example: Control Structured LLM Agent Config Safely",
        markdown: include_str!("../docs/src/examples/example-llm-agent-configuration.md"),
    },
    DocPage {
        id: "example-tenant-specific-runtime-config",
        title: "Example: Manage Tenant Exceptions Without App Branches",
        markdown: include_str!("../docs/src/examples/example-tenant-specific-runtime-config.md"),
    },
    DocPage {
        id: "example-incident-banner",
        title: "Example: Ship an Operational Override Without Redeploying",
        markdown: include_str!("../docs/src/examples/example-incident-banner.md"),
    },
    DocPage {
        id: "example-bucketed-rollout",
        title: "Example: Run a Stable Percentage Rollout from Config",
        markdown: include_str!("../docs/src/examples/example-bucketed-rollout.md"),
    },
    DocPage {
        id: "workspace-manifest-reference",
        title: "Workspace Manifest Reference",
        markdown: include_str!("../docs/src/reference/workspace-manifest-reference.md"),
    },
    DocPage {
        id: "qualifier-reference",
        title: "Qualifier File Reference",
        markdown: include_str!("../docs/src/reference/qualifier-reference.md"),
    },
    DocPage {
        id: "variable-reference",
        title: "Variable File Reference",
        markdown: include_str!("../docs/src/reference/variable-reference.md"),
    },
    DocPage {
        id: "resource-reference",
        title: "Resource Reference",
        markdown: include_str!("../docs/src/reference/resource-reference.md"),
    },
    DocPage {
        id: "predicate-reference",
        title: "Predicate Reference",
        markdown: include_str!("../docs/src/reference/predicate-reference.md"),
    },
    DocPage {
        id: "context-reference",
        title: "Context Reference",
        markdown: include_str!("../docs/src/reference/context-reference.md"),
    },
    DocPage {
        id: "environment-reference",
        title: "Environment Reference",
        markdown: include_str!("../docs/src/reference/environment-reference.md"),
    },
    DocPage {
        id: "value-types-reference",
        title: "Value Types Reference",
        markdown: include_str!("../docs/src/reference/value-types-reference.md"),
    },
    DocPage {
        id: "source-uri-reference",
        title: "Source URI Reference",
        markdown: include_str!("../docs/src/reference/source-uri-reference.md"),
    },
    DocPage {
        id: "cli",
        title: "rototo CLI reference",
        markdown: include_str!("../docs/src/api/cli.md"),
    },
    DocPage {
        id: "sdk",
        title: "rototo Rust SDK",
        markdown: include_str!("../docs/src/api/sdk.md"),
    },
    DocPage {
        id: "diagnostics",
        title: "Diagnostic reference",
        markdown: include_str!("../docs/src/api/diagnostics.md"),
    },
    DocPage {
        id: "json-output-reference",
        title: "JSON Output Reference",
        markdown: include_str!("../docs/src/reference/json-output-reference.md"),
    },
];

pub const DOC_NAV_SECTIONS: &[DocNavSection] = &[
    DocNavSection {
        title: "Start",
        pages: &["index"],
    },
    DocNavSection {
        title: "Concepts",
        pages: &["why-rototo", "model"],
    },
    DocNavSection {
        title: "Tutorials",
        pages: &["quickstart", "production-workflow"],
    },
    DocNavSection {
        title: "How-to: Authoring",
        pages: &[
            "how-to-add-a-new-runtime-config-value",
            "how-to-change-a-config-value-for-one-environment",
            "how-to-add-a-new-context-field",
            "how-to-select-a-value-for-a-runtime-condition",
            "how-to-move-large-values-out-of-toml",
        ],
    },
    DocNavSection {
        title: "How-to: Validation",
        pages: &[
            "how-to-test-a-config-change-before-merge",
            "how-to-enforce-a-config-policy",
        ],
    },
    DocNavSection {
        title: "How-to: Application",
        pages: &[
            "how-to-load-config-from-a-git-repo-in-an-app",
            "how-to-keep-config-fresh-in-a-running-app",
        ],
    },
    DocNavSection {
        title: "How-to: Operations",
        pages: &[
            "how-to-investigate-why-a-value-was-selected",
            "how-to-diagnose-a-failing-workspace",
        ],
    },
    DocNavSection {
        title: "Examples",
        pages: &[
            "example-environment-specific-limits",
            "example-reviewed-account-class",
            "example-llm-agent-configuration",
            "example-tenant-specific-runtime-config",
            "example-incident-banner",
            "example-bucketed-rollout",
        ],
    },
    DocNavSection {
        title: "Reference",
        pages: &[
            "workspace-manifest-reference",
            "qualifier-reference",
            "variable-reference",
            "resource-reference",
            "predicate-reference",
            "context-reference",
            "environment-reference",
            "value-types-reference",
            "source-uri-reference",
        ],
    },
    DocNavSection {
        title: "API",
        pages: &["cli", "sdk", "diagnostics", "json-output-reference"],
    },
];

const STYLE_CSS: &str = r#":root {
  color-scheme: light dark;
  --bg: #f8f8f5;
  --fg: #1f2328;
  --muted: #5f6670;
  --border: #d9d9d2;
  --panel: #ffffff;
  --link: #0b63ce;
  --code: #eff1f3;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #17191c;
    --fg: #e8e8e3;
    --muted: #a8adb5;
    --border: #34383f;
    --panel: #202328;
    --link: #75a7ff;
    --code: #2a2e35;
  }
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  background: var(--bg);
  color: var(--fg);
  font: 16px/1.6 system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

.layout {
  display: grid;
  grid-template-columns: 240px minmax(0, 760px);
  gap: 48px;
  max-width: 1120px;
  margin: 0 auto;
  padding: 40px 24px 64px;
}

nav {
  position: sticky;
  top: 24px;
  align-self: start;
  border-right: 1px solid var(--border);
  padding-right: 24px;
}

.nav-section + .nav-section {
  margin-top: 18px;
}

.nav-section-title {
  margin-bottom: 5px;
  color: var(--muted);
  font-size: 0.78rem;
  font-weight: 700;
  letter-spacing: 0;
  text-transform: uppercase;
}

nav a {
  display: block;
  padding: 3px 0;
  color: var(--link);
  font-size: 0.92rem;
  line-height: 1.35;
  text-decoration: none;
}

nav a[aria-current="page"] {
  color: var(--fg);
  font-weight: 650;
}

main {
  min-width: 0;
}

h1, h2, h3 {
  line-height: 1.25;
}

h1 {
  margin-top: 0;
}

a {
  color: var(--link);
}

code {
  border-radius: 4px;
  background: var(--code);
  padding: 0.1em 0.25em;
}

pre {
  overflow-x: auto;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--panel);
  padding: 16px;
}

pre code {
  background: transparent;
  padding: 0;
}

@media (max-width: 800px) {
  .layout {
    display: block;
    padding: 24px 18px 48px;
  }

  nav {
    position: static;
    border-right: 0;
    border-bottom: 1px solid var(--border);
    margin-bottom: 28px;
    padding: 0 0 18px;
  }
}
"#;

pub fn get_page(id: &str) -> Result<&'static DocPage> {
    let id = normalize_page_id(id);
    DOCS.iter()
        .find(|page| page.id == id)
        .ok_or_else(|| RototoError::new(format!("unknown documentation page: {id}")))
}

pub fn render_page_html(page: &DocPage) -> String {
    let mut body = String::new();
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    html::push_html(&mut body, Parser::new_ext(page.markdown, options));

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="stylesheet" href="styles.css">
</head>
<body>
<div class="layout">
<nav aria-label="Documentation">
{nav}
</nav>
<main>
{body}
</main>
</div>
</body>
</html>
"#,
        title = escape_html(page.title),
        nav = render_nav(page.id),
    )
}

pub async fn export_html(out: &Path) -> Result<()> {
    tokio::fs::create_dir_all(out).await.map_err(|err| {
        RototoError::new(format!(
            "failed to create documentation directory {}: {err}",
            out.display()
        ))
    })?;
    tokio::fs::write(out.join("styles.css"), STYLE_CSS)
        .await
        .map_err(|err| RototoError::new(format!("failed to write styles.css: {err}")))?;
    for page in DOCS {
        let file_name = if page.id == "index" {
            "index.html".to_owned()
        } else {
            format!("{}.html", page.id)
        };
        tokio::fs::write(out.join(file_name), render_page_html(page))
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

pub async fn serve(addr: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|err| RototoError::new(format!("failed to bind {addr}: {err}")))?;
    let local_addr = listener
        .local_addr()
        .map_err(|err| RototoError::new(format!("failed to read local address: {err}")))?;
    println!("serving rototo docs at http://{local_addr}/");

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("failed to accept docs connection: {err}");
                continue;
            }
        };
        tokio::spawn(async move {
            let _ = handle_connection(stream).await;
        });
    }
}

async fn handle_connection(mut stream: TcpStream) -> Result<()> {
    let mut buffer = [0_u8; 8192];
    let read = stream
        .read(&mut buffer)
        .await
        .map_err(|err| RototoError::new(format!("failed to read request: {err}")))?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let response = response_for_request(&request);
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|err| RototoError::new(format!("failed to write response: {err}")))?;
    Ok(())
}

fn response_for_request(request: &str) -> String {
    let Some(request_line) = request.lines().next() else {
        return http_response(
            "400 Bad Request",
            "text/plain; charset=utf-8",
            "bad request",
        );
    };
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();

    if method != "GET" && method != "HEAD" {
        return http_response(
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            "method not allowed",
        );
    }

    let route = route(path);
    let (content_type, body) = match route {
        Route::Style => ("text/css; charset=utf-8", STYLE_CSS.to_owned()),
        Route::Page(page) => ("text/html; charset=utf-8", render_page_html(page)),
        Route::NotFound => {
            let body = "not found";
            return if method == "HEAD" {
                http_head_response("404 Not Found", "text/plain; charset=utf-8", body.len())
            } else {
                http_response("404 Not Found", "text/plain; charset=utf-8", body)
            };
        }
    };
    if method == "HEAD" {
        http_head_response("200 OK", content_type, body.len())
    } else {
        http_response("200 OK", content_type, &body)
    }
}

enum Route {
    Style,
    Page(&'static DocPage),
    NotFound,
}

fn route(path: &str) -> Route {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    if path == "/styles.css" {
        return Route::Style;
    }
    let raw_id = match path {
        "/" | "/index.html" | "/index" => "index",
        _ => path.trim_start_matches('/').trim_end_matches('/'),
    };
    let id = raw_id.strip_suffix(".html").unwrap_or(raw_id);
    DOCS.iter()
        .find(|page| page.id == id)
        .map_or(Route::NotFound, Route::Page)
}

fn http_head_response(status: &str, content_type: &str, content_length: usize) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n"
    )
}

fn http_response(status: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn render_nav(current: &str) -> String {
    let mut nav = String::new();
    for section in DOC_NAV_SECTIONS {
        nav.push_str(&format!(
            r#"<div class="nav-section">
<div class="nav-section-title">{title}</div>
"#,
            title = escape_html(section.title),
        ));
        for page_id in section.pages {
            let page = DOCS
                .iter()
                .find(|page| page.id == *page_id)
                .expect("documentation navigation references an unknown page");
            let href = if page.id == "index" {
                "index.html".to_owned()
            } else {
                format!("{}.html", page.id)
            };
            let current_attr = if page.id == current {
                r#" aria-current="page""#
            } else {
                ""
            };
            nav.push_str(&format!(
                r#"<a href="{href}"{current_attr}>{title}</a>
"#,
                title = escape_html(page.title),
            ));
        }
        nav.push_str("</div>\n");
    }
    nav
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_uses_get_content_length_without_body() {
        let get = response_for_request("GET /cli.html HTTP/1.1\r\n\r\n");
        let head = response_for_request("HEAD /cli.html HTTP/1.1\r\n\r\n");

        let get_length = header_value(&get, "Content-Length").unwrap();
        let head_length = header_value(&head, "Content-Length").unwrap();

        assert_eq!(head_length, get_length);
        assert!(head.ends_with("\r\n\r\n"));
    }

    #[test]
    fn trailing_slash_html_routes_to_page() {
        let response = response_for_request("GET /cli.html/ HTTP/1.1\r\n\r\n");

        assert!(response.starts_with("HTTP/1.1 200 OK"));
    }

    #[test]
    fn unsupported_docs_routes_return_404() {
        let response = response_for_request("GET /missing.html HTTP/1.1\r\n\r\n");

        assert!(response.starts_with("HTTP/1.1 404 Not Found"));
    }

    fn header_value<'a>(response: &'a str, name: &str) -> Option<&'a str> {
        response
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}: ")))
    }
}
