use std::path::PathBuf;

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;
use toml_edit::ImDocument;

use crate::diagnostics::{DiagnosticLocation, EntityId, LintDiagnostic, LintStage, RototoRuleId};
use crate::error::Result;
use crate::model::WorkspaceLint;

use super::builtins;
use super::custom::{RegisteredCustomLint, register_custom_lints, run_registered_custom_lints};
use super::input::LintInput;
use super::nodes::*;
use super::output::sort_diagnostics;
use super::project::{
    project_external_value, project_manifest, project_qualifier, project_variable,
};
use super::references::ReferenceIndex;
use super::source::{DocumentCollection, DocumentKind, SourceStore};
use super::syntax::{
    ParsedToml, SyntaxIndex, json_parse_diagnostic, read_error_diagnostic,
    toml_de_parse_diagnostic, toml_edit_parse_diagnostic,
};
use super::{WORKSPACE_MANIFEST, WorkspaceLintSnapshot};

pub(super) async fn lint_workspace_snapshot(input: LintInput) -> Result<WorkspaceLintSnapshot> {
    LintEngine::new().lint_workspace(input).await
}

struct LintEngine;

impl LintEngine {
    fn new() -> Self {
        Self
    }

    async fn lint_workspace(&self, input: LintInput) -> Result<WorkspaceLintSnapshot> {
        let mut ctx = LintContext::new(input);

        self.run_discover(&mut ctx).await?;
        self.run_parse(&mut ctx);
        self.build_projection(&mut ctx);
        self.build_references(&mut ctx);
        self.run_project(&mut ctx);
        self.run_register(&mut ctx).await;
        self.run_registered_custom_lints(&mut ctx, LintStage::Project)
            .await;
        self.run_reference(&mut ctx);
        self.run_registered_custom_lints(&mut ctx, LintStage::Reference)
            .await;
        self.run_value(&mut ctx);
        self.run_registered_custom_lints(&mut ctx, LintStage::Value)
            .await;
        self.run_graph(&mut ctx);
        self.run_registered_custom_lints(&mut ctx, LintStage::Graph)
            .await;
        self.run_registered_custom_lints(&mut ctx, LintStage::Policy)
            .await;

        Ok(ctx.finish())
    }

    async fn run_discover(&self, ctx: &mut LintContext) -> Result<()> {
        let root = match tokio::fs::canonicalize(&ctx.input.root).await {
            Ok(root) => root,
            Err(err) => {
                ctx.diagnostics.push(LintDiagnostic::rototo(
                    RototoRuleId::WorkspaceNotFound,
                    LintStage::Discover,
                    EntityId::Workspace,
                    DiagnosticLocation::workspace_root(ctx.input.root.display().to_string()),
                    err.to_string(),
                ));
                return Ok(());
            }
        };

        let metadata = match tokio::fs::metadata(&root).await {
            Ok(metadata) => metadata,
            Err(err) => {
                ctx.diagnostics.push(LintDiagnostic::rototo(
                    RototoRuleId::WorkspaceNotFound,
                    LintStage::Discover,
                    EntityId::Workspace,
                    DiagnosticLocation::workspace_root(root.display().to_string()),
                    err.to_string(),
                ));
                return Ok(());
            }
        };

        if !metadata.is_dir() {
            ctx.diagnostics.push(LintDiagnostic::rototo(
                RototoRuleId::WorkspaceNotFound,
                LintStage::Discover,
                EntityId::Workspace,
                DiagnosticLocation::workspace_root(root.display().to_string()),
                "workspace path is not a directory",
            ));
            return Ok(());
        }

        ctx.source.root = root;
        let manifest_path = PathBuf::from(WORKSPACE_MANIFEST);
        if tokio::fs::metadata(ctx.source.root.join(&manifest_path))
            .await
            .is_ok_and(|metadata| metadata.is_file())
        {
            ctx.source
                .add_disk_document(manifest_path, DocumentKind::Manifest)
                .await;
        } else {
            ctx.diagnostics.push(LintDiagnostic::rototo(
                RototoRuleId::WorkspaceManifestMissing,
                LintStage::Discover,
                EntityId::Workspace,
                DiagnosticLocation::workspace_root(ctx.source.root.display().to_string()),
                "workspace manifest is missing",
            ));
            return Ok(());
        }

        ctx.source
            .add_named_toml_documents("qualifiers", DocumentCollection::Qualifiers)
            .await?;
        ctx.source
            .add_named_toml_documents("variables", DocumentCollection::Variables)
            .await?;
        ctx.source.add_schema_documents().await?;
        ctx.source.add_custom_lint_documents().await?;

        Ok(())
    }

    fn run_parse(&self, ctx: &mut LintContext) {
        for document in ctx.source.documents.values() {
            if let Some(read_error) = &document.read_error {
                if !matches!(&document.kind, DocumentKind::CustomLint) {
                    ctx.diagnostics
                        .push(read_error_diagnostic(document, read_error));
                }
                continue;
            }

            match &document.kind {
                DocumentKind::Manifest
                | DocumentKind::Qualifier { .. }
                | DocumentKind::Variable { .. }
                | DocumentKind::ExternalValue { .. } => {
                    match ImDocument::parse(document.text.clone()) {
                        Ok(edit) => match document.text.parse::<TomlValue>() {
                            Ok(plain) => {
                                ctx.syntax
                                    .toml
                                    .insert(document.id, ParsedToml { edit, plain });
                            }
                            Err(err) => {
                                ctx.diagnostics
                                    .push(toml_de_parse_diagnostic(document, &err));
                            }
                        },
                        Err(err) => {
                            ctx.diagnostics
                                .push(toml_edit_parse_diagnostic(document, &err));
                        }
                    }
                }
                DocumentKind::Schema => match serde_json::from_str::<JsonValue>(&document.text) {
                    Ok(value) => {
                        ctx.syntax.json.insert(document.id, value);
                    }
                    Err(err) => {
                        ctx.diagnostics.push(json_parse_diagnostic(document, &err));
                    }
                },
                DocumentKind::CustomLint => {}
            }
        }
    }

    fn build_projection(&self, ctx: &mut LintContext) {
        for document in ctx.source.documents.values() {
            let Some(toml) = ctx.syntax.toml.get(&document.id) else {
                continue;
            };

            match &document.kind {
                DocumentKind::Manifest => {
                    ctx.index.manifest = Some(project_manifest(document, toml));
                }
                DocumentKind::Qualifier { id } => {
                    ctx.index
                        .qualifiers
                        .insert(id.clone(), project_qualifier(document, toml, id));
                }
                DocumentKind::Variable { id } => {
                    ctx.index.variables.insert(
                        id.clone(),
                        project_variable(document, toml, id, &ctx.source),
                    );
                }
                DocumentKind::ExternalValue {
                    variable_id,
                    value_key,
                } => {
                    ctx.index
                        .external_values
                        .entry(variable_id.clone())
                        .or_default()
                        .insert(
                            value_key.clone(),
                            project_external_value(document, toml, value_key),
                        );
                }
                DocumentKind::Schema | DocumentKind::CustomLint => {}
            }
        }
    }

    fn run_project(&self, ctx: &mut LintContext) {
        builtins::run_project(ctx);
    }

    fn build_references(&self, ctx: &mut LintContext) {
        ctx.references = ReferenceIndex::build(&ctx.index, &ctx.source, &ctx.syntax);
    }

    async fn run_register(&self, ctx: &mut LintContext) {
        register_custom_lints(ctx).await;
    }

    fn run_reference(&self, ctx: &mut LintContext) {
        builtins::run_reference(ctx);
    }

    fn run_value(&self, ctx: &mut LintContext) {
        builtins::run_value(ctx);
    }

    fn run_graph(&self, ctx: &mut LintContext) {
        builtins::run_graph(ctx);
    }

    async fn run_registered_custom_lints(&self, ctx: &mut LintContext, stage: LintStage) {
        run_registered_custom_lints(ctx, stage).await;
    }
}

pub(super) struct LintContext {
    pub(super) input: LintInput,
    pub(super) source: SourceStore,
    pub(super) syntax: SyntaxIndex,
    pub(super) index: SemanticIndex,
    pub(super) references: ReferenceIndex,
    pub(super) registered_custom_lints: Vec<RegisteredCustomLint>,
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
            registered_custom_lints: Vec::new(),
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
        assert_eq!(external_value.primary.range.unwrap().start.character, 8);
        assert_eq!(external_value.primary.range.unwrap().end.line, 0);
        assert_eq!(external_value.primary.range.unwrap().end.character, 18);
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
    }

    fn diagnostic_by_rule<'a>(lint: &'a WorkspaceLint, rule: &str) -> &'a LintDiagnostic {
        lint.diagnostics
            .iter()
            .find(|diagnostic| diagnostic.rule.as_string() == rule)
            .unwrap_or_else(|| panic!("diagnostic not found: {rule}"))
    }
}
