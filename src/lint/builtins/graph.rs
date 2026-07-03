use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{
    DiagnosticLocation, LintDiagnostic, LintStage, RelatedLocation, RototoRuleId, SemanticField,
};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::references::VariableReferenceEdge;
use super::super::stages::push_graph_diagnostic;

pub(super) fn lint_variable_cycles(ctx: &mut LintContext) {
    let graph = ctx.references.variable_reference_graph();
    let components = strongly_connected_variables(&graph);
    let mut diagnostics = Vec::new();

    for component in components {
        let component_set: BTreeSet<_> = component.iter().cloned().collect();
        let cycle_edges = component
            .iter()
            .flat_map(|variable_id| graph.get(variable_id).into_iter().flatten())
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

        for variable_id in &component {
            let Some(variable) = ctx.index.variables.get(variable_id) else {
                continue;
            };
            let primary_edge = cycle_edges.iter().find(|edge| edge.from == *variable_id);
            let primary = primary_edge
                .map(|edge| edge.location.clone())
                .unwrap_or_else(|| variable.location.clone());
            let mut diagnostic = LintDiagnostic::rototo(
                RototoRuleId::VariableReferenceCycle,
                LintStage::Graph,
                variable.target(),
                primary.clone(),
                variable_cycle_message(variable_id, &component),
            );
            diagnostic.related = cycle_edges
                .iter()
                .filter(|edge| edge.from != *variable_id || edge.location != primary)
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

fn variable_cycle_message(variable_id: &str, component: &[String]) -> String {
    if component.len() == 1 {
        format!("variable references itself: {variable_id}")
    } else {
        format!(
            "variable participates in a reference cycle with: {}",
            component.join(", ")
        )
    }
}

pub(super) fn lint_shadowed_variable_rules(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for variable in ctx.index.variables.values() {
        let ResolveNode::Resolve { rules, .. } = &variable.resolve else {
            continue;
        };
        let RuleCollection::Rules(rules) = rules else {
            continue;
        };
        let mut seen_conditions: BTreeMap<String, DiagnosticLocation> = BTreeMap::new();

        for rule in rules {
            if rule.invalid_shape {
                continue;
            }
            let Some(ProjectField::Present(when)) = &rule.when else {
                continue;
            };
            let condition = when.value.source().to_owned();

            if let Some(first_location) = seen_conditions.get(&condition) {
                let mut diagnostic = LintDiagnostic::rototo(
                    RototoRuleId::VariableRuleShadowed,
                    LintStage::Graph,
                    rule.field_target(&variable.id, SemanticField::VariableRuleWhen),
                    when.location.clone(),
                    format!("rule is shadowed by an earlier rule with condition: {condition}"),
                );
                diagnostic.related.push(RelatedLocation {
                    location: first_location.clone(),
                    message: format!("first rule using condition: {condition}"),
                });
                diagnostics.push(diagnostic);
            } else {
                seen_conditions.insert(condition, when.location.clone());
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_rules_selecting_default_value(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();

    for variable in ctx.index.variables.values() {
        let ResolveNode::Resolve { default, rules, .. } = &variable.resolve else {
            continue;
        };
        let ProjectField::Present(default_value) = default.as_ref() else {
            continue;
        };
        let RuleCollection::Rules(rules) = rules else {
            continue;
        };

        for rule in rules {
            if rule.invalid_shape {
                continue;
            }
            let ProjectField::Present(rule_value) = &rule.value else {
                continue;
            };
            if rule_value.value != default_value.value {
                continue;
            }

            push_graph_diagnostic(
                &mut diagnostics,
                RototoRuleId::VariableRuleSelectsDefaultValue,
                rule.field_target(&variable.id, SemanticField::VariableRuleValue),
                rule_value.location.clone(),
                format!(
                    "rule selects the same value as the resolve default: {}",
                    rule_value.value
                ),
            );
            if let Some(diagnostic) = diagnostics.last_mut() {
                diagnostic.related.push(RelatedLocation {
                    location: default_value.location.clone(),
                    message: format!("resolve default value: {}", default_value.value),
                });
            }
        }
    }

    ctx.diagnostics.extend(diagnostics);
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

fn strongly_connected_variables(
    graph: &BTreeMap<String, Vec<VariableReferenceEdge>>,
) -> Vec<Vec<String>> {
    let mut state = TarjanState::default();

    for variable_id in graph.keys() {
        if !state.indices.contains_key(variable_id) {
            strong_connect_variable(variable_id, graph, &mut state);
        }
    }

    state.components
}

fn strong_connect_variable(
    variable_id: &str,
    graph: &BTreeMap<String, Vec<VariableReferenceEdge>>,
    state: &mut TarjanState,
) {
    state
        .indices
        .insert(variable_id.to_owned(), state.next_index);
    state
        .lowlinks
        .insert(variable_id.to_owned(), state.next_index);
    state.next_index += 1;
    state.stack.push(variable_id.to_owned());
    state.on_stack.insert(variable_id.to_owned());

    if let Some(edges) = graph.get(variable_id) {
        for edge in edges {
            if !state.indices.contains_key(&edge.to) {
                strong_connect_variable(&edge.to, graph, state);
                let target_lowlink = state.lowlinks[&edge.to];
                let lowlink = state.lowlinks.get_mut(variable_id).unwrap();
                *lowlink = (*lowlink).min(target_lowlink);
            } else if state.on_stack.contains(&edge.to) {
                let target_index = state.indices[&edge.to];
                let lowlink = state.lowlinks.get_mut(variable_id).unwrap();
                *lowlink = (*lowlink).min(target_index);
            }
        }
    }

    if state.lowlinks[variable_id] != state.indices[variable_id] {
        return;
    }

    let mut component = Vec::new();
    while let Some(member) = state.stack.pop() {
        state.on_stack.remove(&member);
        let is_root = member == variable_id;
        component.push(member);
        if is_root {
            break;
        }
    }
    component.sort();
    state.components.push(component);
}
