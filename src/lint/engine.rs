use crate::diagnostics::{LintDiagnostic, LintStage};
use crate::error::Result;
use crate::model::WorkspaceLint;

use super::index::*;
use super::input::LintInput;
use super::output::sort_diagnostics;
use super::references::ReferenceIndex;
use super::source::SourceStore;
use super::syntax::SyntaxIndex;
use super::{WorkspaceLintSnapshot, stages};

pub(super) async fn lint_workspace_snapshot(input: LintInput) -> Result<WorkspaceLintSnapshot> {
    LintEngine::new().lint_workspace(input).await
}

pub(crate) async fn lint_workspace_until(
    input: LintInput,
    stage: LintStage,
) -> Result<LintContext> {
    LintEngine::new().lint_workspace_until(input, stage).await
}

struct LintEngine;

impl LintEngine {
    fn new() -> Self {
        Self
    }

    async fn lint_workspace(&self, input: LintInput) -> Result<WorkspaceLintSnapshot> {
        let mut ctx = LintContext::new(input);
        stages::run_pipeline(&mut ctx).await?;
        Ok(ctx.finish())
    }

    async fn lint_workspace_until(
        &self,
        input: LintInput,
        stage: LintStage,
    ) -> Result<LintContext> {
        let mut ctx = LintContext::new(input);
        stages::run_until(&mut ctx, stage).await?;
        Ok(ctx)
    }
}

pub(crate) struct LintContext {
    pub(super) input: LintInput,
    pub(super) source: SourceStore,
    pub(super) syntax: SyntaxIndex,
    pub(super) index: SemanticIndex,
    pub(super) references: ReferenceIndex,
    pub(super) diagnostics: Vec<LintDiagnostic>,
}

impl LintContext {
    fn new(input: LintInput) -> Self {
        let source = SourceStore::new(input.root.clone(), input.overlays.clone());
        Self {
            source,
            input,
            syntax: SyntaxIndex::default(),
            index: SemanticIndex::default(),
            references: ReferenceIndex::default(),
            diagnostics: Vec::new(),
        }
    }

    fn finish(mut self) -> WorkspaceLintSnapshot {
        sort_diagnostics(&mut self.diagnostics);
        let documents = self.source.document_summaries();
        let lint = WorkspaceLint {
            root: self.source.root,
            documents,
            diagnostics: self.diagnostics,
        };
        WorkspaceLintSnapshot {
            lint,
            index: self.index,
            references: self.references,
        }
    }
}

pub(super) fn variable_values<'a>(
    ctx: &'a LintContext,
    variable: &'a VariableNode,
) -> impl Iterator<Item = &'a ValueNode> {
    variable.values.inline_values.values().chain(
        ctx.index
            .external_values
            .get(&variable.id)
            .into_iter()
            .flat_map(|values| values.values()),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::diagnostics::{CustomRuleId, DiagnosticRule, EntityId, RototoRuleId};

    use super::super::WORKSPACE_MANIFEST;
    use super::super::index::{GateEntity, ValueOrigin};
    use super::super::index::{RegisteredLintEntity, RegisteredLintField, SchemaLintField};
    use super::super::input::OverlayDocument;
    use super::*;

    #[tokio::test]
    async fn snapshot_lints_overlay_without_writing_to_disk_and_groups_empty_documents() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join(WORKSPACE_MANIFEST),
            r#"schema_version = 1

[environments]
values = ["prod"]
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[values]
control = "hello"

[env._]
value = "control"
"#;
        tokio::fs::write(root.join("variables/message.toml"), disk_variable)
            .await
            .unwrap();

        let invalid_overlay = r#"schema_version = 1
type = "mystery"

[values]
control = "hello"

[env._]
value = "control"
"#;
        let mut input = LintInput::new(root.to_path_buf());
        input.overlays.insert(
            "variables/message.toml".to_owned(),
            OverlayDocument {
                text: invalid_overlay.to_owned(),
                version: Some(42),
            },
        );
        let snapshot = lint_workspace_snapshot(input).await.unwrap();
        let lint = &snapshot.lint;

        let diagnostic = lint
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.rule.as_string() == "rototo/variable-unknown-type")
            .unwrap();
        assert_eq!(diagnostic.primary.path, "variables/message.toml");
        assert_eq!(diagnostic.primary.range.unwrap().start.line, 1);

        let variable_document = lint
            .documents
            .iter()
            .find(|document| document.path == "variables/message.toml")
            .unwrap();
        assert_eq!(variable_document.version, Some(42));

        let grouped = lint.diagnostics_by_document();
        assert!(grouped.iter().any(|group| {
            group.document.path == "rototo-workspace.toml" && group.diagnostics.is_empty()
        }));
        assert!(grouped.iter().any(|group| {
            group.document.path == "variables/message.toml" && !group.diagnostics.is_empty()
        }));
        let disk_after_overlay = tokio::fs::read_to_string(root.join("variables/message.toml"))
            .await
            .unwrap();
        assert_eq!(disk_after_overlay, disk_variable);

        let symbols = snapshot.document_symbols("variables/message.toml");
        assert_eq!(symbols[0].name, "message");
        assert!(symbols[0].children.iter().any(|symbol| {
            symbol.name == "values" && symbol.children.iter().any(|child| child.name == "control")
        }));

        let mut cleared_input = LintInput::new(root.to_path_buf());
        cleared_input.overlays.insert(
            "variables/message.toml".to_owned(),
            OverlayDocument {
                text: disk_variable.to_owned(),
                version: Some(43),
            },
        );
        let cleared = lint_workspace_snapshot(cleared_input).await.unwrap();

        assert!(cleared.lint.diagnostics.is_empty());
        let cleared_groups = cleared.lint.diagnostics_by_document();
        let variable_group = cleared_groups
            .iter()
            .find(|group| group.document.path == "variables/message.toml")
            .unwrap();
        assert_eq!(variable_group.document.version, Some(43));
        assert!(variable_group.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn snapshot_discovers_overlay_only_workspace_files() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let mut input = LintInput::new(root.to_path_buf());
        for (path, text) in [
            (
                WORKSPACE_MANIFEST,
                r#"schema_version = 1

[environments]
values = ["prod"]

[context]
schema = "schemas/context.schema.json"

[[lint.rule]]
id = "policy/noop"
title = "No-op policy"
help = "No-op policy used by tests."
"#,
            ),
            (
                "qualifiers/premium.toml",
                r#"schema_version = 1

[[predicate]]
attribute = "account.tier"
op = "eq"
value = "premium"
"#,
            ),
            (
                "variables/message.toml",
                r#"schema_version = 1
schema = "../schemas/message.schema.json"

[env._]
value = "default"

[env.prod]
value = "default"

[[env.prod.rule]]
qualifier = "premium"
value = "premium"
"#,
            ),
            (
                "variables/message-values/default.toml",
                r#"message = "hello""#,
            ),
            (
                "variables/message-values/premium.toml",
                r#"message = "premium""#,
            ),
            (
                "schemas/message.schema.json",
                r#"{
  "type": "object",
  "properties": { "message": { "type": "string" } },
  "required": ["message"],
  "additionalProperties": false
}"#,
            ),
            (
                "schemas/context.schema.json",
                r#"{
  "type": "object",
  "properties": {
    "account": {
      "type": "object",
      "properties": {
        "tier": { "type": "string" }
      }
    }
  }
}"#,
            ),
            (
                "lint/noop.lua",
                r#"function register(lint)
  lint:on({
    stage = "policy",
    entity = "workspace",
    rule = "policy/noop",
    handler = "check_workspace",
  })
end

function check_workspace(ctx)
  return {}
end
"#,
            ),
        ] {
            input.overlays.insert(
                path.to_owned(),
                OverlayDocument {
                    text: text.to_owned(),
                    version: Some(7),
                },
            );
        }

        let snapshot = lint_workspace_snapshot(input).await.unwrap();
        let lint = &snapshot.lint;
        let paths = lint
            .documents
            .iter()
            .map(|document| document.path.as_str())
            .collect::<Vec<_>>();

        assert!(lint.diagnostics.is_empty(), "{:#?}", lint.diagnostics);
        for expected in [
            WORKSPACE_MANIFEST,
            "qualifiers/premium.toml",
            "variables/message.toml",
            "variables/message-values/default.toml",
            "variables/message-values/premium.toml",
            "schemas/message.schema.json",
            "schemas/context.schema.json",
            "lint/noop.lua",
        ] {
            assert!(
                paths.contains(&expected),
                "missing overlay document {expected}"
            );
        }

        let symbols = snapshot.document_symbols("variables/message.toml");
        assert_eq!(symbols[0].name, "message");
    }

    #[tokio::test]
    async fn snapshot_diagnostic_ranges_cover_references_and_external_values() {
        let reference_snapshot = lint_workspace_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/workspaces/rules/reference/variable-rule-unknown-qualifier",
        )))
        .await
        .unwrap();
        let reference = diagnostic_by_rule(
            &reference_snapshot.lint,
            "rototo/variable-rule-unknown-qualifier",
        );
        assert_eq!(reference.primary.path, "variables/checkout-redesign.toml");
        assert_eq!(reference.primary.range.unwrap().start.line, 14);
        assert_eq!(reference.primary.range.unwrap().start.character, 12);
        assert_eq!(reference.primary.range.unwrap().end.line, 14);
        assert_eq!(reference.primary.range.unwrap().end.character, 27);

        let external_value_snapshot = lint_workspace_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/workspaces/rules/project/variable-external-value-duplicate",
        )))
        .await
        .unwrap();
        let external_value = diagnostic_by_rule(
            &external_value_snapshot.lint,
            "rototo/variable-external-value-duplicate",
        );
        assert_eq!(
            external_value.primary.path,
            "variables/external-message-values/default.toml"
        );
        assert_eq!(external_value.primary.range.unwrap().start.line, 0);
        assert_eq!(external_value.primary.range.unwrap().start.character, 0);
        assert_eq!(external_value.primary.range.unwrap().end.line, 1);
        assert_eq!(external_value.primary.range.unwrap().end.character, 0);

        let external_node = external_value_snapshot
            .index
            .external_values
            .get("external-message")
            .and_then(|values| values.get("default"))
            .expect("external value node");
        assert_eq!(external_node.variable_id, "external-message");
        assert!(matches!(
            &external_node.origin,
            ValueOrigin::External { path, .. }
                if path == "variables/external-message-values/default.toml"
        ));
    }

    #[tokio::test]
    async fn snapshot_index_records_custom_lint_registry() {
        let snapshot = lint_workspace_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/workspaces/custom-targets",
        )))
        .await
        .unwrap();
        let registry = &snapshot.index.custom_lints;

        let schema_rule = CustomRuleId::parse("targets/schema-json").unwrap();
        let schema_definition = registry.rules.get(&schema_rule).unwrap();
        assert_eq!(
            schema_definition.definition.title,
            "Schema JSON target was checked"
        );
        assert_eq!(schema_definition.location.path, "rototo-workspace.toml");

        let file = registry.files.get("lint/targets.lua").unwrap();
        assert_eq!(file.path, "lint/targets.lua");
        assert_eq!(file.location.path, "lint/targets.lua");

        let registration = registry
            .registrations
            .iter()
            .find(|registration| registration.rule == schema_rule)
            .unwrap();
        assert_eq!(registration.file_path, "lint/targets.lua");
        assert_eq!(registration.stage, LintStage::Value);
        assert_eq!(registration.location.path, "lint/targets.lua");
        assert!(matches!(
            registration.selector.entity,
            RegisteredLintEntity::Schema
        ));
        assert!(matches!(
            &registration.selector.field,
            Some(RegisteredLintField::Schema(SchemaLintField::JsonPath(path)))
                if path.as_slice() == ["properties"]
        ));
    }

    #[tokio::test]
    async fn snapshot_gate_index_records_source_backed_failures() {
        let parse_snapshot = lint_workspace_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/workspaces/rules/parse/variable-parse-failed",
        )))
        .await
        .unwrap();
        let parse_gate = parse_snapshot
            .index
            .gates
            .entity_state
            .get(&GateEntity::Variable("broken".to_owned()))
            .expect("variable parse gate");
        assert_eq!(parse_gate.blocked_at, LintStage::Parse);
        assert_eq!(
            parse_gate.diagnostic.as_ref(),
            Some(&DiagnosticRule::Rototo(RototoRuleId::VariableParseFailed))
        );

        let register_snapshot = lint_workspace_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/workspaces/rules/register/custom-lint-failed",
        )))
        .await
        .unwrap();
        let register_gate = register_snapshot
            .index
            .gates
            .entity_state
            .get(&GateEntity::CustomLintFile("lint/broken.lua".to_owned()))
            .expect("custom lint file gate");
        assert_eq!(register_gate.blocked_at, LintStage::Register);
        assert_eq!(
            register_gate.diagnostic.as_ref(),
            Some(&DiagnosticRule::Rototo(RototoRuleId::CustomLintFailed))
        );
    }

    #[tokio::test]
    async fn snapshot_reference_index_records_resolved_and_unresolved_edges() {
        use super::super::references::{ReferenceSource, ReferenceTarget};

        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("qualifiers"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("schemas"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join(WORKSPACE_MANIFEST),
            r#"schema_version = 1

[environments]
values = ["prod", "stage"]

[context]
schema = "schemas/context.schema.json"
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("qualifiers/beta.toml"),
            r#"schema_version = 1

[[predicate]]
attribute = "account.beta"
op = "eq"
value = true
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("qualifiers/premium.toml"),
            r#"schema_version = 1

[[predicate]]
attribute = "qualifier.beta"
op = "eq"
value = true

[[predicate]]
attribute = "account.region"
op = "eq"
value = "eu"
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("variables/message.toml"),
            r#"schema_version = 1
schema = "../schemas/message.schema.json"

[values]
control = "hello"
treatment = "welcome"

[env._]
value = "control"

[env.prod]
value = "control"
rule = [
  { qualifier = "premium", value = "treatment" },
]

[env.stage]
value = "missing"
rule = [
  { qualifier = "missing", value = "absent" },
]
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("schemas/context.schema.json"),
            r#"{"type":"object"}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("schemas/message.schema.json"),
            r#"{"type":"string"}"#,
        )
        .await
        .unwrap();

        let snapshot = lint_workspace_snapshot(LintInput::new(root.to_path_buf()))
            .await
            .unwrap();

        assert!(snapshot.references.edges().iter().any(|edge| {
            matches!(edge.source, ReferenceSource::ManifestContextSchema)
                && edge.target == ReferenceTarget::Schema("schemas/context.schema.json".to_owned())
                && edge.is_resolved()
        }));
        assert!(snapshot.references.edges().iter().any(|edge| {
            matches!(
                edge.source,
                ReferenceSource::QualifierPredicateQualifier { .. }
            ) && edge.target == ReferenceTarget::Qualifier("beta".to_owned())
                && edge.is_resolved()
        }));
        assert!(snapshot.references.edges().iter().any(|edge| {
            matches!(
                edge.source,
                ReferenceSource::QualifierPredicateContextAttribute { .. }
            ) && edge.target == ReferenceTarget::ContextAttribute("account.region".to_owned())
        }));
        assert!(snapshot.references.edges().iter().any(|edge| {
            matches!(edge.source, ReferenceSource::VariableRuleQualifier { .. })
                && edge.target == ReferenceTarget::Qualifier("missing".to_owned())
                && !edge.is_resolved()
        }));
        assert!(snapshot.references.edges().iter().any(|edge| {
            matches!(edge.source, ReferenceSource::VariableRuleValue { .. })
                && edge.target
                    == ReferenceTarget::VariableValue {
                        variable: "message".to_owned(),
                        value: "absent".to_owned(),
                    }
                && !edge.is_resolved()
        }));

        let referenced_qualifiers = snapshot.references.referenced_qualifier_ids();
        assert!(referenced_qualifiers.contains("beta"));
        assert!(referenced_qualifiers.contains("premium"));
        assert!(
            snapshot
                .references
                .qualifier_reference_sites("premium")
                .iter()
                .any(|site| matches!(
                site.from,
                EntityId::Rule {
                    ref variable,
                    ref environment,
                    index: 0,
                } if variable == "message" && environment == "prod"
                ) && site.location.path == "variables/message.toml")
        );
        assert!(
            snapshot
                .references
                .variable_value_reference_sites("message", "treatment")
                .iter()
                .any(|site| matches!(
                    site.from,
                    EntityId::Rule {
                        ref variable,
                        ref environment,
                        index: 0,
                    } if variable == "message" && environment == "prod"
                ))
        );
        assert!(
            snapshot
                .references
                .variable_value_reference_sites("message", "absent")
                .is_empty()
        );

        let context_schema = snapshot
            .index
            .schemas
            .get("schemas/context.schema.json")
            .expect("context schema node");
        assert_eq!(context_schema.doc, context_schema.location.doc().unwrap());
        assert_eq!(context_schema.path, "schemas/context.schema.json");
        assert!(context_schema.json.is_some());
        assert!(context_schema.validator.is_some());
        assert!(context_schema.invalid_message.is_none());
    }

    fn diagnostic_by_rule<'a>(lint: &'a WorkspaceLint, rule: &str) -> &'a LintDiagnostic {
        lint.diagnostics
            .iter()
            .find(|diagnostic| diagnostic.rule.as_string() == rule)
            .unwrap_or_else(|| panic!("diagnostic not found: {rule}"))
    }
}
