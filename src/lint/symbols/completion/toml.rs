use super::*;

pub(super) struct TomlCompletionSpec {
    label: &'static str,
    detail: &'static str,
    insert_text: &'static str,
}

pub(super) const VARIABLE_TOP_LEVEL_COMPLETIONS: &[TomlCompletionSpec] = &[
    TomlCompletionSpec {
        label: "schema_version",
        detail: "variable field",
        insert_text: "schema_version = 1",
    },
    TomlCompletionSpec {
        label: "description",
        detail: "variable field",
        insert_text: "description = \"\"",
    },
    TomlCompletionSpec {
        label: "type",
        detail: "variable field",
        insert_text: "type = \"string\"",
    },
    TomlCompletionSpec {
        label: "[resolve]",
        detail: "variable block",
        insert_text: "[resolve]\ndefault = ",
    },
    TomlCompletionSpec {
        label: "[[resolve.rule]]",
        detail: "variable block",
        insert_text: "[[resolve.rule]]\nwhen = \"\"\nvalue = ",
    },
];

pub(super) const VARIABLE_RESOLVE_COMPLETIONS: &[TomlCompletionSpec] = &[
    TomlCompletionSpec {
        label: "method",
        detail: "variable field",
        insert_text: "method = \"rules\"",
    },
    TomlCompletionSpec {
        label: "default",
        detail: "variable field",
        insert_text: "default = ",
    },
    TomlCompletionSpec {
        label: "from",
        detail: "variable field",
        insert_text: "from = \"\"",
    },
    TomlCompletionSpec {
        label: "filter",
        detail: "variable field",
        insert_text: "filter = \"\"",
    },
    TomlCompletionSpec {
        label: "sort",
        detail: "variable field",
        insert_text: "sort = \"\"",
    },
    TomlCompletionSpec {
        label: "order",
        detail: "variable field",
        insert_text: "order = \"asc\"",
    },
    TomlCompletionSpec {
        label: "limit",
        detail: "variable field",
        insert_text: "limit = ",
    },
    TomlCompletionSpec {
        label: "allocation",
        detail: "variable field",
        insert_text: "allocation = \"\"",
    },
    TomlCompletionSpec {
        label: "[[resolve.rule]]",
        detail: "variable block",
        insert_text: "[[resolve.rule]]\nwhen = \"\"\nvalue = ",
    },
    TomlCompletionSpec {
        label: "[[resolve.assign]]",
        detail: "variable block",
        insert_text: "[[resolve.assign]]\narm = \"\"\nvalue = ",
    },
];

pub(super) const VARIABLE_ASSIGN_COMPLETIONS: &[TomlCompletionSpec] = &[
    TomlCompletionSpec {
        label: "arm",
        detail: "variable field",
        insert_text: "arm = \"\"",
    },
    TomlCompletionSpec {
        label: "value",
        detail: "variable field",
        insert_text: "value = ",
    },
    TomlCompletionSpec {
        label: "[[resolve.assign]]",
        detail: "variable block",
        insert_text: "[[resolve.assign]]\narm = \"\"\nvalue = ",
    },
];

pub(super) const VARIABLE_RULE_COMPLETIONS: &[TomlCompletionSpec] = &[
    TomlCompletionSpec {
        label: "when",
        detail: "variable field",
        insert_text: "when = \"\"",
    },
    TomlCompletionSpec {
        label: "value",
        detail: "variable field",
        insert_text: "value = ",
    },
    TomlCompletionSpec {
        label: "[[resolve.rule]]",
        detail: "variable block",
        insert_text: "[[resolve.rule]]\nwhen = \"\"\nvalue = ",
    },
];

pub(super) const CUSTOM_LINT_FIELD_SELECTORS: &[&str] = &[
    "description",
    "extends",
    "id",
    "json",
    "json.",
    "key",
    "not",
    "predicates",
    "resolve",
    "schema",
    "type",
    "value",
    "value.",
    "values",
];

pub(super) fn variable_field_completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Vec<PackageCompletionItem> {
    let context = toml_completion_context(snapshot, path, position);
    match context.table.as_deref() {
        None => toml_completion_items(VARIABLE_TOP_LEVEL_COMPLETIONS, &context),
        Some("resolve") => toml_completion_items(VARIABLE_RESOLVE_COMPLETIONS, &context),
        Some("resolve.rule") => toml_completion_items(VARIABLE_RULE_COMPLETIONS, &context),
        Some("resolve.assign") => toml_completion_items(VARIABLE_ASSIGN_COMPLETIONS, &context),
        Some(_) => Vec::new(),
    }
}

pub(super) fn toml_completion_items(
    specs: &[TomlCompletionSpec],
    context: &TomlCompletionContext,
) -> Vec<PackageCompletionItem> {
    specs
        .iter()
        .filter(|spec| toml_completion_spec_is_available(spec, context))
        .map(|spec| {
            PackageCompletionItem::new(
                spec.label,
                PackageCompletionItemKind::FieldSelector,
                spec.detail,
            )
            .with_insert_text(spec.insert_text)
        })
        .collect()
}

pub(super) struct TomlCompletionContext {
    table: Option<String>,
    keys: BTreeSet<String>,
    tables: BTreeSet<String>,
}

pub(super) fn toml_completion_context(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> TomlCompletionContext {
    let Some(text) = snapshot.source_text(path) else {
        return TomlCompletionContext {
            table: None,
            keys: BTreeSet::new(),
            tables: BTreeSet::new(),
        };
    };
    let lines = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>();
    let tables = lines
        .iter()
        .filter_map(|line| toml_table_header(line))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let (table, start_line) = toml_table_before_position(&lines, position.line);
    let end_line = toml_table_end_line(&lines, start_line);
    let keys = lines
        .iter()
        .enumerate()
        .skip(start_line)
        .take(end_line.saturating_sub(start_line))
        .filter(|(line, _)| *line != position.line)
        .filter_map(|(_, line)| toml_key(line))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

    TomlCompletionContext {
        table,
        keys,
        tables,
    }
}

pub(super) fn toml_completion_spec_is_available(
    spec: &TomlCompletionSpec,
    context: &TomlCompletionContext,
) -> bool {
    if spec.label == "[[resolve.rule]]" {
        return true;
    }
    if spec.label == "[resolve]" {
        return !context.tables.contains("resolve");
    }

    let key = spec.label;
    if context.keys.contains(key) {
        return false;
    }
    true
}

pub(super) fn toml_table_before_position(
    lines: &[&str],
    position_line: usize,
) -> (Option<String>, usize) {
    let mut table = None;
    let mut start_line = 0;
    for (line_number, line) in lines.iter().enumerate().take(position_line) {
        if let Some(header) = toml_table_header(line) {
            table = Some(header.to_owned());
            start_line = line_number + 1;
        }
    }
    (table, start_line)
}

pub(super) fn toml_table_end_line(lines: &[&str], start_line: usize) -> usize {
    lines
        .iter()
        .enumerate()
        .skip(start_line)
        .find_map(|(line_number, line)| toml_table_header(line).map(|_| line_number))
        .unwrap_or(lines.len())
}

pub(super) fn toml_table_header(line: &str) -> Option<&str> {
    let line = line.split('#').next().unwrap_or(line).trim();
    if let Some(rest) = line.strip_prefix("[[") {
        return rest
            .find("]]")
            .map(|end| rest[..end].trim())
            .filter(|name| !name.is_empty());
    }
    if let Some(rest) = line.strip_prefix('[') {
        if rest.starts_with('[') {
            return None;
        }
        return rest
            .find(']')
            .map(|end| rest[..end].trim())
            .filter(|name| !name.is_empty());
    }
    None
}

pub(super) fn toml_key(line: &str) -> Option<&str> {
    let line = line.split('#').next().unwrap_or(line).trim();
    if line.starts_with('[') {
        return None;
    }
    let (key, _) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() { None } else { Some(key) }
}

pub(super) fn catalog_entry_field_completion_items(
    snapshot: &PackageLintSnapshot,
    path: &str,
    position: SourcePosition,
) -> Vec<PackageCompletionItem> {
    let Some(catalog_id) = catalog_id_for_entry_path(path) else {
        return Vec::new();
    };
    let context = toml_completion_context(snapshot, path, position);
    if context.table.is_some() {
        return Vec::new();
    }
    let Some(properties) = snapshot
        .index
        .catalogs
        .get(catalog_id)
        .and_then(|catalog| catalog.json.as_ref())
        .and_then(|json| json.get("properties"))
        .and_then(serde_json::Value::as_object)
    else {
        return Vec::new();
    };

    properties
        .keys()
        .filter(|field| !context.keys.contains(field.as_str()))
        .map(|field| {
            PackageCompletionItem::new(
                field.clone(),
                PackageCompletionItemKind::FieldSelector,
                "catalog entry field",
            )
            .with_insert_text(format!("{field} = "))
        })
        .collect()
}

pub(super) fn catalog_id_for_entry_path(path: &str) -> Option<&str> {
    let path = path.strip_prefix("data/catalogs/")?;
    let (directory, entry) = path.split_once('/')?;
    if entry.is_empty() || !entry.ends_with(".toml") {
        return None;
    }
    (!directory.is_empty()).then_some(directory)
}

pub(super) fn custom_lint_field_selector_completion_items() -> Vec<PackageCompletionItem> {
    CUSTOM_LINT_FIELD_SELECTORS
        .iter()
        .copied()
        .map(|field| {
            PackageCompletionItem::new(
                field,
                PackageCompletionItemKind::FieldSelector,
                "custom lint field selector",
            )
        })
        .collect()
}
