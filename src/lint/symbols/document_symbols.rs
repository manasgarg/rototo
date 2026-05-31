use super::super::nodes::*;
use super::common::{predicate_op_project_field_value, string_project_field_value};
use super::{WorkspaceDocumentSymbol, WorkspaceDocumentSymbolKind};

pub(crate) fn document_symbols(index: &SemanticIndex, path: &str) -> Vec<WorkspaceDocumentSymbol> {
    let mut symbols = Vec::new();

    if let Some(manifest) = &index.manifest
        && manifest.location.path == path
        && let Some(symbol) = workspace_environments_symbol(&manifest.environments)
    {
        symbols.push(symbol);
    }

    for qualifier in index.qualifiers.values() {
        if qualifier.location.path == path {
            symbols.push(qualifier_document_symbol(qualifier));
        }
    }

    for variable in index.variables.values() {
        if variable.location.path == path {
            symbols.push(variable_document_symbol(variable));
        }
    }

    for (variable_id, values) in &index.external_values {
        for value in values.values() {
            if value.location.path == path {
                symbols.push(external_value_document_symbol(variable_id, value));
            }
        }
    }

    sort_workspace_document_symbols(&mut symbols);
    symbols
}

fn workspace_environments_symbol(
    environments: &WorkspaceEnvironmentCollection,
) -> Option<WorkspaceDocumentSymbol> {
    match environments {
        WorkspaceEnvironmentCollection::Missing => None,
        WorkspaceEnvironmentCollection::Invalid { location } => Some(WorkspaceDocumentSymbol::new(
            "environments",
            WorkspaceDocumentSymbolKind::WorkspaceEnvironments,
            location.clone(),
            Vec::new(),
        )),
        WorkspaceEnvironmentCollection::Environments { location, values } => {
            Some(WorkspaceDocumentSymbol::new(
                "environments",
                WorkspaceDocumentSymbolKind::WorkspaceEnvironments,
                location.clone(),
                values
                    .iter()
                    .map(|environment| {
                        WorkspaceDocumentSymbol::new(
                            environment.name.clone(),
                            WorkspaceDocumentSymbolKind::Environment,
                            environment.location.clone(),
                            Vec::new(),
                        )
                    })
                    .collect(),
            ))
        }
    }
}

fn qualifier_document_symbol(qualifier: &QualifierNode) -> WorkspaceDocumentSymbol {
    let children = match &qualifier.predicates {
        PredicateCollection::Predicates(predicates) => predicates
            .iter()
            .map(predicate_document_symbol)
            .collect::<Vec<_>>(),
        PredicateCollection::Missing { .. } | PredicateCollection::Invalid { .. } => Vec::new(),
    };
    WorkspaceDocumentSymbol::new(
        qualifier.id.clone(),
        WorkspaceDocumentSymbolKind::Qualifier,
        qualifier.location.clone(),
        children,
    )
}

fn predicate_document_symbol(predicate: &PredicateNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        predicate_symbol_name(predicate),
        WorkspaceDocumentSymbolKind::Predicate,
        predicate.location.clone(),
        Vec::new(),
    )
}

fn variable_document_symbol(variable: &VariableNode) -> WorkspaceDocumentSymbol {
    let mut children = Vec::new();
    if let Some(values) = variable_values_document_symbol(variable) {
        children.push(values);
    }
    children.extend(variable_environment_document_symbols(variable));

    WorkspaceDocumentSymbol::new(
        variable.id.clone(),
        WorkspaceDocumentSymbolKind::Variable,
        variable.location.clone(),
        children,
    )
}

fn variable_values_document_symbol(variable: &VariableNode) -> Option<WorkspaceDocumentSymbol> {
    if variable.values.inline_values.is_empty() && !variable.values.invalid_shape {
        return None;
    }

    Some(WorkspaceDocumentSymbol::new(
        "values",
        WorkspaceDocumentSymbolKind::Values,
        variable.values.location.clone(),
        variable
            .values
            .inline_values
            .values()
            .map(value_document_symbol)
            .collect(),
    ))
}

fn variable_environment_document_symbols(variable: &VariableNode) -> Vec<WorkspaceDocumentSymbol> {
    let EnvironmentCollection::Environments(environments) = &variable.environments else {
        return Vec::new();
    };

    environments
        .values()
        .map(|block| {
            let children = match &block.rules {
                RuleCollection::Rules(rules) => rules
                    .iter()
                    .map(variable_rule_document_symbol)
                    .collect::<Vec<_>>(),
                RuleCollection::Invalid { .. } => Vec::new(),
            };
            WorkspaceDocumentSymbol::new(
                format!("env.{}", block.environment),
                WorkspaceDocumentSymbolKind::EnvironmentBlock,
                block.location.clone(),
                children,
            )
        })
        .collect()
}

fn variable_rule_document_symbol(rule: &VariableRuleNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        variable_rule_symbol_name(rule),
        WorkspaceDocumentSymbolKind::Rule,
        rule.location.clone(),
        Vec::new(),
    )
}

fn external_value_document_symbol(variable_id: &str, value: &ValueNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        format!("{}.{}", variable_id, value.key),
        WorkspaceDocumentSymbolKind::Value,
        value.location.clone(),
        Vec::new(),
    )
}

fn value_document_symbol(value: &ValueNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        value.key.clone(),
        WorkspaceDocumentSymbolKind::Value,
        value.location.clone(),
        Vec::new(),
    )
}

fn predicate_symbol_name(predicate: &PredicateNode) -> String {
    let index = predicate.index + 1;
    let Some(attribute) = string_project_field_value(&predicate.attribute) else {
        return format!("predicate {index}");
    };
    let Some(op) = predicate_op_project_field_value(&predicate.op) else {
        return format!("predicate {index}: {attribute}");
    };
    format!("predicate {index}: {attribute} {op}")
}

fn variable_rule_symbol_name(rule: &VariableRuleNode) -> String {
    let index = rule.index + 1;
    match (
        string_project_field_value(&rule.qualifier),
        string_project_field_value(&rule.value),
    ) {
        (Some(qualifier), Some(value)) => format!("rule {index}: {qualifier} -> {value}"),
        (Some(qualifier), None) => format!("rule {index}: {qualifier}"),
        (None, Some(value)) => format!("rule {index}: {value}"),
        (None, None) => format!("rule {index}"),
    }
}

fn sort_workspace_document_symbols(symbols: &mut [WorkspaceDocumentSymbol]) {
    for symbol in symbols.iter_mut() {
        sort_workspace_document_symbols(&mut symbol.children);
    }
    symbols.sort_by(|left, right| {
        symbol_position(left)
            .cmp(&symbol_position(right))
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn symbol_position(symbol: &WorkspaceDocumentSymbol) -> (usize, usize) {
    symbol
        .location
        .range
        .map(|range| (range.start.line, range.start.character))
        .unwrap_or((0, 0))
}
