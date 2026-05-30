use assert_cmd::Command;
use predicates::prelude::*;
use std::collections::BTreeSet;

#[test]
fn lints_basic_workspace() {
    let workspace = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", workspace.display());

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["workspace", "lint", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::eq(expected));
}

#[test]
fn lints_workspace_with_workspace_flag() {
    let workspace = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", workspace.display());

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["workspace", "lint", "--workspace", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::eq(expected));
}

#[test]
fn rejects_duplicate_workspace_inputs() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "workspace",
            "lint",
            "examples/basic",
            "--workspace",
            "examples/basic",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "pass workspace either as --workspace or as a positional argument, not both",
        ));
}

#[test]
fn lints_discovered_workspace() {
    let workspace = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", workspace.display());

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .args(["workspace", "lint"])
        .assert()
        .success()
        .stdout(predicate::eq(expected));
}

#[test]
fn reports_workspace_manifest_missing() {
    assert_lint_code(
        "tests/fixtures/workspaces/missing-manifest",
        "rototo/workspace-manifest-missing",
    );
}

#[test]
fn reports_workspace_manifest_parse_failed() {
    assert_lint_code(
        "tests/fixtures/workspaces/invalid-workspace-toml",
        "rototo/workspace-manifest-parse-failed",
    );
}

#[test]
fn reports_workspace_manifest_schema_failed() {
    assert_lint_code(
        "tests/fixtures/workspaces/missing-environments",
        "rototo/workspace-manifest-schema-failed",
    );
}

#[test]
fn reports_workspace_file_parse_failed() {
    assert_lint_code(
        "tests/fixtures/workspaces/invalid-workspace-file-toml",
        "rototo/workspace-toml-file-parse-failed",
    );
}

#[test]
fn reports_core_workspace_file_failures() {
    assert_lint_messages(
        "tests/fixtures/workspaces/lint-failures",
        &[
            "bucket range must satisfy 0 <= start < end <= 10000",
            r#""rule": "rototo/qualifier/predicate/bucket""#,
            "predicate references unknown qualifier: missing-qualifier",
            r#""rule": "rototo/qualifier/predicate/unknown-qualifier""#,
            "in predicate value must be a list",
            r#""rule": "rototo/qualifier/predicate/value""#,
            "gte predicate value must be a number",
            "predicate has unknown op: contains",
            r#""rule": "rototo/qualifier/predicate/unknown-op""#,
            "variable references undeclared environment: qa",
            r#""rule": "rototo/variable/env/unknown-environment""#,
            "environment references unknown value: missing-value",
            r#""rule": "rototo/variable/value/unknown""#,
            "rule references unknown qualifier: missing-qualifier",
            r#""rule": "rototo/variable/rule/unknown-qualifier""#,
            "rule references unknown value: another-missing-value",
            "schema is invalid:",
            r#""rule": "rototo/variable/schema/ref""#,
            "schemas/invalid-json.schema.json",
            r#""rule": "rototo/json-schema-file/parse-failed""#,
            "value broken does not match schema:",
            r#""rule": "rototo/variable/value/schema-mismatch""#,
            "value bad does not match type int",
            r#""rule": "rototo/variable/value/type-mismatch""#,
            "variable declares unknown type: currency",
            r#""rule": "rototo/variable/type/unknown""#,
            "custom lint rejected custom-lint",
            "custom value lint rejected custom-value-lint.default",
            "rototo/variable-custom-lint-failed",
            r#""rule": "rototo/variable/custom-lint/failed""#,
        ],
    );
}

#[test]
fn reports_context_schema_contract_failures() {
    assert_lint_messages(
        "tests/fixtures/workspaces/context-schema-attribute",
        &[
            "context schema does not declare attribute: account.plan",
            "rototo/workspace-context-schema-failed",
            r#""rule": "rototo/workspace/context-schema/attribute""#,
        ],
    );
}

#[test]
fn reports_malformed_context_schema_references() {
    assert_lint_messages(
        "tests/fixtures/workspaces/bad-context-config",
        &[
            "[context] must be a table",
            "rototo/workspace-context-schema-failed",
            r#""rule": "rototo/workspace/context-schema/ref""#,
        ],
    );
    assert_lint_messages(
        "tests/fixtures/workspaces/context-schema-missing-field",
        &["[context] must declare schema"],
    );
    assert_lint_messages(
        "tests/fixtures/workspaces/context-schema-empty-path",
        &["context schema path must be a relative path inside the workspace"],
    );
    assert_lint_messages(
        "tests/fixtures/workspaces/context-schema-missing-file",
        &["context schema could not be read:"],
    );
    assert_lint_messages(
        "tests/fixtures/workspaces/context-schema-invalid-json",
        &["context schema could not be parsed:"],
    );
    assert_lint_messages(
        "tests/fixtures/workspaces/context-schema-invalid-schema",
        &["context schema is invalid:"],
    );
}

#[test]
fn reports_unsafe_context_schema_paths() {
    assert_lint_messages(
        "tests/fixtures/workspaces/context-schema-path-escape",
        &[
            "context schema path must be a relative path inside the workspace",
            "rototo/workspace-context-schema-failed",
            r#""rule": "rototo/workspace/context-schema/ref""#,
        ],
    );
}

#[test]
fn covers_every_diagnostic_code_and_lint_rule() {
    let fixtures = [
        "tests/fixtures/workspaces/does-not-exist",
        "tests/fixtures/workspaces/missing-manifest",
        "tests/fixtures/workspaces/invalid-workspace-toml",
        "tests/fixtures/workspaces/missing-environments",
        "tests/fixtures/workspaces/invalid-workspace-file-toml",
        "tests/fixtures/workspaces/lint-failures",
        "tests/fixtures/workspaces/context-schema-attribute",
        "tests/fixtures/workspaces/context-schema-path-escape",
        "tests/fixtures/workspaces/rule-coverage",
    ];

    let mut codes = BTreeSet::new();
    let mut rules = BTreeSet::new();
    for fixture in fixtures {
        let lint = lint_json(fixture);
        for diagnostic in lint["diagnostics"].as_array().unwrap() {
            codes.insert(diagnostic["code"].as_str().unwrap().to_owned());
            if let Some(rule) = diagnostic.get("rule").and_then(serde_json::Value::as_str) {
                rules.insert(rule.to_owned());
            }
        }
    }

    assert_eq!(
        codes,
        BTreeSet::from([
            "rototo/json-schema-file-invalid".to_owned(),
            "rototo/json-schema-file-parse-failed".to_owned(),
            "rototo/variable-custom-lint-failed".to_owned(),
            "rototo/workspace-context-schema-failed".to_owned(),
            "rototo/workspace-manifest-missing".to_owned(),
            "rototo/workspace-manifest-parse-failed".to_owned(),
            "rototo/workspace-manifest-schema-failed".to_owned(),
            "rototo/workspace-not-found".to_owned(),
            "rototo/workspace-toml-file-invalid".to_owned(),
            "rototo/workspace-toml-file-parse-failed".to_owned(),
        ])
    );
    assert_eq!(
        rules,
        BTreeSet::from([
            "rototo/json-schema-file/parse-failed".to_owned(),
            "rototo/qualifier/missing-table".to_owned(),
            "rototo/qualifier/predicate/bucket".to_owned(),
            "rototo/qualifier/predicate/missing".to_owned(),
            "rototo/qualifier/predicate/shape".to_owned(),
            "rototo/qualifier/predicate/unknown-op".to_owned(),
            "rototo/qualifier/predicate/unknown-qualifier".to_owned(),
            "rototo/qualifier/predicate/value".to_owned(),
            "rototo/qualifier/schema-version".to_owned(),
            "rototo/schema/invalid".to_owned(),
            "rototo/variable/custom-lint/failed".to_owned(),
            "rototo/variable/env/missing-default".to_owned(),
            "rototo/variable/env/shape".to_owned(),
            "rototo/variable/env/unknown-environment".to_owned(),
            "rototo/variable/lint/shape".to_owned(),
            "rototo/variable/missing-table".to_owned(),
            "rototo/variable/rule/shape".to_owned(),
            "rototo/variable/rule/unknown-qualifier".to_owned(),
            "rototo/variable/schema-version".to_owned(),
            "rototo/variable/schema/ref".to_owned(),
            "rototo/variable/type-or-schema".to_owned(),
            "rototo/variable/type/unknown".to_owned(),
            "rototo/variable/value/schema-mismatch".to_owned(),
            "rototo/variable/value/type-mismatch".to_owned(),
            "rototo/variable/value/unknown".to_owned(),
            "rototo/variable/values/missing".to_owned(),
            "rototo/workspace/context-schema/attribute".to_owned(),
            "rototo/workspace/context-schema/ref".to_owned(),
            "rototo/workspace-file/toml-parse-failed".to_owned(),
            "rototo/workspace/manifest/missing".to_owned(),
            "rototo/workspace/manifest/parse-failed".to_owned(),
            "rototo/workspace/manifest/schema-failed".to_owned(),
            "rototo/workspace/not-found".to_owned(),
        ])
    );
}

fn assert_lint_code(workspace: &str, code: &str) {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["workspace", "lint", workspace, "--json"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(format!(r#""code": "{code}""#)));
}

fn lint_json(workspace: &str) -> serde_json::Value {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args(["workspace", "lint", workspace, "--json"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "fixture should produce diagnostics: {workspace}"
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_lint_messages(workspace: &str, messages: &[&str]) {
    let mut assertion = Command::cargo_bin("rototo")
        .unwrap()
        .args(["workspace", "lint", workspace, "--json"])
        .assert()
        .failure();

    for message in messages {
        assertion = assertion.stdout(predicate::str::contains(*message));
    }
}
