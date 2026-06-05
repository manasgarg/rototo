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
        .args(["lint", "examples/basic"])
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
    assert!(document_paths(&lint).contains(&"resources/llm-agent-config.toml".to_owned()));
    assert!(
        document_paths(&lint).contains(&"resources/llm-agent-config-objects/local.toml".to_owned())
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
fn lints_discovered_workspace() {
    let workspace = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", workspace.display());

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .arg("lint")
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
    assert!(diagnostic.get("primary").is_none());
    assert!(diagnostic["location"].get("doc").is_none());
    assert!(diagnostic["location"]["range"].is_null());
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
    assert_eq!(diagnostic["location"]["path"], "rototo-workspace.toml");
    assert!(diagnostic["location"]["range"].is_object());
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
    assert_eq!(diagnostic["location"]["path"], "rototo-workspace.toml");
    assert!(diagnostic["location"]["range"].is_null());
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
    assert_eq!(qualifier["location"]["path"], "qualifiers/broken.toml");
    assert!(qualifier["location"]["range"].is_object());

    let variable = diagnostic_for_rule(&lint, "rototo/variable-parse-failed");
    assert_eq!(variable["stage"], "parse");
    assert_eq!(variable["entity"]["kind"], "variable");
    assert_eq!(variable["entity"]["id"], "broken");
    assert_eq!(variable["location"]["path"], "variables/broken.toml");
    assert!(variable["location"]["range"].is_object());
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
        diagnostic["location"]["path"],
        "schemas/invalid-json.schema.json"
    );
    assert!(diagnostic["location"]["range"].is_object());
}

#[test]
fn parse_diagnostics_handle_multibyte_text_near_syntax_errors() {
    let temp = tempfile::TempDir::new().unwrap();
    let toml_root = temp.path().join("toml");
    std::fs::create_dir_all(&toml_root).unwrap();
    std::fs::write(
        toml_root.join("rototo-workspace.toml"),
        "schema_version = 1\n[environments]\nvalues = [\"prod\"]\nlabel = \"café\n",
    )
    .unwrap();
    let toml_lint = lint_json(toml_root.to_str().unwrap(), false);
    assert_eq!(
        only_diagnostic(&toml_lint)["rule"],
        "rototo/workspace-manifest-parse-failed"
    );

    let json_root = temp.path().join("json");
    std::fs::create_dir_all(json_root.join("schemas")).unwrap();
    std::fs::write(
        json_root.join("rototo-workspace.toml"),
        r#"schema_version = 1

[environments]
values = ["prod"]
"#,
    )
    .unwrap();
    std::fs::write(
        json_root.join("schemas/broken.schema.json"),
        "{\"title\":\"café\",\"type\":}",
    )
    .unwrap();
    let json_lint = lint_json(json_root.to_str().unwrap(), false);
    assert_eq!(
        only_diagnostic(&json_lint)["rule"],
        "rototo/schema-parse-failed"
    );
}

#[test]
fn reports_workspace_context_schema_ref_failures() {
    for workspace in [
        "tests/fixtures/workspaces/context-schema-invalid-json",
        "tests/fixtures/workspaces/context-schema-invalid-schema",
    ] {
        let lint = lint_json(workspace, false);
        assert_project_rule(
            &lint,
            "rototo/workspace-context-schema-ref",
            "schemas/context.schema.json",
        );

        let diagnostic = diagnostic_for_rule(&lint, "rototo/workspace-context-schema-ref");
        assert_eq!(diagnostic["entity"]["kind"], "schema");
        assert_eq!(diagnostic["entity"]["path"], "schemas/context.schema.json");
    }
}

#[test]
fn accepts_path_safety_normalized_refs() {
    let lint = lint_json("tests/fixtures/workspaces/path-safety-valid", true);

    assert!(lint["diagnostics"].as_array().unwrap().is_empty());
    assert!(document_paths(&lint).contains(&"rototo-workspace.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"schemas/context.schema.json".to_owned()));
    assert!(document_paths(&lint).contains(&"schemas/value.schema.json".to_owned()));
    assert!(document_paths(&lint).contains(&"variables/message.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"resources/message.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"resources/message-objects/default.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"lint/ok.lua".to_owned()));
}

#[test]
fn rejects_path_safety_escaping_refs_and_lint_files() {
    let lint = lint_json("tests/fixtures/workspaces/path-safety-escapes", false);

    assert_reference_rule(
        &lint,
        "rototo/resource-schema-ref",
        "resources/message.toml",
    );
    assert_register_rule(&lint, "rototo/custom-lint-failed", "lint/escape.lua");

    let lint_file = diagnostic_for_rule(&lint, "rototo/custom-lint-failed");
    assert_eq!(lint_file["entity"]["kind"], "custom_lint");
    assert_eq!(lint_file["entity"]["path"], "lint/escape.lua");
    assert!(
        lint_file["message"]
            .as_str()
            .unwrap()
            .contains("path escapes workspace")
    );
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
        diagnostic["location"]["path"],
        "qualifiers/missing-context-contract.toml"
    );
    assert!(diagnostic["location"]["range"].is_object());
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
        "rototo/variable-type-source",
        "variables/type-or-schema.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-values-missing",
        "variables/values-missing.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-resolve-missing-default",
        "variables/env-missing-default.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-resolve-shape",
        "variables/env-shape.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-rule-shape",
        "variables/rule-shape.toml",
    );
}

#[test]
fn reports_project_stage_predicate_failures() {
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
}

#[test]
fn resource_object_file_can_represent_object_with_value_key() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("variables")).unwrap();
    std::fs::create_dir_all(root.join("resources/message-objects")).unwrap();
    std::fs::create_dir_all(root.join("schemas")).unwrap();
    std::fs::write(
        root.join("rototo-workspace.toml"),
        r#"schema_version = 1
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("schemas/message.schema.json"),
        r#"{
  "type": "object",
  "properties": { "value": { "type": "string" } },
  "required": ["value"],
  "additionalProperties": false
}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("resources/message.toml"),
        r#"schema_version = 1
schema = "../schemas/message.schema.json"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("variables/message.toml"),
        r#"schema_version = 1
type = "resource:message"

[resolve]
default = "default"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("resources/message-objects/default.toml"),
        r#"value = "literal object field""#,
    )
    .unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", root.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn resource_backed_variable_values_are_rejected_before_value_validation() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("variables")).unwrap();
    std::fs::create_dir_all(root.join("resources/message-objects")).unwrap();
    std::fs::create_dir_all(root.join("schemas")).unwrap();
    std::fs::write(
        root.join("rototo-workspace.toml"),
        r#"schema_version = 1
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("schemas/message.schema.json"),
        r#"{
  "type": "object",
  "properties": { "value": { "type": "string" } },
  "required": ["value"],
  "additionalProperties": false
}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("resources/message.toml"),
        r#"schema_version = 1
schema = "../schemas/message.schema.json"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("resources/message-objects/default.toml"),
        r#"value = "resource object""#,
    )
    .unwrap();
    std::fs::write(
        root.join("variables/message.toml"),
        r#"schema_version = 1
type = "resource:message"

[values]
default = "inline"

[resolve]
default = "default"
"#,
    )
    .unwrap();

    let lint = lint_json(root.to_str().unwrap(), false);
    let rules = diagnostic_rules(&lint);

    assert!(rules.contains(&"rototo/variable-values-disallowed".to_owned()));
    assert!(
        !rules.contains(&"rototo/variable-value-type-mismatch".to_owned()),
        "{lint:#}"
    );
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
        "rototo/variable-rule-unknown-qualifier",
        "variables/bad-env.toml",
    );
    assert_reference_rule(
        &lint,
        "rototo/variable-unknown-value",
        "variables/bad-env.toml",
    );
    assert_reference_rule(
        &lint,
        "rototo/resource-schema-ref",
        "resources/bad-schema-ref.toml",
    );

    let qualifier = diagnostic_for_rule(&lint, "rototo/qualifier-predicate-unknown-qualifier");
    assert_eq!(qualifier["entity"]["kind"], "predicate");
    assert_eq!(qualifier["entity"]["qualifier"], "bad-reference");
    assert_eq!(qualifier["entity"]["index"], 0);

    let unknown_value_messages =
        diagnostic_messages_for_rule(&lint, "rototo/variable-unknown-value");
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
                index: 0,
            },
            primary: ExpectedPrimaryLocation::Document {
                path: "variables/checkout-redesign.toml",
                range: Some(ExpectedRange {
                    start_line: 11,
                    start_character: 12,
                    end_line: 11,
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
        "rototo/resource-object-schema-mismatch",
        "resources/bad-schema-value-objects/broken.toml",
    );
    assert_value_rule(
        &lint,
        "rototo/variable-value-type-mismatch",
        "variables/bad-type-value.toml",
    );
    assert_value_rule(
        &lint,
        "rototo/variable-unknown-type",
        "variables/unknown-type.toml",
    );

    let unknown_type = diagnostic_for_rule(&lint, "rototo/variable-unknown-type");
    assert_eq!(unknown_type["entity"]["kind"], "variable");
    assert_eq!(unknown_type["entity"]["id"], "unknown-type");
    assert!(unknown_type["location"]["range"].is_object());

    let schema_mismatch = diagnostic_for_rule(&lint, "rototo/resource-object-schema-mismatch");
    assert_eq!(schema_mismatch["entity"]["kind"], "resource_object");
    assert_eq!(schema_mismatch["entity"]["resource"], "bad-schema-value");
    assert_eq!(schema_mismatch["entity"]["key"], "broken");

    let type_mismatch = diagnostic_for_rule(&lint, "rototo/variable-value-type-mismatch");
    assert_eq!(type_mismatch["entity"]["kind"], "value");
    assert_eq!(type_mismatch["entity"]["variable"], "bad-type-value");
    assert_eq!(type_mismatch["entity"]["key"], "bad");
    assert!(type_mismatch["location"]["range"].is_object());
}

#[test]
fn schema_contract_normalizes_and_deduplicates_schema_documents() {
    let lint = lint_json("tests/fixtures/workspaces/schema-contract-normalized", true);
    let schema_documents = document_paths(&lint)
        .into_iter()
        .filter(|path| path == "schemas/value.schema.json")
        .count();

    assert!(lint["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(schema_documents, 1, "{lint:#}");
}

#[test]
fn schema_contract_skips_value_validation_when_schema_cannot_compile() {
    let lint = lint_json("tests/fixtures/workspaces/schema-contract-invalid", false);
    let rules = diagnostic_rules(&lint);

    assert_eq!(rules, vec!["rototo/schema-invalid"], "{lint:#}");
    let diagnostic = only_diagnostic(&lint);
    assert_eq!(diagnostic["stage"], "project");
    assert_eq!(diagnostic["entity"]["kind"], "schema");
    assert_eq!(diagnostic["entity"]["path"], "schemas/value.schema.json");
    assert_eq!(diagnostic["location"]["path"], "schemas/value.schema.json");
}

#[test]
fn schema_contract_skips_value_validation_when_schema_cannot_parse() {
    let lint = lint_json(
        "tests/fixtures/workspaces/schema-contract-parse-failed",
        false,
    );
    let rules = diagnostic_rules(&lint);

    assert_eq!(rules, vec!["rototo/schema-parse-failed"], "{lint:#}");
    let diagnostic = only_diagnostic(&lint);
    assert_eq!(diagnostic["entity"]["kind"], "schema");
    assert_eq!(diagnostic["entity"]["path"], "schemas/value.schema.json");
    assert_eq!(diagnostic["location"]["path"], "schemas/value.schema.json");
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
            .all(|diagnostic| diagnostic["location"]["range"].is_object())
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
    assert_eq!(
        alpha["message"],
        "qualifier participates in a reference cycle with: alpha, beta"
    );
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
fn self_referencing_qualifier_does_not_also_report_unreferenced() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("qualifiers")).unwrap();
    std::fs::write(
        root.join("rototo-workspace.toml"),
        r#"schema_version = 1
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("qualifiers/self.toml"),
        r#"schema_version = 1

[[predicate]]
attribute = "qualifier.self"
op = "eq"
value = true
"#,
    )
    .unwrap();

    let lint = lint_json(root.to_str().unwrap(), false);
    let rules = diagnostic_rules(&lint);

    assert_eq!(rules, vec!["rototo/qualifier-cycle"], "{lint:#}");
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
fn diagnostics_are_sorted_by_path_range_rule_and_message() {
    let lint = lint_json("tests/fixtures/workspaces/lint-failures", false);
    let keys = lint["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(diagnostic_order_key)
        .collect::<Vec<_>>();
    let mut sorted = keys.clone();
    sorted.sort();

    assert_eq!(keys, sorted, "{lint:#}");
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
    assert!(diagnostic["location"]["range"].is_object());
}

#[test]
fn reports_custom_registration_contract_failures() {
    let lint = lint_json(
        "tests/fixtures/workspaces/custom-registration-contract",
        false,
    );
    let invalid_messages =
        diagnostic_messages_for_rule(&lint, "rototo/custom-lint-registration-invalid");

    assert_eq!(invalid_messages.len(), 4, "{lint:#}");
    assert!(
        invalid_messages
            .contains(&"custom lint registration has unsupported stage: parse".to_owned())
    );
    assert!(
        invalid_messages
            .contains(&"custom lint registration has unsupported entity: predicate".to_owned())
    );
    assert!(
        invalid_messages
            .contains(&"custom lint registration has unsupported field: value.".to_owned())
    );
    assert!(
        invalid_messages.contains(
            &"custom lint registration has unsupported field: value.bad segment".to_owned()
        )
    );
    for diagnostic in diagnostics_for_rule(&lint, "rototo/custom-lint-registration-invalid") {
        assert_eq!(diagnostic["stage"], "register");
        assert_eq!(diagnostic["location"]["path"], "lint/register.lua");
    }
}

#[test]
fn reports_registered_custom_lint_targets() {
    let lint = lint_json("tests/fixtures/workspaces/custom-targets", false);

    assert_project_rule(&lint, "targets/workspace-extends", "rototo-workspace.toml");
    assert_project_rule(
        &lint,
        "targets/qualifier-predicates",
        "qualifiers/premium-users.toml",
    );
    assert_value_rule(
        &lint,
        "targets/variable-type",
        "variables/agent-config.toml",
    );
    assert_value_rule(
        &lint,
        "targets/returned-variable-type",
        "variables/agent-config.toml",
    );
    assert_value_rule(
        &lint,
        "targets/invalid-returned-field",
        "variables/agent-config.toml",
    );
    assert_value_rule(&lint, "targets/schema-json", "schemas/config.schema.json");

    let workspace = diagnostic_for_rule(&lint, "targets/workspace-extends");
    assert_eq!(workspace["entity"]["kind"], "workspace");
    assert_eq!(workspace["stage"], "project");
    assert!(workspace["location"]["range"].is_object());

    let qualifier = diagnostic_for_rule(&lint, "targets/qualifier-predicates");
    assert_eq!(qualifier["entity"]["kind"], "qualifier");
    assert_eq!(qualifier["entity"]["id"], "premium-users");
    assert_eq!(qualifier["stage"], "project");
    assert!(qualifier["location"]["range"].is_object());

    let variable = diagnostic_for_rule(&lint, "targets/variable-type");
    assert_eq!(variable["entity"]["kind"], "variable");
    assert_eq!(variable["entity"]["id"], "agent-config");
    assert_eq!(variable["stage"], "value");
    assert!(variable["location"]["range"].is_object());

    let returned = diagnostic_for_rule(&lint, "targets/returned-variable-type");
    assert_eq!(returned["entity"]["kind"], "variable");
    assert_eq!(returned["entity"]["id"], "agent-config");
    assert_eq!(returned["location"]["range"]["start"]["line"], 3);
    assert!(
        returned["location"]["range"]["start"]["character"]
            .as_u64()
            .unwrap()
            > 0
    );

    let invalid = diagnostic_for_rule(&lint, "targets/invalid-returned-field");
    assert_eq!(invalid["entity"]["kind"], "variable");
    assert_eq!(invalid["entity"]["id"], "agent-config");
    assert!(invalid["location"]["range"].is_null());

    let schema = diagnostic_for_rule(&lint, "targets/schema-json");
    assert_eq!(schema["entity"]["kind"], "schema");
    assert_eq!(schema["entity"]["path"], "schemas/config.schema.json");
    assert_eq!(schema["stage"], "value");
}

#[test]
fn reports_custom_warning_lint_without_failing() {
    let lint = lint_json("tests/fixtures/workspaces/custom-warning", true);
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "policy/advisory");
    assert_eq!(diagnostic["severity"], "warning");
    assert_eq!(diagnostic["stage"], "policy");
    assert_eq!(diagnostic["entity"]["kind"], "variable");
    assert_eq!(diagnostic["entity"]["id"], "message");
    assert_eq!(diagnostic["location"]["path"], "variables/message.toml");
    assert!(diagnostic["location"]["range"].is_object());
}

fn lint_json(workspace: &str, success: bool) -> serde_json::Value {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", workspace, "--json"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.success(),
        success,
        "unexpected lint status for {workspace}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "failed to parse lint JSON for {workspace}: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
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

fn diagnostic_order_key(diagnostic: &serde_json::Value) -> (String, u64, u64, String, String) {
    let range = &diagnostic["location"]["range"];
    let line = range["start"]["line"].as_u64().unwrap_or(0);
    let character = range["start"]["character"].as_u64().unwrap_or(0);
    (
        diagnostic["location"]["path"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        line,
        character,
        diagnostic["rule"].as_str().unwrap().to_owned(),
        diagnostic["message"].as_str().unwrap().to_owned(),
    )
}

fn assert_project_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "project");
    assert_eq!(diagnostic["location"]["path"], path);
}

fn assert_register_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "register");
    assert_eq!(diagnostic["location"]["path"], path);
}

fn assert_reference_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "reference");
    assert_eq!(diagnostic["location"]["path"], path);
}

fn assert_value_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "value");
    assert_eq!(diagnostic["location"]["path"], path);
}

fn assert_graph_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostics_for_rule(lint, rule)
        .into_iter()
        .find(|diagnostic| diagnostic["location"]["path"] == path)
        .unwrap_or_else(|| panic!("diagnostic not found: {rule} at {path}\n{lint:#}"));
    assert_eq!(diagnostic["stage"], "graph");
    assert_eq!(diagnostic["location"]["path"], path);
}

fn assert_policy_rule(lint: &serde_json::Value, rule: &str, path: &str) {
    let diagnostic = diagnostic_for_rule(lint, rule);
    assert_eq!(diagnostic["stage"], "policy");
    assert_eq!(diagnostic["location"]["path"], path);
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
    Rule {
        variable: &'static str,
        index: usize,
    },
    Schema(&'static str),
    CustomLintFile(&'static str),
    CustomRule(&'static str),
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct ExpectedRelatedLocation {
    path: &'static str,
    range: Option<ExpectedRange>,
    message: &'static str,
}

fn canonical_rule_fixtures() -> &'static [CanonicalRuleFixture] {
    &[
        CanonicalRuleFixture {
            rule: RototoRuleId::WorkspaceNotFound,
            workspace: "tests/fixtures/workspaces/rules/discover/workspace-not-found",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/workspace-not-found",
                severity: "error",
                stage: LintStage::Discover,
                entity: ExpectedEntity::Workspace,
                primary: ExpectedPrimaryLocation::WorkspaceRoot,
                related: &[],
            }],
        },
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
                        start_character: 0,
                        end_line: 2,
                        end_character: 1,
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
            rule: RototoRuleId::VariableResolveMissingDefault,
            workspace: "tests/fixtures/workspaces/rules/project/variable-env-missing-default",
            success: false,
            expected: &[
                ExpectedDiagnostic {
                    rule: "rototo/variable-resolve-missing-default",
                    severity: "error",
                    stage: LintStage::Project,
                    entity: ExpectedEntity::Variable("message"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "variables/message.toml",
                        range: Some(ExpectedRange {
                            start_line: 6,
                            start_character: 0,
                            end_line: 7,
                            end_character: 0,
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
            rule: RototoRuleId::VariableResolveShape,
            workspace: "tests/fixtures/workspaces/rules/project/variable-env-shape",
            success: false,
            expected: &[
                ExpectedDiagnostic {
                    rule: "rototo/variable-resolve-shape",
                    severity: "error",
                    stage: LintStage::Project,
                    entity: ExpectedEntity::Variable("message"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "variables/message.toml",
                        range: Some(ExpectedRange {
                            start_line: 3,
                            start_character: 10,
                            end_line: 3,
                            end_character: 15,
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
                            start_line: 6,
                            start_character: 10,
                            end_line: 6,
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
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 9,
                        start_character: 8,
                        end_line: 9,
                        end_character: 21,
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
                stage: LintStage::Register,
                entity: ExpectedEntity::CustomLintFile("lint/conflict.lua"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "lint/conflict.lua",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::WorkspaceContextSchemaRef,
            workspace: "tests/fixtures/workspaces/rules/project/workspace-context-schema-ref",
            success: false,
            expected: &[
                ExpectedDiagnostic {
                    rule: "rototo/workspace-context-schema-ref",
                    severity: "error",
                    stage: LintStage::Project,
                    entity: ExpectedEntity::Schema("schemas/context.schema.json"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "schemas/context.schema.json",
                        range: None,
                    },
                    related: &[],
                },
                ExpectedDiagnostic {
                    rule: "rototo/schema-invalid",
                    severity: "error",
                    stage: LintStage::Project,
                    entity: ExpectedEntity::Schema("schemas/context.schema.json"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "schemas/context.schema.json",
                        range: None,
                    },
                    related: &[],
                },
            ],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::WorkspaceContextSchemaReservedField,
            workspace: "tests/fixtures/workspaces/rules/project/workspace-context-schema-reserved-field",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/workspace-context-schema-reserved-field",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Schema("schemas/context.schema.json"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "schemas/context.schema.json",
                    range: None,
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
            rule: RototoRuleId::QualifierPredicateContextTypeMismatch,
            workspace: "tests/fixtures/workspaces/rules/reference/qualifier-predicate-context-type-mismatch",
            success: false,
            expected: &[
                ExpectedDiagnostic {
                    rule: "rototo/qualifier-predicate-context-type-mismatch",
                    severity: "error",
                    stage: LintStage::Reference,
                    entity: ExpectedEntity::Predicate {
                        qualifier: "boolean-in-string",
                        index: 0,
                    },
                    primary: ExpectedPrimaryLocation::Document {
                        path: "qualifiers/boolean-in-string.toml",
                        range: Some(ExpectedRange {
                            start_line: 5,
                            start_character: 8,
                            end_line: 5,
                            end_character: 16,
                        }),
                    },
                    related: &[],
                },
                ExpectedDiagnostic {
                    rule: "rototo/qualifier-predicate-context-type-mismatch",
                    severity: "error",
                    stage: LintStage::Reference,
                    entity: ExpectedEntity::Predicate {
                        qualifier: "integer-eq-string",
                        index: 0,
                    },
                    primary: ExpectedPrimaryLocation::Document {
                        path: "qualifiers/integer-eq-string.toml",
                        range: Some(ExpectedRange {
                            start_line: 5,
                            start_character: 8,
                            end_line: 5,
                            end_character: 11,
                        }),
                    },
                    related: &[],
                },
                ExpectedDiagnostic {
                    rule: "rototo/qualifier-predicate-context-type-mismatch",
                    severity: "error",
                    stage: LintStage::Reference,
                    entity: ExpectedEntity::Predicate {
                        qualifier: "object-bucket",
                        index: 0,
                    },
                    primary: ExpectedPrimaryLocation::Document {
                        path: "qualifiers/object-bucket.toml",
                        range: Some(ExpectedRange {
                            start_line: 4,
                            start_character: 5,
                            end_line: 4,
                            end_character: 13,
                        }),
                    },
                    related: &[],
                },
                ExpectedDiagnostic {
                    rule: "rototo/qualifier-predicate-context-type-mismatch",
                    severity: "error",
                    stage: LintStage::Reference,
                    entity: ExpectedEntity::Predicate {
                        qualifier: "string-gt-number",
                        index: 0,
                    },
                    primary: ExpectedPrimaryLocation::Document {
                        path: "qualifiers/string-gt-number.toml",
                        range: Some(ExpectedRange {
                            start_line: 4,
                            start_character: 5,
                            end_line: 4,
                            end_character: 9,
                        }),
                    },
                    related: &[],
                },
            ],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::SchemaUnreferenced,
            workspace: "tests/fixtures/workspaces/rules/reference/schema-unreferenced",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/schema-unreferenced",
                severity: "warning",
                stage: LintStage::Reference,
                entity: ExpectedEntity::Schema("schemas/unused.schema.json"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "schemas/unused.schema.json",
                    range: None,
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
            rule: RototoRuleId::VariableUnknownValue,
            workspace: "tests/fixtures/workspaces/rules/reference/variable-unknown-value",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-unknown-value",
                severity: "error",
                stage: LintStage::Reference,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 7,
                        start_character: 10,
                        end_line: 7,
                        end_character: 19,
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
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/checkout-redesign.toml",
                    range: Some(ExpectedRange {
                        start_line: 11,
                        start_character: 12,
                        end_line: 11,
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
                stage: LintStage::Value,
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
            rule: RototoRuleId::SchemaInvalid,
            workspace: "tests/fixtures/workspaces/rules/project/schema-invalid",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/schema-invalid",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Schema("schemas/broken.schema.json"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "schemas/broken.schema.json",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierCycle,
            workspace: "tests/fixtures/workspaces/rules/graph/qualifier-cycle",
            success: false,
            expected: &[
                ExpectedDiagnostic {
                    rule: "rototo/qualifier-cycle",
                    severity: "error",
                    stage: LintStage::Graph,
                    entity: ExpectedEntity::Qualifier("alpha"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "qualifiers/alpha.toml",
                        range: Some(ExpectedRange {
                            start_line: 3,
                            start_character: 12,
                            end_line: 3,
                            end_character: 28,
                        }),
                    },
                    related: &[ExpectedRelatedLocation {
                        path: "qualifiers/beta.toml",
                        range: Some(ExpectedRange {
                            start_line: 3,
                            start_character: 12,
                            end_line: 3,
                            end_character: 29,
                        }),
                        message: "cycle reference: beta -> alpha",
                    }],
                },
                ExpectedDiagnostic {
                    rule: "rototo/qualifier-cycle",
                    severity: "error",
                    stage: LintStage::Graph,
                    entity: ExpectedEntity::Qualifier("beta"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "qualifiers/beta.toml",
                        range: Some(ExpectedRange {
                            start_line: 3,
                            start_character: 12,
                            end_line: 3,
                            end_character: 29,
                        }),
                    },
                    related: &[ExpectedRelatedLocation {
                        path: "qualifiers/alpha.toml",
                        range: Some(ExpectedRange {
                            start_line: 3,
                            start_character: 12,
                            end_line: 3,
                            end_character: 28,
                        }),
                        message: "cycle reference: alpha -> beta",
                    }],
                },
                ExpectedDiagnostic {
                    rule: "rototo/qualifier-cycle",
                    severity: "error",
                    stage: LintStage::Graph,
                    entity: ExpectedEntity::Qualifier("self"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "qualifiers/self.toml",
                        range: Some(ExpectedRange {
                            start_line: 3,
                            start_character: 12,
                            end_line: 3,
                            end_character: 28,
                        }),
                    },
                    related: &[],
                },
            ],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierUnreferenced,
            workspace: "tests/fixtures/workspaces/rules/graph/qualifier-unreferenced",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/qualifier-unreferenced",
                severity: "warning",
                stage: LintStage::Graph,
                entity: ExpectedEntity::Qualifier("unused"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/unused.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierUnreachable,
            workspace: "tests/fixtures/workspaces/rules/graph/qualifier-unreachable",
            success: true,
            expected: &[
                ExpectedDiagnostic {
                    rule: "rototo/qualifier-unreachable",
                    severity: "warning",
                    stage: LintStage::Graph,
                    entity: ExpectedEntity::Qualifier("dead-leaf"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "qualifiers/dead-leaf.toml",
                        range: None,
                    },
                    related: &[],
                },
                ExpectedDiagnostic {
                    rule: "rototo/qualifier-unreferenced",
                    severity: "warning",
                    stage: LintStage::Graph,
                    entity: ExpectedEntity::Qualifier("dead-root"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "qualifiers/dead-root.toml",
                        range: None,
                    },
                    related: &[],
                },
            ],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::QualifierPredicateDuplicate,
            workspace: "tests/fixtures/workspaces/rules/graph/qualifier-predicate-duplicate",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/qualifier-predicate-duplicate",
                severity: "warning",
                stage: LintStage::Graph,
                entity: ExpectedEntity::Predicate {
                    qualifier: "premium-users",
                    index: 1,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "qualifiers/premium-users.toml",
                    range: Some(ExpectedRange {
                        start_line: 7,
                        start_character: 0,
                        end_line: 10,
                        end_character: 17,
                    }),
                },
                related: &[ExpectedRelatedLocation {
                    path: "qualifiers/premium-users.toml",
                    range: Some(ExpectedRange {
                        start_line: 2,
                        start_character: 0,
                        end_line: 5,
                        end_character: 17,
                    }),
                    message: "first matching predicate: 1",
                }],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleShadowed,
            workspace: "tests/fixtures/workspaces/rules/graph/variable-rule-shadowed",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-rule-shadowed",
                severity: "warning",
                stage: LintStage::Graph,
                entity: ExpectedEntity::Rule {
                    variable: "checkout",
                    index: 1,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/checkout.toml",
                    range: Some(ExpectedRange {
                        start_line: 16,
                        start_character: 12,
                        end_line: 16,
                        end_character: 27,
                    }),
                },
                related: &[ExpectedRelatedLocation {
                    path: "variables/checkout.toml",
                    range: Some(ExpectedRange {
                        start_line: 12,
                        start_character: 12,
                        end_line: 12,
                        end_character: 27,
                    }),
                    message: "first rule using qualifier: premium-users",
                }],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleSelectsDefaultValue,
            workspace: "tests/fixtures/workspaces/rules/graph/variable-rule-selects-default-value",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-rule-selects-default-value",
                severity: "warning",
                stage: LintStage::Graph,
                entity: ExpectedEntity::Rule {
                    variable: "message",
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 11,
                        start_character: 8,
                        end_line: 11,
                        end_character: 17,
                    }),
                },
                related: &[ExpectedRelatedLocation {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 7,
                        start_character: 10,
                        end_line: 7,
                        end_character: 19,
                    }),
                    message: "resolve default value: control",
                }],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableValueUnused,
            workspace: "tests/fixtures/workspaces/rules/graph/variable-value-unused",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-value-unused",
                severity: "warning",
                stage: LintStage::Graph,
                entity: ExpectedEntity::Value {
                    variable: "message",
                    key: "unused",
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 5,
                        start_character: 9,
                        end_line: 5,
                        end_character: 23,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::CustomLintFailed,
            workspace: "tests/fixtures/workspaces/rules/register/custom-lint-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/custom-lint-failed",
                severity: "error",
                stage: LintStage::Register,
                entity: ExpectedEntity::CustomLintFile("lint/broken.lua"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "lint/broken.lua",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::CustomLintFileUnregistered,
            workspace: "tests/fixtures/workspaces/rules/register/custom-lint-file-unregistered",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/custom-lint-file-unregistered",
                severity: "warning",
                stage: LintStage::Register,
                entity: ExpectedEntity::CustomLintFile("lint/empty.lua"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "lint/empty.lua",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::CustomLintRegistrationDuplicate,
            workspace: "tests/fixtures/workspaces/rules/register/custom-lint-registration-duplicate",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/custom-lint-registration-duplicate",
                severity: "warning",
                stage: LintStage::Register,
                entity: ExpectedEntity::CustomLintFile("lint/duplicate.lua"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "lint/duplicate.lua",
                    range: None,
                },
                related: &[ExpectedRelatedLocation {
                    path: "lint/duplicate.lua",
                    range: None,
                    message: "first matching registration",
                }],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::CustomLintRegistrationInvalid,
            workspace: "tests/fixtures/workspaces/rules/register/custom-lint-registration-invalid",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/custom-lint-registration-invalid",
                severity: "error",
                stage: LintStage::Register,
                entity: ExpectedEntity::CustomLintFile("lint/invalid.lua"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "lint/invalid.lua",
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
            rule: RototoRuleId::VariableTypeSource,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableUnknownResource,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableValuesDisallowed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::ResourceParseFailed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::ResourceObjectParseFailed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::ResourceSchemaVersion,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::ResourceSchemaRef,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::ResourceObjectSchemaMismatch,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::ResourceObjectUnknownReference,
        },
    ]
}

fn assert_canonical_fixture(fixture: &CanonicalRuleFixture) {
    let lint = lint_json(fixture.workspace, fixture.success);
    assert_expected_diagnostics(&lint, fixture.expected);
}

fn assert_expected_diagnostics(lint: &serde_json::Value, expected: &[ExpectedDiagnostic]) {
    let diagnostics = lint["diagnostics"].as_array().unwrap();
    let mut actual = diagnostics
        .iter()
        .map(diagnostic_contract_value)
        .map(|value| serde_json::to_string(&value).unwrap())
        .collect::<Vec<_>>();
    let mut expected = expected
        .iter()
        .map(|expected| expected_diagnostic_value(lint, *expected))
        .map(|value| serde_json::to_string(&value).unwrap())
        .collect::<Vec<_>>();

    actual.sort();
    expected.sort();
    assert_eq!(actual, expected, "unexpected diagnostics\n{lint:#}");
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
    assert!(diagnostic.get("primary").is_none());
    assert_expected_primary_location(lint, &diagnostic["location"], expected.primary);

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
            expected
                .range
                .map_or(serde_json::Value::Null, expected_range_value)
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
            assert!(primary.get("doc").is_none());
            assert!(primary["range"].is_null());
        }
        ExpectedPrimaryLocation::Document { path, range } => {
            assert_eq!(primary["path"], path);
            assert!(primary.get("doc").is_none());
            match range {
                Some(range) => assert_eq!(primary["range"], expected_range_value(range)),
                None => assert!(primary["range"].is_null()),
            }
        }
    }
}

fn diagnostic_contract_value(diagnostic: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "rule": diagnostic["rule"],
        "severity": diagnostic["severity"],
        "stage": diagnostic["stage"],
        "entity": diagnostic["entity"],
        "location": {
            "path": diagnostic["location"]["path"],
            "range": diagnostic["location"]["range"],
        },
        "related": diagnostic["related"]
            .as_array()
            .unwrap()
            .iter()
            .map(|related| {
                serde_json::json!({
                    "path": related["location"]["path"],
                    "range": related["location"]["range"],
                    "message": related["message"],
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn expected_diagnostic_value(
    lint: &serde_json::Value,
    expected: ExpectedDiagnostic,
) -> serde_json::Value {
    serde_json::json!({
        "rule": expected.rule,
        "severity": expected.severity,
        "stage": expected_stage_label(expected.stage),
        "entity": expected_entity_value(expected.entity),
        "location": expected_primary_location_value(lint, expected.primary),
        "related": expected.related
            .iter()
            .map(|related| {
                serde_json::json!({
                    "path": related.path,
                    "range": related.range.map_or(serde_json::Value::Null, expected_range_value),
                    "message": related.message,
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn expected_primary_location_value(
    lint: &serde_json::Value,
    expected: ExpectedPrimaryLocation,
) -> serde_json::Value {
    match expected {
        ExpectedPrimaryLocation::WorkspaceRoot => {
            serde_json::json!({
                "path": lint["workspace"],
                "range": serde_json::Value::Null,
            })
        }
        ExpectedPrimaryLocation::Document { path, range } => {
            serde_json::json!({
                "path": path,
                "range": range.map_or(serde_json::Value::Null, expected_range_value),
            })
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
        ExpectedEntity::Rule { variable, index } => {
            serde_json::json!({
                "kind": "rule",
                "variable": variable,
                "index": index,
            })
        }
        ExpectedEntity::Schema(path) => {
            serde_json::json!({ "kind": "schema", "path": path })
        }
        ExpectedEntity::CustomLintFile(path) => {
            serde_json::json!({ "kind": "custom_lint", "path": path })
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
