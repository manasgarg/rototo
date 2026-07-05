use super::*;

pub(super) fn context_path_completion_items(
    snapshot: &PackageLintSnapshot,
    token: &str,
) -> Vec<PackageCompletionItem> {
    let Some(path) = token.strip_prefix("context.") else {
        return Vec::new();
    };
    let parent = parent_path_segments(path);
    let mut fields = BTreeSet::new();

    for context in snapshot.index.evaluation_contexts.values() {
        if let Some(properties) = context
            .json
            .as_ref()
            .and_then(|schema| schema_properties_at_path(schema, &parent))
        {
            fields.extend(properties.keys().cloned());
        }
    }

    path_completion_items("context", &parent, fields, "context field")
}

pub(super) fn entry_path_completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    token: &str,
) -> Vec<PackageCompletionItem> {
    let Some(path_suffix) = token.strip_prefix("entry.") else {
        return Vec::new();
    };
    let Some(catalog_id) = current_variable_query_catalog_id(&snapshot.index, path) else {
        return Vec::new();
    };
    let Some(catalog) = snapshot.index.catalogs.get(&catalog_id) else {
        return Vec::new();
    };
    let parent = parent_path_segments(path_suffix);
    let Some(properties) = catalog
        .json
        .as_ref()
        .and_then(|schema| schema_properties_at_path(schema, &parent))
    else {
        return Vec::new();
    };

    path_completion_items(
        "entry",
        &parent,
        properties.keys().cloned().collect(),
        "entry field",
    )
}

pub(super) fn parent_path_segments(path: &str) -> Vec<&str> {
    let mut segments = path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if !path.ends_with('.') {
        segments.pop();
    }
    segments
}

pub(super) fn schema_properties_at_path<'a>(
    schema: &'a JsonValue,
    parent: &[&str],
) -> Option<&'a serde_json::Map<String, JsonValue>> {
    let mut current = schema;
    for segment in parent {
        let properties = current.get("properties").and_then(JsonValue::as_object)?;
        current = properties.get(*segment)?;
    }
    current.get("properties").and_then(JsonValue::as_object)
}

pub(super) fn path_completion_items(
    root: &str,
    parent: &[&str],
    fields: BTreeSet<String>,
    detail: &'static str,
) -> Vec<PackageCompletionItem> {
    let prefix = if parent.is_empty() {
        format!("{root}.")
    } else {
        format!("{}.{}.", root, parent.join("."))
    };

    fields
        .into_iter()
        .map(|field| {
            PackageCompletionItem::new(
                format!("{prefix}{field}"),
                PackageCompletionItemKind::FieldSelector,
                detail,
            )
        })
        .collect()
}

/// Value completions for the operand after a comparison: when the other side
/// is a context or entry path pinned to a closed set (an `x-rototo-ref`
/// `enum=<id>` or `catalog=<id>` annotation), the legal literals are known and
/// offered ahead of the generic roots.
pub(super) fn typed_operand_completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    cursor: &ExpressionCursor,
) -> Vec<PackageCompletionItem> {
    let Some((root, segments)) = comparison_lhs_path(&cursor.prefix, &cursor.token) else {
        return Vec::new();
    };
    let pin = match root {
        "context" => snapshot
            .index
            .evaluation_contexts
            .values()
            .find_map(|context| {
                let schema = context.json.as_ref()?;
                schema_ref_pin(schema, &segments)
            }),
        "entry" => {
            current_variable_query_catalog_id(&snapshot.index, path).and_then(|catalog_id| {
                snapshot
                    .index
                    .catalogs
                    .get(&catalog_id)
                    .and_then(|catalog| catalog.json.as_ref())
                    .and_then(|schema| schema_ref_pin(schema, &segments))
            })
        }
        _ => None,
    };
    match pin {
        Some(SchemaRefPin::Enum(id)) => enum_member_literal_items(&snapshot.index, &id),
        Some(SchemaRefPin::Catalog(id)) => snapshot
            .index
            .catalog_entries
            .get(&id)
            .into_iter()
            .flat_map(|entries| entries.keys())
            .map(|entry| {
                PackageCompletionItem::new(
                    format!("\"{entry}\""),
                    PackageCompletionItemKind::Value,
                    "catalog entry id",
                )
            })
            .collect(),
        None => Vec::new(),
    }
}

pub(super) enum SchemaRefPin {
    Enum(String),
    Catalog(String),
}

/// The `x-rototo-ref` pin on the schema property a dotted path names, if any.
pub(super) fn schema_ref_pin(schema: &JsonValue, segments: &[&str]) -> Option<SchemaRefPin> {
    let mut current = schema;
    for segment in segments {
        let properties = current.get("properties").and_then(JsonValue::as_object)?;
        current = properties.get(*segment)?;
    }
    let target = current.get("x-rototo-ref")?.as_str()?;
    if let Some(id) = target.strip_prefix("enum=") {
        return Some(SchemaRefPin::Enum(id.to_owned()));
    }
    if let Some(id) = target.strip_prefix("catalog=") {
        return Some(SchemaRefPin::Catalog(id.to_owned()));
    }
    None
}

/// Enum members as ready-to-insert literals: strings arrive quoted, numbers
/// and booleans bare, exactly the spelling the expression needs.
pub(super) fn enum_member_literal_items(
    index: &SemanticIndex,
    id: &str,
) -> Vec<PackageCompletionItem> {
    let Some(ProjectField::Present(members)) =
        index.enum_members.get(id).map(|members| &members.members)
    else {
        return Vec::new();
    };
    members
        .value
        .iter()
        .map(|member| {
            PackageCompletionItem::new(
                member.value.to_string(),
                PackageCompletionItemKind::Value,
                "enum member",
            )
        })
        .collect()
}

pub(super) fn current_variable_query_catalog_id(
    index: &SemanticIndex,
    path: &str,
) -> Option<String> {
    let variable = current_variable_for_path(index, path)?;
    let type_kind = variable_type_kind(&variable.type_source)?;
    match &type_kind.value {
        VariableTypeKind::Catalog(catalog) => Some(catalog.clone()),
        kind => kind.list_catalog().map(ToOwned::to_owned),
    }
}

pub(super) fn variable_completion_items(index: &SemanticIndex) -> Vec<PackageCompletionItem> {
    index
        .variables
        .keys()
        .map(|variable| {
            PackageCompletionItem::new(
                variable.clone(),
                PackageCompletionItemKind::Variable,
                "variable",
            )
        })
        .collect()
}

pub(super) fn current_variable_value_completion_items(
    index: &SemanticIndex,
    path: &str,
) -> Vec<PackageCompletionItem> {
    let Some(variable) = current_variable_for_path(index, path) else {
        return Vec::new();
    };

    // The declared type pins the legal values: catalog-backed variables take
    // entry ids, enum-typed variables take members.
    let type_kind = variable_type_kind(&variable.type_source).map(|kind| kind.value);
    let catalog_id = match &type_kind {
        Some(VariableTypeKind::Catalog(catalog)) => Some(catalog.clone()),
        Some(kind) => kind.list_catalog().map(ToOwned::to_owned),
        None => None,
    };
    if let Some(catalog) = catalog_id {
        return index
            .catalog_entries
            .get(&catalog)
            .into_iter()
            .flat_map(|entries| entries.keys())
            .map(|value| {
                PackageCompletionItem::new(
                    value.clone(),
                    PackageCompletionItemKind::Value,
                    "catalog value",
                )
            })
            .collect();
    }
    let enum_id = match &type_kind {
        Some(VariableTypeKind::Enum(id)) => Some(id.clone()),
        Some(VariableTypeKind::List(item)) => match item.as_ref() {
            VariableTypeKind::Enum(id) => Some(id.clone()),
            _ => None,
        },
        _ => None,
    };
    if let Some(id) = enum_id {
        return enum_member_literal_items(index, &id);
    }
    match &variable.type_source {
        TypeSourceNode::Catalog(catalog) => index
            .catalog_entries
            .get(&catalog.value)
            .into_iter()
            .flat_map(|entries| entries.keys())
            .map(|value| {
                PackageCompletionItem::new(
                    value.clone(),
                    PackageCompletionItemKind::Value,
                    "catalog value",
                )
            })
            .collect(),
        _ => variable
            .values
            .inline_values
            .keys()
            .map(|value| {
                PackageCompletionItem::new(
                    value.clone(),
                    PackageCompletionItemKind::Value,
                    "variable value",
                )
            })
            .collect(),
    }
}

pub(super) fn current_variable_for_path<'a>(
    index: &'a SemanticIndex,
    path: &str,
) -> Option<&'a VariableNode> {
    index
        .variables
        .values()
        .find(|variable| variable.location.path == path)
}
