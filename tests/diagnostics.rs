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
        .stdout(predicate::str::contains("rototo/package-not-found"))
        .stdout(predicate::str::contains("rototo/variable-parse-failed"))
        .stdout(predicate::str::contains(
            "Variable TOML file could not be parsed",
        ))
        .stdout(predicate::str::contains("help:").not());
}

#[test]
fn lists_package_scoped_diagnostics_when_requested() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--lint-rules"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rototo/variable-parse-failed"))
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
            r#""rule": "rototo/package-not-found""#,
        ))
        .stdout(predicate::str::contains(
            r#""rule": "rototo/variable-rule-shadowed""#,
        ))
        .stdout(predicate::str::contains(r#""severity": "warning""#));
}

#[test]
fn gets_package_diagnostic() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "--lint-rule", "rototo/variable-parse-failed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rototo/variable-parse-failed"))
        .stdout(predicate::str::contains("entity: variable"));
}

#[test]
fn gets_package_custom_diagnostic() {
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
fn lists_package_level_custom_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "show",
            "tests/fixtures/packages/custom-targets",
            "--lint-rules",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("targets/variable-type"))
        .stdout(predicate::str::contains("targets/package-extends"));
}

#[test]
fn lists_package_custom_warning_severity() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "show",
            "tests/fixtures/packages/custom-warning",
            "--lint-rules",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""rule": "policy/advisory""#))
        .stdout(predicate::str::contains(r#""severity": "warning""#));
}

#[test]
fn custom_diagnostic_catalog_entries_do_not_claim_variable_entity() {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "show",
            "tests/fixtures/packages/custom-targets",
            "--lint-rules",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let catalog: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let custom_rule = catalog["lint_rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["rule"] == "targets/variable-type")
        .unwrap();

    assert!(custom_rule.get("entity").is_none(), "{custom_rule:#}");
}

#[test]
fn lists_custom_lint_example_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/custom_lint", "--lint-rules"])
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
