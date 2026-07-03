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
        .stdout(predicate::str::contains("variables"))
        .stdout(predicate::str::contains("model/catalogs"))
        .stdout(predicate::str::contains("data/catalogs"))
        .stdout(predicate::str::contains("model/context"))
        .stdout(predicate::str::contains("lint"));

    assert!(package.join("rototo-package.toml").is_file());
    assert!(package.join("variables").is_dir());
    assert!(package.join("model/catalogs").is_dir());
    assert!(package.join("data/catalogs").is_dir());
    assert!(package.join("model/context").is_dir());
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
            "max_output_tokens",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("rototo-package.toml"))
        .stdout(predicate::str::contains("variables/max_output_tokens.toml"));

    assert!(package.join("rototo-package.toml").is_file());
    assert!(package.join("variables").is_dir());
    assert!(package.join("model/catalogs").is_dir());
    assert!(package.join("data/catalogs").is_dir());
    assert!(package.join("model/context").is_dir());
    assert!(package.join("lint").is_dir());
    assert!(package.join("variables/max_output_tokens.toml").is_file());

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", package.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn init_variable_and_context_templates() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");
    init_package(&package);

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "init",
            package.to_str().unwrap(),
            "--variable",
            "premium_users",
        ])
        .assert()
        .success();

    fs::write(
        package.join("variables/premium_users.toml"),
        r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = 'context.user.tier == "premium"'
value = true
"#,
    )
    .unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["init", package.to_str().unwrap(), "--evaluation-context"])
        .assert()
        .success();

    let schema = read_json(package.join("model/context/evaluation.schema.json"));
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
            "checkout_redesign",
        ])
        .assert()
        .success();

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "init",
            package.to_str().unwrap(),
            "--catalog",
            "checkout_redesign",
        ])
        .assert()
        .success();

    let variable = fs::read_to_string(package.join("variables/checkout_redesign.toml")).unwrap();
    assert!(variable.contains("type = \"string\""));
    assert!(variable.contains("[resolve]"));
    assert!(variable.contains("bool, int, number, string"));
    assert!(variable.contains("context.account.plan == \"enterprise\""));
    assert!(variable.contains("query = 'entry.enabled == true"));
    assert!(!variable.contains("[env."));

    let catalog =
        fs::read_to_string(package.join("model/catalogs/checkout_redesign.schema.json")).unwrap();
    assert!(catalog.contains("\"$schema\""));
    assert!(
        package
            .join("data/catalogs/checkout_redesign/default.toml")
            .is_file()
    );

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", package.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn init_evaluation_context_accepts_explicit_id() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");
    init_package(&package);

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "init",
            package.to_str().unwrap(),
            "--evaluation-context",
            "request",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "model/context/request.schema.json",
        ));

    assert!(package.join("model/context/request.schema.json").is_file());
}

#[test]
fn init_rejects_invalid_evaluation_context_id() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "init",
            package.to_str().unwrap(),
            "--evaluation-context",
            "../request",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "evaluation context id must not start with '.'",
        ));
}

#[test]
fn init_context_infers_variable_paths_with_types() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");
    init_package(&package);

    fs::write(
        package.join("variables/premium_users.toml"),
        r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = 'context.user.tier == "premium"'
value = true
"#,
    )
    .unwrap();
    fs::write(
        package.join("variables/checkout_redesign.toml"),
        r#"schema_version = 1
type = "string"

[resolve]
default = "control"

[[resolve.rule]]
when = 'variables["premium_users"] && context.account.seats >= 10 && context.flags.enabled'
value = "treatment"
"#,
    )
    .unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["init", package.to_str().unwrap(), "--evaluation-context"])
        .assert()
        .success();

    let schema = read_json(package.join("model/context/evaluation.schema.json"));
    assert_eq!(
        schema["properties"]["user"]["properties"]["tier"]["type"],
        "string"
    );
    assert_eq!(
        schema["properties"]["account"]["properties"]["seats"]["type"],
        "number"
    );
    assert_eq!(
        schema["properties"]["flags"]["properties"]["enabled"]["type"],
        "boolean"
    );
}

#[test]
fn init_context_update_adds_missing_paths_and_reports_conflicts() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("config");
    init_package(&package);

    fs::write(
        package.join("variables/checkout_redesign.toml"),
        r#"schema_version = 1
type = "string"

[resolve]
default = "control"

[[resolve.rule]]
when = 'context.account.seats >= 10 && context.flags.enabled'
value = "treatment"
"#,
    )
    .unwrap();
    fs::write(
        package.join("model/context/evaluation.schema.json"),
        r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "account": {
      "type": "object",
      "description": "Preserved account contract",
      "additionalProperties": false,
      "properties": {
        "tier": {
          "type": "string",
          "enum": ["standard", "premium"]
        }
      }
    },
    "flags": {
      "type": "object",
      "properties": {
        "enabled": { "type": "string" }
      }
    }
  }
}
"#,
    )
    .unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["init", package.to_str().unwrap(), "--evaluation-context"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("file already exists"));

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "init",
            package.to_str().unwrap(),
            "--evaluation-context",
            "--update",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("context.account.seats"))
        .stdout(predicate::str::contains("context.flags.enabled"))
        .stdout(predicate::str::contains("conflict"));

    let schema = read_json(package.join("model/context/evaluation.schema.json"));
    assert_eq!(
        schema["properties"]["account"]["description"],
        "Preserved account contract"
    );
    assert_eq!(
        schema["properties"]["account"]["additionalProperties"],
        false
    );
    assert_eq!(
        schema["properties"]["account"]["properties"]["tier"]["enum"][1],
        "premium"
    );
    assert_eq!(
        schema["properties"]["account"]["properties"]["seats"]["type"],
        "number"
    );
    assert_eq!(
        schema["properties"]["flags"]["properties"]["enabled"]["type"],
        "string"
    );
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

fn read_json(path: impl AsRef<std::path::Path>) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}
