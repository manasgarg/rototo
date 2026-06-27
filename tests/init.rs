use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn init_creates_package_skeleton() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["init", package.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("package:"))
        .stdout(predicate::str::contains("rototo-package.toml"))
        .stdout(predicate::str::contains("qualifiers"))
        .stdout(predicate::str::contains("variables"))
        .stdout(predicate::str::contains("catalogs"))
        .stdout(predicate::str::contains("evaluation-contexts"))
        .stdout(predicate::str::contains("lint"));

    assert!(package.join("rototo-package.toml").is_file());
    assert!(package.join("qualifiers").is_dir());
    assert!(package.join("variables").is_dir());
    assert!(package.join("catalogs").is_dir());
    assert!(package.join("evaluation-contexts").is_dir());
    assert!(package.join("lint").is_dir());

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", package.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn init_entity_implicitly_creates_package_skeleton() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "init",
            package.to_str().unwrap(),
            "--variable",
            "max-output-tokens",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("rototo-package.toml"))
        .stdout(predicate::str::contains("variables/max-output-tokens.toml"));

    assert!(package.join("rototo-package.toml").is_file());
    assert!(package.join("qualifiers").is_dir());
    assert!(package.join("variables").is_dir());
    assert!(package.join("catalogs").is_dir());
    assert!(package.join("evaluation-contexts").is_dir());
    assert!(package.join("lint").is_dir());
    assert!(package.join("variables/max-output-tokens.toml").is_file());

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", package.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn init_qualifier_and_context_templates() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");
    init_package(&package);

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "init",
            package.to_str().unwrap(),
            "--qualifier",
            "premium-users",
        ])
        .assert()
        .success();

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["init", package.to_str().unwrap(), "--evaluation-context"])
        .assert()
        .success();

    let qualifier = fs::read_to_string(package.join("qualifiers/premium-users.toml")).unwrap();
    assert!(qualifier.contains("schema_version = 1"));
    assert!(qualifier.contains("when = \"context.user.tier == \\\"premium\\\"\""));
    assert!(qualifier.contains("context.request.country in"));
    assert!(qualifier.contains("bucket(context.user.id"));

    let schema: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(package.join("evaluation-contexts/request.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        schema["properties"]["user"]["properties"]["tier"]["type"],
        "string"
    );

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", package.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn init_variable_and_catalog_templates() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");
    init_package(&package);

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "init",
            package.to_str().unwrap(),
            "--variable",
            "checkout-redesign",
        ])
        .assert()
        .success();

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "init",
            package.to_str().unwrap(),
            "--catalog",
            "checkout-redesign",
        ])
        .assert()
        .success();

    let variable = fs::read_to_string(package.join("variables/checkout-redesign.toml")).unwrap();
    assert!(variable.contains("type = \"string\""));
    assert!(variable.contains("[resolve]"));
    assert!(!variable.contains("[env."));

    let catalog =
        fs::read_to_string(package.join("catalogs/checkout-redesign.schema.json")).unwrap();
    assert!(catalog.contains("\"$schema\""));
    assert!(
        package
            .join("catalogs/checkout-redesign-entries/default.toml")
            .is_file()
    );

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", package.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn init_refuses_to_overwrite_without_force() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");
    init_package(&package);

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["init", package.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("file already exists"));

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["init", package.to_str().unwrap(), "--force"])
        .assert()
        .success();
}

#[test]
fn init_json_dry_run_reports_planned_writes() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");

    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args(["--json", "init", package.to_str().unwrap(), "--dry-run"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!package.exists());

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["command"], "init");
    assert_eq!(report["dry_run"], true);
    assert!(
        report["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "rototo-package.toml" && file["action"] == "would_create")
    );
}

fn init_package(package: &std::path::Path) {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["init", package.to_str().unwrap()])
        .assert()
        .success();
}
