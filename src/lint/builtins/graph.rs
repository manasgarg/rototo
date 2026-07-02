use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{
    DiagnosticLocation, LintDiagnostic, LintStage, RelatedLocation, RototoRuleId, SemanticEntity,
    SemanticField, Severity,
};

use super::super::engine::LintContext;
use super::super::index::*;
use super::super::references::QualifierReferenceEdge;
use super::super::stages::push_graph_diagnostic;

pub(super) fn lint_qualifier_cycles(ctx: &mut LintContext) {
    let graph = ctx.references.qualifier_reference_graph();
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
                qualifier.target(),
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

pub(super) fn lint_variable_cycles(ctx: &mut LintContext) {
    let graph = ctx.references.variable_reference_graph();
    let components = strongly_connected_qualifiers(&graph);
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

fn qualifier_cycle_message(qualifier_id: &str, component: &[String]) -> String {
    if component.len() == 1 {
        format!("qualifier references itself: {qualifier_id}")
    } else {
        format!(
            "qualifier participates in a reference cycle with: {}",
            component.join(", ")
        )
    }
}

pub(super) fn lint_unreferenced_qualifiers(ctx: &mut LintContext) {
    let referenced = ctx.references.referenced_qualifier_ids();
    let graph = ctx.references.qualifier_reference_graph();
    let mut diagnostics = Vec::new();

    for qualifier in ctx.index.qualifiers.values() {
        if referenced.contains(&qualifier.id)
            || graph
                .get(&qualifier.id)
                .into_iter()
                .flatten()
                .any(|edge| edge.from == qualifier.id && edge.to == qualifier.id)
        {
            continue;
        }

        push_graph_diagnostic(
            &mut diagnostics,
            RototoRuleId::QualifierUnreferenced,
            qualifier.target(),
            qualifier.location.clone(),
            format!("qualifier is not referenced: {}", qualifier.id),
        );
    }

    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_unreachable_qualifiers(ctx: &mut LintContext) {
    let reachable = ctx.references.resolution_reachable_qualifier_ids();
    let referenced = ctx.references.referenced_qualifier_ids();
    let mut diagnostics = Vec::new();
    for qualifier in ctx.index.qualifiers.values() {
        if reachable.contains(&qualifier.id)
            || !referenced.contains(&qualifier.id)
            || qualifier_has_existing_error(ctx, &qualifier.id)
        {
            continue;
        }

        push_graph_diagnostic(
            &mut diagnostics,
            RototoRuleId::QualifierUnreachable,
            qualifier.target(),
            qualifier.location.clone(),
            format!("qualifier cannot affect resolution: {}", qualifier.id),
        );
    }

    ctx.diagnostics.extend(diagnostics);
}

fn qualifier_has_existing_error(ctx: &LintContext, qualifier_id: &str) -> bool {
    ctx.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == Severity::Error
            && match &diagnostic.target.entity {
                SemanticEntity::Qualifier { id } => id == qualifier_id,
                SemanticEntity::Predicate { qualifier, .. } => qualifier == qualifier_id,
                _ => false,
            }
    })
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
