use super::*;

pub(super) fn inspect_catalog(
    snapshot: &PackageLintSnapshot,
    id: &str,
) -> Result<CatalogInspectReport> {
    let catalog = snapshot
        .index
        .catalogs
        .get(id)
        .ok_or_else(|| RototoError::new(format!("catalog not found: catalog://{id}")))?;
    let (_source_uri, path) = document_uri_path(snapshot, catalog.doc);
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_belongs_to_catalog(diagnostic, id))
        .cloned()
        .collect();

    Ok(CatalogInspectReport {
        id: id.to_owned(),
        uri: format!("catalog://{id}"),
        path,
        description: catalog
            .json
            .as_ref()
            .and_then(|json| json.get("description"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        schema: Some(catalog.path.clone()),
        entries: catalog_entries(snapshot, id),
        dependencies: catalog_dependencies(snapshot, id),
        consumers: catalog_consumers(snapshot, id),
        diagnostics,
    })
}

pub(super) fn catalog_entries(
    snapshot: &PackageLintSnapshot,
    id: &str,
) -> Vec<CatalogEntryInspectReport> {
    snapshot
        .index
        .catalog_entries
        .get(id)
        .into_iter()
        .flat_map(|entries| entries.values())
        .map(|entry| CatalogEntryInspectReport {
            key: entry.key.clone(),
            value: entry.value.clone(),
            location: entry.location.clone(),
        })
        .collect()
}

pub(super) fn catalog_dependencies(
    _snapshot: &PackageLintSnapshot,
    _catalog: &str,
) -> DependencyInspectReport {
    DependencyInspectReport {
        variables: Vec::new(),
        context_paths: Vec::new(),
        catalogs: Vec::new(),
    }
}

pub(super) fn catalog_consumers(
    snapshot: &PackageLintSnapshot,
    catalog: &str,
) -> Vec<ReferenceInspectReport> {
    snapshot
        .references
        .edges()
        .iter()
        .filter_map(|edge| {
            let ReferenceTarget::Catalog(target) = &edge.target else {
                return None;
            };
            if target != catalog {
                return None;
            }
            Some(ReferenceInspectReport {
                kind: reference_source_kind(&edge.source).to_owned(),
                label: reference_source_label(&edge.source),
                location: edge.location.clone(),
            })
        })
        .collect()
}

pub(super) fn reference_source_kind(source: &ReferenceSource) -> &'static str {
    match source {
        ReferenceSource::VariableRuleConditionVariable { .. }
        | ReferenceSource::VariableRuleContextAttribute { .. }
        | ReferenceSource::VariableRuleValue { .. }
        | ReferenceSource::VariableResolveDefault { .. }
        | ReferenceSource::VariableCatalog { .. }
        | ReferenceSource::VariableQueryVariable { .. }
        | ReferenceSource::VariableQueryContextAttribute { .. }
        | ReferenceSource::VariableAllocation { .. } => "variable",
    }
}

pub(super) fn reference_source_label(source: &ReferenceSource) -> String {
    match source {
        ReferenceSource::VariableRuleConditionVariable { variable, rule }
        | ReferenceSource::VariableRuleContextAttribute { variable, rule }
        | ReferenceSource::VariableRuleValue { variable, rule } => {
            format!("variable {variable} resolve.rule[{rule}]")
        }
        ReferenceSource::VariableResolveDefault { variable } => {
            format!("variable {variable} resolve.default")
        }
        ReferenceSource::VariableCatalog { variable } => format!("variable {variable}"),
        ReferenceSource::VariableQueryVariable { variable }
        | ReferenceSource::VariableQueryContextAttribute { variable } => {
            format!("variable {variable} resolve query")
        }
        ReferenceSource::VariableAllocation { variable } => {
            format!("variable {variable} resolve allocation")
        }
    }
}
