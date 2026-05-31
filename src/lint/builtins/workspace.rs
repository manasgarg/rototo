use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticLocation, EntityId, LintDiagnostic, LintStage,
    RototoRuleId, Severity,
};
use crate::workspace::workspace_environments;

use super::super::engine::{LintContext, push_project_diagnostic};
use super::super::nodes::*;
use super::field_is_not_present;

pub(super) fn lint_manifest_shape(ctx: &mut LintContext) {
    let Some(manifest) = &ctx.index.manifest else {
        return;
    };
    let Some(parsed) = ctx.syntax.toml.get(&manifest.doc) else {
        return;
    };

    if let Err(err) = workspace_environments(&parsed.plain) {
        ctx.diagnostics.push(LintDiagnostic::rototo(
            RototoRuleId::WorkspaceManifestSchemaFailed,
            LintStage::Project,
            EntityId::Manifest,
            manifest.location.clone(),
            err.to_string(),
        ));
    }
}

pub(super) fn lint_manifest_custom_rule_shapes(ctx: &mut LintContext) {
    let Some(manifest) = &ctx.index.manifest else {
        return;
    };

    match &manifest.custom_rules {
        CustomRuleCollection::Invalid { location } => push_project_diagnostic(
            &mut ctx.diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            location.clone(),
            "workspace lint rule declarations must use [[lint.rule]] tables",
        ),
        CustomRuleCollection::Rules(rules) => {
            for rule in rules {
                lint_workspace_custom_rule_declaration_shape(&mut ctx.diagnostics, rule);
            }
        }
    }
}

fn lint_workspace_custom_rule_declaration_shape(
    diagnostics: &mut Vec<LintDiagnostic>,
    rule: &CustomRuleDeclarationNode,
) {
    if field_is_not_present(&rule.id) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            rule.id.location(),
            "custom lint rule must contain id",
        );
    }
    if field_is_not_present(&rule.title) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            rule.title.location(),
            "custom lint rule must contain title",
        );
    }
    if field_is_not_present(&rule.help) {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            rule.help.location(),
            "custom lint rule must contain help",
        );
    }
    if let Some(ProjectField::Invalid { location }) = &rule.severity {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintRuleShape,
            EntityId::Manifest,
            location.clone(),
            "custom lint rule severity must be error or warning",
        );
    }

    if let ProjectField::Present(id) = &rule.id
        && let Err(err) = CustomRuleId::parse(&id.value)
    {
        push_project_diagnostic(
            diagnostics,
            RototoRuleId::CustomLintInvalidRule,
            EntityId::Manifest,
            id.location.clone(),
            format!("custom lint rule id is invalid: {err}"),
        );
    }
}

pub(super) fn lint_custom_rule_conflicts(ctx: &mut LintContext) {
    let mut declared: BTreeMap<CustomRuleId, CustomRuleDefinition> = BTreeMap::new();
    let mut diagnostics = Vec::new();

    for (definition, location, entity) in custom_rule_definition_entries(ctx) {
        match declared.get(&definition.rule) {
            Some(existing) if !existing.same_metadata(&definition) => {
                push_project_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::CustomLintRuleConflict,
                    entity,
                    location,
                    format!("custom lint rule metadata conflicts: {}", definition.rule),
                );
            }
            Some(_) => {}
            None => {
                declared.insert(definition.rule.clone(), definition);
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn custom_rule_definition_entries(
    ctx: &LintContext,
) -> Vec<(CustomRuleDefinition, DiagnosticLocation, EntityId)> {
    let mut definitions = Vec::new();

    if let Some(manifest) = &ctx.index.manifest {
        definitions.extend(
            custom_rule_definitions_from_collection(&manifest.custom_rules)
                .into_iter()
                .map(|(definition, location)| (definition, location, EntityId::Manifest)),
        );
    }

    definitions
}

pub(crate) fn workspace_custom_rule_definitions(
    ctx: &LintContext,
) -> BTreeMap<CustomRuleId, CustomRuleDefinition> {
    let Some(manifest) = &ctx.index.manifest else {
        return BTreeMap::new();
    };
    custom_rule_definitions_from_collection(&manifest.custom_rules)
        .into_iter()
        .map(|(definition, _)| (definition.rule.clone(), definition))
        .collect()
}

pub(crate) fn custom_rule_definitions_from_collection(
    rules: &CustomRuleCollection,
) -> Vec<(CustomRuleDefinition, DiagnosticLocation)> {
    let CustomRuleCollection::Rules(rules) = rules else {
        return Vec::new();
    };
    custom_rule_definitions_from_rules(rules)
}

fn custom_rule_definitions_from_rules(
    rules: &[CustomRuleDeclarationNode],
) -> Vec<(CustomRuleDefinition, DiagnosticLocation)> {
    rules
        .iter()
        .filter_map(|rule| {
            let (
                ProjectField::Present(id),
                ProjectField::Present(title),
                ProjectField::Present(help),
            ) = (&rule.id, &rule.title, &rule.help)
            else {
                return None;
            };
            let Ok(rule_id) = CustomRuleId::parse(&id.value) else {
                return None;
            };
            let severity = match &rule.severity {
                Some(ProjectField::Present(severity)) => severity.value,
                Some(ProjectField::Invalid { .. }) => return None,
                Some(ProjectField::Missing { .. }) | None => Severity::Error,
            };
            Some((
                CustomRuleDefinition::with_severity(
                    rule_id,
                    severity,
                    title.value.clone(),
                    help.value.clone(),
                ),
                rule.location.clone(),
            ))
        })
        .collect()
}

pub(crate) fn declared_workspace_environments(ctx: &LintContext) -> Option<BTreeSet<String>> {
    let manifest = ctx.index.manifest.as_ref()?;
    let parsed = ctx.syntax.toml.get(&manifest.doc)?;
    workspace_environments(&parsed.plain)
        .ok()
        .map(|environments| environments.into_iter().collect())
}
