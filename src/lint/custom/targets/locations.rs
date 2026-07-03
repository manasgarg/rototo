use crate::diagnostics::{
    DiagnosticLocation, DocId, SemanticEntity, SemanticField, SemanticTarget,
};
use crate::lint::engine::LintContext;
use crate::lint::index::*;
use crate::lint::syntax::item_location;

use super::data::RegisteredLintTargetInstance;
use super::{find_rule, find_value};

pub(crate) struct RegisteredLintOutputAnchor {
    pub(crate) target: SemanticTarget,
    pub(crate) location: DiagnosticLocation,
}

pub(crate) fn registered_lint_output_anchor(
    ctx: &LintContext,
    target: &RegisteredLintTargetInstance,
    path: Option<&str>,
) -> RegisteredLintOutputAnchor {
    let current = RegisteredLintOutputAnchor {
        target: target.target.clone(),
        location: target.location.clone(),
    };
    let Some(path) = path.map(str::trim) else {
        return current;
    };
    let Some(tokens) = parse_json_pointer(path) else {
        return current;
    };
    resolve_output_pointer(ctx, &target.target.entity, &tokens).unwrap_or(current)
}

fn resolve_output_pointer(
    ctx: &LintContext,
    entity: &SemanticEntity,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    match entity {
        SemanticEntity::Package => package_pointer(ctx, tokens),
        SemanticEntity::Enum { .. } | SemanticEntity::Layer { .. } | SemanticEntity::Governance => {
            None
        }
        SemanticEntity::Variable { id } => variable_pointer(ctx, id, tokens),
        SemanticEntity::Value { variable, key } => value_pointer(ctx, variable, key, tokens),
        SemanticEntity::Rule { variable, index } => rule_pointer(ctx, variable, *index, tokens),
        SemanticEntity::Catalog { id } => catalog_pointer(ctx, id, tokens),
        SemanticEntity::CatalogEntry { catalog, key } => {
            catalog_entry_pointer(ctx, catalog, key, tokens)
        }
        SemanticEntity::EvaluationContext { id } => evaluation_context_pointer(ctx, id, tokens),
        SemanticEntity::EvaluationContextSample {
            evaluation_context,
            key,
        } => evaluation_context_sample_pointer(ctx, evaluation_context, key, tokens),
        SemanticEntity::Manifest | SemanticEntity::CustomLint { .. } => None,
    }
}

fn registered_lint_output_location(
    ctx: &LintContext,
    entity: &SemanticEntity,
    field: Option<&SemanticField>,
    fallback: DiagnosticLocation,
) -> DiagnosticLocation {
    let Some(field) = field else {
        return fallback;
    };
    match (entity, field) {
        (SemanticEntity::Package, SemanticField::PackageExtends) => ctx
            .index
            .manifest
            .as_ref()
            .map(|manifest| registered_package_location(ctx, manifest, Some(field)))
            .unwrap_or(fallback),
        (SemanticEntity::Variable { id }, _) => ctx
            .index
            .variables
            .get(id)
            .map(|variable| registered_variable_location(ctx, variable, Some(field)))
            .unwrap_or(fallback),
        (SemanticEntity::Value { variable, key }, _) => ctx
            .index
            .variables
            .get(variable)
            .and_then(|variable| find_value(variable, key))
            .map(|value| value.location.clone())
            .unwrap_or(fallback),
        (SemanticEntity::Rule { variable, index }, _) => ctx
            .index
            .variables
            .get(variable)
            .and_then(|variable| find_rule(variable, *index))
            .map(|rule| registered_rule_location(rule, Some(field)))
            .unwrap_or(fallback),
        (SemanticEntity::Catalog { id }, _) => ctx
            .index
            .catalogs
            .get(id)
            .map(|catalog| catalog.location.clone())
            .unwrap_or(fallback),
        (SemanticEntity::CatalogEntry { catalog, key }, _) => ctx
            .index
            .catalog_entries
            .get(catalog)
            .and_then(|entries| entries.get(key))
            .map(|entry| entry.location.clone())
            .unwrap_or(fallback),
        (SemanticEntity::EvaluationContext { id }, _) => ctx
            .index
            .evaluation_contexts
            .get(id)
            .map(|evaluation_context| evaluation_context.location.clone())
            .unwrap_or(fallback),
        (
            SemanticEntity::EvaluationContextSample {
                evaluation_context,
                key,
            },
            _,
        ) => ctx
            .index
            .evaluation_context_samples
            .get(evaluation_context)
            .and_then(|entries| entries.get(key))
            .map(|entry| entry.location.clone())
            .unwrap_or(fallback),
        _ => fallback,
    }
}

fn registered_lint_output_anchor_for(
    ctx: &LintContext,
    entity: SemanticEntity,
    field: Option<&SemanticField>,
) -> Option<RegisteredLintOutputAnchor> {
    let target = SemanticTarget::entity(entity);
    let location = entity_location(ctx, &target.entity)?;
    Some(anchor_with_location(ctx, target, field, location))
}

fn anchor_with_location(
    ctx: &LintContext,
    target: SemanticTarget,
    field: Option<&SemanticField>,
    location: DiagnosticLocation,
) -> RegisteredLintOutputAnchor {
    let output_target = match field {
        Some(field) => SemanticTarget::field(target.entity.clone(), field.clone()),
        None => target,
    };
    let location =
        registered_lint_output_location(ctx, &output_target.entity, field, location.clone());
    RegisteredLintOutputAnchor {
        target: output_target,
        location,
    }
}

fn field_anchor(
    ctx: &LintContext,
    entity: SemanticEntity,
    field: SemanticField,
    fallback: DiagnosticLocation,
) -> RegisteredLintOutputAnchor {
    anchor_with_location(ctx, SemanticTarget::entity(entity), Some(&field), fallback)
}

fn entity_location(ctx: &LintContext, entity: &SemanticEntity) -> Option<DiagnosticLocation> {
    match entity {
        SemanticEntity::Package => ctx
            .index
            .manifest
            .as_ref()
            .map(|manifest| registered_package_location(ctx, manifest, None)),
        SemanticEntity::Variable { id } => ctx
            .index
            .variables
            .get(id)
            .map(|variable| variable.location.clone()),
        SemanticEntity::Layer { id } => {
            ctx.index.layers.get(id).map(|layer| layer.location.clone())
        }
        SemanticEntity::Governance => ctx
            .index
            .governance
            .as_ref()
            .map(|governance| governance.location.clone()),
        SemanticEntity::Value { variable, key } => ctx
            .index
            .variables
            .get(variable)
            .and_then(|variable| find_value(variable, key))
            .map(|value| value.location.clone()),
        SemanticEntity::Rule { variable, index } => ctx
            .index
            .variables
            .get(variable)
            .and_then(|variable| find_rule(variable, *index))
            .map(|rule| rule.location.clone()),
        SemanticEntity::Catalog { id } => ctx
            .index
            .catalogs
            .get(id)
            .map(|catalog| catalog.location.clone()),
        SemanticEntity::CatalogEntry { catalog, key } => ctx
            .index
            .catalog_entries
            .get(catalog)
            .and_then(|entries| entries.get(key))
            .map(|entry| entry.location.clone()),
        SemanticEntity::EvaluationContext { id } => ctx
            .index
            .evaluation_contexts
            .get(id)
            .map(|evaluation_context| evaluation_context.location.clone()),
        SemanticEntity::EvaluationContextSample {
            evaluation_context,
            key,
        } => ctx
            .index
            .evaluation_context_samples
            .get(evaluation_context)
            .and_then(|entries| entries.get(key))
            .map(|entry| entry.location.clone()),
        SemanticEntity::Enum { id } => ctx
            .index
            .enums
            .get(id)
            .map(|declaration| declaration.location.clone()),
        SemanticEntity::Manifest | SemanticEntity::CustomLint { .. } => None,
    }
}

fn package_pointer(ctx: &LintContext, tokens: &[String]) -> Option<RegisteredLintOutputAnchor> {
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, SemanticEntity::Package, None);
    }
    let manifest = ctx.index.manifest.as_ref()?;
    match tokens {
        [segment, ..] if segment == "extends" => Some(field_anchor(
            ctx,
            SemanticEntity::Package,
            SemanticField::PackageExtends,
            registered_package_location(ctx, manifest, None),
        )),
        [segment] if segment == "manifest" => {
            registered_lint_output_anchor_for(ctx, SemanticEntity::Package, None)
        }
        [segment, field, ..] if segment == "manifest" && field == "extends" => Some(field_anchor(
            ctx,
            SemanticEntity::Package,
            SemanticField::PackageExtends,
            registered_package_location(ctx, manifest, None),
        )),
        [segment, id, rest @ ..] if segment == "variables" => variable_pointer(ctx, id, rest)
            .or_else(|| registered_lint_output_anchor_for(ctx, SemanticEntity::Package, None)),
        [segment, id, rest @ ..] if segment == "catalogs" => catalog_pointer(ctx, id, rest)
            .or_else(|| registered_lint_output_anchor_for(ctx, SemanticEntity::Package, None)),
        [segment, id, rest @ ..] if segment == "evaluation_contexts" => {
            evaluation_context_pointer(ctx, id, rest)
                .or_else(|| registered_lint_output_anchor_for(ctx, SemanticEntity::Package, None))
        }
        _ => registered_lint_output_anchor_for(ctx, SemanticEntity::Package, None),
    }
}

fn variable_pointer(
    ctx: &LintContext,
    id: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let variable = ctx.index.variables.get(id)?;
    let entity = SemanticEntity::Variable { id: id.to_owned() };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment, ..] if segment == "description" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::Description,
            variable.location.clone(),
        )),
        [segment, ..] if segment == "declaration" || segment == "type" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableType,
            variable.location.clone(),
        )),
        [segment, ..] if segment == "schema" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableSchema,
            variable.location.clone(),
        )),
        [segment] if segment == "values" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableValues,
            variable.location.clone(),
        )),
        [segment, key, rest @ ..] if segment == "values" => value_pointer(ctx, id, key, rest)
            .or_else(|| {
                Some(field_anchor(
                    ctx,
                    entity,
                    SemanticField::VariableValues,
                    variable.location.clone(),
                ))
            }),
        [segment] if segment == "resolve" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableResolve,
            variable.location.clone(),
        )),
        [segment, field, ..] if segment == "resolve" && field == "default" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableResolveDefault,
            variable.location.clone(),
        )),
        [segment, field] if segment == "resolve" && field == "rules" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableResolve,
            variable.location.clone(),
        )),
        [segment, field, index, rest @ ..] if segment == "resolve" && field == "rules" => {
            let Some(index) = parse_pointer_index(index) else {
                return Some(field_anchor(
                    ctx,
                    entity,
                    SemanticField::VariableResolve,
                    variable.location.clone(),
                ));
            };
            rule_pointer(ctx, id, index, rest).or_else(|| {
                Some(field_anchor(
                    ctx,
                    entity,
                    SemanticField::VariableResolve,
                    variable.location.clone(),
                ))
            })
        }
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn value_pointer(
    ctx: &LintContext,
    variable_id: &str,
    key: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let variable = ctx.index.variables.get(variable_id)?;
    let value = find_value(variable, key)?;
    let entity = SemanticEntity::Value {
        variable: variable_id.to_owned(),
        key: key.to_owned(),
    };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::Value,
            value.location.clone(),
        )),
        [segment, rest @ ..] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::ValueJsonPath {
                path: rest.to_vec(),
            },
            value.location.clone(),
        )),
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn rule_pointer(
    ctx: &LintContext,
    variable_id: &str,
    index: usize,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let variable = ctx.index.variables.get(variable_id)?;
    let rule = find_rule(variable, index)?;
    let entity = SemanticEntity::Rule {
        variable: variable_id.to_owned(),
        index,
    };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens.first().map(String::as_str) {
        Some("when") => Some(field_anchor(
            ctx,
            entity.clone(),
            SemanticField::VariableRuleWhen,
            rule.when
                .as_ref()
                .map(ProjectField::location)
                .unwrap_or_else(|| rule.location.clone()),
        )),
        Some("value") => Some(field_anchor(
            ctx,
            entity,
            SemanticField::VariableRuleValue,
            rule.location.clone(),
        )),
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn catalog_pointer(
    ctx: &LintContext,
    id: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let catalog = ctx.index.catalogs.get(id)?;
    let entity = SemanticEntity::Catalog { id: id.to_owned() };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment] if segment == "json" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::SchemaJson,
            catalog.location.clone(),
        )),
        [segment, rest @ ..] if segment == "json" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::SchemaJsonPath {
                path: rest.to_vec(),
            },
            catalog.location.clone(),
        )),
        [segment, key, rest @ ..] if segment == "entries" => {
            catalog_entry_pointer(ctx, id, key, rest)
                .or_else(|| registered_lint_output_anchor_for(ctx, entity, None))
        }
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn catalog_entry_pointer(
    ctx: &LintContext,
    catalog_id: &str,
    key: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let entry = ctx.index.catalog_entries.get(catalog_id)?.get(key)?;
    let entity = SemanticEntity::CatalogEntry {
        catalog: catalog_id.to_owned(),
        key: key.to_owned(),
    };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::CatalogEntry,
            entry.location.clone(),
        )),
        [segment, rest @ ..] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::ValueJsonPath {
                path: rest.to_vec(),
            },
            entry.location.clone(),
        )),
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn evaluation_context_pointer(
    ctx: &LintContext,
    id: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let evaluation_context = ctx.index.evaluation_contexts.get(id)?;
    let entity = SemanticEntity::EvaluationContext { id: id.to_owned() };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment] if segment == "json" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::SchemaJson,
            evaluation_context.location.clone(),
        )),
        [segment, rest @ ..] if segment == "json" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::SchemaJsonPath {
                path: rest.to_vec(),
            },
            evaluation_context.location.clone(),
        )),
        [segment, key, rest @ ..] if segment == "samples" => {
            evaluation_context_sample_pointer(ctx, id, key, rest)
                .or_else(|| registered_lint_output_anchor_for(ctx, entity, None))
        }
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

fn evaluation_context_sample_pointer(
    ctx: &LintContext,
    evaluation_context_id: &str,
    key: &str,
    tokens: &[String],
) -> Option<RegisteredLintOutputAnchor> {
    let entry = ctx
        .index
        .evaluation_context_samples
        .get(evaluation_context_id)?
        .get(key)?;
    let entity = SemanticEntity::EvaluationContextSample {
        evaluation_context: evaluation_context_id.to_owned(),
        key: key.to_owned(),
    };
    if tokens.is_empty() {
        return registered_lint_output_anchor_for(ctx, entity, None);
    }
    match tokens {
        [segment] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::EvaluationContextSample,
            entry.location.clone(),
        )),
        [segment, rest @ ..] if segment == "value" => Some(field_anchor(
            ctx,
            entity,
            SemanticField::ValueJsonPath {
                path: rest.to_vec(),
            },
            entry.location.clone(),
        )),
        _ => registered_lint_output_anchor_for(ctx, entity, None),
    }
}

pub(super) fn registered_package_location(
    ctx: &LintContext,
    manifest: &ManifestNode,
    field: Option<&SemanticField>,
) -> DiagnosticLocation {
    match field {
        Some(SemanticField::PackageExtends) => {
            toml_root_item_location(ctx, manifest.doc, "extends")
                .unwrap_or_else(|| manifest.location.clone())
        }
        _ => manifest.location.clone(),
    }
}

fn registered_variable_location(
    ctx: &LintContext,
    variable: &VariableNode,
    field: Option<&SemanticField>,
) -> DiagnosticLocation {
    match field {
        Some(SemanticField::Description) => {
            toml_root_item_location(ctx, variable.doc, "description")
                .unwrap_or_else(|| variable.location.clone())
        }
        Some(SemanticField::VariableType)
            if matches!(
                &variable.type_source,
                TypeSourceNode::Primitive(_) | TypeSourceNode::Catalog(_)
            ) =>
        {
            variable.type_source.location()
        }
        Some(SemanticField::VariableSchema)
            if matches!(&variable.type_source, TypeSourceNode::Schema(_)) =>
        {
            variable.type_source.location()
        }
        Some(SemanticField::VariableValues) => variable.values.location.clone(),
        Some(SemanticField::VariableResolve) => {
            toml_root_item_location(ctx, variable.doc, "resolve")
                .unwrap_or_else(|| variable.resolve.location())
        }
        Some(SemanticField::VariableResolveDefault) => match &variable.resolve {
            ResolveNode::Resolve { default, .. } => default.location(),
            _ => variable.resolve.location(),
        },
        _ => variable.location.clone(),
    }
}

fn registered_rule_location(
    rule: &VariableRuleNode,
    field: Option<&SemanticField>,
) -> DiagnosticLocation {
    match field {
        Some(SemanticField::VariableRuleWhen) => rule
            .when
            .as_ref()
            .map(ProjectField::location)
            .unwrap_or_else(|| rule.location.clone()),
        Some(SemanticField::VariableRuleValue) => rule.value.location(),
        _ => rule.location.clone(),
    }
}

fn toml_root_item_location(ctx: &LintContext, doc: DocId, key: &str) -> Option<DiagnosticLocation> {
    let document = ctx.source.documents.get(&doc)?;
    let parsed = ctx.syntax.toml.get(&doc)?;
    parsed
        .root_table()?
        .get(key)
        .map(|item| item_location(document, item))
}

fn parse_json_pointer(pointer: &str) -> Option<Vec<String>> {
    if pointer.is_empty() {
        return Some(Vec::new());
    }
    let rest = pointer.strip_prefix('/')?;
    rest.split('/').map(decode_json_pointer_token).collect()
}

fn decode_json_pointer_token(token: &str) -> Option<String> {
    let mut decoded = String::new();
    let mut chars = token.chars();
    while let Some(ch) = chars.next() {
        if ch != '~' {
            decoded.push(ch);
            continue;
        }
        match chars.next()? {
            '0' => decoded.push('~'),
            '1' => decoded.push('/'),
            _ => return None,
        }
    }
    Some(decoded)
}

fn parse_pointer_index(token: &str) -> Option<usize> {
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}
