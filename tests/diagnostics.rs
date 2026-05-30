use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn lists_global_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["diagnostics", "list"])
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
        .args(["diagnostics", "list", "--workspace", "examples/basic"])
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
        .args(["diagnostics", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""scope": "global""#))
        .stdout(predicate::str::contains(r#""subject": "global""#))
        .stdout(predicate::str::contains(
            r#""rule": "rototo/workspace-not-found""#,
        ));
}

#[test]
fn gets_workspace_diagnostic() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["diagnostics", "get", "rototo/qualifier-parse-failed"])
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
            "diagnostics",
            "get",
            "consumer-experience/checkout-heading-required",
            "--workspace",
            "examples/basic",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "consumer-experience/checkout-heading-required",
        ))
        .stdout(predicate::str::contains("Checkout heading is missing"));
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
