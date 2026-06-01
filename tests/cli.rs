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
        .stdout(predicate::str::contains("rototo 0.1.0-alpha.1"));
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
        .stdout(predicate::str::contains(
            "rototo docs -p source-uri-reference",
        ))
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
        .args([
            "--quiet",
            "lint",
            "tests/fixtures/workspaces/missing-environments",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "error[rototo/workspace-manifest-schema-failed]",
        ));
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
        .args(["docs", "-p", "cli"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# rototo CLI reference"));
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
    assert!(index.contains("rototo introduction"));
    assert!(index.contains(r#"<header class="topbar">"#));
    assert!(index.contains(r#"<aside class="sidenav" aria-label="Documentation">"#));
    assert!(index.contains(r#"<nav class="page-nav" aria-label="Page">"#));
    assert!(site.join("cli-reference.html").is_file());
    assert!(site.join("assets/rototo-docs.css").is_file());
    assert!(site.join("assets/favicon.svg").is_file());
    assert!(site.join("assets/rototo-wordmark.svg").is_file());

    let quickstart = fs::read_to_string(site.join("quickstart.html")).unwrap();
    assert!(quickstart.contains(r#"<pre class="code-block language-toml">"#));
    assert!(quickstart.contains(r#"<span class="sx-section">[environments]</span>"#));
    assert!(quickstart.contains(r#"<span class="sx-key">schema_version</span>"#));
}

#[test]
fn docs_page_prefix_reports_ambiguity() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "-p", "how-to"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "multiple documentation pages match",
        ))
        .stdout(predicate::str::contains("rototo docs -p"));
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
