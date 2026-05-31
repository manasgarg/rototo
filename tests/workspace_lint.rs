use assert_cmd::Command;
use predicates::prelude::*;
use rototo::diagnostics::{LintStage, RototoRuleId};
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
fn lints_basic_workspace_as_json_with_documents() {
    let lint = lint_json("examples/basic", true);

    assert!(lint["diagnostics"].as_array().unwrap().is_empty());
    assert!(document_paths(&lint).contains(&"rototo-workspace.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"qualifiers/premium-users.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"variables/checkout-redesign.toml".to_owned()));
    assert!(
        document_paths(&lint)
            .contains(&"variables/directory-backed-message-values/control.toml".to_owned())
    );
    assert!(document_paths(&lint).contains(&"schemas/context.schema.json".to_owned()));
}

#[test]
fn lints_curated_examples() {
    for workspace in [
        "examples/quickstart",
        "examples/production",
        "examples/custom-lint",
    ] {
        let lint = lint_json(workspace, true);
        assert!(
            lint["diagnostics"].as_array().unwrap().is_empty(),
            "{workspace} should stay lint-clean\n{lint:#}"
        );
    }
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
    let lint = lint_json("tests/fixtures/workspaces/missing-manifest", false);
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "rototo/workspace-manifest-missing");
    assert_eq!(diagnostic["stage"], "discover");
    assert_eq!(diagnostic["entity"]["kind"], "workspace");
    assert!(diagnostic["primary"]["doc"].is_null());
    assert!(diagnostic["primary"]["range"].is_null());
    assert!(lint["documents"].as_array().unwrap().is_empty());
}

#[test]
fn canonical_discover_fixture_reports_workspace_manifest_missing() {
    let lint = lint_json(
        "tests/fixtures/workspaces/rules/discover/workspace-manifest-missing",
        false,
    );

    assert_only_expected_diagnostic(
        &lint,
        ExpectedDiagnostic {
            rule: "rototo/workspace-manifest-missing",
            severity: "error",
            stage: LintStage::Discover,
            entity: ExpectedEntity::Workspace,
            primary: ExpectedPrimaryLocation::WorkspaceRoot,
            related: &[],
        },
    );
    assert!(lint["documents"].as_array().unwrap().is_empty());
}

#[test]
fn canonical_rule_fixture_table_covers_every_rototo_rule() {
    let mut covered = BTreeSet::new();

    for fixture in canonical_rule_fixtures() {
        assert!(
            covered.insert(fixture.rule.meta().rule),
            "duplicate canonical fixture table entry: {}",
            fixture.rule.meta().rule
        );
        assert_canonical_fixture(fixture);
    }
    for pending in pending_canonical_rule_fixtures() {
        assert!(
            covered.insert(pending.rule.meta().rule),
            "duplicate canonical fixture table entry: {}",
            pending.rule.meta().rule
        );
    }

    let expected = RototoRuleId::iter()
        .map(|rule| rule.meta().rule)
        .collect::<BTreeSet<_>>();
    assert_eq!(covered, expected);
}

#[test]
fn reports_workspace_manifest_parse_failed() {
    let lint = lint_json("tests/fixtures/workspaces/invalid-workspace-toml", false);
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "rototo/workspace-manifest-parse-failed");
    assert_eq!(diagnostic["stage"], "parse");
    assert_eq!(diagnostic["entity"]["kind"], "manifest");
    assert_eq!(diagnostic["primary"]["path"], "rototo-workspace.toml");
    assert!(diagnostic["primary"]["range"].is_object());
}

#[test]
fn reports_workspace_manifest_schema_failed() {
    let lint = lint_json("tests/fixtures/workspaces/missing-environments", false);
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(
        diagnostic["rule"],
        "rototo/workspace-manifest-schema-failed"
    );
    assert_eq!(diagnostic["stage"], "project");
    assert_eq!(diagnostic["entity"]["kind"], "manifest");
    assert_eq!(diagnostic["primary"]["path"], "rototo-workspace.toml");
    assert!(diagnostic["primary"]["range"].is_null());
}

#[test]
fn reports_workspace_file_parse_failed() {
    let lint = lint_json(
        "tests/fixtures/workspaces/invalid-workspace-file-toml",
        false,
    );
    let rules = diagnostic_rules(&lint);

    assert_eq!(
        rules,
        vec![
            "rototo/qualifier-parse-failed".to_owned(),
            "rototo/variable-parse-failed".to_owned(),
        ]
    );

    let qualifier = diagnostic_for_rule(&lint, "rototo/qualifier-parse-failed");
    assert_eq!(qualifier["stage"], "parse");
    assert_eq!(qualifier["entity"]["kind"], "qualifier");
    assert_eq!(qualifier["entity"]["id"], "broken");
    assert_eq!(qualifier["primary"]["path"], "qualifiers/broken.toml");
    assert!(qualifier["primary"]["range"].is_object());

    let variable = diagnostic_for_rule(&lint, "rototo/variable-parse-failed");
    assert_eq!(variable["stage"], "parse");
    assert_eq!(variable["entity"]["kind"], "variable");
    assert_eq!(variable["entity"]["id"], "broken");
    assert_eq!(variable["primary"]["path"], "variables/broken.toml");
    assert!(variable["primary"]["range"].is_object());
}

#[test]
fn reports_schema_parse_failed() {
    let lint = lint_json("tests/fixtures/workspaces/lint-failures", false);
    let diagnostic = diagnostic_for_rule(&lint, "rototo/schema-parse-failed");

    assert_eq!(diagnostic["rule"], "rototo/schema-parse-failed");
    assert_eq!(diagnostic["stage"], "parse");
    assert_eq!(diagnostic["entity"]["kind"], "schema");
    assert_eq!(
        diagnostic["entity"]["path"],
        "schemas/invalid-json.schema.json"
    );
    assert_eq!(
        diagnostic["primary"]["path"],
        "schemas/invalid-json.schema.json"
    );
    assert!(diagnostic["primary"]["range"].is_object());
}

#[test]
fn reports_workspace_context_schema_ref_failures() {
    for workspace in [
        "tests/fixtures/workspaces/bad-context-config",
        "tests/fixtures/workspaces/context-schema-empty-path",
        "tests/fixtures/workspaces/context-schema-invalid-json",
        "tests/fixtures/workspaces/context-schema-invalid-schema",
        "tests/fixtures/workspaces/context-schema-missing-field",
        "tests/fixtures/workspaces/context-schema-missing-file",
        "tests/fixtures/workspaces/context-schema-path-escape",
    ] {
        let lint = lint_json(workspace, false);
        assert_reference_rule(
            &lint,
            "rototo/workspace-context-schema-ref",
            "rototo-workspace.toml",
        );

        let diagnostic = diagnostic_for_rule(&lint, "rototo/workspace-context-schema-ref");
        assert_eq!(diagnostic["entity"]["kind"], "manifest");
    }
}

#[test]
fn reports_workspace_context_schema_attribute_failures() {
    let lint = lint_json("tests/fixtures/workspaces/context-schema-attribute", false);
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(
        diagnostic["rule"],
        "rototo/workspace-context-schema-attribute"
    );
    assert_eq!(diagnostic["stage"], "reference");
    assert_eq!(diagnostic["entity"]["kind"], "predicate");
    assert_eq!(
        diagnostic["primary"]["path"],
        "qualifiers/missing-context-contract.toml"
    );
    assert!(diagnostic["primary"]["range"].is_object());
}

#[test]
fn reports_project_stage_qualifier_shape_failures() {
    let lint = lint_json("tests/fixtures/workspaces/rule-coverage", false);

    assert_project_rule(
        &lint,
        "rototo/qualifier-schema-version",
        "qualifiers/missing-schema-version.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/qualifier-predicate-missing",
        "qualifiers/missing-predicate.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/qualifier-predicate-shape",
        "qualifiers/predicate-shape.toml",
    );
}

#[test]
fn reports_project_stage_variable_shape_failures() {
    let lint = lint_json("tests/fixtures/workspaces/rule-coverage", false);

    assert_project_rule(
        &lint,
        "rototo/variable-schema-version",
        "variables/missing-schema-version.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-type-or-schema",
        "variables/type-or-schema.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-values-missing",
        "variables/values-missing.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-env-missing-default",
        "variables/env-missing-default.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-env-shape",
        "variables/env-shape.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-rule-shape",
        "variables/rule-shape.toml",
    );
}

#[test]
fn reports_project_stage_predicate_and_type_failures() {
    let lint = lint_json("tests/fixtures/workspaces/lint-failures", false);

    assert_project_rule(
        &lint,
        "rototo/qualifier-predicate-bucket",
        "qualifiers/bad-bucket.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/qualifier-predicate-unknown-op",
        "qualifiers/bad-value-shape.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/qualifier-predicate-value",
        "qualifiers/bad-value-shape.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-unknown-type",
        "variables/unknown-type.toml",
    );

    let diagnostic = diagnostic_for_rule(&lint, "rototo/variable-unknown-type");
    assert_eq!(diagnostic["entity"]["kind"], "variable");
    assert_eq!(diagnostic["entity"]["id"], "unknown-type");
    assert!(diagnostic["primary"]["range"].is_object());
}

#[test]
fn reports_project_stage_external_value_integrity_failures() {
    let lint = lint_json("tests/fixtures/workspaces/lint-failures", false);

    assert_project_rule(
        &lint,
        "rototo/variable-external-value-duplicate",
        "variables/external-duplicate-values/default.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-external-values-load-failed",
        "variables/external-load.toml",
    );

    let duplicate = diagnostic_for_rule(&lint, "rototo/variable-external-value-duplicate");
    assert_eq!(duplicate["entity"]["kind"], "value");
    assert_eq!(duplicate["entity"]["variable"], "external-duplicate");
    assert_eq!(duplicate["entity"]["key"], "default");
    assert!(duplicate["primary"]["range"].is_object());

    let load_failed = diagnostic_for_rule(&lint, "rototo/variable-external-values-load-failed");
    assert_eq!(load_failed["entity"]["kind"], "variable");
    assert_eq!(load_failed["entity"]["id"], "external-load");
    assert!(load_failed["primary"]["range"].is_object());
}

#[test]
fn reports_reference_stage_failures() {
    let lint = lint_json("tests/fixtures/workspaces/lint-failures", false);

    assert_reference_rule(
        &lint,
        "rototo/qualifier-predicate-unknown-qualifier",
        "qualifiers/bad-reference.toml",
    );
    assert_reference_rule(
        &lint,
        "rototo/variable-unknown-environment",
        "variables/bad-env.toml",
    );
    assert_reference_rule(
        &lint,
        "rototo/variable-rule-unknown-qualifier",
        "variables/bad-env.toml",
    );
    assert_reference_rule(
        &lint,
        "rototo/variable-unknown-value",
        "variables/bad-env.toml",
    );

    let qualifier = diagnostic_for_rule(&lint, "rototo/qualifier-predicate-unknown-qualifier");
    assert_eq!(qualifier["entity"]["kind"], "predicate");
    assert_eq!(qualifier["entity"]["qualifier"], "bad-reference");
    assert_eq!(qualifier["entity"]["index"], 0);

    let unknown_value_messages =
        diagnostic_messages_for_rule(&lint, "rototo/variable-unknown-value");
    assert!(
        unknown_value_messages
            .contains(&"environment references unknown value: missing-value".to_owned())
    );
    assert!(
        unknown_value_messages
            .contains(&"rule references unknown value: another-missing-value".to_owned())
    );
}

#[test]
fn canonical_reference_fixture_reports_variable_rule_unknown_qualifier() {
    let lint = lint_json(
        "tests/fixtures/workspaces/rules/reference/variable-rule-unknown-qualifier",
        false,
    );

    assert_only_expected_diagnostic(
        &lint,
        ExpectedDiagnostic {
            rule: "rototo/variable-rule-unknown-qualifier",
            severity: "error",
            stage: LintStage::Reference,
            entity: ExpectedEntity::Rule {
                variable: "checkout-redesign",
                environment: "prod",
                index: 0,
            },
            primary: ExpectedPrimaryLocation::Document {
                path: "variables/checkout-redesign.toml",
                range: Some(ExpectedRange {
                    start_line: 14,
                    start_character: 12,
                    end_line: 14,
                    end_character: 27,
                }),
            },
            related: &[],
        },
    );
}

#[test]
fn reports_value_stage_failures() {
    let lint = lint_json("tests/fixtures/workspaces/lint-failures", false);

    assert_value_rule(
        &lint,
        "rototo/schema-invalid",
        "schemas/invalid.schema.json",
    );
    assert_value_rule(
        &lint,
        "rototo/variable-schema-ref",
        "variables/bad-schema-ref.toml",
    );
    assert_value_rule(
        &lint,
        "rototo/variable-value-schema-mismatch",
        "variables/bad-schema-value.toml",
    );
    assert_value_rule(
        &lint,
        "rototo/variable-value-type-mismatch",
        "variables/bad-type-value.toml",
    );

    let schema_mismatch = diagnostic_for_rule(&lint, "rototo/variable-value-schema-mismatch");
    assert_eq!(schema_mismatch["entity"]["kind"], "value");
    assert_eq!(schema_mismatch["entity"]["variable"], "bad-schema-value");
    assert_eq!(schema_mismatch["entity"]["key"], "broken");

    let type_mismatch = diagnostic_for_rule(&lint, "rototo/variable-value-type-mismatch");
    assert_eq!(type_mismatch["entity"]["kind"], "value");
    assert_eq!(type_mismatch["entity"]["variable"], "bad-type-value");
    assert_eq!(type_mismatch["entity"]["key"], "bad");
    assert!(type_mismatch["primary"]["range"].is_object());
}

#[test]
fn reports_graph_stage_qualifier_cycles() {
    let lint = lint_json(
        "tests/fixtures/workspaces/rules/graph/qualifier-cycle",
        false,
    );
    let diagnostics = diagnostics_for_rule(&lint, "rototo/qualifier-cycle");

    assert_eq!(diagnostics.len(), 3, "{lint:#}");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["stage"] == "graph")
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["severity"] == "error")
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["primary"]["range"].is_object())
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["entity"]["id"] == "self")
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["entity"]["id"] == "alpha")
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["entity"]["id"] == "beta")
    );

    let alpha = diagnostics
        .iter()
        .find(|diagnostic| diagnostic["entity"]["id"] == "alpha")
        .unwrap();
    assert!(!alpha["related"].as_array().unwrap().is_empty());
}

#[test]
fn reports_graph_stage_qualifier_unreferenced_warning_without_failing() {
    let lint = lint_json(
        "tests/fixtures/workspaces/rules/graph/qualifier-unreferenced",
        true,
    );
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "rototo/qualifier-unreferenced");
    assert_eq!(diagnostic["severity"], "warning");
    assert_eq!(diagnostic["stage"], "graph");
    assert_eq!(diagnostic["entity"]["kind"], "qualifier");
    assert_eq!(diagnostic["entity"]["id"], "unused");
}

#[test]
fn reports_graph_stage_shadowed_rule_warning_without_failing() {
    let lint = lint_json(
        "tests/fixtures/workspaces/rules/graph/variable-rule-shadowed",
        true,
    );
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "rototo/variable-rule-shadowed");
    assert_eq!(diagnostic["severity"], "warning");
    assert_eq!(diagnostic["stage"], "graph");
    assert_eq!(diagnostic["entity"]["kind"], "rule");
    assert_eq!(diagnostic["entity"]["variable"], "checkout");
    assert_eq!(diagnostic["entity"]["environment"], "prod");
    assert_eq!(diagnostic["entity"]["index"], 1);
    assert_eq!(diagnostic["related"].as_array().unwrap().len(), 1);
}

#[test]
fn reports_graph_stage_unused_value_warning_without_failing() {
    let lint = lint_json(
        "tests/fixtures/workspaces/rules/graph/variable-value-unused",
        true,
    );
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "rototo/variable-value-unused");
    assert_eq!(diagnostic["severity"], "warning");
    assert_eq!(diagnostic["stage"], "graph");
    assert_eq!(diagnostic["entity"]["kind"], "value");
    assert_eq!(diagnostic["entity"]["variable"], "message");
    assert_eq!(diagnostic["entity"]["key"], "unused");
}

#[test]
fn lint_failures_fixture_covers_graph_rules() {
    let lint = lint_json("tests/fixtures/workspaces/lint-failures", false);

    assert_graph_rule(&lint, "rototo/qualifier-cycle", "qualifiers/cycle-a.toml");
    assert_graph_rule(
        &lint,
        "rototo/qualifier-unreferenced",
        "qualifiers/unreferenced.toml",
    );
    assert_graph_rule(
        &lint,
        "rototo/variable-rule-shadowed",
        "variables/graph-warnings.toml",
    );
    assert_graph_rule(
        &lint,
        "rototo/variable-value-unused",
        "variables/graph-warnings.toml",
    );
}

#[test]
fn reports_workspace_custom_lint_failures() {
    let lint = lint_json("tests/fixtures/workspaces/lint-failures", false);

    assert_policy_rule(
        &lint,
        "fixture/custom-variable-rejected",
        "variables/custom-lint.toml",
    );
    assert_value_rule(
        &lint,
        "fixture/custom-value-rejected",
        "variables/custom-value-lint.toml",
    );

    let variable = diagnostic_for_rule(&lint, "fixture/custom-variable-rejected");
    assert_eq!(variable["entity"]["kind"], "variable");
    assert_eq!(variable["entity"]["id"], "custom-lint");

    let value = diagnostic_for_rule(&lint, "fixture/custom-value-rejected");
    assert_eq!(value["entity"]["kind"], "value");
    assert_eq!(value["entity"]["variable"], "custom-value-lint");
    assert_eq!(value["entity"]["key"], "default");
}

#[test]
fn reports_custom_lint_contract_failures() {
    let lint = lint_json("tests/fixtures/workspaces/custom-lint-contract", false);

    assert_project_rule(
        &lint,
        "rototo/custom-lint-rule-conflict",
        "rototo-workspace.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/custom-lint-invalid-rule",
        "rototo-workspace.toml",
    );
    assert_policy_rule(
        &lint,
        "rototo/custom-lint-failed",
        "variables/custom-failed.toml",
    );
    assert_policy_rule(
        &lint,
        "payments/max-token-budget",
        "variables/custom-valid.toml",
    );
}

#[test]
fn reports_registered_custom_lint_failures() {
    let lint = lint_json("tests/fixtures/workspaces/custom-register", false);

    assert_register_rule(
        &lint,
        "rototo/custom-lint-registration-invalid",
        "lint/payments.lua",
    );
    assert_register_rule(
        &lint,
        "rototo/custom-lint-unknown-rule",
        "lint/payments.lua",
    );
    assert_value_rule(
        &lint,
        "payments/max-token-budget",
        "variables/agent-config.toml",
    );

    let diagnostic = diagnostic_for_rule(&lint, "payments/max-token-budget");
    assert_eq!(diagnostic["entity"]["kind"], "value");
    assert_eq!(diagnostic["entity"]["variable"], "agent-config");
    assert_eq!(diagnostic["entity"]["key"], "standard");
    assert_eq!(diagnostic["stage"], "value");
    assert!(diagnostic["primary"]["range"].is_object());
}

#[test]
fn reports_registered_custom_lint_targets() {
    let lint = lint_json("tests/fixtures/workspaces/custom-targets", false);

    assert_project_rule(
        &lint,
        "targets/workspace-environments",
        "rototo-workspace.toml",
    );
    assert_project_rule(
        &lint,
        "targets/qualifier-predicates",
        "qualifiers/premium-users.toml",
    );
    assert_value_rule(
        &lint,
        "targets/variable-schema",
        "variables/agent-config.toml",
    );
    assert_value_rule(&lint, "targets/schema-json", "schemas/config.schema.json");

    let workspace = diagnostic_for_rule(&lint, "targets/workspace-environments");
    assert_eq!(workspace["entity"]["kind"], "workspace");
    assert_eq!(workspace["stage"], "project");
    assert!(workspace["primary"]["range"].is_object());

    let qualifier = diagnostic_for_rule(&lint, "targets/qualifier-predicates");
    assert_eq!(qualifier["entity"]["kind"], "qualifier");
    assert_eq!(qualifier["entity"]["id"], "premium-users");
    assert_eq!(qualifier["stage"], "project");
    assert!(qualifier["primary"]["range"].is_object());

    let variable = diagnostic_for_rule(&lint, "targets/variable-schema");
    assert_eq!(variable["entity"]["kind"], "variable");
    assert_eq!(variable["entity"]["id"], "agent-config");
    assert_eq!(variable["stage"], "value");
    assert!(variable["primary"]["range"].is_object());

    let schema = diagnostic_for_rule(&lint, "targets/schema-json");
    assert_eq!(schema["entity"]["kind"], "schema");
    assert_eq!(schema["entity"]["path"], "schemas/config.schema.json");
    assert_eq!(schema["stage"], "value");
}

fn lint_json(workspace: &str, success: bool) -> serde_json::Value {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args(["workspace", "lint", workspace, "--json"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.success(),
        success,
        "unexpected lint status for {workspace}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn only_diagnostic(lint: &serde_json::Value) -> &serde_json::Value {
    let diagnostics = lint["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{lint:#}");
    &diagnostics[0]
}

fn diagnostic_rules(lint: &serde_json::Value) -> Vec<String> {
    lint["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["rule"].as_str().unwrap().to_owned())
        .collect()
}

fn diagnostic_for_rule<'a>(lint: &'a serde_json::Value, rule: &str) -> &'a serde_json::Value {
    lint["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["rule"] == rule)
        .unwrap_or_else(|| panic!("diagnostic not found: {rule}\n{lint:#}"))
}

fn diagnostics_for_rule<'a>(lint: &'a serde_json::Value, rule: &str) -> Vec<&'a serde_json::Value> {
    lint["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["rule"] == rule)
        .collect()
}

fn diagnostic_messages_for_rule(lint: &serde_json::Value, rule: &str) -> Vec<String> {
    lint["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["rule"] == rule)
        .map(|diagnostic| diagnostic["message"].as_str().unwrap().to_owned())
        .collect()
}

fn assert_project_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "project");
    assert_eq!(diagnostic["primary"]["path"], path);
}

fn assert_register_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "register");
    assert_eq!(diagnostic["primary"]["path"], path);
}

fn assert_reference_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "reference");
    assert_eq!(diagnostic["primary"]["path"], path);
}

fn assert_value_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "value");
    assert_eq!(diagnostic["primary"]["path"], path);
}

fn assert_graph_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostics_for_rule(lint, rule)
        .into_iter()
        .find(|diagnostic| diagnostic["primary"]["path"] == path)
        .unwrap_or_else(|| panic!("diagnostic not found: {rule} at {path}\n{lint:#}"));
    assert_eq!(diagnostic["stage"], "graph");
    assert_eq!(diagnostic["primary"]["path"], path);
}

fn assert_policy_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "policy");
    assert_eq!(diagnostic["primary"]["path"], path);
}

#[derive(Clone, Copy)]
struct CanonicalRuleFixture {
    rule: RototoRuleId,
    workspace: &'static str,
    success: bool,
    expected: &'static [ExpectedDiagnostic],
}

#[derive(Clone, Copy)]
struct PendingCanonicalRuleFixture {
    rule: RototoRuleId,
}

#[derive(Clone, Copy)]
struct ExpectedDiagnostic {
    rule: &'static str,
    severity: &'static str,
    stage: LintStage,
    entity: ExpectedEntity,
    primary: ExpectedPrimaryLocation,
    related: &'static [ExpectedRelatedLocation],
}

#[derive(Clone, Copy)]
enum ExpectedPrimaryLocation {
    WorkspaceRoot,
    Document {
        path: &'static str,
        range: Option<ExpectedRange>,
    },
}

#[derive(Clone, Copy)]
struct ExpectedRange {
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum ExpectedEntity {
    Workspace,
    Manifest,
    Qualifier(&'static str),
    Predicate {
        qualifier: &'static str,
        index: usize,
    },
    Variable(&'static str),
    Value {
        variable: &'static str,
        key: &'static str,
    },
    EnvironmentBlock {
        variable: &'static str,
        environment: &'static str,
    },
    Rule {
        variable: &'static str,
        environment: &'static str,
        index: usize,
    },
    Schema(&'static str),
    CustomRule(&'static str),
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct ExpectedRelatedLocation {
    path: &'static str,
    range: ExpectedRange,
    message: &'static str,
}

fn canonical_rule_fixtures() -> &'static [CanonicalRuleFixture] {
    &[
        CanonicalRuleFixture {
            rule: RototoRuleId::WorkspaceManifestMissing,
            workspace: "tests/fixtures/workspaces/rules/discover/workspace-manifest-missing",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/workspace-manifest-missing",
                severity: "error",
                stage: LintStage::Discover,
                entity: ExpectedEntity::Workspace,
                primary: ExpectedPrimaryLocation::WorkspaceRoot,
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::WorkspaceManifestParseFailed,
            workspace: "tests/fixtures/workspaces/rules/parse/workspace-manifest-parse-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/workspace-manifest-parse-failed",
                severity: "error",
                stage: LintStage::Parse,
                entity: ExpectedEntity::Manifest,
                primary: ExpectedPrimaryLocation::Document {
                    path: "rototo-workspace.toml",
                    range: Some(ExpectedRange {
                        start_line: 2,
                        start_character: 13,
                        end_line: 3,
                        end_character: 0,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierParseFailed,
            workspace: "tests/fixtures/workspaces/rules/parse/qualifier-parse-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/qualifier-parse-failed",
                severity: "error",
                stage: LintStage::Parse,
                entity: ExpectedEntity::Qualifier("broken"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/broken.toml",
                    range: Some(ExpectedRange {
                        start_line: 2,
                        start_character: 10,
                        end_line: 3,
                        end_character: 0,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableParseFailed,
            workspace: "tests/fixtures/workspaces/rules/parse/variable-parse-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-parse-failed",
                severity: "error",
                stage: LintStage::Parse,
                entity: ExpectedEntity::Variable("broken"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/broken.toml",
                    range: Some(ExpectedRange {
                        start_line: 3,
                        start_character: 6,
                        end_line: 4,
                        end_character: 0,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableExternalValueParseFailed,
            workspace: "tests/fixtures/workspaces/rules/parse/variable-external-value-parse-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-external-value-parse-failed",
                severity: "error",
                stage: LintStage::Parse,
                entity: ExpectedEntity::Value {
                    variable: "external-message",
                    key: "broken",
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/external-message-values/broken.toml",
                    range: Some(ExpectedRange {
                        start_line: 0,
                        start_character: 7,
                        end_line: 1,
                        end_character: 0,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::SchemaParseFailed,
            workspace: "tests/fixtures/workspaces/rules/parse/schema-parse-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/schema-parse-failed",
                severity: "error",
                stage: LintStage::Parse,
                entity: ExpectedEntity::Schema("schemas/broken.schema.json"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "schemas/broken.schema.json",
                    range: Some(ExpectedRange {
                        start_line: 2,
                        start_character: 1,
                        end_line: 3,
                        end_character: 0,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::WorkspaceManifestSchemaFailed,
            workspace: "tests/fixtures/workspaces/rules/project/workspace-manifest-schema-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/workspace-manifest-schema-failed",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Manifest,
                primary: ExpectedPrimaryLocation::Document {
                    path: "rototo-workspace.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierSchemaVersion,
            workspace: "tests/fixtures/workspaces/rules/project/qualifier-schema-version",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/qualifier-schema-version",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Qualifier("premium-users"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/premium-users.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierPredicateMissing,
            workspace: "tests/fixtures/workspaces/rules/project/qualifier-predicate-missing",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/qualifier-predicate-missing",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Qualifier("premium-users"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/premium-users.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierPredicateShape,
            workspace: "tests/fixtures/workspaces/rules/project/qualifier-predicate-shape",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/qualifier-predicate-shape",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Predicate {
                    qualifier: "premium-users",
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/premium-users.toml",
                    range: Some(ExpectedRange {
                        start_line: 2,
                        start_character: 0,
                        end_line: 4,
                        end_character: 17,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierPredicateUnknownOp,
            workspace: "tests/fixtures/workspaces/rules/project/qualifier-predicate-unknown-op",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/qualifier-predicate-unknown-op",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Predicate {
                    qualifier: "premium-users",
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/premium-users.toml",
                    range: Some(ExpectedRange {
                        start_line: 4,
                        start_character: 5,
                        end_line: 4,
                        end_character: 15,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierPredicateBucket,
            workspace: "tests/fixtures/workspaces/rules/project/qualifier-predicate-bucket",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/qualifier-predicate-bucket",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Predicate {
                    qualifier: "beta-bucket",
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/beta-bucket.toml",
                    range: Some(ExpectedRange {
                        start_line: 6,
                        start_character: 8,
                        end_line: 6,
                        end_character: 18,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierPredicateValue,
            workspace: "tests/fixtures/workspaces/rules/project/qualifier-predicate-value",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/qualifier-predicate-value",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Predicate {
                    qualifier: "premium-users",
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/premium-users.toml",
                    range: Some(ExpectedRange {
                        start_line: 5,
                        start_character: 8,
                        end_line: 5,
                        end_character: 17,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableSchemaVersion,
            workspace: "tests/fixtures/workspaces/rules/project/variable-schema-version",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-schema-version",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableTypeOrSchema,
            workspace: "tests/fixtures/workspaces/rules/project/variable-type-or-schema",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-type-or-schema",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableValuesMissing,
            workspace: "tests/fixtures/workspaces/rules/project/variable-values-missing",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-values-missing",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableEnvMissingDefault,
            workspace: "tests/fixtures/workspaces/rules/project/variable-env-missing-default",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-env-missing-default",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableEnvShape,
            workspace: "tests/fixtures/workspaces/rules/project/variable-env-shape",
            success: false,
            expected: &[
                ExpectedDiagnostic {
                    rule: "rototo/variable-env-shape",
                    severity: "error",
                    stage: LintStage::Project,
                    entity: ExpectedEntity::EnvironmentBlock {
                        variable: "message",
                        environment: "_",
                    },
                    primary: ExpectedPrimaryLocation::Document {
                        path: "variables/message.toml",
                        range: Some(ExpectedRange {
                            start_line: 7,
                            start_character: 4,
                            end_line: 7,
                            end_character: 13,
                        }),
                    },
                    related: &[],
                },
                ExpectedDiagnostic {
                    rule: "rototo/variable-value-unused",
                    severity: "warning",
                    stage: LintStage::Graph,
                    entity: ExpectedEntity::Value {
                        variable: "message",
                        key: "control",
                    },
                    primary: ExpectedPrimaryLocation::Document {
                        path: "variables/message.toml",
                        range: Some(ExpectedRange {
                            start_line: 4,
                            start_character: 10,
                            end_line: 4,
                            end_character: 19,
                        }),
                    },
                    related: &[],
                },
            ],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleShape,
            workspace: "tests/fixtures/workspaces/rules/project/variable-rule-shape",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-rule-shape",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Rule {
                    variable: "message",
                    environment: "_",
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 8,
                        start_character: 8,
                        end_line: 8,
                        end_character: 21,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableExternalValuesLoadFailed,
            workspace: "tests/fixtures/workspaces/rules/project/variable-external-values-load-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-external-values-load-failed",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Variable("external-message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/external-message.toml",
                    range: Some(ExpectedRange {
                        start_line: 2,
                        start_character: 9,
                        end_line: 2,
                        end_character: 22,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableExternalValueDuplicate,
            workspace: "tests/fixtures/workspaces/rules/project/variable-external-value-duplicate",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-external-value-duplicate",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Value {
                    variable: "external-message",
                    key: "default",
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/external-message-values/default.toml",
                    range: Some(ExpectedRange {
                        start_line: 0,
                        start_character: 8,
                        end_line: 0,
                        end_character: 18,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::CustomLintRuleShape,
            workspace: "tests/fixtures/workspaces/rules/project/custom-lint-rule-shape",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/custom-lint-rule-shape",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Manifest,
                primary: ExpectedPrimaryLocation::Document {
                    path: "rototo-workspace.toml",
                    range: Some(ExpectedRange {
                        start_line: 5,
                        start_character: 0,
                        end_line: 7,
                        end_character: 22,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::CustomLintInvalidRule,
            workspace: "tests/fixtures/workspaces/rules/project/custom-lint-invalid-rule",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/custom-lint-invalid-rule",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Manifest,
                primary: ExpectedPrimaryLocation::Document {
                    path: "rototo-workspace.toml",
                    range: Some(ExpectedRange {
                        start_line: 6,
                        start_character: 5,
                        end_line: 6,
                        end_character: 25,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::CustomLintRuleConflict,
            workspace: "tests/fixtures/workspaces/rules/project/custom-lint-rule-conflict",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/custom-lint-rule-conflict",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Manifest,
                primary: ExpectedPrimaryLocation::Document {
                    path: "rototo-workspace.toml",
                    range: Some(ExpectedRange {
                        start_line: 10,
                        start_character: 0,
                        end_line: 13,
                        end_character: 21,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::WorkspaceContextSchemaRef,
            workspace: "tests/fixtures/workspaces/rules/reference/workspace-context-schema-ref",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/workspace-context-schema-ref",
                severity: "error",
                stage: LintStage::Reference,
                entity: ExpectedEntity::Manifest,
                primary: ExpectedPrimaryLocation::Document {
                    path: "rototo-workspace.toml",
                    range: Some(ExpectedRange {
                        start_line: 6,
                        start_character: 9,
                        end_line: 6,
                        end_character: 38,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::WorkspaceContextSchemaAttribute,
            workspace: "tests/fixtures/workspaces/rules/reference/workspace-context-schema-attribute",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/workspace-context-schema-attribute",
                severity: "error",
                stage: LintStage::Reference,
                entity: ExpectedEntity::Predicate {
                    qualifier: "missing-context",
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/missing-context.toml",
                    range: Some(ExpectedRange {
                        start_line: 3,
                        start_character: 12,
                        end_line: 3,
                        end_character: 28,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierPredicateUnknownQualifier,
            workspace: "tests/fixtures/workspaces/rules/reference/qualifier-predicate-unknown-qualifier",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/qualifier-predicate-unknown-qualifier",
                severity: "error",
                stage: LintStage::Reference,
                entity: ExpectedEntity::Predicate {
                    qualifier: "derived",
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/derived.toml",
                    range: Some(ExpectedRange {
                        start_line: 3,
                        start_character: 12,
                        end_line: 3,
                        end_character: 31,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableUnknownEnvironment,
            workspace: "tests/fixtures/workspaces/rules/reference/variable-unknown-environment",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-unknown-environment",
                severity: "error",
                stage: LintStage::Reference,
                entity: ExpectedEntity::EnvironmentBlock {
                    variable: "message",
                    environment: "stage",
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 10,
                        start_character: 8,
                        end_line: 10,
                        end_character: 17,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableUnknownValue,
            workspace: "tests/fixtures/workspaces/rules/reference/variable-unknown-value",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-unknown-value",
                severity: "error",
                stage: LintStage::Reference,
                entity: ExpectedEntity::EnvironmentBlock {
                    variable: "message",
                    environment: "_",
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 7,
                        start_character: 8,
                        end_line: 7,
                        end_character: 17,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleUnknownQualifier,
            workspace: "tests/fixtures/workspaces/rules/reference/variable-rule-unknown-qualifier",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-rule-unknown-qualifier",
                severity: "error",
                stage: LintStage::Reference,
                entity: ExpectedEntity::Rule {
                    variable: "checkout-redesign",
                    environment: "prod",
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/checkout-redesign.toml",
                    range: Some(ExpectedRange {
                        start_line: 14,
                        start_character: 12,
                        end_line: 14,
                        end_character: 27,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableUnknownType,
            workspace: "tests/fixtures/workspaces/rules/value/variable-unknown-type",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-unknown-type",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 1,
                        start_character: 7,
                        end_line: 1,
                        end_character: 13,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableValueTypeMismatch,
            workspace: "tests/fixtures/workspaces/rules/value/variable-value-type-mismatch",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-value-type-mismatch",
                severity: "error",
                stage: LintStage::Value,
                entity: ExpectedEntity::Value {
                    variable: "enabled",
                    key: "control",
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/enabled.toml",
                    range: Some(ExpectedRange {
                        start_line: 4,
                        start_character: 10,
                        end_line: 4,
                        end_character: 22,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableValueSchemaMismatch,
            workspace: "tests/fixtures/workspaces/rules/value/variable-value-schema-mismatch",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-value-schema-mismatch",
                severity: "error",
                stage: LintStage::Value,
                entity: ExpectedEntity::Value {
                    variable: "message",
                    key: "control",
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 3,
                        start_character: 0,
                        end_line: 4,
                        end_character: 31,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableSchemaRef,
            workspace: "tests/fixtures/workspaces/rules/value/variable-schema-ref",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-schema-ref",
                severity: "error",
                stage: LintStage::Value,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 1,
                        start_character: 9,
                        end_line: 1,
                        end_character: 41,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::SchemaInvalid,
            workspace: "tests/fixtures/workspaces/rules/value/schema-invalid",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/schema-invalid",
                severity: "error",
                stage: LintStage::Value,
                entity: ExpectedEntity::Schema("schemas/broken.schema.json"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "schemas/broken.schema.json",
                    range: None,
                },
                related: &[],
            }],
        },
    ]
}

fn pending_canonical_rule_fixtures() -> &'static [PendingCanonicalRuleFixture] {
    &[
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::WorkspaceNotFound,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::QualifierCycle,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::QualifierUnreferenced,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleShadowed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableValueUnused,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::CustomLintFailed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::CustomLintRegistrationInvalid,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::CustomLintUnknownRule,
        },
    ]
}

fn assert_canonical_fixture(fixture: &CanonicalRuleFixture) {
    let lint = lint_json(fixture.workspace, fixture.success);
    assert_expected_diagnostics(&lint, fixture.expected);
}

fn assert_expected_diagnostics(lint: &serde_json::Value, expected: &[ExpectedDiagnostic]) {
    let diagnostics = lint["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics.len(),
        expected.len(),
        "unexpected diagnostic count\n{lint:#}"
    );
    for expected in expected {
        let diagnostic = diagnostic_for_rule(lint, expected.rule);
        assert_expected_diagnostic(lint, diagnostic, *expected);
    }
}

fn assert_only_expected_diagnostic(lint: &serde_json::Value, expected: ExpectedDiagnostic) {
    let diagnostic = only_diagnostic(lint);
    assert_expected_diagnostic(lint, diagnostic, expected);
}

fn assert_expected_diagnostic(
    lint: &serde_json::Value,
    diagnostic: &serde_json::Value,
    expected: ExpectedDiagnostic,
) {
    assert_eq!(diagnostic["rule"], expected.rule);
    assert_eq!(diagnostic["severity"], expected.severity);
    assert_eq!(diagnostic["stage"], expected_stage_label(expected.stage));
    assert_eq!(diagnostic["entity"], expected_entity_value(expected.entity));
    assert_expected_primary_location(lint, &diagnostic["primary"], expected.primary);

    let related = diagnostic["related"].as_array().unwrap();
    assert_eq!(
        related.len(),
        expected.related.len(),
        "unexpected related locations for diagnostic\n{diagnostic:#}"
    );
    for (actual, expected) in related.iter().zip(expected.related) {
        assert_eq!(actual["location"]["path"], expected.path);
        assert_eq!(
            actual["location"]["range"],
            expected_range_value(expected.range)
        );
        assert_eq!(actual["message"], expected.message);
    }
}

fn assert_expected_primary_location(
    lint: &serde_json::Value,
    primary: &serde_json::Value,
    expected: ExpectedPrimaryLocation,
) {
    match expected {
        ExpectedPrimaryLocation::WorkspaceRoot => {
            assert_eq!(primary["path"], lint["workspace"]);
            assert!(primary["doc"].is_null());
            assert!(primary["range"].is_null());
        }
        ExpectedPrimaryLocation::Document { path, range } => {
            assert_eq!(primary["path"], path);
            match range {
                Some(range) => assert_eq!(primary["range"], expected_range_value(range)),
                None => assert!(primary["range"].is_null()),
            }
        }
    }
}

fn expected_stage_label(stage: LintStage) -> &'static str {
    match stage {
        LintStage::Discover => "discover",
        LintStage::Parse => "parse",
        LintStage::Project => "project",
        LintStage::Register => "register",
        LintStage::Reference => "reference",
        LintStage::Value => "value",
        LintStage::Graph => "graph",
        LintStage::Policy => "policy",
    }
}

fn expected_entity_value(entity: ExpectedEntity) -> serde_json::Value {
    match entity {
        ExpectedEntity::Workspace => serde_json::json!({ "kind": "workspace" }),
        ExpectedEntity::Manifest => serde_json::json!({ "kind": "manifest" }),
        ExpectedEntity::Qualifier(id) => {
            serde_json::json!({ "kind": "qualifier", "id": id })
        }
        ExpectedEntity::Predicate { qualifier, index } => {
            serde_json::json!({
                "kind": "predicate",
                "qualifier": qualifier,
                "index": index,
            })
        }
        ExpectedEntity::Variable(id) => {
            serde_json::json!({ "kind": "variable", "id": id })
        }
        ExpectedEntity::Value { variable, key } => {
            serde_json::json!({
                "kind": "value",
                "variable": variable,
                "key": key,
            })
        }
        ExpectedEntity::EnvironmentBlock {
            variable,
            environment,
        } => {
            serde_json::json!({
                "kind": "environment_block",
                "variable": variable,
                "environment": environment,
            })
        }
        ExpectedEntity::Rule {
            variable,
            environment,
            index,
        } => {
            serde_json::json!({
                "kind": "rule",
                "variable": variable,
                "environment": environment,
                "index": index,
            })
        }
        ExpectedEntity::Schema(path) => {
            serde_json::json!({ "kind": "schema", "path": path })
        }
        ExpectedEntity::CustomRule(rule) => {
            serde_json::json!({ "kind": "custom_rule", "rule": rule })
        }
    }
}

fn expected_range_value(range: ExpectedRange) -> serde_json::Value {
    serde_json::json!({
        "start": {
            "line": range.start_line,
            "character": range.start_character,
        },
        "end": {
            "line": range.end_line,
            "character": range.end_character,
        },
    })
}

fn document_paths(lint: &serde_json::Value) -> Vec<String> {
    lint["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|document| document["path"].as_str().unwrap().to_owned())
        .collect()
}
