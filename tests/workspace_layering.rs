use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

const CHILD: &str = "examples/layered/team-payments";

fn inspect_json(workspace: &str) -> Value {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", workspace, "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

fn variable<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["variables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|variable| variable["id"] == id)
        .unwrap_or_else(|| panic!("variable {id} not found in composed workspace"))
}

#[test]
fn composed_workspace_lints_clean() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", CHILD])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ok:"));
}

#[test]
fn inspect_reports_layer_provenance() {
    let report = inspect_json(CHILD);
    let layers = report["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 2, "expected base + child layers: {report:#}");
    // base is first, most-derived child is last.
    assert!(
        layers[0]["source"].as_str().unwrap().contains("base"),
        "base layer should come first: {layers:#?}"
    );
    assert!(
        layers[1]["source"]
            .as_str()
            .unwrap()
            .contains("team-payments"),
        "child layer should come last: {layers:#?}"
    );
}

#[test]
fn non_layered_workspace_reports_no_layers() {
    let report = inspect_json("examples/layered/base");
    assert!(
        report.get("layers").is_none(),
        "a single-layer workspace should omit `layers`: {report:#}"
    );
}

#[test]
fn child_inherits_parent_entities() {
    let report = inspect_json(CHILD);
    // welcome-banner and premium-users come only from the base layer.
    variable(&report, "welcome-banner");
    let qualifier_ids: Vec<&str> = report["qualifiers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|qualifier| qualifier["id"].as_str().unwrap())
        .collect();
    assert!(
        qualifier_ids.contains(&"premium-users"),
        "{qualifier_ids:?}"
    );
    assert!(
        qualifier_ids.contains(&"high-value-cart"),
        "{qualifier_ids:?}"
    );
}

#[test]
fn child_adds_new_entities() {
    let report = inspect_json(CHILD);
    variable(&report, "payment-retries");
}

#[test]
fn child_overrides_parent_entity_wholesale() {
    // The child's checkout-discount replaces the base file: premium == 15, and
    // the new `launch` value exists, neither of which is in the base.
    let report = inspect_json(CHILD);
    let checkout = variable(&report, "checkout-discount");
    let values: std::collections::BTreeMap<&str, i64> = checkout["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| {
            (
                value["key"].as_str().unwrap(),
                value["value"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(values.get("premium"), Some(&15));
    assert_eq!(values.get("launch"), Some(&20));
}

#[test]
fn child_overrides_environment_set() {
    let report = inspect_json(CHILD);
    let environments: Vec<&str> = report["environments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|environment| environment.as_str().unwrap())
        .collect();
    assert!(environments.contains(&"canary"), "{environments:?}");
}

#[test]
fn resolves_overridden_value_from_child() {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            CHILD,
            "--variable",
            "checkout-discount",
            "--env",
            "prod",
            "--context",
            "user.tier=premium",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    let resolution = &report["variables"].as_array().unwrap()[0]["resolution"];
    assert_eq!(resolution["value"].as_i64(), Some(15));
}

#[test]
fn rejects_extends_cycle() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", "tests/fixtures/workspaces/layering/cycle-a"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cycle"));
}

#[test]
fn rejects_schema_version_mismatch() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "lint",
            "tests/fixtures/workspaces/layering/bad-version-child",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("schema_version"));
}

#[test]
fn rejects_missing_parent() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "lint",
            "tests/fixtures/workspaces/layering/missing-parent-child",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does-not-exist"));
}

#[test]
fn rejects_non_string_extends() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "lint",
            "tests/fixtures/workspaces/layering/extends-not-string",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a string"));
}
