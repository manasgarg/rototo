use assert_cmd::Command;
use predicates::prelude::*;
use std::collections::BTreeSet;

use rototo::diagnostics::RototoRuleId;

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
    assert_lint_rule(
        "tests/fixtures/workspaces/missing-manifest",
        "rototo/workspace-manifest-missing",
    );
}

#[test]
fn reports_workspace_manifest_parse_failed() {
    assert_lint_rule(
        "tests/fixtures/workspaces/invalid-workspace-toml",
        "rototo/workspace-manifest-parse-failed",
    );
}

#[test]
fn reports_workspace_manifest_schema_failed() {
    assert_lint_rule(
        "tests/fixtures/workspaces/missing-environments",
        "rototo/workspace-manifest-schema-failed",
    );
}

#[test]
fn reports_workspace_file_parse_failed() {
    assert_lint_rule(
        "tests/fixtures/workspaces/invalid-workspace-file-toml",
        "rototo/qualifier-parse-failed",
    );
    assert_lint_rule(
        "tests/fixtures/workspaces/invalid-workspace-file-toml",
        "rototo/variable-parse-failed",
    );
}

#[test]
fn reports_core_workspace_file_failures() {
    assert_lint_messages(
        "tests/fixtures/workspaces/lint-failures",
        &[
            "bucket range must satisfy 0 <= start < end <= 10000",
            r#""rule": "rototo/qualifier-predicate-bucket""#,
            "predicate references unknown qualifier: missing-qualifier",
            r#""rule": "rototo/qualifier-predicate-unknown-qualifier""#,
            "in predicate value must be a list",
            r#""rule": "rototo/qualifier-predicate-value""#,
            "gte predicate value must be a number",
            "predicate has unknown op: contains",
            r#""rule": "rototo/qualifier-predicate-unknown-op""#,
            "variable references undeclared environment: qa",
            r#""rule": "rototo/variable-unknown-environment""#,
            "environment references unknown value: missing-value",
            r#""rule": "rototo/variable-unknown-value""#,
            "rule references unknown qualifier: missing-qualifier",
            r#""rule": "rototo/variable-rule-unknown-qualifier""#,
            "rule references unknown value: another-missing-value",
            "schema is invalid:",
            r#""rule": "rototo/variable-schema-ref""#,
            "schemas/invalid-json.schema.json",
            r#""rule": "rototo/schema-parse-failed""#,
            "value broken does not match schema:",
            r#""rule": "rototo/variable-value-schema-mismatch""#,
            "value bad does not match type int",
            r#""rule": "rototo/variable-value-type-mismatch""#,
            "variable declares unknown type: currency",
            r#""rule": "rototo/variable-unknown-type""#,
            "variable value is declared more than once: default",
            r#""rule": "rototo/variable-external-value-duplicate""#,
            "failed to parse",
            r#""rule": "rototo/variable-external-value-parse-failed""#,
            "variable values must be a table",
            r#""rule": "rototo/variable-external-values-load-failed""#,
            "custom lint rejected custom-lint",
            "custom value lint rejected custom-value-lint.default",
            r#""rule": "fixture/custom-variable-rejected""#,
            r#""rule": "fixture/custom-value-rejected""#,
        ],
    );
}

#[test]
fn reports_custom_lint_contract_failures() {
    assert_lint_messages(
        "tests/fixtures/workspaces/custom-lint-contract",
        &[
            r#""rule": "payments/max-token-budget""#,
            "custom lint emitted invalid rule rototo/not-allowed",
            r#""rule": "rototo/custom-lint-invalid-rule""#,
            "custom lint emitted undeclared rule: payments/undeclared-rule",
            r#""rule": "rototo/custom-lint-unknown-rule""#,
            "custom lint rule metadata conflicts: billing/conflicting-rule",
            r#""rule": "rototo/custom-lint-rule-conflict""#,
            "script failed for custom-failed",
            r#""rule": "rototo/custom-lint-failed""#,
        ],
    );
}

#[test]
fn reports_context_schema_contract_failures() {
    assert_lint_messages(
        "tests/fixtures/workspaces/context-schema-attribute",
        &[
            "context schema does not declare attribute: account.plan",
            r#""rule": "rototo/workspace-context-schema-attribute""#,
        ],
    );
}

#[test]
fn reports_malformed_context_schema_references() {
    assert_lint_messages(
        "tests/fixtures/workspaces/bad-context-config",
        &[
            "[context] must be a table",
            r#""rule": "rototo/workspace-context-schema-ref""#,
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
            r#""rule": "rototo/workspace-context-schema-ref""#,
        ],
    );
}

#[test]
fn covers_every_rototo_rule_with_targeted_fixtures() {
    let targets = [
        (
            RototoRuleId::WorkspaceNotFound,
            "tests/fixtures/workspaces/does-not-exist",
        ),
        (
            RototoRuleId::WorkspaceManifestMissing,
            "tests/fixtures/workspaces/missing-manifest",
        ),
        (
            RototoRuleId::WorkspaceManifestParseFailed,
            "tests/fixtures/workspaces/invalid-workspace-toml",
        ),
        (
            RototoRuleId::WorkspaceManifestSchemaFailed,
            "tests/fixtures/workspaces/missing-environments",
        ),
        (
            RototoRuleId::WorkspaceContextSchemaRef,
            "tests/fixtures/workspaces/context-schema-path-escape",
        ),
        (
            RototoRuleId::WorkspaceContextSchemaAttribute,
            "tests/fixtures/workspaces/context-schema-attribute",
        ),
        (
            RototoRuleId::QualifierParseFailed,
            "tests/fixtures/workspaces/invalid-workspace-file-toml",
        ),
        (
            RototoRuleId::QualifierSchemaVersion,
            "tests/fixtures/workspaces/rule-coverage",
        ),
        (
            RototoRuleId::QualifierPredicateMissing,
            "tests/fixtures/workspaces/rule-coverage",
        ),
        (
            RototoRuleId::QualifierPredicateShape,
            "tests/fixtures/workspaces/rule-coverage",
        ),
        (
            RototoRuleId::QualifierPredicateUnknownOp,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::QualifierPredicateUnknownQualifier,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::QualifierPredicateBucket,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::QualifierPredicateValue,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::VariableParseFailed,
            "tests/fixtures/workspaces/invalid-workspace-file-toml",
        ),
        (
            RototoRuleId::VariableSchemaVersion,
            "tests/fixtures/workspaces/rule-coverage",
        ),
        (
            RototoRuleId::VariableTypeOrSchema,
            "tests/fixtures/workspaces/rule-coverage",
        ),
        (
            RototoRuleId::VariableUnknownType,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::VariableLintShape,
            "tests/fixtures/workspaces/rule-coverage",
        ),
        (
            RototoRuleId::VariableValuesMissing,
            "tests/fixtures/workspaces/rule-coverage",
        ),
        (
            RototoRuleId::VariableUnknownValue,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::VariableValueTypeMismatch,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::VariableValueSchemaMismatch,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::VariableSchemaRef,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::VariableEnvMissingDefault,
            "tests/fixtures/workspaces/rule-coverage",
        ),
        (
            RototoRuleId::VariableUnknownEnvironment,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::VariableEnvShape,
            "tests/fixtures/workspaces/rule-coverage",
        ),
        (
            RototoRuleId::VariableRuleShape,
            "tests/fixtures/workspaces/rule-coverage",
        ),
        (
            RototoRuleId::VariableRuleUnknownQualifier,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::VariableExternalValuesLoadFailed,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::VariableExternalValueParseFailed,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::VariableExternalValueDuplicate,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::CustomLintFailed,
            "tests/fixtures/workspaces/custom-lint-contract",
        ),
        (
            RototoRuleId::CustomLintInvalidRule,
            "tests/fixtures/workspaces/custom-lint-contract",
        ),
        (
            RototoRuleId::CustomLintUnknownRule,
            "tests/fixtures/workspaces/custom-lint-contract",
        ),
        (
            RototoRuleId::CustomLintRuleConflict,
            "tests/fixtures/workspaces/custom-lint-contract",
        ),
        (
            RototoRuleId::SchemaParseFailed,
            "tests/fixtures/workspaces/lint-failures",
        ),
        (
            RototoRuleId::SchemaInvalid,
            "tests/fixtures/workspaces/lint-failures",
        ),
    ];

    let mut covered = BTreeSet::new();
    for (rule, fixture) in targets {
        let rule = rule.meta().rule;
        assert_workspace_emits_rule(fixture, rule);
        covered.insert(rule.to_owned());
    }

    assert_eq!(
        covered,
        RototoRuleId::iter()
            .map(|rule| rule.meta().rule.to_owned())
            .collect()
    );
}

fn assert_lint_rule(workspace: &str, rule: &str) {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["workspace", "lint", workspace, "--json"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(format!(r#""rule": "{rule}""#)));
}

fn assert_workspace_emits_rule(workspace: &str, rule: &str) {
    let lint = lint_json(workspace);
    let rules: BTreeSet<_> = lint["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|diagnostic| diagnostic["rule"].as_str())
        .collect();
    assert!(rules.contains(rule), "{workspace} did not emit {rule}");
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
