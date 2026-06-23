use std::collections::{BTreeMap, BTreeSet};

use super::PackageLintSnapshot;
use crate::expression::Expression;

use super::index::{ProjectField, SemanticIndex};
use super::references::{ReferenceIndex, ReferenceSource, ReferenceTarget};

#[derive(Debug, Clone, Default)]
pub(crate) struct RequestContextCompatibility {
    pub(crate) qualifiers: BTreeMap<String, BTreeSet<String>>,
    pub(crate) variables: BTreeMap<String, BTreeSet<String>>,
}

pub(crate) fn compatibility(snapshot: &PackageLintSnapshot) -> RequestContextCompatibility {
    compatibility_for(&snapshot.index, &snapshot.references)
}

pub(in crate::lint) fn compatibility_for(
    index: &SemanticIndex,
    _references: &ReferenceIndex,
) -> RequestContextCompatibility {
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

    RequestContextCompatibility {
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
            let path_contexts = self
                .index
                .request_contexts
                .values()
                .filter(|context| {
                    context
                        .json
                        .as_ref()
                        .is_some_and(|schema| context_schema_field(schema, path).is_some())
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
