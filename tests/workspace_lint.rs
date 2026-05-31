use assert_cmd::Command;
use predicates::prelude::*;

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
    assert_project_rule(
        &lint,
        "rototo/variable-lint-shape",
        "variables/lint-shape.toml",
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
fn reports_variable_scoped_custom_lint_failures() {
    let lint = lint_json("tests/fixtures/workspaces/lint-failures", false);

    assert_policy_rule(
        &lint,
        "fixture/custom-variable-rejected",
        "variables/custom-lint.toml",
    );
    assert_policy_rule(
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
        "variables/conflict-b.toml",
    );
    assert_policy_rule(
        &lint,
        "rototo/custom-lint-failed",
        "variables/custom-failed.toml",
    );
    assert_policy_rule(
        &lint,
        "rototo/custom-lint-invalid-rule",
        "variables/custom-invalid.toml",
    );
    assert_policy_rule(
        &lint,
        "rototo/custom-lint-unknown-rule",
        "variables/custom-unknown.toml",
    );
    assert_policy_rule(
        &lint,
        "payments/max-token-budget",
        "variables/custom-valid.toml",
    );
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

fn assert_policy_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "policy");
    assert_eq!(diagnostic["primary"]["path"], path);
}

fn document_paths(lint: &serde_json::Value) -> Vec<String> {
    lint["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|document| document["path"].as_str().unwrap().to_owned())
        .collect()
}
