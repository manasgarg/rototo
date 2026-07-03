use assert_cmd::Command;
use predicates::prelude::*;
use rototo::diagnostics::{LintStage, RototoRuleId};
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn lints_basic_package() {
    let package = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", package.display());

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::eq(expected));
}

#[test]
fn lints_basic_package_as_json_with_documents() {
    let lint = lint_json("examples/basic", true);

    assert!(lint["diagnostics"].as_array().unwrap().is_empty());
    assert!(document_paths(&lint).contains(&"rototo-package.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"variables/premium_users.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"variables/checkout_redesign.toml".to_owned()));
    assert!(
        document_paths(&lint).contains(&"model/catalogs/llm_agent_config.schema.json".to_owned())
    );
    assert!(
        document_paths(&lint).contains(&"data/catalogs/llm_agent_config/local.toml".to_owned())
    );
    assert!(document_paths(&lint).contains(&"model/context/request.schema.json".to_owned()));
}

#[test]
fn lints_curated_examples() {
    for package in [
        "examples/quickstart",
        "examples/production",
        "examples/custom_lint",
    ] {
        let lint = lint_json(package, true);
        assert!(
            lint["diagnostics"].as_array().unwrap().is_empty(),
            "{package} should stay lint-clean\n{lint:#}"
        );
    }
}

#[test]
fn lints_catalog_references() {
    let lint = lint_json("tests/fixtures/packages/catalog-refs", true);
    assert!(
        lint["diagnostics"].as_array().unwrap().is_empty(),
        "{lint:#}"
    );
}

#[test]
fn reports_catalog_reference_failures() {
    let lint = lint_json("tests/fixtures/packages/catalog-ref-failures", false);
    let messages = diagnostic_messages_for_rule(&lint, "rototo/catalog-entry-unknown-reference");

    assert_eq!(messages.len(), 5, "{lint:#}");
    for expected in [
        "$.unknown_catalog references unknown catalog: missing-template",
        "$.unknown_entry references unknown email_template entry: absent",
        "$.invalid_pointer references invalid JSON Pointer: body",
        "$.missing_pointer references missing path /missing in email_template entry: welcome",
        "$.ambiguous_template references ambiguous catalog entry shared; found in catalogs: email_template, sms_template",
    ] {
        assert!(
            messages.contains(&expected.to_owned()),
            "missing {expected:?} in {messages:#?}"
        );
    }
}

#[test]
fn lints_discovered_package() {
    let package = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", package.display());

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .arg("lint")
        .assert()
        .success()
        .stdout(predicate::eq(expected));
}

#[test]
fn local_lint_applies_package_layers() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let child = temp.path().join("child");
    std::fs::create_dir_all(base.join("variables")).unwrap();
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(base.join("rototo-package.toml"), "schema_version = 1\n").unwrap();
    std::fs::write(
        base.join("variables/message.toml"),
        r#"schema_version = 1
type = "string"

[resolve]
default = "hello"
"#,
    )
    .unwrap();
    std::fs::write(
        child.join("rototo-package.toml"),
        r#"schema_version = 1
extends = ["../base"]
"#,
    )
    .unwrap();

    let lint = lint_json(child.to_str().unwrap(), true);
    assert!(document_paths(&lint).contains(&"variables/message.toml".to_owned()));
}

#[test]
fn local_lint_reports_invalid_extends_sources() {
    let temp = tempfile::TempDir::new().unwrap();
    let child = temp.path().join("child");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(
        child.join("rototo-package.toml"),
        r#"schema_version = 1
extends = ["../base", "  "]
"#,
    )
    .unwrap();

    let lint = lint_json(child.to_str().unwrap(), false);
    let diagnostic = only_diagnostic(&lint);
    assert_eq!(diagnostic["rule"], "rototo/package-manifest-schema-failed");
    assert_eq!(
        diagnostic["message"],
        "package extends source must not be blank"
    );
}

#[test]
fn reports_package_manifest_missing() {
    let lint = lint_json("tests/fixtures/packages/missing-manifest", false);
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "rototo/package-manifest-missing");
    assert_eq!(diagnostic["stage"], "discover");
    assert_eq!(diagnostic["target"]["entity"]["kind"], "package");
    assert!(diagnostic.get("primary").is_none());
    assert!(diagnostic["location"].get("doc").is_none());
    assert!(diagnostic["location"]["range"].is_null());
    assert!(lint["documents"].as_array().unwrap().is_empty());
}

#[test]
fn canonical_discover_fixture_reports_package_manifest_missing() {
    let lint = lint_json(
        "tests/fixtures/packages/rules/discover/package-manifest-missing",
        false,
    );

    assert_only_expected_diagnostic(
        &lint,
        ExpectedDiagnostic {
            rule: "rototo/package-manifest-missing",
            severity: "error",
            stage: LintStage::Discover,
            entity: ExpectedEntity::Package,
            primary: ExpectedPrimaryLocation::PackageRoot,
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
fn builtin_diagnostic_rule_ids_are_flat_rototo_ids() {
    for rule in RototoRuleId::iter() {
        let id = rule.meta().rule;
        assert!(
            id.starts_with("rototo/"),
            "built-in diagnostic id must start with rototo/: {id}"
        );
        assert_eq!(
            id.matches('/').count(),
            1,
            "built-in diagnostic id must be flat, for example rototo/variable-unknown-value: {id}"
        );
    }
}

#[test]
fn lint_failures_fixture_reports_expected_rule_ids() {
    let lint = lint_json("tests/fixtures/packages/lint-failures", false);
    let actual = lint["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["rule"].as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    let expected = lint_failures_expected_rule_ids()
        .iter()
        .map(|rule| (*rule).to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected, "unexpected lint-failures rules\n{lint:#}");
}

#[test]
fn package_fixture_parse_failures_are_intentional() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages");
    let expected = intentionally_malformed_fixture_files()
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    collect_fixture_parse_failures(&root, &root, &expected, &mut actual);

    assert_eq!(
        actual, expected,
        "fixture TOML/JSON parse failures should be explicit; update intentionally_malformed_fixture_files() for new parse-failure fixtures"
    );
}

#[test]
fn reports_package_manifest_parse_failed() {
    let lint = lint_json("tests/fixtures/packages/invalid-package-toml", false);
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "rototo/package-manifest-parse-failed");
    assert_eq!(diagnostic["stage"], "parse");
    assert_eq!(diagnostic["target"]["entity"]["kind"], "manifest");
    assert_eq!(diagnostic["location"]["path"], "rototo-package.toml");
    assert!(diagnostic["location"]["range"].is_object());
}

#[test]
fn reports_package_manifest_schema_failed() {
    let lint = lint_json("tests/fixtures/packages/unsupported-schema-version", false);
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "rototo/package-manifest-schema-failed");
    assert_eq!(diagnostic["stage"], "project");
    assert_eq!(diagnostic["target"]["entity"]["kind"], "manifest");
    assert_eq!(diagnostic["location"]["path"], "rototo-package.toml");
    assert!(diagnostic["location"]["range"].is_null());
}

#[test]
fn reports_package_file_parse_failed() {
    let lint = lint_json("tests/fixtures/packages/invalid-package-file-toml", false);
    let rules = diagnostic_rules(&lint);

    assert_eq!(rules, vec!["rototo/variable-parse-failed".to_owned()]);

    let variable = diagnostic_for_rule(&lint, "rototo/variable-parse-failed");
    assert_eq!(variable["stage"], "parse");
    assert_eq!(variable["target"]["entity"]["kind"], "variable");
    assert_eq!(variable["target"]["entity"]["id"], "broken");
    assert_eq!(variable["location"]["path"], "variables/broken.toml");
    assert!(variable["location"]["range"].is_object());
}

#[test]
fn reports_schema_ui_hint_rules() {
    let lint = lint_json(
        "tests/fixtures/packages/rules/project/schema-ui-unknown-widget",
        true,
    );
    let unknown = diagnostic_for_rule(&lint, "rototo/schema-ui-unknown-widget");
    assert_eq!(unknown["severity"], "warning");
    assert_eq!(unknown["target"]["entity"]["kind"], "catalog");
    assert_eq!(unknown["target"]["entity"]["id"], "panel");
    assert!(
        unknown["message"]
            .as_str()
            .unwrap()
            .contains("#/properties/title"),
        "{unknown:#}"
    );

    let lint = lint_json(
        "tests/fixtures/packages/rules/project/schema-ui-widget-type-mismatch",
        true,
    );
    let mismatch = diagnostic_for_rule(&lint, "rototo/schema-ui-widget-type-mismatch");
    assert!(
        mismatch["message"]
            .as_str()
            .unwrap()
            .contains("ui widget slider supports integer, number"),
        "{mismatch:#}"
    );

    let lint = lint_json(
        "tests/fixtures/packages/rules/project/schema-ui-widget-params",
        true,
    );
    let params = diagnostic_messages_for_rule(&lint, "rototo/schema-ui-widget-params");
    assert_eq!(params.len(), 1, "{lint:#}");
    assert!(
        params
            .iter()
            .any(|message| message.contains("unknown x-rototo-ui parameter steps")),
        "{params:#?}"
    );
}

#[test]
fn parse_diagnostics_handle_multibyte_text_near_syntax_errors() {
    let temp = tempfile::TempDir::new().unwrap();
    let toml_root = temp.path().join("toml");
    std::fs::create_dir_all(&toml_root).unwrap();
    std::fs::write(
        toml_root.join("rototo-package.toml"),
        "schema_version = 1\n[package]\nlabel = \"café\n",
    )
    .unwrap();
    let toml_lint = lint_json(toml_root.to_str().unwrap(), false);
    assert_eq!(
        only_diagnostic(&toml_lint)["rule"],
        "rototo/package-manifest-parse-failed"
    );

    let json_root = temp.path().join("json");
    std::fs::create_dir_all(json_root.join("model/catalogs")).unwrap();
    std::fs::write(
        json_root.join("rototo-package.toml"),
        "schema_version = 1\n",
    )
    .unwrap();
    std::fs::write(
        json_root.join("model/catalogs/broken.schema.json"),
        "{\"title\":\"café\",\"type\":}",
    )
    .unwrap();
    let json_lint = lint_json(json_root.to_str().unwrap(), false);
    assert_eq!(
        only_diagnostic(&json_lint)["rule"],
        "rototo/catalog-parse-failed"
    );
}

#[test]
fn reports_package_context_schema_ref_failures() {
    let parse_lint = lint_json("tests/fixtures/packages/context-schema-invalid-json", false);
    let parse_diagnostic = only_diagnostic(&parse_lint);
    assert_eq!(
        parse_diagnostic["rule"],
        "rototo/evaluation-context-parse-failed"
    );
    assert_eq!(
        parse_diagnostic["location"]["path"],
        "model/context/request.schema.json"
    );

    let schema_lint = lint_json(
        "tests/fixtures/packages/context-schema-invalid-schema",
        false,
    );
    assert_project_rule(
        &schema_lint,
        "rototo/evaluation-context-schema-invalid",
        "model/context/request.schema.json",
    );
}

#[test]
fn accepts_path_safety_normalized_refs() {
    let lint = lint_json("tests/fixtures/packages/path-safety-valid", true);

    assert!(lint["diagnostics"].as_array().unwrap().is_empty());
    assert!(document_paths(&lint).contains(&"rototo-package.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"model/context/request.schema.json".to_owned()));
    assert!(document_paths(&lint).contains(&"variables/message.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"model/catalogs/message.schema.json".to_owned()));
    assert!(document_paths(&lint).contains(&"data/catalogs/message/default.toml".to_owned()));
    assert!(document_paths(&lint).contains(&"lint/ok.lua".to_owned()));
}

#[test]
fn rejects_path_safety_escaping_refs_and_lint_files() {
    let lint = lint_json("tests/fixtures/packages/path-safety-escapes", false);

    assert_register_rule(&lint, "rototo/custom-lint-failed", "lint/escape.lua");

    let lint_file = diagnostic_for_rule(&lint, "rototo/custom-lint-failed");
    assert_eq!(lint_file["target"]["entity"]["kind"], "custom_lint");
    assert_eq!(lint_file["target"]["entity"]["path"], "lint/escape.lua");
    assert!(
        lint_file["message"]
            .as_str()
            .unwrap()
            .contains("path escapes package")
    );
}

#[test]
fn reports_package_context_schema_attribute_failures() {
    let lint = lint_json("tests/fixtures/packages/context-schema-attribute", false);
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(
        diagnostic["rule"],
        "rototo/variable-rule-undeclared-context-path"
    );
    assert_eq!(diagnostic["stage"], "graph");
    assert_eq!(diagnostic["target"]["entity"]["kind"], "rule");
    assert_eq!(diagnostic["target"]["entity"]["variable"], "message");
    assert_eq!(
        diagnostic["message"],
        "rule references undeclared context path: context.account.plan"
    );
    assert_eq!(diagnostic["location"]["path"], "variables/message.toml");
}

#[test]
fn reports_project_stage_variable_shape_failures() {
    let lint = lint_json("tests/fixtures/packages/rule-coverage", false);

    assert_project_rule(
        &lint,
        "rototo/variable-schema-version",
        "variables/missing_schema_version.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-type-source",
        "variables/type_or_schema.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-resolve-missing-default",
        "variables/resolve_missing_default.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-resolve-shape",
        "variables/resolve_shape.toml",
    );
    assert_project_rule(
        &lint,
        "rototo/variable-rule-shape",
        "variables/rule_shape.toml",
    );
}

#[test]
fn reports_project_stage_variable_when_failures() {
    let lint = lint_json("tests/fixtures/packages/lint-failures", false);

    assert_project_rule(
        &lint,
        "rototo/variable-rule-shape",
        "variables/bad_value_shape.toml",
    );
}

#[test]
fn enums_declare_members_and_type_variables() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("model/enums")).unwrap();
    std::fs::create_dir_all(root.join("data/enums")).unwrap();
    std::fs::create_dir_all(root.join("variables")).unwrap();
    std::fs::create_dir_all(root.join("model/context")).unwrap();
    std::fs::write(root.join("rototo-package.toml"), "schema_version = 1\n").unwrap();
    std::fs::write(
        root.join("model/context/request.schema.json"),
        r#"{"type":"object","properties":{"account":{"type":"object","properties":{"paid":{"type":"boolean"}}}}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("model/enums/plan_tiers.toml"),
        "schema_version = 1\ndescription = \"Plan tiers\"\ntype = \"string\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("data/enums/plan_tiers.toml"),
        "members = [\"free\", \"team\", \"business\"]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("variables/plan_tier.toml"),
        r#"schema_version = 1
type = "enum:plan_tiers"

[resolve]
default = "free"

[[resolve.rule]]
when = 'context.account.paid == true'
value = "team"
"#,
    )
    .unwrap();

    // Schema-level enum references: a catalog entry field and a context field
    // both pin their values to the enum with x-rototo-ref.
    std::fs::create_dir_all(root.join("model/catalogs")).unwrap();
    std::fs::create_dir_all(root.join("data/catalogs/plans")).unwrap();
    std::fs::create_dir_all(root.join("model/context/request-samples")).unwrap();
    std::fs::write(
        root.join("model/catalogs/plans.schema.json"),
        r#"{"type":"object","required":["tier"],"properties":{"tier":{"type":"string","x-rototo-ref":"enum:plan_tiers"}}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("data/catalogs/plans/starter.toml"),
        "tier = \"free\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("model/context/request-samples/paid.json"),
        r#"{"account":{"paid":true}}"#,
    )
    .unwrap();

    let lint = lint_json(root.to_str().unwrap(), true);
    assert!(
        lint["diagnostics"].as_array().unwrap().is_empty(),
        "{lint:#}"
    );

    // An entry value outside the member set fails the reference lint.
    std::fs::write(
        root.join("data/catalogs/plans/bogus.toml"),
        "tier = \"platinum\"\n",
    )
    .unwrap();
    let lint = lint_json(root.to_str().unwrap(), false);
    assert!(
        diagnostic_rules(&lint).contains(&"rototo/catalog-entry-unknown-reference".to_owned()),
        "{lint:#}"
    );
    std::fs::remove_file(root.join("data/catalogs/plans/bogus.toml")).unwrap();

    // A value outside the member set is rejected, an unknown enum is rejected,
    // and both halves of the enum must exist.
    std::fs::write(
        root.join("variables/bad_tier.toml"),
        r#"schema_version = 1
type = "enum:plan_tiers"

[resolve]
default = "platinum"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("variables/unknown_enum.toml"),
        r#"schema_version = 1
type = "enum:missing"

[resolve]
default = "anything"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("model/enums/orphan.toml"),
        "schema_version = 1\ntype = \"string\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("data/enums/undeclared.toml"),
        "members = [\"a\"]\n",
    )
    .unwrap();

    let lint = lint_json(root.to_str().unwrap(), false);
    let rules = diagnostic_rules(&lint);
    assert!(
        rules.contains(&"rototo/variable-unknown-value".to_owned()),
        "{lint:#}"
    );
    assert!(
        rules.contains(&"rototo/variable-unknown-enum".to_owned()),
        "{lint:#}"
    );
    assert!(
        rules.contains(&"rototo/enum-members-missing".to_owned()),
        "{lint:#}"
    );
    assert!(
        rules.contains(&"rototo/enum-members-undeclared".to_owned()),
        "{lint:#}"
    );
}

#[test]
fn catalog_schemas_support_union_and_nullable_scalars() {
    // Money-shaped catalogs need union and sentinel types: a tier bound that is
    // an integer or the literal "inf", and an amount that may be null. Plain
    // JSON Schema expresses both (type arrays and oneOf), and entry validation
    // enforces them. TOML cannot write null, so a nullable field is expressed
    // by omitting it (leave it out of required).
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("model/catalogs")).unwrap();
    std::fs::create_dir_all(root.join("data/catalogs/prices")).unwrap();
    std::fs::write(root.join("rototo-package.toml"), "schema_version = 1\n").unwrap();
    std::fs::write(
        root.join("model/catalogs/prices.schema.json"),
        r#"{
  "type": "object",
  "required": ["up_to"],
  "properties": {
    "amount": { "type": ["number", "null"] },
    "up_to": { "oneOf": [{ "type": "integer" }, { "const": "inf" }] }
  }
}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("data/catalogs/prices/tier_one.toml"),
        "amount = 10.5\nup_to = 100\n",
    )
    .unwrap();
    std::fs::write(
        root.join("data/catalogs/prices/tier_top.toml"),
        "up_to = \"inf\"\n",
    )
    .unwrap();

    let lint = lint_json(root.to_str().unwrap(), true);
    assert!(
        lint["diagnostics"].as_array().unwrap().is_empty(),
        "{lint:#}"
    );

    std::fs::write(
        root.join("data/catalogs/prices/bad.toml"),
        "amount = \"oops\"\nup_to = 2.5\n",
    )
    .unwrap();
    let lint = lint_json(root.to_str().unwrap(), false);
    let diagnostic = only_diagnostic(&lint);
    assert_eq!(diagnostic["rule"], "rototo/catalog-entry-schema-mismatch");
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("is not of types"),
        "{lint:#}"
    );
}

#[test]
fn reports_variable_rule_without_selector() {
    let lint = lint_json("tests/fixtures/packages/lint-failures", false);

    assert!(
        diagnostics_for_rule(&lint, "rototo/variable-rule-shape")
            .iter()
            .any(|diagnostic| {
                diagnostic["stage"] == "project"
                    && diagnostic["location"]["path"] == "variables/missing_rule_selector.toml"
            }),
        "{lint:#}"
    );
}

#[test]
fn catalog_value_file_can_represent_object_with_value_field() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("variables")).unwrap();
    std::fs::create_dir_all(root.join("data/catalogs/message")).unwrap();
    std::fs::create_dir_all(root.join("model/catalogs")).unwrap();
    std::fs::write(
        root.join("rototo-package.toml"),
        r#"schema_version = 1
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("model/catalogs/message.schema.json"),
        r#"{
  "type": "object",
  "properties": { "value": { "type": "string" } },
  "required": ["value"],
  "additionalProperties": false
}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("variables/message.toml"),
        r#"schema_version = 1
type = "catalog:message"

[resolve]
default = "default"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("data/catalogs/message/default.toml"),
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
fn catalog_backed_variable_values_are_rejected_before_value_validation() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("variables")).unwrap();
    std::fs::create_dir_all(root.join("data/catalogs/message")).unwrap();
    std::fs::create_dir_all(root.join("model/catalogs")).unwrap();
    std::fs::write(
        root.join("rototo-package.toml"),
        r#"schema_version = 1
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("model/catalogs/message.schema.json"),
        r#"{
  "type": "object",
  "properties": { "value": { "type": "string" } },
  "required": ["value"],
  "additionalProperties": false
}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("data/catalogs/message/default.toml"),
        r#"value = "catalog value""#,
    )
    .unwrap();
    std::fs::write(
        root.join("variables/message.toml"),
        r#"schema_version = 1
type = "catalog:message"

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
    let lint = lint_json("tests/fixtures/packages/lint-failures", false);

    assert_reference_rule(
        &lint,
        "rototo/variable-rule-unknown-variable",
        "variables/bad_resolve.toml",
    );
    assert_reference_rule(
        &lint,
        "rototo/variable-unknown-value",
        "variables/bad_resolve.toml",
    );
    let unknown_variable = diagnostic_for_rule(&lint, "rototo/variable-rule-unknown-variable");
    assert_eq!(unknown_variable["target"]["entity"]["kind"], "rule");
    assert_eq!(
        unknown_variable["target"]["entity"]["variable"],
        "bad_resolve"
    );
    assert_eq!(
        unknown_variable["target"]["field"]["kind"],
        "variable_rule_when"
    );

    let unknown_value_messages =
        diagnostic_messages_for_rule(&lint, "rototo/variable-unknown-value");
    assert!(
        unknown_value_messages
            .contains(&"rule references unknown catalog value: another-missing-value".to_owned())
    );
}

#[test]
fn reports_value_stage_failures() {
    let lint = lint_json("tests/fixtures/packages/lint-failures", false);

    assert_value_rule(
        &lint,
        "rototo/catalog-entry-schema-mismatch",
        "data/catalogs/bad_schema_value/broken.toml",
    );
    assert_value_rule(
        &lint,
        "rototo/variable-value-type-mismatch",
        "variables/bad_type_value.toml",
    );
    assert_value_rule(
        &lint,
        "rototo/variable-unknown-type",
        "variables/unknown_type.toml",
    );

    let unknown_type = diagnostic_for_rule(&lint, "rototo/variable-unknown-type");
    assert_eq!(unknown_type["target"]["entity"]["kind"], "variable");
    assert_eq!(unknown_type["target"]["entity"]["id"], "unknown_type");
    assert!(unknown_type["location"]["range"].is_object());

    let schema_mismatch = diagnostic_for_rule(&lint, "rototo/catalog-entry-schema-mismatch");
    assert_eq!(schema_mismatch["target"]["entity"]["kind"], "catalog_entry");
    assert_eq!(
        schema_mismatch["target"]["entity"]["catalog"],
        "bad_schema_value"
    );
    assert_eq!(schema_mismatch["target"]["entity"]["key"], "broken");

    let type_mismatch = diagnostic_for_rule(&lint, "rototo/variable-value-type-mismatch");
    assert_eq!(type_mismatch["target"]["entity"]["kind"], "variable");
    assert_eq!(type_mismatch["target"]["entity"]["id"], "bad_type_value");
    assert_eq!(
        type_mismatch["target"]["field"]["kind"],
        "variable_resolve_default"
    );
    assert!(type_mismatch["location"]["range"].is_object());
}

#[test]
fn schema_contract_discovers_direct_catalog_schema_documents() {
    let lint = lint_json("tests/fixtures/packages/schema-contract-normalized", true);
    let catalog_schema_documents = document_paths(&lint)
        .into_iter()
        .filter(|path| path.starts_with("model/catalogs/") && path.ends_with(".schema.json"))
        .count();

    assert!(lint["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(catalog_schema_documents, 2, "{lint:#}");
}

#[test]
fn schema_contract_skips_value_validation_when_schema_cannot_compile() {
    let lint = lint_json("tests/fixtures/packages/schema-contract-invalid", false);
    let rules = diagnostic_rules(&lint);

    assert_eq!(rules, vec!["rototo/catalog-schema-invalid"], "{lint:#}");
    let diagnostic = only_diagnostic(&lint);
    assert_eq!(diagnostic["stage"], "project");
    assert_eq!(diagnostic["target"]["entity"]["kind"], "catalog");
    assert_eq!(diagnostic["target"]["entity"]["id"], "message");
    assert_eq!(
        diagnostic["location"]["path"],
        "model/catalogs/message.schema.json"
    );
}

#[test]
fn schema_contract_skips_value_validation_when_schema_cannot_parse() {
    let lint = lint_json(
        "tests/fixtures/packages/schema-contract-parse-failed",
        false,
    );
    let rules = diagnostic_rules(&lint);

    assert_eq!(rules, vec!["rototo/catalog-parse-failed"], "{lint:#}");
    let diagnostic = only_diagnostic(&lint);
    assert_eq!(diagnostic["target"]["entity"]["kind"], "catalog");
    assert_eq!(diagnostic["target"]["entity"]["id"], "message");
    assert_eq!(
        diagnostic["location"]["path"],
        "model/catalogs/message.schema.json"
    );
}

#[test]
fn reports_graph_stage_shadowed_rule_warning_without_failing() {
    let lint = lint_json(
        "tests/fixtures/packages/rules/graph/variable-rule-shadowed",
        true,
    );
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "rototo/variable-rule-shadowed");
    assert_eq!(diagnostic["severity"], "warning");
    assert_eq!(diagnostic["stage"], "graph");
    assert_eq!(diagnostic["target"]["entity"]["kind"], "rule");
    assert_eq!(diagnostic["target"]["entity"]["variable"], "checkout");
    assert_eq!(diagnostic["target"]["entity"]["index"], 1);
    assert_eq!(diagnostic["related"].as_array().unwrap().len(), 1);
}

#[test]
fn lint_failures_fixture_covers_graph_rules() {
    let lint = lint_json("tests/fixtures/packages/lint-failures", false);

    assert_graph_rule(
        &lint,
        "rototo/variable-reference-cycle",
        "variables/cycle_a.toml",
    );
    assert_graph_rule(
        &lint,
        "rototo/variable-reference-cycle",
        "variables/self_cycle.toml",
    );
    assert_graph_rule(
        &lint,
        "rototo/variable-rule-shadowed",
        "variables/graph_warnings.toml",
    );
}

#[test]
fn diagnostics_are_sorted_by_path_range_rule_and_message() {
    let lint = lint_json("tests/fixtures/packages/lint-failures", false);
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
fn reports_package_custom_lint_failures() {
    let lint = lint_json("tests/fixtures/packages/lint-failures", false);

    assert_policy_rule(
        &lint,
        "fixture/custom-variable-rejected",
        "variables/custom_lint.toml",
    );
    assert_policy_rule(
        &lint,
        "fixture/custom-value-rejected",
        "variables/custom_value_lint.toml",
    );

    let variable = diagnostic_for_rule(&lint, "fixture/custom-variable-rejected");
    assert_eq!(variable["target"]["entity"]["kind"], "variable");
    assert_eq!(variable["target"]["entity"]["id"], "custom_lint");

    let value = diagnostic_for_rule(&lint, "fixture/custom-value-rejected");
    assert_eq!(value["target"]["entity"]["kind"], "variable");
    assert_eq!(value["target"]["entity"]["id"], "custom_value_lint");
    assert_eq!(value["target"]["field"]["kind"], "variable_resolve_default");
}

#[test]
fn reports_custom_lint_contract_failures() {
    let lint = lint_json("tests/fixtures/packages/custom-lint-contract", false);

    assert_policy_rule(
        &lint,
        "rototo/custom-lint-failed",
        "variables/custom_failed.toml",
    );
    assert_policy_rule(
        &lint,
        "payments/max-token-budget",
        "variables/custom_valid.toml",
    );
}

#[test]
fn reports_registered_custom_lint_failures() {
    let lint = lint_json("tests/fixtures/packages/custom-register", false);

    assert_register_rule(
        &lint,
        "rototo/custom-lint-registration-invalid",
        "lint/payments.lua",
    );
    assert_policy_rule(
        &lint,
        "payments/max-token-budget",
        "variables/agent_config.toml",
    );

    let diagnostic = diagnostic_for_rule(&lint, "payments/max-token-budget");
    assert_eq!(diagnostic["target"]["entity"]["kind"], "variable");
    assert_eq!(diagnostic["target"]["entity"]["id"], "agent_config");
    assert_eq!(diagnostic["stage"], "policy");
    assert!(diagnostic["location"]["range"].is_object());
}

#[test]
fn reports_custom_registration_contract_failures() {
    let lint = lint_json(
        "tests/fixtures/packages/custom-registration-contract",
        false,
    );
    let invalid_messages =
        diagnostic_messages_for_rule(&lint, "rototo/custom-lint-registration-invalid");

    assert_eq!(invalid_messages.len(), 4, "{lint:#}");
    for expected in [
        "custom lint registration has unsupported target: variables",
        "custom lint registration has unsupported target: /unknown",
        "custom lint registration has unsupported target: /variables/message/value",
        "custom lint registration has unsupported target: /variables/message/rules/not-number",
    ] {
        assert!(
            invalid_messages.contains(&expected.to_owned()),
            "missing {expected:?} in {invalid_messages:#?}"
        );
    }
    for diagnostic in diagnostics_for_rule(&lint, "rototo/custom-lint-registration-invalid") {
        assert_eq!(diagnostic["stage"], "register");
        assert_eq!(diagnostic["location"]["path"], "lint/register.lua");
    }
}

#[test]
fn reports_registered_custom_lint_targets() {
    let lint = lint_json("tests/fixtures/packages/custom-targets", false);

    assert_policy_rule(&lint, "targets/package-extends", "rototo-package.toml");
    assert_policy_rule(
        &lint,
        "targets/variable-type",
        "variables/agent_config.toml",
    );
    assert_policy_rule(
        &lint,
        "targets/returned-variable-type",
        "variables/agent_config.toml",
    );
    assert_policy_rule(
        &lint,
        "targets/invalid-returned-field",
        "variables/agent_config.toml",
    );
    assert_policy_rule(
        &lint,
        "targets/package-variable-default",
        "variables/agent_config.toml",
    );
    assert_policy_rule(
        &lint,
        "targets/catalog-entry-json-pointer",
        "data/catalogs/agent_config/standard.toml",
    );
    let package = diagnostic_for_rule(&lint, "targets/package-extends");
    assert_eq!(package["target"]["entity"]["kind"], "package");
    assert_eq!(package["stage"], "policy");
    assert!(package["location"]["range"].is_object());

    let variable = diagnostic_for_rule(&lint, "targets/variable-type");
    assert_eq!(variable["target"]["entity"]["kind"], "variable");
    assert_eq!(variable["target"]["entity"]["id"], "agent_config");
    assert_eq!(variable["stage"], "policy");
    assert!(variable["location"]["range"].is_object());

    let returned = diagnostic_for_rule(&lint, "targets/returned-variable-type");
    assert_eq!(returned["target"]["entity"]["kind"], "variable");
    assert_eq!(returned["target"]["entity"]["id"], "agent_config");
    assert_eq!(returned["location"]["range"]["start"]["line"], 3);
    assert!(
        returned["location"]["range"]["start"]["character"]
            .as_u64()
            .unwrap()
            > 0
    );

    let invalid = diagnostic_for_rule(&lint, "targets/invalid-returned-field");
    assert_eq!(invalid["target"]["entity"]["kind"], "variable");
    assert_eq!(invalid["target"]["entity"]["id"], "agent_config");
    assert!(invalid["location"]["range"].is_null());

    let default = diagnostic_for_rule(&lint, "targets/package-variable-default");
    assert_eq!(default["target"]["entity"]["kind"], "variable");
    assert_eq!(default["target"]["entity"]["id"], "agent_config");
    assert_eq!(
        default["target"]["field"]["kind"],
        "variable_resolve_default"
    );
    assert!(default["location"]["range"].is_object());

    let catalog_entry = diagnostic_for_rule(&lint, "targets/catalog-entry-json-pointer");
    assert_eq!(catalog_entry["target"]["entity"]["kind"], "catalog_entry");
    assert_eq!(catalog_entry["target"]["entity"]["catalog"], "agent_config");
    assert_eq!(catalog_entry["target"]["entity"]["key"], "standard");
    assert_eq!(catalog_entry["target"]["field"]["kind"], "value_json_path");
    assert_eq!(
        catalog_entry["target"]["field"]["path"],
        serde_json::json!(["max_output_tokens"])
    );
}

#[test]
fn reports_custom_warning_lint_without_failing() {
    let lint = lint_json("tests/fixtures/packages/custom-warning", true);
    let diagnostic = only_diagnostic(&lint);

    assert_eq!(diagnostic["rule"], "policy/advisory");
    assert_eq!(diagnostic["severity"], "warning");
    assert_eq!(diagnostic["stage"], "policy");
    assert_eq!(diagnostic["target"]["entity"]["kind"], "variable");
    assert_eq!(diagnostic["target"]["entity"]["id"], "message");
    assert_eq!(diagnostic["location"]["path"], "variables/message.toml");
    assert!(diagnostic["location"]["range"].is_object());
}

#[test]
fn enforces_json_schema_formats_on_catalog_entries() {
    // A catalog entry whose date-time field holds a non-timestamp is rejected,
    // proving JSON Schema `format` is asserted (not just annotated).
    let lint = lint_json("tests/fixtures/packages/format-enforcement", false);
    let diagnostics = lint["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic["message"]
            .as_str()
            .unwrap_or_default()
            .contains("date-time")),
        "expected a date-time format violation\n{lint:#}"
    );
}

#[test]
fn flags_cidr_use_of_a_context_path_without_an_ip_format() {
    // cidr() reads its subject as an IP, so the context schema must declare that
    // path with an ip format. A plain `type: string` declaration is a type match
    // but a refined-format miss, and is reported as a context-path-type mismatch.
    let lint = lint_json("tests/fixtures/packages/refined-context-types", false);
    let diagnostics = lint["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic["rule"].as_str() == Some("rototo/variable-rule-context-path-type-mismatch")
                && diagnostic["message"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("an IP address")
        }),
        "expected an IP-address refined-type mismatch\n{lint:#}"
    );
}

fn lint_json(package: &str, success: bool) -> serde_json::Value {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", package, "--json"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.success(),
        success,
        "unexpected lint status for {package}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "failed to parse lint JSON for {package}: {err}\nstdout:\n{}\nstderr:\n{}",
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
    package: &'static str,
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
    PackageRoot,
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
    Package,
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
    Catalog(&'static str),
    Layer(&'static str),
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
            rule: RototoRuleId::PackageNotFound,
            package: "tests/fixtures/packages/rules/discover/package-not-found",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/package-not-found",
                severity: "error",
                stage: LintStage::Discover,
                entity: ExpectedEntity::Package,
                primary: ExpectedPrimaryLocation::PackageRoot,
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::PackageManifestMissing,
            package: "tests/fixtures/packages/rules/discover/package-manifest-missing",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/package-manifest-missing",
                severity: "error",
                stage: LintStage::Discover,
                entity: ExpectedEntity::Package,
                primary: ExpectedPrimaryLocation::PackageRoot,
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::PackageManifestParseFailed,
            package: "tests/fixtures/packages/rules/parse/package-manifest-parse-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/package-manifest-parse-failed",
                severity: "error",
                stage: LintStage::Parse,
                entity: ExpectedEntity::Manifest,
                primary: ExpectedPrimaryLocation::Document {
                    path: "rototo-package.toml",
                    range: Some(ExpectedRange {
                        start_line: 2,
                        start_character: 8,
                        end_line: 3,
                        end_character: 0,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableParseFailed,
            package: "tests/fixtures/packages/rules/parse/variable-parse-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-parse-failed",
                severity: "error",
                stage: LintStage::Parse,
                entity: ExpectedEntity::Variable("broken"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/broken.toml",
                    range: Some(ExpectedRange {
                        start_line: 4,
                        start_character: 8,
                        end_line: 5,
                        end_character: 0,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::PackageManifestSchemaFailed,
            package: "tests/fixtures/packages/rules/project/package-manifest-schema-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/package-manifest-schema-failed",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Manifest,
                primary: ExpectedPrimaryLocation::Document {
                    path: "rototo-package.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableSchemaVersion,
            package: "tests/fixtures/packages/rules/project/variable-schema-version",
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
            rule: RototoRuleId::VariableResolveMissingDefault,
            package: "tests/fixtures/packages/rules/project/variable-resolve-missing-default",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-resolve-missing-default",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 4,
                        start_character: 0,
                        end_line: 5,
                        end_character: 0,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableResolveShape,
            package: "tests/fixtures/packages/rules/project/variable-resolve-shape",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-resolve-shape",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 4,
                        start_character: 10,
                        end_line: 4,
                        end_character: 15,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleShape,
            package: "tests/fixtures/packages/rules/project/variable-rule-shape",
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
                        start_line: 7,
                        start_character: 8,
                        end_line: 7,
                        end_character: 21,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::LayerParseFailed,
            package: "tests/fixtures/packages/rules/parse/layer-parse-failed",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/layer-parse-failed",
                severity: "error",
                stage: LintStage::Parse,
                entity: ExpectedEntity::Layer("broken"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "layers/broken.toml",
                    range: Some(ExpectedRange {
                        start_line: 1,
                        start_character: 23,
                        end_line: 2,
                        end_character: 0,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::LayerSchemaVersion,
            package: "tests/fixtures/packages/rules/project/layer-schema-version",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/layer-schema-version",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Layer("checkout"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "layers/checkout.toml",
                    range: Some(ExpectedRange {
                        start_line: 0,
                        start_character: 17,
                        end_line: 0,
                        end_character: 18,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::LayerShape,
            package: "tests/fixtures/packages/rules/project/layer-shape",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/layer-shape",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Layer("checkout"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "layers/checkout.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::LayerBucketOverlap,
            package: "tests/fixtures/packages/rules/project/layer-bucket-overlap",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/layer-bucket-overlap",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Layer("checkout"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "layers/checkout.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableQueryShape,
            package: "tests/fixtures/packages/rules/project/variable-query-shape",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-query-shape",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 7,
                        start_character: 8,
                        end_line: 7,
                        end_character: 14,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::CustomLintRuleConflict,
            package: "tests/fixtures/packages/rules/project/custom-lint-rule-conflict",
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
            rule: RototoRuleId::VariableUnknownValue,
            package: "tests/fixtures/packages/rules/reference/variable-unknown-value",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-unknown-value",
                severity: "error",
                stage: LintStage::Reference,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 5,
                        start_character: 10,
                        end_line: 5,
                        end_character: 19,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleUnknownVariable,
            package: "tests/fixtures/packages/rules/reference/variable-rule-unknown-variable",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-rule-unknown-variable",
                severity: "error",
                stage: LintStage::Reference,
                entity: ExpectedEntity::Rule {
                    variable: "checkout_redesign",
                    index: 0,
                },
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/checkout_redesign.toml",
                    range: Some(ExpectedRange {
                        start_line: 8,
                        start_character: 7,
                        end_line: 8,
                        end_character: 34,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableReferenceCycle,
            package: "tests/fixtures/packages/rules/graph/variable-reference-cycle",
            success: false,
            expected: &[
                ExpectedDiagnostic {
                    rule: "rototo/variable-reference-cycle",
                    severity: "error",
                    stage: LintStage::Graph,
                    entity: ExpectedEntity::Variable("loop_a"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "variables/loop_a.toml",
                        range: Some(ExpectedRange {
                            start_line: 7,
                            start_character: 7,
                            end_line: 7,
                            end_character: 28,
                        }),
                    },
                    related: &[ExpectedRelatedLocation {
                        path: "variables/loop_b.toml",
                        range: Some(ExpectedRange {
                            start_line: 7,
                            start_character: 7,
                            end_line: 7,
                            end_character: 28,
                        }),
                        message: "cycle reference: loop_b -> loop_a",
                    }],
                },
                ExpectedDiagnostic {
                    rule: "rototo/variable-reference-cycle",
                    severity: "error",
                    stage: LintStage::Graph,
                    entity: ExpectedEntity::Variable("loop_b"),
                    primary: ExpectedPrimaryLocation::Document {
                        path: "variables/loop_b.toml",
                        range: Some(ExpectedRange {
                            start_line: 7,
                            start_character: 7,
                            end_line: 7,
                            end_character: 28,
                        }),
                    },
                    related: &[ExpectedRelatedLocation {
                        path: "variables/loop_a.toml",
                        range: Some(ExpectedRange {
                            start_line: 7,
                            start_character: 7,
                            end_line: 7,
                            end_character: 28,
                        }),
                        message: "cycle reference: loop_a -> loop_b",
                    }],
                },
            ],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::IdNotSnakeCase,
            package: "tests/fixtures/packages/rules/project/id-not-snake-case",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/id-not-snake-case",
                severity: "error",
                stage: LintStage::Project,
                entity: ExpectedEntity::Variable("premium-users"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/premium-users.toml",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableUnknownType,
            package: "tests/fixtures/packages/rules/value/variable-unknown-type",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-unknown-type",
                severity: "error",
                stage: LintStage::Value,
                entity: ExpectedEntity::Variable("message"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 2,
                        start_character: 7,
                        end_line: 2,
                        end_character: 13,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableValueTypeMismatch,
            package: "tests/fixtures/packages/rules/value/variable-value-type-mismatch",
            success: false,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/variable-value-type-mismatch",
                severity: "error",
                stage: LintStage::Value,
                entity: ExpectedEntity::Variable("enabled"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "variables/enabled.toml",
                    range: Some(ExpectedRange {
                        start_line: 5,
                        start_character: 10,
                        end_line: 5,
                        end_character: 22,
                    }),
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleShadowed,
            package: "tests/fixtures/packages/rules/graph/variable-rule-shadowed",
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
                        start_line: 12,
                        start_character: 7,
                        end_line: 12,
                        end_character: 39,
                    }),
                },
                related: &[ExpectedRelatedLocation {
                    path: "variables/checkout.toml",
                    range: Some(ExpectedRange {
                        start_line: 8,
                        start_character: 7,
                        end_line: 8,
                        end_character: 39,
                    }),
                    message: "first rule using condition: context.user.tier == \"premium\"",
                }],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleSelectsDefaultValue,
            package: "tests/fixtures/packages/rules/graph/variable-rule-selects-default-value",
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
                        start_line: 9,
                        start_character: 8,
                        end_line: 9,
                        end_character: 17,
                    }),
                },
                related: &[ExpectedRelatedLocation {
                    path: "variables/message.toml",
                    range: Some(ExpectedRange {
                        start_line: 5,
                        start_character: 10,
                        end_line: 5,
                        end_character: 19,
                    }),
                    message: "resolve default value: \"control\"",
                }],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::CustomLintFailed,
            package: "tests/fixtures/packages/rules/register/custom-lint-failed",
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
            package: "tests/fixtures/packages/rules/register/custom-lint-file-unregistered",
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
            package: "tests/fixtures/packages/rules/register/custom-lint-registration-duplicate",
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
            package: "tests/fixtures/packages/rules/register/custom-lint-registration-invalid",
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
        CanonicalRuleFixture {
            rule: RototoRuleId::SchemaUiUnknownWidget,
            package: "tests/fixtures/packages/rules/project/schema-ui-unknown-widget",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/schema-ui-unknown-widget",
                severity: "warning",
                stage: LintStage::Project,
                entity: ExpectedEntity::Catalog("panel"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "model/catalogs/panel.schema.json",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::SchemaUiWidgetTypeMismatch,
            package: "tests/fixtures/packages/rules/project/schema-ui-widget-type-mismatch",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/schema-ui-widget-type-mismatch",
                severity: "warning",
                stage: LintStage::Project,
                entity: ExpectedEntity::Catalog("panel"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "model/catalogs/panel.schema.json",
                    range: None,
                },
                related: &[],
            }],
        },
        CanonicalRuleFixture {
            rule: RototoRuleId::SchemaUiWidgetParams,
            package: "tests/fixtures/packages/rules/project/schema-ui-widget-params",
            success: true,
            expected: &[ExpectedDiagnostic {
                rule: "rototo/schema-ui-widget-params",
                severity: "warning",
                stage: LintStage::Project,
                entity: ExpectedEntity::Catalog("panel"),
                primary: ExpectedPrimaryLocation::Document {
                    path: "model/catalogs/panel.schema.json",
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
            rule: RototoRuleId::EnumParseFailed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EnumSchemaVersion,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EnumShape,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EnumMembersParseFailed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EnumMembersShape,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EnumMembersMissing,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EnumMembersUndeclared,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableUnknownEnum,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::TraceWhenMissing,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::TraceWhenShape,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::TraceWhenInvalidReference,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleInvalidReference,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableTypeSource,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableUnknownCatalog,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableValuesDisallowed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::CatalogParseFailed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::CatalogEntryParseFailed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::CatalogSchemaInvalid,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::CatalogEntrySchemaMismatch,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::CatalogEntryUnknownReference,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EvaluationContextSchemaInvalid,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EvaluationContextSampleSchemaMismatch,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EvaluationContextSampleShape,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableEvaluationContextConflict,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleUndeclaredContextPath,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::VariableRuleContextPathTypeMismatch,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EvaluationContextParseFailed,
        },
        PendingCanonicalRuleFixture {
            rule: RototoRuleId::EvaluationContextSampleParseFailed,
        },
    ]
}

fn assert_canonical_fixture(fixture: &CanonicalRuleFixture) {
    let lint = lint_json(fixture.package, fixture.success);
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
    assert_eq!(
        diagnostic["target"]["entity"],
        expected_entity_value(expected.entity)
    );
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
        ExpectedPrimaryLocation::PackageRoot => {
            assert_eq!(primary["path"], lint["package"]);
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
        "entity": diagnostic["target"]["entity"],
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
        ExpectedPrimaryLocation::PackageRoot => {
            serde_json::json!({
                "path": lint["package"],
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
        ExpectedEntity::Package => serde_json::json!({ "kind": "package" }),
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
        ExpectedEntity::Catalog(id) => {
            serde_json::json!({ "kind": "catalog", "id": id })
        }
        ExpectedEntity::Layer(id) => {
            serde_json::json!({ "kind": "layer", "id": id })
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

fn lint_failures_expected_rule_ids() -> &'static [&'static str] {
    &[
        "fixture/custom-value-rejected",
        "fixture/custom-variable-rejected",
        "rototo/catalog-entry-schema-mismatch",
        "rototo/catalog-schema-invalid",
        "rototo/layer-bucket-overlap",
        "rototo/layer-shape",
        "rototo/schema-ui-unknown-widget",
        "rototo/schema-ui-widget-params",
        "rototo/schema-ui-widget-type-mismatch",
        "rototo/trace-when-invalid-reference",
        "rototo/trace-when-missing",
        "rototo/trace-when-shape",
        "rototo/variable-evaluation-context-conflict",
        "rototo/variable-query-shape",
        "rototo/variable-reference-cycle",
        "rototo/variable-rule-context-path-type-mismatch",
        "rototo/variable-rule-invalid-reference",
        "rototo/variable-rule-shadowed",
        "rototo/variable-rule-shape",
        "rototo/variable-rule-undeclared-context-path",
        "rototo/variable-rule-unknown-variable",
        "rototo/variable-unknown-type",
        "rototo/variable-unknown-value",
        "rototo/variable-value-type-mismatch",
    ]
}

fn intentionally_malformed_fixture_files() -> &'static [&'static str] {
    &[
        "context-schema-invalid-json/model/context/request.schema.json",
        "invalid-package-file-toml/variables/broken.toml",
        "invalid-package-toml/rototo-package.toml",
        "rules/parse/variable-external-value-parse-failed/variables/external_message-values/broken.toml",
        "rules/parse/layer-parse-failed/layers/broken.toml",
        "rules/parse/variable-parse-failed/variables/broken.toml",
        "rules/parse/package-manifest-parse-failed/rototo-package.toml",
        "schema-contract-parse-failed/model/catalogs/message.schema.json",
    ]
}

fn collect_fixture_parse_failures(
    root: &Path,
    dir: &Path,
    expected: &BTreeSet<String>,
    failures: &mut BTreeSet<String>,
) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_fixture_parse_failures(root, &path, expected, failures);
            continue;
        }
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("toml") => {
                if let Err(err) = std::fs::read_to_string(&path)
                    .map_err(|err| err.to_string())
                    .and_then(|text| {
                        toml::from_str::<toml::Value>(&text).map_err(|err| err.to_string())
                    })
                {
                    let relative = relative_fixture_path(root, &path);
                    assert!(
                        expected.contains(&relative),
                        "unexpected TOML parse failure in {relative}: {err}"
                    );
                    failures.insert(relative);
                }
            }
            Some("json") => {
                if let Err(err) = std::fs::read_to_string(&path)
                    .map_err(|err| err.to_string())
                    .and_then(|text| {
                        serde_json::from_str::<serde_json::Value>(&text)
                            .map_err(|err| err.to_string())
                    })
                {
                    let relative = relative_fixture_path(root, &path);
                    assert!(
                        expected.contains(&relative),
                        "unexpected JSON parse failure in {relative}: {err}"
                    );
                    failures.insert(relative);
                }
            }
            _ => {}
        }
    }
}

fn relative_fixture_path(root: &Path, path: &Path) -> String {
    let relative = path
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    assert!(
        !relative.is_empty(),
        "fixture parser reported an empty path for {path:?}"
    );
    relative
}
