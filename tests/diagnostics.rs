use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn lists_global_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["diagnostics", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("code"))
        .stdout(predicate::str::contains("source"))
        .stdout(predicate::str::contains("rototo/workspace-not-found"))
        .stdout(predicate::str::contains(
            "rototo/workspace-toml-file-parse-failed",
        ))
        .stdout(predicate::str::contains(
            "Workspace TOML file could not be parsed",
        ))
        .stdout(predicate::str::contains("help:").not());
}

#[test]
fn lists_workspace_scoped_diagnostics_when_requested() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["diagnostics", "list", "--workspace", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "rototo/workspace-toml-file-parse-failed",
        ))
        .stdout(predicate::str::contains("help:").not());
}

#[test]
fn lists_global_diagnostics_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["diagnostics", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""scope": "global""#))
        .stdout(predicate::str::contains(r#""subject": "global""#))
        .stdout(predicate::str::contains(
            r#""code": "rototo/workspace-not-found""#,
        ));
}

#[test]
fn gets_workspace_diagnostic() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "diagnostics",
            "get",
            "rototo/workspace-toml-file-parse-failed",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "rototo/workspace-toml-file-parse-failed",
        ))
        .stdout(predicate::str::contains("source: kernel"));
}

#[test]
fn missing_diagnostic_fails() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["diagnostics", "get", "rototo/missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "diagnostic not found: rototo/missing",
        ));
}
