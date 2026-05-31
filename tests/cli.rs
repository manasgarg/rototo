use assert_cmd::Command;
use predicates::prelude::*;
use rototo::docs::DOCS;

#[test]
fn prints_version() {
    Command::cargo_bin("rototo")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("rototo 0.1.0-alpha.1"));
}

#[test]
fn quiet_suppresses_successful_lint_output() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["--quiet", "workspace", "lint", "examples/basic"])
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
            "workspace",
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
fn lists_bundled_docs() {
    let assert = Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "list"])
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
fn shows_bundled_docs_as_markdown() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "show", "cli"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# rototo CLI reference"));
}

#[test]
fn shows_bundled_docs_as_html() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "show", "cli", "--format", "html"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<!doctype html>"))
        .stdout(predicate::str::contains("<h1>rototo CLI reference</h1>"));
}

#[test]
fn exports_bundled_docs_as_static_html() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "export", "--out"])
        .arg(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("exported rototo docs"));

    for page in DOCS {
        let path = temp.path().join(format!("{}.html", page.id));
        assert!(
            path.exists(),
            "docs export did not create {}",
            path.display()
        );
    }
    assert!(temp.path().join("styles.css").exists());
}
