use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use rototo::docs::DOCS;

#[test]
fn prints_version() {
    Command::cargo_bin("rototo")
        .unwrap()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "rototo {}",
            env!("CARGO_PKG_VERSION")
        )));
}

#[test]
fn top_level_help_is_task_oriented() {
    Command::cargo_bin("rototo")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Workspace commands"))
        .stdout(predicate::str::contains("Utility commands"))
        .stdout(predicate::str::contains("lint"))
        .stdout(predicate::str::contains("rototo docs -p index"))
        .stdout(predicate::str::contains("rototo help workspace-sources").not())
        .stdout(predicate::str::contains("git+https://").not());
}

#[test]
fn custom_help_topics_are_not_supported() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["help", "context"])
        .assert()
        .failure();
}

#[test]
fn quiet_suppresses_successful_lint_output() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["--quiet", "lint", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::eq(""));
}

#[test]
fn quiet_keeps_lint_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["--quiet", "lint", "tests/fixtures/workspaces/lint-failures"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("error[rototo/"));
}

#[test]
fn old_noun_commands_are_removed() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["workspace", "lint", "examples/basic"])
        .assert()
        .failure();
}

#[test]
fn generates_zsh_completions() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef rototo"));
}

#[test]
fn exposes_lsp_command() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lsp", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Run the rototo Language Server Protocol server over stdio",
        ));
}

#[test]
fn resolve_help_omits_lint_selectors() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["resolve", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--variable"))
        .stdout(predicate::str::contains("--qualifier"))
        .stdout(predicate::str::contains("--context"))
        .stdout(predicate::str::contains("--lint-rule").not())
        .stdout(predicate::str::contains("--lint-authority").not())
        .stdout(predicate::str::contains("--linter").not());
}

#[test]
fn resolve_rejects_lint_selectors_at_parse_time() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--lint-rule",
            "consumer-experience/message-not-empty",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--lint-rule"));
}

#[test]
fn lists_bundled_docs() {
    let assert = Command::cargo_bin("rototo")
        .unwrap()
        .arg("docs")
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    for page in DOCS {
        assert!(
            stdout.contains(page.id),
            "docs list did not include page id `{}`",
            page.id
        );
    }
}

#[test]
fn shows_bundled_docs_by_prefix_as_markdown() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "-p", "index"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "rototo is a control plane for runtime configuration",
        ))
        .stdout(predicate::str::contains(
            "refresh (rototo docs -p reference-sdk-refresh)",
        ))
        .stdout(predicate::str::contains("[refresh](reference-sdk-refresh.html)").not());
}

#[test]
fn exports_bundled_docs_as_static_site() {
    let temp = tempfile::tempdir().unwrap();
    let site = temp.path().join("site");

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "--export", site.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("exported documentation to"));

    let index = fs::read_to_string(site.join("index.html")).unwrap();
    assert!(index.contains("<!doctype html>"));
    assert!(index.contains("rototo is a control plane for runtime configuration"));
    assert!(index.contains(r#"<header class="topbar">"#));
    assert!(
        index.contains(
            r#"<img class="brand-wordmark" src="assets/rototo-wordmark.svg" alt="rototo">"#
        )
    );
    assert!(index.contains(r#"<aside class="tree sidenav" aria-label="Documentation">"#));
    assert!(index.contains(r#"<aside class="toc" aria-label="On this page">"#));
    assert!(index.contains(r#"<h2 id="why-rototo-exists">Why rototo exists</h2>"#));
    assert!(index.contains(r#"<nav class="page-nav" aria-label="Page">"#));
    assert!(site.join("getting-started.html").is_file());
    assert!(site.join("operational-switches.html").is_file());
    assert!(site.join("incident-banner.html").is_file());
    assert!(site.join("onboarding-checklist.html").is_file());
    assert!(site.join("bucketed-rollout.html").is_file());
    assert!(site.join("notification-delivery-policy.html").is_file());
    assert!(site.join("service-degradation-policy.html").is_file());
    assert!(site.join("workspace-layering.html").is_file());
    assert!(site.join("reference-workspace-manifest.html").is_file());
    assert!(site.join("reference-workspace-layout.html").is_file());
    assert!(site.join("reference-workspace-sources.html").is_file());
    assert!(site.join("reference-workspace-layering.html").is_file());
    assert!(site.join("reference-context.html").is_file());
    assert!(site.join("reference-qualifiers.html").is_file());
    assert!(site.join("reference-predicate-operators.html").is_file());
    assert!(site.join("reference-variables.html").is_file());
    assert!(site.join("reference-variable-values.html").is_file());
    assert!(site.join("reference-resources.html").is_file());
    assert!(site.join("reference-qualifier-resolution.html").is_file());
    assert!(site.join("reference-variable-resolution.html").is_file());
    assert!(site.join("reference-resolution-output.html").is_file());
    assert!(site.join("reference-cli-overview.html").is_file());
    assert!(site.join("reference-cli-commands.html").is_file());
    assert!(site.join("reference-sdk-loading.html").is_file());
    assert!(site.join("reference-sdk-resolution.html").is_file());
    assert!(site.join("reference-sdk-refresh.html").is_file());
    assert!(site.join("reference-sdk-rust.html").is_file());
    assert!(site.join("reference-sdk-python.html").is_file());
    assert!(site.join("reference-sdk-typescript.html").is_file());
    assert!(site.join("reference-sdk-java.html").is_file());
    assert!(site.join("reference-sdk-go.html").is_file());
    assert!(site.join("reference-lint-overview.html").is_file());
    assert!(site.join("reference-diagnostics.html").is_file());
    assert!(site.join("reference-custom-lua-lint.html").is_file());
    assert!(site.join("reference-json-output.html").is_file());
    assert!(site.join("modeling-runtime-configuration.html").is_file());
    assert!(site.join("application-integration.html").is_file());
    assert!(site.join("testing-runtime-configuration.html").is_file());
    assert!(site.join("operating-runtime-configuration.html").is_file());
    assert!(site.join("production-workflow.html").is_file());
    assert!(site.join("assets/rototo-docs.css").is_file());
    assert!(site.join("assets/favicon.svg").is_file());
    assert!(site.join("assets/rototo-mark.svg").is_file());
    assert!(site.join("assets/rototo-wordmark.svg").is_file());

    let css = fs::read_to_string(site.join("assets/rototo-docs.css")).unwrap();
    assert!(css.contains("text-size-adjust: 100%"));
    assert!(css.contains("--sea-500: oklch(0.572 0.148 178);"));
    assert!(css.contains("--cyan-500: oklch(0.640 0.152 205);"));
    assert!(css.contains("--ok-500: oklch(0.625 0.165 150);"));
    assert!(css.contains("--term-violet"));
    assert!(css.contains("--text-6xl"));
    assert!(css.contains("--leading-normal"));
    assert!(css.contains("--shadow-4"));
    assert!(css.contains("--grid-line"));
    assert!(css.contains("--ct-keyword"));
    assert!(css.contains(".doc :not(pre) > code"));
    assert!(css.contains(".hljs-keyword"));
    assert!(css.contains(".hljs-title.function_"));
    assert!(css.contains(".hljs-string"));
    assert!(css.contains(".hljs-addition"));
    assert!(css.contains(".token.comment"));
    assert!(css.contains(".token.inserted-sign"));
    assert!(!css.contains(".sx-"));
    assert!(!css.contains("--clay-500"));
    assert!(!css.contains(".doc pre.language-text"));

    let app_page = fs::read_to_string(site.join("application-integration.html")).unwrap();
    assert!(app_page.contains(r#"<pre class="code-block language-rust sdk-snippet""#));
    assert!(app_page.contains(r#"<pre class="code-block language-python sdk-snippet""#));
    assert!(app_page.contains(r#"<pre class="code-block language-typescript sdk-snippet""#));
    assert!(app_page.contains(r#"<pre class="code-block language-java sdk-snippet""#));
    assert!(app_page.contains(r#"<pre class="code-block language-go sdk-snippet""#));
    assert!(app_page.contains(r#"<code class="language-rust">use rototo::"#));
    assert!(app_page.contains(
        r#"<code class="language-typescript">import { Workspace } from &quot;rototo&quot;;"#
    ));
    assert!(app_page.contains(r#"<code class="language-plaintext">ROTOTO_WORKSPACE_SOURCE="#));
    assert!(app_page.contains("ROTOTO_WORKSPACE_SOURCE"));
    assert!(!app_page.contains(r#"<span class="sx-"#));

    let getting_started_page = fs::read_to_string(site.join("getting-started.html")).unwrap();
    assert!(getting_started_page.contains(
        r#"<pre class="code-block language-sh"><code class="language-bash">cargo install rototo"#
    ));
    assert!(getting_started_page.contains(
        r#"<pre class="code-block language-toml"><code class="language-ini">schema_version = 1"#
    ));
    assert!(!getting_started_page.contains(r#"<span class="sx-"#));

    let wordmark = fs::read_to_string(site.join("assets/rototo-wordmark.svg")).unwrap();
    assert!(wordmark.contains("#006252"));
    assert!(!wordmark.contains("#008572"));

    let favicon = fs::read_to_string(site.join("assets/favicon.svg")).unwrap();
    assert!(favicon.contains("#006252"));
    assert!(!favicon.contains("#008572"));

    let sdk_page = fs::read_to_string(site.join("reference-sdk-resolution.html")).unwrap();
    assert!(sdk_page.contains("https://unpkg.com/@highlightjs/cdn-assets@11.9.0/highlight.min.js"));
    assert!(
        sdk_page.contains("https://unpkg.com/@highlightjs/cdn-assets@11.9.0/languages/bash.min.js")
    );
    assert!(
        sdk_page
            .contains("https://unpkg.com/@highlightjs/cdn-assets@11.9.0/languages/gradle.min.js")
    );
    assert!(
        sdk_page
            .contains(r#"window.hljs.registerAliases(["sh", "shell"], { languageName: "bash" });"#)
    );
    assert!(sdk_page.contains("window.hljs.highlightAll();"));
    assert!(!sdk_page.contains(r#"id="sdk-language""#));
    assert!(sdk_page.contains(r#"<div class="sdk-snippet-toolbar">"#));
    assert!(sdk_page.contains(
        r#"<select class="sdk-language-select" aria-label="SDK language for this code sample">"#
    ));
    assert!(sdk_page.contains(r#"data-sdk-lang="rust""#));
    assert!(sdk_page.contains(r#"data-sdk-lang="python""#));
    assert!(sdk_page.contains(r#"data-sdk-lang="typescript""#));
    assert!(sdk_page.contains(r#"data-sdk-lang="java""#));
    assert!(sdk_page.contains(r#"data-sdk-lang="go""#));
    assert!(sdk_page.contains(r#"<pre class="code-block language-python sdk-snippet""#));
    assert!(sdk_page.contains(r#"<pre class="code-block language-typescript sdk-snippet""#));
    assert!(sdk_page.contains(r#"<pre class="code-block language-java sdk-snippet""#));
    assert!(sdk_page.contains(r#"<pre class="code-block language-go sdk-snippet""#));
    assert!(sdk_page.contains(r#"<code class="language-typescript">const context = {"#));
    assert!(sdk_page.contains("await workspace.resolveVariable"));
    assert!(!sdk_page.contains(r#"<p><span class="sx-"#));
    assert!(!sdk_page.contains(r#"<span class="sx-"#));
}

#[test]
fn generates_python_package_readme_from_docs() {
    let temp = tempfile::tempdir().unwrap();
    let readme = temp.path().join("README.md");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "docs",
            "--package-readme",
            "python",
            "--out",
            readme.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated python package README"));

    let actual = fs::read_to_string(readme).unwrap();
    let expected = rototo::docs::render_package_readme("python").unwrap();
    assert_eq!(actual, expected);
    assert!(actual.contains("https://docs.rototo.dev/reference-workspace-sources.html"));
}

#[test]
fn generates_typescript_package_readme_from_docs() {
    let temp = tempfile::tempdir().unwrap();
    let readme = temp.path().join("README.md");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "docs",
            "--package-readme",
            "typescript",
            "--out",
            readme.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "generated typescript package README",
        ));

    let actual = fs::read_to_string(readme).unwrap();
    let expected = rototo::docs::render_package_readme("typescript").unwrap();
    assert_eq!(actual, expected);
    assert!(actual.contains("https://docs.rototo.dev/reference-workspace-sources.html"));
}

#[test]
fn generates_package_readme_with_custom_docs_base_url() {
    let temp = tempfile::tempdir().unwrap();
    let readme = temp.path().join("README.md");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "docs",
            "--package-readme",
            "python",
            "--docs-base-url",
            "https://docs.example.test/base/",
            "--out",
            readme.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated python package README"));

    let actual = fs::read_to_string(readme).unwrap();
    let expected = rototo::docs::render_package_readme_with_base_url(
        "python",
        "https://docs.example.test/base/",
    )
    .unwrap();
    assert_eq!(actual, expected);
    assert!(actual.contains("https://docs.example.test/base/reference-workspace-sources.html"));
    assert!(!actual.contains("https://docs.rototo.dev/"));
}

#[test]
fn generates_java_package_readme_from_docs() {
    let temp = tempfile::tempdir().unwrap();
    let readme = temp.path().join("README.md");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "docs",
            "--package-readme",
            "java",
            "--out",
            readme.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated java package README"));

    let actual = fs::read_to_string(readme).unwrap();
    let expected = rototo::docs::render_package_readme("java").unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn generates_go_package_readme_from_docs() {
    let temp = tempfile::tempdir().unwrap();
    let readme = temp.path().join("README.md");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "docs",
            "--package-readme",
            "go",
            "--out",
            readme.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated go package README"));

    let actual = fs::read_to_string(readme).unwrap();
    let expected = rototo::docs::render_package_readme("go").unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn docs_page_prefix_reports_unknown_page() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "-p", "missing-page"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("documentation page not found"));
}

#[test]
fn docs_search_uses_regex() {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "-s", "workspace source"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\x1b[7mworkspace source\x1b[0m"));
    assert!(
        !stdout
            .lines()
            .any(|line| line.trim_start().starts_with('^')),
        "docs search should highlight matches inline instead of printing marker lines:\n{stdout}"
    );
}

#[test]
fn docs_search_rejects_invalid_regex() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "-s", "["])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "invalid documentation search regex",
        ));
}
