use super::super::index::*;
use super::common::{expression_project_field_label, json_project_field_label};
use super::{WorkspaceDocumentSymbol, WorkspaceDocumentSymbolKind};

pub(crate) fn document_symbols(index: &SemanticIndex, path: &str) -> Vec<WorkspaceDocumentSymbol> {
    let mut symbols = Vec::new();

    if let Some(manifest) = &index.manifest
        && manifest.location.path == path
        && let Some(symbol) = workspace_extends_symbol(&manifest.extends)
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

    for catalog in index.catalogs.values() {
        if catalog.location.path == path {
            symbols.push(catalog_document_symbol(catalog));
        }
    }

    for entries in index.catalog_entries.values() {
        for entry in entries.values() {
            if entry.location.path == path {
                symbols.push(catalog_entry_document_symbol(entry));
            }
        }
    }

    sort_workspace_document_symbols(&mut symbols);
    symbols
}

fn workspace_extends_symbol(
    extends: &WorkspaceExtendsCollection,
) -> Option<WorkspaceDocumentSymbol> {
    match extends {
        WorkspaceExtendsCollection::Missing => None,
        WorkspaceExtendsCollection::Invalid { location } => Some(WorkspaceDocumentSymbol::new(
            "extends",
            WorkspaceDocumentSymbolKind::WorkspaceExtends,
            location.clone(),
            Vec::new(),
        )),
        WorkspaceExtendsCollection::Sources { location, values } => {
            Some(WorkspaceDocumentSymbol::new(
                "extends",
                WorkspaceDocumentSymbolKind::WorkspaceExtends,
                location.clone(),
                values
                    .iter()
                    .map(|source| {
                        WorkspaceDocumentSymbol::new(
                            source.source.clone(),
                            WorkspaceDocumentSymbolKind::WorkspaceExtendSource,
                            source.location.clone(),
                            Vec::new(),
                        )
                    })
                    .collect(),
            ))
        }
    }
}

fn qualifier_document_symbol(qualifier: &QualifierNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        qualifier.id.clone(),
        WorkspaceDocumentSymbolKind::Qualifier,
        qualifier.location.clone(),
        Vec::new(),
    )
}

fn variable_document_symbol(variable: &VariableNode) -> WorkspaceDocumentSymbol {
    let mut children = Vec::new();
    if let Some(values) = variable_values_document_symbol(variable) {
        children.push(values);
    }
    if let Some(resolve) = variable_resolve_document_symbol(variable) {
        children.push(resolve);
    }

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

fn variable_resolve_document_symbol(variable: &VariableNode) -> Option<WorkspaceDocumentSymbol> {
    let ResolveNode::Resolve {
        location, rules, ..
    } = &variable.resolve
    else {
        return None;
    };
    let children = match rules {
        RuleCollection::Rules(rules) => rules
            .iter()
            .map(variable_rule_document_symbol)
            .collect::<Vec<_>>(),
        RuleCollection::Invalid { .. } => Vec::new(),
    };
    Some(WorkspaceDocumentSymbol::new(
        "resolve",
        WorkspaceDocumentSymbolKind::Resolve,
        location.clone(),
        children,
    ))
}

fn variable_rule_document_symbol(rule: &VariableRuleNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        variable_rule_symbol_name(rule),
        WorkspaceDocumentSymbolKind::Rule,
        rule.location.clone(),
        Vec::new(),
    )
}

fn catalog_document_symbol(catalog: &CatalogNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        catalog.id.clone(),
        WorkspaceDocumentSymbolKind::Catalog,
        catalog.location.clone(),
        Vec::new(),
    )
}

fn catalog_entry_document_symbol(entry: &CatalogEntryNode) -> WorkspaceDocumentSymbol {
    WorkspaceDocumentSymbol::new(
        format!("{}.{}", entry.catalog_id, entry.key),
        WorkspaceDocumentSymbolKind::CatalogEntry,
        entry.location.clone(),
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

fn variable_rule_symbol_name(rule: &VariableRuleNode) -> String {
    let index = rule.index + 1;
    let selector = expression_project_field_label(&rule.when)
        .or_else(|| expression_project_field_label(&rule.query));
    match (selector, json_project_field_label(&rule.value)) {
        (Some(condition), Some(value)) => format!("rule {index}: {condition} -> {value}"),
        (Some(condition), None) => format!("rule {index}: {condition}"),
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
