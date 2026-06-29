use std::collections::{BTreeMap, BTreeSet};

use super::PackageLintSnapshot;
use crate::expression::{ContextScalarType, Expression};

use super::index::{ProjectField, SemanticIndex};
use super::references::{ReferenceIndex, ReferenceSource, ReferenceTarget};

#[derive(Debug, Clone, Default)]
pub(crate) struct EvaluationContextCompatibility {
    pub(crate) qualifiers: BTreeMap<String, BTreeSet<String>>,
    pub(crate) variables: BTreeMap<String, BTreeSet<String>>,
}

pub(crate) fn compatibility(snapshot: &PackageLintSnapshot) -> EvaluationContextCompatibility {
    compatibility_for(&snapshot.index, &snapshot.references)
}

pub(in crate::lint) fn compatibility_for(
    index: &SemanticIndex,
    _references: &ReferenceIndex,
) -> EvaluationContextCompatibility {
    let mut builder = CompatibilityBuilder {
        index,
        qualifier_cache: BTreeMap::new(),
        visiting: BTreeSet::new(),
    };

    let mut qualifiers = BTreeMap::new();
    for qualifier_id in index.qualifiers.keys() {
        let contexts = builder.qualifier_contexts(qualifier_id);
        qualifiers.insert(qualifier_id.clone(), contexts);
    }

    let mut variables = BTreeMap::new();
    for (variable_id, variable) in &index.variables {
        let mut contexts: Option<BTreeSet<String>> = None;
        let Some(resolve) = variable.resolve.as_rules() else {
            variables.insert(variable_id.clone(), BTreeSet::new());
            continue;
        };
        for rule in resolve {
            let mut rule_contexts: Option<BTreeSet<String>> = None;
            for expression in [&rule.when, &rule.query].into_iter().flatten() {
                let ProjectField::Present(expression) = expression else {
                    continue;
                };
                let expression_contexts = builder.expression_contexts(&expression.value);
                rule_contexts = Some(match rule_contexts {
                    Some(current) => current
                        .intersection(&expression_contexts)
                        .cloned()
                        .collect(),
                    None => expression_contexts,
                });
            }
            let Some(rule_contexts) = rule_contexts else {
                continue;
            };
            contexts = Some(match contexts {
                Some(current) => current.intersection(&rule_contexts).cloned().collect(),
                None => rule_contexts,
            });
        }
        variables.insert(variable_id.clone(), contexts.unwrap_or_default());
    }

    EvaluationContextCompatibility {
        qualifiers,
        variables,
    }
}

struct CompatibilityBuilder<'a> {
    index: &'a SemanticIndex,
    qualifier_cache: BTreeMap<String, BTreeSet<String>>,
    visiting: BTreeSet<String>,
}

impl<'a> CompatibilityBuilder<'a> {
    fn qualifier_contexts(&mut self, qualifier_id: &str) -> BTreeSet<String> {
        if let Some(contexts) = self.qualifier_cache.get(qualifier_id) {
            return contexts.clone();
        }
        if !self.visiting.insert(qualifier_id.to_owned()) {
            return BTreeSet::new();
        }

        let contexts = self.qualifier_contexts_uncached(qualifier_id);
        self.visiting.remove(qualifier_id);
        self.qualifier_cache
            .insert(qualifier_id.to_owned(), contexts.clone());
        contexts
    }

    fn qualifier_contexts_uncached(&mut self, qualifier_id: &str) -> BTreeSet<String> {
        let Some(qualifier) = self.index.qualifiers.get(qualifier_id) else {
            return BTreeSet::new();
        };
        let ProjectField::Present(when) = &qualifier.when else {
            return BTreeSet::new();
        };
        self.expression_contexts(&when.value)
    }

    fn expression_contexts(&mut self, expression: &Expression) -> BTreeSet<String> {
        let mut contexts: Option<BTreeSet<String>> = None;

        for qualifier in &expression.references().qualifiers {
            let nested_contexts = self.qualifier_contexts(qualifier);
            contexts = Some(match contexts {
                Some(current) => current.intersection(&nested_contexts).cloned().collect(),
                None => nested_contexts,
            });
        }

        for path in &expression.references().context_paths {
            if path.is_empty() {
                continue;
            }
            let constraints = expression
                .references()
                .context_path_types
                .get(path)
                .cloned()
                .unwrap_or_default();
            let path_contexts = self
                .index
                .evaluation_contexts
                .values()
                .filter(|context| {
                    context.json.as_ref().is_some_and(|schema| {
                        matches!(
                            context_path_type_fit(schema, path, &constraints),
                            ContextPathTypeFit::Ok
                        )
                    })
                })
                .map(|context| context.id.clone())
                .collect::<BTreeSet<_>>();
            contexts = Some(match contexts {
                Some(current) => current.intersection(&path_contexts).cloned().collect(),
                None => path_contexts,
            });
        }

        contexts.unwrap_or_default()
    }
}

pub(in crate::lint) fn qualifier_uses_context_attribute(
    references: &ReferenceIndex,
    qualifier_id: &str,
) -> bool {
    references.edges().iter().any(|edge| {
        matches!(
            &edge.source,
            ReferenceSource::QualifierWhenContextAttribute { qualifier }
                if qualifier == qualifier_id
        ) && matches!(&edge.target, ReferenceTarget::ContextAttribute(_))
    })
}

pub(in crate::lint) fn variable_rule_condition_reference_count(
    index: &SemanticIndex,
    variable_id: &str,
) -> usize {
    let Some(variable) = index.variables.get(variable_id) else {
        return 0;
    };
    variable
        .resolve
        .as_rules()
        .map(|rules| {
            rules
                .iter()
                .filter(|rule| {
                    [&rule.when, &rule.query]
                        .into_iter()
                        .flatten()
                        .any(|expression| {
                            let ProjectField::Present(expression) = expression else {
                                return false;
                            };
                            let references = expression.value.references();
                            !references.qualifiers.is_empty()
                                || !references.context_paths.is_empty()
                        })
                })
                .count()
        })
        .unwrap_or_default()
}

pub(in crate::lint) fn path_declared_in_any_context(index: &SemanticIndex, path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    index.evaluation_contexts.values().any(|context| {
        context
            .json
            .as_ref()
            .is_some_and(|schema| context_schema_field(schema, path).is_some())
    })
}

pub(in crate::lint) fn variable_resolve_rules(
    variable: &super::index::VariableNode,
) -> Option<&[super::index::VariableRuleNode]> {
    variable.resolve.as_rules()
}

/// How a context schema's declaration of a path lines up with the scalar types
/// an expression requires of that path.
pub(in crate::lint) enum ContextPathTypeFit {
    /// The schema does not declare the path at all.
    Missing,
    /// The path is declared but carries no JSON Schema `type` to check against.
    Untyped,
    /// The declared type cannot satisfy how the expression uses the path.
    Mismatch,
    /// The path is declared and its type satisfies every constraint (or the
    /// expression imposes no scalar constraint, so existence is enough).
    Ok,
}

pub(in crate::lint) fn context_path_type_fit(
    schema: &serde_json::Value,
    path: &str,
    constraints: &BTreeSet<ContextScalarType>,
) -> ContextPathTypeFit {
    let Some(field) = context_schema_field(schema, path) else {
        return ContextPathTypeFit::Missing;
    };
    if constraints.is_empty() {
        return ContextPathTypeFit::Ok;
    }
    let Some(declared) = schema_field_type_tokens(field) else {
        return ContextPathTypeFit::Untyped;
    };
    let declared_format = field.get("format").and_then(serde_json::Value::as_str);
    let satisfied = constraints.iter().all(|constraint| {
        let type_ok = declared
            .iter()
            .any(|token| constraint.matches_schema_type(token));
        let format_ok = match constraint.required_formats() {
            [] => true,
            formats => declared_format.is_some_and(|declared| formats.contains(&declared)),
        };
        type_ok && format_ok
    });
    if satisfied {
        ContextPathTypeFit::Ok
    } else {
        ContextPathTypeFit::Mismatch
    }
}

/// The JSON Schema scalar type tokens a field constrains its value to. A field
/// can pin its type with `type`, but also implicitly through `const` or `enum`,
/// so those are honored too. Returns `None` when no scalar type is declared.
fn schema_field_type_tokens(field: &serde_json::Value) -> Option<BTreeSet<String>> {
    if let Some(declared) = field.get("type") {
        return match declared {
            serde_json::Value::String(token) => Some(BTreeSet::from([token.clone()])),
            serde_json::Value::Array(tokens) => {
                let set = tokens
                    .iter()
                    .filter_map(|token| token.as_str().map(str::to_owned))
                    .collect::<BTreeSet<_>>();
                (!set.is_empty()).then_some(set)
            }
            _ => None,
        };
    }

    if let Some(constant) = field.get("const") {
        return json_value_type_token(constant).map(|token| BTreeSet::from([token]));
    }

    if let Some(serde_json::Value::Array(values)) = field.get("enum") {
        let set = values
            .iter()
            .filter_map(json_value_type_token)
            .collect::<BTreeSet<_>>();
        return (!set.is_empty()).then_some(set);
    }

    None
}

/// How a single evaluation context declares a path: `None` when the schema does
/// not declare it, `Some(types)` when it does (an empty `types` means the path
/// is declared without a checkable scalar type).
pub(in crate::lint) fn context_path_declaration(
    schema: &serde_json::Value,
    path: &str,
) -> Option<Vec<String>> {
    let field = context_schema_field(schema, path)?;
    Some(
        schema_field_type_tokens(field)
            .map(|tokens| tokens.into_iter().collect())
            .unwrap_or_default(),
    )
}

fn json_value_type_token(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(_) => Some("string".to_owned()),
        serde_json::Value::Bool(_) => Some("boolean".to_owned()),
        serde_json::Value::Number(_) => Some("number".to_owned()),
        _ => None,
    }
}

/// A human-readable list of the scalar families an expression requires of a
/// path, for diagnostics (for example `number or string`).
pub(in crate::lint) fn expected_type_label(constraints: &BTreeSet<ContextScalarType>) -> String {
    constraints
        .iter()
        .map(|constraint| constraint.label())
        .collect::<Vec<_>>()
        .join(" or ")
}

fn context_schema_field<'a>(
    schema: &'a serde_json::Value,
    attribute: &str,
) -> Option<&'a serde_json::Value> {
    if attribute.is_empty() {
        return None;
    }

    let mut current = schema;
    for segment in attribute.split('.') {
        let properties = current
            .get("properties")
            .and_then(serde_json::Value::as_object)?;
        current = properties.get(segment)?;
    }
    Some(current)
}

trait ResolveRulesExt {
    fn as_rules(&self) -> Option<&[super::index::VariableRuleNode]>;
}

impl ResolveRulesExt for super::index::ResolveNode {
    fn as_rules(&self) -> Option<&[super::index::VariableRuleNode]> {
        let super::index::ResolveNode::Resolve { rules, .. } = self else {
            return None;
        };
        let super::index::RuleCollection::Rules(rules) = rules else {
            return None;
        };
        Some(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::ContextScalarType;

    fn ip_field(format: Option<&str>) -> serde_json::Value {
        let field = match format {
            Some(format) => serde_json::json!({ "type": "string", "format": format }),
            None => serde_json::json!({ "type": "string" }),
        };
        serde_json::json!({
            "type": "object",
            "properties": { "net": { "type": "object", "properties": { "ip": field } } }
        })
    }

    #[test]
    fn refined_ip_type_requires_a_matching_format() {
        let constraints = BTreeSet::from([ContextScalarType::Ip]);

        // A plain string declaration is a type-level match but a refined-format
        // miss, so the path does not satisfy a cidr() use.
        assert!(matches!(
            context_path_type_fit(&ip_field(None), "net.ip", &constraints),
            ContextPathTypeFit::Mismatch
        ));

        // The same path declared with an ip format is sound.
        for format in ["ipv4", "ipv6"] {
            assert!(matches!(
                context_path_type_fit(&ip_field(Some(format)), "net.ip", &constraints),
                ContextPathTypeFit::Ok
            ));
        }

        // A different format (date-time) does not satisfy an ip constraint.
        assert!(matches!(
            context_path_type_fit(&ip_field(Some("date-time")), "net.ip", &constraints),
            ContextPathTypeFit::Mismatch
        ));
    }
}
