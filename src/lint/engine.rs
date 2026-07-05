use crate::diagnostics::LintDiagnostic;
use crate::error::Result;
use crate::model::PackageLint;

use super::index::*;
use super::input::LintInput;
use super::output::sort_diagnostics;
use super::references::ReferenceIndex;
use super::source::SourceStore;
use super::syntax::SyntaxIndex;
use super::{PackageLintSnapshot, stages};

pub(super) async fn lint_package_snapshot(input: LintInput) -> Result<PackageLintSnapshot> {
    LintEngine::new().lint_package(input).await
}

struct LintEngine;

impl LintEngine {
    fn new() -> Self {
        Self
    }

    async fn lint_package(&self, input: LintInput) -> Result<PackageLintSnapshot> {
        let mut ctx = LintContext::new(input);
        stages::run_pipeline(&mut ctx).await?;
        Ok(ctx.finish())
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

    fn finish(mut self) -> PackageLintSnapshot {
        sort_diagnostics(&mut self.diagnostics);
        let documents = self.source.document_summaries();
        let source_texts = self.source.document_texts();
        let lint = PackageLint {
            root: self.source.root,
            documents,
            diagnostics: self.diagnostics,
        };
        PackageLintSnapshot {
            lint,
            index: self.index,
            references: self.references,
            source_texts,
        }
    }
}

pub(super) fn variable_values<'a>(
    _ctx: &'a LintContext,
    variable: &'a VariableNode,
) -> impl Iterator<Item = &'a ValueNode> {
    variable.values.inline_values.values()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::diagnostics::{CustomRuleId, LintStage};

    use super::super::PACKAGE_MANIFEST;
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
            root.join(PACKAGE_MANIFEST),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[resolve]
default = "hello"
"#;
        tokio::fs::write(root.join("variables/message.toml"), disk_variable)
            .await
            .unwrap();

        let invalid_overlay = r#"schema_version = 1
type = "mystery"

[resolve]
default = "hello"
"#;
        let mut input = LintInput::new(root.to_path_buf());
        input.overlays.insert(
            "variables/message.toml".to_owned(),
            OverlayDocument {
                text: invalid_overlay.to_owned(),
                version: Some(42),
            },
        );
        let snapshot = lint_package_snapshot(input).await.unwrap();
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
            group.document.path == "rototo-package.toml" && group.diagnostics.is_empty()
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
        assert!(
            symbols[0]
                .children
                .iter()
                .any(|symbol| symbol.name == "resolve")
        );

        let mut cleared_input = LintInput::new(root.to_path_buf());
        cleared_input.overlays.insert(
            "variables/message.toml".to_owned(),
            OverlayDocument {
                text: disk_variable.to_owned(),
                version: Some(43),
            },
        );
        let cleared = lint_package_snapshot(cleared_input).await.unwrap();

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
    async fn snapshot_discovers_overlay_only_package_files() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let mut input = LintInput::new(root.to_path_buf());
        for (path, text) in [
            (
                PACKAGE_MANIFEST,
                r#"schema_version = 1
"#,
            ),
            (
                "variables/premium.toml",
                r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "context.account.tier == \"premium\""
value = true
"#,
            ),
            (
                "variables/message.toml",
                r#"schema_version = 1
type = "catalog:message"

[resolve]
default = "default"

[[resolve.rule]]
when = 'variables["premium"]'
value = "premium"
"#,
            ),
            (
                "model/catalogs/message.schema.json",
                r#"{
  "type": "object",
  "properties": { "message": { "type": "string" } },
  "required": ["message"],
  "additionalProperties": false
}"#,
            ),
            ("data/catalogs/message/default.toml", r#"message = "hello""#),
            (
                "data/catalogs/message/premium.toml",
                r#"message = "premium""#,
            ),
            (
                "model/context/request.schema.json",
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
  lint:rule({
    id = "policy/noop",
    title = "No-op policy",
    help = "No-op policy used by tests.",
    handler = "check_package",
  })
end

function check_package(package, target)
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

        let snapshot = lint_package_snapshot(input).await.unwrap();
        let lint = &snapshot.lint;
        let paths = lint
            .documents
            .iter()
            .map(|document| document.path.as_str())
            .collect::<Vec<_>>();

        assert!(lint.diagnostics.is_empty(), "{:#?}", lint.diagnostics);
        for expected in [
            PACKAGE_MANIFEST,
            "variables/premium.toml",
            "variables/message.toml",
            "model/catalogs/message.schema.json",
            "data/catalogs/message/default.toml",
            "data/catalogs/message/premium.toml",
            "model/context/request.schema.json",
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
    async fn snapshot_diagnostic_ranges_cover_references() {
        let reference_snapshot = lint_package_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/packages/rules/reference/variable-rule-unknown-variable",
        )))
        .await
        .unwrap();
        let reference = diagnostic_by_rule(
            &reference_snapshot.lint,
            "rototo/variable-rule-unknown-variable",
        );
        assert_eq!(reference.primary.path, "variables/checkout_redesign.toml");
        assert_eq!(reference.primary.range.unwrap().start.line, 8);
        assert_eq!(reference.primary.range.unwrap().start.character, 7);
        assert_eq!(reference.primary.range.unwrap().end.line, 8);
        assert_eq!(reference.primary.range.unwrap().end.character, 34);
    }

    #[tokio::test]
    async fn snapshot_index_records_custom_lint_registry() {
        let snapshot = lint_package_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/packages/custom-targets",
        )))
        .await
        .unwrap();
        let registry = &snapshot.index.custom_lints;

        let variable_rule = CustomRuleId::parse("targets/variable-type").unwrap();
        let variable_definition = registry.rules.get(&variable_rule).unwrap();
        assert_eq!(
            variable_definition.definition.title,
            "Variable type target was checked"
        );

        let file = registry.files.get("lint/targets.lua").unwrap();
        assert_eq!(file.path, "lint/targets.lua");
        assert_eq!(file.location.path, "lint/targets.lua");

        let registration = registry
            .registrations
            .iter()
            .find(|registration| registration.rule == variable_rule)
            .unwrap();
        assert_eq!(registration.file_path, "lint/targets.lua");
        assert_eq!(registration.stage, LintStage::Policy);
        assert_eq!(registration.location.path, "lint/targets.lua");
        assert_eq!(
            registration.selector.address.to_string(),
            "variable=agent_config"
        );
    }

    #[tokio::test]
    async fn snapshot_records_source_backed_failure_diagnostics() {
        let parse_snapshot = lint_package_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/packages/rules/parse/variable-parse-failed",
        )))
        .await
        .unwrap();
        assert!(
            diagnostic_by_rule(&parse_snapshot.lint, "rototo/variable-parse-failed")
                .primary
                .path
                .ends_with("variables/broken.toml")
        );

        let register_snapshot = lint_package_snapshot(LintInput::new(PathBuf::from(
            "tests/fixtures/packages/rules/register/custom-lint-failed",
        )))
        .await
        .unwrap();
        assert!(
            diagnostic_by_rule(&register_snapshot.lint, "rototo/custom-lint-failed")
                .primary
                .path
                .ends_with("lint/broken.lua")
        );
    }

    #[tokio::test]
    async fn snapshot_reference_index_records_resolved_and_unresolved_edges() {
        use super::super::references::{ReferenceSource, ReferenceTarget};

        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("model/catalogs"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("data/catalogs/message"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("model/context"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join(PACKAGE_MANIFEST),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("variables/beta.toml"),
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "context.account.beta == true"
value = true
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("variables/premium.toml"),
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "variables[\"beta\"] && context.account.region == \"eu\""
value = true
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("variables/message.toml"),
            r#"schema_version = 1
type = "catalog:message"

[resolve]
default = "missing"

[[resolve.rule]]
when = 'variables["premium"]'
value = "welcome"

[[resolve.rule]]
when = 'variables["missing"]'
value = "absent"
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("model/catalogs/message.schema.json"),
            r#"{
  "type": "object",
  "properties": { "text": { "type": "string" } },
  "required": ["text"],
  "additionalProperties": false
}
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("data/catalogs/message/welcome.toml"),
            r#"text = "welcome"
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("model/context/request.schema.json"),
            r#"{"type":"object"}"#,
        )
        .await
        .unwrap();
        let snapshot = lint_package_snapshot(LintInput::new(root.to_path_buf()))
            .await
            .unwrap();

        assert!(snapshot.references.edges().iter().any(|edge| {
            matches!(
                &edge.source,
                ReferenceSource::VariableRuleConditionVariable { variable, .. }
                    if variable == "premium"
            ) && edge.target == ReferenceTarget::Variable("beta".to_owned())
                && edge.is_resolved()
        }));
        assert!(snapshot.references.edges().iter().any(|edge| {
            matches!(
                edge.source,
                ReferenceSource::VariableRuleConditionVariable { .. }
            ) && edge.target == ReferenceTarget::Variable("missing".to_owned())
                && !edge.is_resolved()
        }));
        assert!(snapshot.references.edges().iter().any(|edge| {
            matches!(edge.source, ReferenceSource::VariableRuleValue { .. })
                && edge.target
                    == ReferenceTarget::CatalogEntry {
                        catalog: "message".to_owned(),
                        value: "absent".to_owned(),
                    }
                && !edge.is_resolved()
        }));

        assert!(snapshot.references.edges().iter().any(|edge| {
            matches!(
                &edge.source,
                ReferenceSource::VariableRuleConditionVariable { variable, rule }
                    if variable == "message" && *rule == 0
            ) && edge.target == ReferenceTarget::Variable("premium".to_owned())
                && edge.location.path == "variables/message.toml"
                && edge.is_resolved()
        }));
        assert!(snapshot.references.edges().iter().any(|edge| {
            matches!(
                &edge.source,
                ReferenceSource::VariableRuleValue { variable, rule }
                    if variable == "message" && *rule == 0
            ) && edge.target
                == ReferenceTarget::CatalogEntry {
                    catalog: "message".to_owned(),
                    value: "welcome".to_owned(),
                }
                && edge.is_resolved()
        }));
        assert!(!snapshot.references.edges().iter().any(|edge| {
            matches!(
                &edge.source,
                ReferenceSource::VariableRuleValue { variable, .. } if variable == "message"
            ) && edge.target
                == ReferenceTarget::CatalogEntry {
                    catalog: "message".to_owned(),
                    value: "absent".to_owned(),
                }
                && edge.is_resolved()
        }));

        let context_schema = snapshot
            .index
            .evaluation_contexts
            .get("request")
            .expect("context schema node");
        assert_eq!(context_schema.path, "model/context/request.schema.json");
        assert!(context_schema.json.is_some());
        assert!(context_schema.validator.is_some());
        assert!(context_schema.invalid_message.is_none());
    }

    fn diagnostic_by_rule<'a>(lint: &'a PackageLint, rule: &str) -> &'a LintDiagnostic {
        lint.diagnostics
            .iter()
            .find(|diagnostic| diagnostic.rule.as_string() == rule)
            .unwrap_or_else(|| panic!("diagnostic not found: {rule}"))
    }
}
