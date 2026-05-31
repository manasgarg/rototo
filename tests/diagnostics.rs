use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn lists_global_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "--lint-rules"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rule"))
        .stdout(predicate::str::contains("entity"))
        .stdout(predicate::str::contains("rototo/workspace-not-found"))
        .stdout(predicate::str::contains("rototo/qualifier-parse-failed"))
        .stdout(predicate::str::contains(
            "Qualifier TOML file could not be parsed",
        ))
        .stdout(predicate::str::contains("help:").not());
}

#[test]
fn lists_workspace_scoped_diagnostics_when_requested() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--lint-rules"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rototo/qualifier-parse-failed"))
        .stdout(predicate::str::contains(
            "consumer-experience/checkout-heading-required",
        ))
        .stdout(predicate::str::contains("help:").not());
}

#[test]
fn lists_global_diagnostics_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "--lint-rules", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""scope": "global""#))
        .stdout(predicate::str::contains(r#""subject": "global""#))
        .stdout(predicate::str::contains(
            r#""rule": "rototo/workspace-not-found""#,
        ))
        .stdout(predicate::str::contains(
            r#""rule": "rototo/qualifier-unreferenced""#,
        ))
        .stdout(predicate::str::contains(r#""severity": "warning""#));
}

#[test]
fn retired_rototo_rules_are_not_listed() {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "--lint-rules", "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "diagnostics list failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let rules = diagnostics["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["rule"].as_str().unwrap())
        .collect::<Vec<_>>();

    for retired in [
        "rototo/variable-lint-shape",
        "rototo/qualifier-missing-table",
        "rototo/variable-missing-table",
    ] {
        assert!(
            !rules.contains(&retired),
            "retired rule is listed: {retired}"
        );
    }
}

#[test]
fn gets_workspace_diagnostic() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "--lint-rule", "rototo/qualifier-parse-failed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rototo/qualifier-parse-failed"))
        .stdout(predicate::str::contains("entity: qualifier"));
}

#[test]
fn gets_workspace_custom_diagnostic() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "show",
            "examples/basic",
            "--lint-rule",
            "consumer-experience/checkout-heading-required",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "consumer-experience/checkout-heading-required",
        ))
        .stdout(predicate::str::contains("Checkout heading is missing"));
}

#[test]
fn lists_workspace_level_custom_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "show",
            "tests/fixtures/workspaces/custom-targets",
            "--lint-rules",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("targets/workspace-environments"))
        .stdout(predicate::str::contains("targets/schema-json"));
}

#[test]
fn lists_workspace_custom_warning_severity() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "show",
            "tests/fixtures/workspaces/custom-warning",
            "--lint-rules",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""rule": "policy/advisory""#))
        .stdout(predicate::str::contains(r#""severity": "warning""#));
}

#[test]
fn lists_custom_lint_example_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/custom-lint", "--lint-rules"])
        .assert()
        .success()
        .stdout(predicate::str::contains("operations/message-not-empty"));
}

#[test]
fn missing_diagnostic_fails() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "--lint-rule", "rototo/missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "diagnostic not found: rototo/missing",
        ));
}
