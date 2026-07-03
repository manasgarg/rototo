use std::collections::{BTreeMap, BTreeSet};

use super::PackageLintSnapshot;
use crate::expression::{ContextScalarType, Expression};

use super::index::{ProjectField, SemanticIndex};
use super::references::ReferenceIndex;

#[derive(Debug, Clone, Default)]
pub(crate) struct EvaluationContextCompatibility {
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
        variable_cache: BTreeMap::new(),
        visiting: BTreeSet::new(),
    };

    let mut variables = BTreeMap::new();
    for variable_id in index.variables.keys() {
        let contexts = builder.variable_contexts(variable_id).unwrap_or_default();
        variables.insert(variable_id.clone(), contexts);
    }

    EvaluationContextCompatibility { variables }
}

struct CompatibilityBuilder<'a> {
    index: &'a SemanticIndex,
    variable_cache: BTreeMap<String, Option<BTreeSet<String>>>,
    visiting: BTreeSet<String>,
}

impl<'a> CompatibilityBuilder<'a> {
    /// The evaluation contexts compatible with a variable's rule expressions,
    /// following `variables["<id>"]` references transitively. `None` means the
    /// variable imposes no context requirement (no rules, or no rule carries a
    /// context-constraining expression).
    fn variable_contexts(&mut self, variable_id: &str) -> Option<BTreeSet<String>> {
        if let Some(contexts) = self.variable_cache.get(variable_id) {
            return contexts.clone();
        }
        let key = format!("variable://{variable_id}");
        if !self.visiting.insert(key.clone()) {
            // A reference cycle; the graph lint owns reporting it.
            return Some(BTreeSet::new());
        }

        let contexts = self.variable_contexts_uncached(variable_id);
        self.visiting.remove(&key);
        self.variable_cache
            .insert(variable_id.to_owned(), contexts.clone());
        contexts
    }

    fn variable_contexts_uncached(&mut self, variable_id: &str) -> Option<BTreeSet<String>> {
        let variable = self.index.variables.get(variable_id)?;
        let mut contexts: Option<BTreeSet<String>> = None;
        for rule in variable.resolve.as_rules().unwrap_or_default() {
            let mut rule_contexts: Option<BTreeSet<String>> = None;
            for expression in [&rule.when].into_iter().flatten() {
                let ProjectField::Present(expression) = expression else {
                    continue;
                };
                let expression_contexts = self.expression_contexts(&expression.value);
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
        for expression in variable_query_expressions(variable) {
            // Query expressions that only read `entry` (or `env`) impose no
            // context requirement; only context paths and variable references
            // narrow the compatible set.
            let references = expression.value.references();
            if references.variables.is_empty()
                && references.context_paths.iter().all(|path| path.is_empty())
            {
                continue;
            }
            let expression_contexts = self.expression_contexts(&expression.value);
            contexts = Some(match contexts {
                Some(current) => current
                    .intersection(&expression_contexts)
                    .cloned()
                    .collect(),
                None => expression_contexts,
            });
        }
        for expression in variable_allocation_expressions(self.index, variable) {
            let references = expression.references();
            if references.variables.is_empty()
                && references.context_paths.iter().all(|path| path.is_empty())
            {
                continue;
            }
            let expression_contexts = self.expression_contexts(expression);
            contexts = Some(match contexts {
                Some(current) => current
                    .intersection(&expression_contexts)
                    .cloned()
                    .collect(),
                None => expression_contexts,
            });
        }
        contexts
    }

    fn expression_contexts(&mut self, expression: &Expression) -> BTreeSet<String> {
        let mut contexts: Option<BTreeSet<String>> = None;

        for variable in &expression.references().variables {
            let Some(nested_contexts) = self.variable_contexts(variable) else {
                continue;
            };
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
                    [&rule.when].into_iter().flatten().any(|expression| {
                        let ProjectField::Present(expression) = expression else {
                            return false;
                        };
                        let references = expression.value.references();
                        !references.variables.is_empty() || !references.context_paths.is_empty()
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

/// The layer expressions behind a `method = "allocation"` variable: the
/// diversion's `unit` and the allocation's `eligibility`, when present.
pub(in crate::lint) fn variable_allocation_expressions<'a>(
    index: &'a super::index::SemanticIndex,
    variable: &super::index::VariableNode,
) -> Vec<&'a crate::expression::Expression> {
    let Some(assignments) = variable.resolve.as_assignments() else {
        return Vec::new();
    };
    let super::index::ProjectField::Present(allocation_id) = &assignments.allocation else {
        return Vec::new();
    };
    let Some((layer, allocation)) = index.layers.values().find_map(|layer| {
        layer
            .allocations
            .iter()
            .find(|candidate| {
                matches!(&candidate.id, super::index::ProjectField::Present(id) if id.value == allocation_id.value)
            })
            .map(|allocation| (layer, allocation))
    }) else {
        return Vec::new();
    };
    let mut expressions = Vec::new();
    if let super::index::ProjectField::Present(unit) = &layer.unit {
        expressions.push(&unit.value);
    }
    if let Some(super::index::ProjectField::Present(eligibility)) = &allocation.eligibility {
        expressions.push(&eligibility.value);
    }
    expressions
}

/// The present `filter`/`sort` expressions of a variable's `method = "query"`
/// pipeline, if any.
pub(in crate::lint) fn variable_query_expressions(
    variable: &super::index::VariableNode,
) -> Vec<&super::index::Spanned<crate::expression::Expression>> {
    let Some(query) = variable.resolve.as_query() else {
        return Vec::new();
    };
    [&query.filter, &query.sort]
        .iter()
        .filter_map(|field| match field {
            Some(super::index::ProjectField::Present(expression)) => Some(expression),
            _ => None,
        })
        .collect()
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
