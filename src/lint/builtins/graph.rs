use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{
    DiagnosticLocation, EntityId, LintDiagnostic, LintStage, RelatedLocation, RototoRuleId,
};

use super::super::engine::{LintContext, variable_values};
use super::super::nodes::*;
use super::super::stages::push_graph_diagnostic;
use super::qualifier_reference;

#[derive(Clone)]
struct QualifierReferenceEdge {
    from: String,
    to: String,
    location: DiagnosticLocation,
}

pub(super) fn lint_qualifier_cycles(ctx: &mut LintContext) {
    let graph = qualifier_reference_graph(ctx);
    let components = strongly_connected_qualifiers(&graph);
    let mut diagnostics = Vec::new();

    for component in components {
        let component_set: BTreeSet<_> = component.iter().cloned().collect();
        let cycle_edges = component
            .iter()
            .flat_map(|qualifier_id| graph.get(qualifier_id).into_iter().flatten())
            .filter(|edge| component_set.contains(&edge.to))
            .cloned()
            .collect::<Vec<_>>();
        let is_cycle = component.len() > 1
            || cycle_edges
                .iter()
                .any(|edge| edge.from == edge.to && component_set.contains(&edge.from));
        if !is_cycle {
            continue;
        }

        for qualifier_id in &component {
            let Some(qualifier) = ctx.index.qualifiers.get(qualifier_id) else {
                continue;
            };
            let primary_edge = cycle_edges.iter().find(|edge| edge.from == *qualifier_id);
            let primary = primary_edge
                .map(|edge| edge.location.clone())
                .unwrap_or_else(|| qualifier.location.clone());
            let mut diagnostic = LintDiagnostic::rototo(
                RototoRuleId::QualifierCycle,
                LintStage::Graph,
                EntityId::Qualifier {
                    id: qualifier_id.clone(),
                },
                primary.clone(),
                qualifier_cycle_message(qualifier_id, &component),
            );
            diagnostic.related = cycle_edges
                .iter()
                .filter(|edge| edge.from != *qualifier_id || edge.location != primary)
                .map(|edge| RelatedLocation {
                    location: edge.location.clone(),
                    message: format!("cycle reference: {} -> {}", edge.from, edge.to),
                })
                .collect();
            diagnostics.push(diagnostic);
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn qualifier_cycle_message(qualifier_id: &str, component: &[String]) -> String {
    if component.len() == 1 {
        format!("qualifier references itself: {qualifier_id}")
    } else {
        format!(
            "qualifier participates in a reference cycle: {}",
            component.join(" -> ")
        )
    }
}

pub(super) fn lint_unreferenced_qualifiers(ctx: &mut LintContext) {
    let referenced = referenced_qualifier_ids(ctx);
    let mut diagnostics = Vec::new();

    for qualifier in ctx.index.qualifiers.values() {
        if referenced.contains(&qualifier.id) {
            continue;
        }

        push_graph_diagnostic(
            &mut diagnostics,
            RototoRuleId::QualifierUnreferenced,
            EntityId::Qualifier {
                id: qualifier.id.clone(),
            },
            qualifier.location.clone(),
            format!("qualifier is not referenced: {}", qualifier.id),
        );
    }

    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_shadowed_variable_rules(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for variable in ctx.index.variables.values() {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };

        for block in environments.values() {
            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };
            let mut seen_qualifiers: BTreeMap<String, DiagnosticLocation> = BTreeMap::new();

            for rule in rules {
                if rule.invalid_shape {
                    continue;
                }
                let ProjectField::Present(qualifier) = &rule.qualifier else {
                    continue;
                };

                if let Some(first_location) = seen_qualifiers.get(&qualifier.value) {
                    let mut diagnostic = LintDiagnostic::rototo(
                        RototoRuleId::VariableRuleShadowed,
                        LintStage::Graph,
                        EntityId::Rule {
                            variable: variable.id.clone(),
                            environment: block.environment.clone(),
                            index: rule.index,
                        },
                        qualifier.location.clone(),
                        format!(
                            "rule is shadowed by an earlier rule with qualifier: {}",
                            qualifier.value
                        ),
                    );
                    diagnostic.related.push(RelatedLocation {
                        location: first_location.clone(),
                        message: format!("first rule using qualifier: {}", qualifier.value),
                    });
                    diagnostics.push(diagnostic);
                } else {
                    seen_qualifiers.insert(qualifier.value.clone(), qualifier.location.clone());
                }
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_unused_variable_values(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for variable in ctx.index.variables.values() {
        let referenced = referenced_variable_value_keys(variable);
        for value in variable_values(ctx, variable) {
            if referenced.contains(&value.key) {
                continue;
            }

            push_graph_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableValueUnused,
                EntityId::Value {
                    variable: variable.id.clone(),
                    key: value.key.clone(),
                },
                value.location.clone(),
                format!("variable value is not referenced: {}", value.key),
            );
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

fn qualifier_reference_graph(ctx: &LintContext) -> BTreeMap<String, Vec<QualifierReferenceEdge>> {
    let known_qualifiers: BTreeSet<_> = ctx.index.qualifiers.keys().cloned().collect();
    let mut graph = known_qualifiers
        .iter()
        .map(|qualifier_id| (qualifier_id.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();

    for qualifier in ctx.index.qualifiers.values() {
        let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
            continue;
        };

        for predicate in predicates {
            let ProjectField::Present(attribute) = &predicate.attribute else {
                continue;
            };
            let Some(referenced_qualifier) = qualifier_reference(&attribute.value) else {
                continue;
            };
            if !known_qualifiers.contains(referenced_qualifier) {
                continue;
            }

            graph
                .entry(qualifier.id.clone())
                .or_default()
                .push(QualifierReferenceEdge {
                    from: qualifier.id.clone(),
                    to: referenced_qualifier.to_owned(),
                    location: attribute.location.clone(),
                });
        }
    }

    graph
}

#[derive(Default)]
struct TarjanState {
    next_index: usize,
    stack: Vec<String>,
    indices: BTreeMap<String, usize>,
    lowlinks: BTreeMap<String, usize>,
    on_stack: BTreeSet<String>,
    components: Vec<Vec<String>>,
}

fn strongly_connected_qualifiers(
    graph: &BTreeMap<String, Vec<QualifierReferenceEdge>>,
) -> Vec<Vec<String>> {
    let mut state = TarjanState::default();

    for qualifier_id in graph.keys() {
        if !state.indices.contains_key(qualifier_id) {
            strong_connect_qualifier(qualifier_id, graph, &mut state);
        }
    }

    state.components
}

fn strong_connect_qualifier(
    qualifier_id: &str,
    graph: &BTreeMap<String, Vec<QualifierReferenceEdge>>,
    state: &mut TarjanState,
) {
    state
        .indices
        .insert(qualifier_id.to_owned(), state.next_index);
    state
        .lowlinks
        .insert(qualifier_id.to_owned(), state.next_index);
    state.next_index += 1;
    state.stack.push(qualifier_id.to_owned());
    state.on_stack.insert(qualifier_id.to_owned());

    if let Some(edges) = graph.get(qualifier_id) {
        for edge in edges {
            if !state.indices.contains_key(&edge.to) {
                strong_connect_qualifier(&edge.to, graph, state);
                let target_lowlink = state.lowlinks[&edge.to];
                let lowlink = state.lowlinks.get_mut(qualifier_id).unwrap();
                *lowlink = (*lowlink).min(target_lowlink);
            } else if state.on_stack.contains(&edge.to) {
                let target_index = state.indices[&edge.to];
                let lowlink = state.lowlinks.get_mut(qualifier_id).unwrap();
                *lowlink = (*lowlink).min(target_index);
            }
        }
    }

    if state.lowlinks[qualifier_id] != state.indices[qualifier_id] {
        return;
    }

    let mut component = Vec::new();
    while let Some(member) = state.stack.pop() {
        state.on_stack.remove(&member);
        let is_root = member == qualifier_id;
        component.push(member);
        if is_root {
            break;
        }
    }
    component.sort();
    state.components.push(component);
}

fn referenced_qualifier_ids(ctx: &LintContext) -> BTreeSet<String> {
    let known_qualifiers: BTreeSet<_> = ctx.index.qualifiers.keys().cloned().collect();
    let mut referenced = BTreeSet::new();

    for edges in qualifier_reference_graph(ctx).values() {
        for edge in edges {
            if edge.from != edge.to {
                referenced.insert(edge.to.clone());
            }
        }
    }

    for variable in ctx.index.variables.values() {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            continue;
        };
        for block in environments.values() {
            let RuleCollection::Rules(rules) = &block.rules else {
                continue;
            };
            for rule in rules {
                if rule.invalid_shape {
                    continue;
                }
                let ProjectField::Present(qualifier) = &rule.qualifier else {
                    continue;
                };
                if known_qualifiers.contains(&qualifier.value) {
                    referenced.insert(qualifier.value.clone());
                }
            }
        }
    }

    referenced
}

fn referenced_variable_value_keys(variable: &VariableNode) -> BTreeSet<String> {
    let mut referenced = BTreeSet::new();
    let EnvironmentCollection::Environments(environments) = &variable.environments else {
        return referenced;
    };

    for block in environments.values() {
        if let ProjectField::Present(value) = &block.value {
            referenced.insert(value.value.clone());
        }
        let RuleCollection::Rules(rules) = &block.rules else {
            continue;
        };
        for rule in rules {
            if rule.invalid_shape {
                continue;
            }
            if let ProjectField::Present(value) = &rule.value {
                referenced.insert(value.value.clone());
            }
        }
    }

    referenced
}
