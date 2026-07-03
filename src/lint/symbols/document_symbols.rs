use super::super::index::*;
use super::common::{expression_project_field_label, json_project_field_label};
use super::{PackageDocumentSymbol, PackageDocumentSymbolKind};

pub(crate) fn document_symbols(index: &SemanticIndex, path: &str) -> Vec<PackageDocumentSymbol> {
    let mut symbols = Vec::new();

    if let Some(manifest) = &index.manifest
        && manifest.location.path == path
        && let Some(symbol) = package_extends_symbol(&manifest.extends)
    {
        symbols.push(symbol);
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

    sort_package_document_symbols(&mut symbols);
    symbols
}

fn package_extends_symbol(extends: &PackageExtendsCollection) -> Option<PackageDocumentSymbol> {
    match extends {
        PackageExtendsCollection::Missing => None,
        PackageExtendsCollection::Invalid { location } => Some(PackageDocumentSymbol::new(
            "extends",
            PackageDocumentSymbolKind::PackageExtends,
            location.clone(),
            Vec::new(),
        )),
        PackageExtendsCollection::Sources { location, values } => Some(PackageDocumentSymbol::new(
            "extends",
            PackageDocumentSymbolKind::PackageExtends,
            location.clone(),
            values
                .iter()
                .map(|source| {
                    PackageDocumentSymbol::new(
                        source.source.clone(),
                        PackageDocumentSymbolKind::PackageExtendSource,
                        source.location.clone(),
                        Vec::new(),
                    )
                })
                .collect(),
        )),
    }
}

fn variable_document_symbol(variable: &VariableNode) -> PackageDocumentSymbol {
    let mut children = Vec::new();
    if let Some(values) = variable_values_document_symbol(variable) {
        children.push(values);
    }
    if let Some(resolve) = variable_resolve_document_symbol(variable) {
        children.push(resolve);
    }

    PackageDocumentSymbol::new(
        variable.id.clone(),
        PackageDocumentSymbolKind::Variable,
        variable.location.clone(),
        children,
    )
}

fn variable_values_document_symbol(variable: &VariableNode) -> Option<PackageDocumentSymbol> {
    if variable.values.inline_values.is_empty() && !variable.values.invalid_shape {
        return None;
    }

    Some(PackageDocumentSymbol::new(
        "values",
        PackageDocumentSymbolKind::Values,
        variable.values.location.clone(),
        variable
            .values
            .inline_values
            .values()
            .map(value_document_symbol)
            .collect(),
    ))
}

fn variable_resolve_document_symbol(variable: &VariableNode) -> Option<PackageDocumentSymbol> {
    let ResolveNode::Resolve {
        location,
        rules,
        query,
        ..
    } = &variable.resolve
    else {
        return None;
    };
    let mut children = match rules {
        RuleCollection::Rules(rules) => rules
            .iter()
            .map(variable_rule_document_symbol)
            .collect::<Vec<_>>(),
        RuleCollection::Invalid { .. } => Vec::new(),
    };
    if let Some(query) = query {
        children.push(variable_query_document_symbol(query));
    }
    Some(PackageDocumentSymbol::new(
        "resolve",
        PackageDocumentSymbolKind::Resolve,
        location.clone(),
        children,
    ))
}

fn variable_query_document_symbol(query: &QueryNode) -> PackageDocumentSymbol {
    let name = match string_project_field_label(&query.from) {
        Some(from) => format!("query: {from}"),
        None => "query".to_owned(),
    };
    PackageDocumentSymbol::new(
        name,
        PackageDocumentSymbolKind::Rule,
        query.location.clone(),
        Vec::new(),
    )
}

fn string_project_field_label(field: &ProjectField<String>) -> Option<String> {
    match field {
        ProjectField::Present(value) => Some(value.value.clone()),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => None,
    }
}

fn variable_rule_document_symbol(rule: &VariableRuleNode) -> PackageDocumentSymbol {
    PackageDocumentSymbol::new(
        variable_rule_symbol_name(rule),
        PackageDocumentSymbolKind::Rule,
        rule.location.clone(),
        Vec::new(),
    )
}

fn catalog_document_symbol(catalog: &CatalogNode) -> PackageDocumentSymbol {
    PackageDocumentSymbol::new(
        catalog.id.clone(),
        PackageDocumentSymbolKind::Catalog,
        catalog.location.clone(),
        Vec::new(),
    )
}

fn catalog_entry_document_symbol(entry: &CatalogEntryNode) -> PackageDocumentSymbol {
    PackageDocumentSymbol::new(
        format!("{}.{}", entry.catalog_id, entry.key),
        PackageDocumentSymbolKind::CatalogEntry,
        entry.location.clone(),
        Vec::new(),
    )
}

fn value_document_symbol(value: &ValueNode) -> PackageDocumentSymbol {
    PackageDocumentSymbol::new(
        value.key.clone(),
        PackageDocumentSymbolKind::Value,
        value.location.clone(),
        Vec::new(),
    )
}

fn variable_rule_symbol_name(rule: &VariableRuleNode) -> String {
    let index = rule.index + 1;
    let selector = expression_project_field_label(&rule.when);
    match (selector, json_project_field_label(&rule.value)) {
        (Some(condition), Some(value)) => format!("rule {index}: {condition} -> {value}"),
        (Some(condition), None) => format!("rule {index}: {condition}"),
        (None, Some(value)) => format!("rule {index}: {value}"),
        (None, None) => format!("rule {index}"),
    }
}

fn sort_package_document_symbols(symbols: &mut [PackageDocumentSymbol]) {
    for symbol in symbols.iter_mut() {
        sort_package_document_symbols(&mut symbol.children);
    }
    symbols.sort_by(|left, right| {
        symbol_position(left)
            .cmp(&symbol_position(right))
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn symbol_position(symbol: &PackageDocumentSymbol) -> (usize, usize) {
    symbol
        .location
        .range
        .map(|range| (range.start.line, range.start.character))
        .unwrap_or((0, 0))
}
