use std::collections::BTreeSet;

use serde_json::Value as JsonValue;

use crate::lint::RuntimePackage;

pub(super) fn catalog_entry_view(
    runtime: &RuntimePackage,
    catalog: &str,
    name: &str,
    entry: &JsonValue,
) -> JsonValue {
    let mut stack = BTreeSet::new();
    hydrate_catalog_entry(runtime, catalog, name, entry, &mut stack)
}

fn hydrate_catalog_entry(
    runtime: &RuntimePackage,
    catalog: &str,
    name: &str,
    entry: &JsonValue,
    stack: &mut BTreeSet<(String, String)>,
) -> JsonValue {
    let mut value = entry.clone();
    if let JsonValue::Object(object) = &mut value {
        object.insert("id".to_owned(), JsonValue::String(name.to_owned()));
    }
    if !stack.insert((catalog.to_owned(), name.to_owned())) {
        return value;
    }
    let hydrated = runtime
        .catalog_schemas
        .get(catalog)
        .map(|schema| hydrate_schema_value(runtime, catalog, schema, &value, stack))
        .unwrap_or(value);
    stack.remove(&(catalog.to_owned(), name.to_owned()));
    hydrated
}

fn hydrate_schema_value(
    runtime: &RuntimePackage,
    schema_catalog: &str,
    schema: &JsonValue,
    value: &JsonValue,
    stack: &mut BTreeSet<(String, String)>,
) -> JsonValue {
    if let Some(reference) = schema.get("$ref").and_then(JsonValue::as_str)
        && let Some((catalog, schema)) = resolve_schema_ref(runtime, schema_catalog, reference)
    {
        return hydrate_schema_value(runtime, &catalog, schema, value, stack);
    }

    if let Some(ref_value) = schema.get("x-rototo-ref")
        && let Some(hydrated) = hydrate_catalog_reference(runtime, ref_value, value, stack)
    {
        return hydrated;
    }

    let mut hydrated = value.clone();

    if let (Some(properties), JsonValue::Object(object)) = (
        schema.get("properties").and_then(JsonValue::as_object),
        &mut hydrated,
    ) {
        for (key, subschema) in properties {
            let Some(child) = object.get(key).cloned() else {
                continue;
            };
            object.insert(
                key.clone(),
                hydrate_schema_value(runtime, schema_catalog, subschema, &child, stack),
            );
        }
    }

    if let (Some(items), JsonValue::Array(values)) = (schema.get("items"), &mut hydrated) {
        for value in values {
            let child = value.clone();
            *value = hydrate_schema_value(runtime, schema_catalog, items, &child, stack);
        }
    }

    for keyword in ["allOf", "anyOf", "oneOf"] {
        let Some(schemas) = schema.get(keyword).and_then(JsonValue::as_array) else {
            continue;
        };
        for subschema in schemas {
            hydrated = hydrate_schema_value(runtime, schema_catalog, subschema, &hydrated, stack);
        }
    }

    hydrated
}

fn hydrate_catalog_reference(
    runtime: &RuntimePackage,
    ref_spec: &JsonValue,
    value: &JsonValue,
    stack: &mut BTreeSet<(String, String)>,
) -> Option<JsonValue> {
    if ref_spec == &JsonValue::Bool(true) {
        let object = value.as_object()?;
        let catalog = object.get("catalog")?.as_str()?;
        let entry = object.get("entry")?.as_str()?;
        let pointer = object.get("pointer").and_then(JsonValue::as_str);
        return hydrate_catalog_reference_target(runtime, catalog, entry, pointer, value, stack);
    }

    // Only catalog=<id> targets hydrate; list members are already literal
    // scalars, so a list reference passes the value through untouched.
    let target_catalogs: Vec<&str> = if let Some(target) = ref_spec.as_str() {
        vec![target.strip_prefix("catalog=")?]
    } else {
        ref_spec
            .as_array()?
            .iter()
            .filter_map(JsonValue::as_str)
            .filter_map(|target| target.strip_prefix("catalog="))
            .collect()
    };

    let target = value.as_str()?;
    let (entry, pointer) = split_catalog_entry_ref(target)?;
    for catalog in target_catalogs {
        if runtime
            .catalog_entries
            .get(catalog)
            .is_some_and(|entries| entries.contains_key(entry))
        {
            return hydrate_catalog_reference_target(
                runtime, catalog, entry, pointer, value, stack,
            );
        }
    }
    None
}

fn hydrate_catalog_reference_target(
    runtime: &RuntimePackage,
    catalog: &str,
    entry: &str,
    pointer: Option<&str>,
    fallback: &JsonValue,
    stack: &mut BTreeSet<(String, String)>,
) -> Option<JsonValue> {
    if stack.contains(&(catalog.to_owned(), entry.to_owned())) {
        return Some(fallback.clone());
    }
    let entry_value = runtime.catalog_entries.get(catalog)?.get(entry)?;
    let hydrated = hydrate_catalog_entry(runtime, catalog, entry, entry_value, stack);
    match pointer {
        Some(pointer) => hydrated
            .pointer(pointer)
            .cloned()
            .or_else(|| Some(fallback.clone())),
        None => Some(hydrated),
    }
}

pub(crate) fn split_catalog_entry_ref(value: &str) -> Option<(&str, Option<&str>)> {
    let Some((entry, pointer)) = value.split_once('#') else {
        return (!value.is_empty()).then_some((value, None));
    };
    if entry.is_empty() || !is_json_pointer(pointer) {
        return None;
    }
    Some((entry, (!pointer.is_empty()).then_some(pointer)))
}

fn is_json_pointer(value: &str) -> bool {
    value.is_empty() || value.starts_with('/')
}

/// One reference found in a catalog value: where it sits in the value (a
/// JSON pointer) and what it names.
pub(crate) struct CollectedReference {
    pub(crate) pointer: String,
    pub(crate) catalogs: Vec<String>,
    pub(crate) entry: String,
    pub(crate) entry_pointer: Option<String>,
}

/// Walks a value together with its catalog's schema and reports every field
/// whose schema carries `x-rototo-ref`, without splicing anything: the
/// reporting twin of hydration, for apps that follow references explicitly.
pub(crate) fn collect_catalog_references(
    runtime: &RuntimePackage,
    catalog: &str,
    value: &JsonValue,
) -> Vec<CollectedReference> {
    let mut found = Vec::new();
    if let Some(schema) = runtime.catalog_schemas.get(catalog) {
        collect_schema_references(runtime, catalog, schema, value, String::new(), &mut found);
    }
    found
}

fn collect_schema_references(
    runtime: &RuntimePackage,
    schema_catalog: &str,
    schema: &JsonValue,
    value: &JsonValue,
    pointer: String,
    found: &mut Vec<CollectedReference>,
) {
    if let Some(reference) = schema.get("$ref").and_then(JsonValue::as_str)
        && let Some((catalog, schema)) = resolve_schema_ref(runtime, schema_catalog, reference)
    {
        let catalog = catalog.clone();
        return collect_schema_references(runtime, &catalog, schema, value, pointer, found);
    }

    if let Some(ref_value) = schema.get("x-rototo-ref")
        && let Some(reference) = classify_reference(ref_value, value, &pointer)
    {
        found.push(reference);
        return;
    }

    if let (Some(properties), JsonValue::Object(object)) = (
        schema.get("properties").and_then(JsonValue::as_object),
        value,
    ) {
        for (key, subschema) in properties {
            let Some(child) = object.get(key) else {
                continue;
            };
            let escaped = key.replace('~', "~0").replace('/', "~1");
            collect_schema_references(
                runtime,
                schema_catalog,
                subschema,
                child,
                format!("{pointer}/{escaped}"),
                found,
            );
        }
    }

    if let (Some(items), JsonValue::Array(values)) = (schema.get("items"), value) {
        for (index, child) in values.iter().enumerate() {
            collect_schema_references(
                runtime,
                schema_catalog,
                items,
                child,
                format!("{pointer}/{index}"),
                found,
            );
        }
    }

    for keyword in ["allOf", "anyOf", "oneOf"] {
        let Some(schemas) = schema.get(keyword).and_then(JsonValue::as_array) else {
            continue;
        };
        for subschema in schemas {
            collect_schema_references(
                runtime,
                schema_catalog,
                subschema,
                value,
                pointer.clone(),
                found,
            );
        }
    }
}

fn classify_reference(
    ref_spec: &JsonValue,
    value: &JsonValue,
    pointer: &str,
) -> Option<CollectedReference> {
    if ref_spec == &JsonValue::Bool(true) {
        let object = value.as_object()?;
        return Some(CollectedReference {
            pointer: pointer.to_owned(),
            catalogs: vec![object.get("catalog")?.as_str()?.to_owned()],
            entry: object.get("entry")?.as_str()?.to_owned(),
            entry_pointer: object
                .get("pointer")
                .and_then(JsonValue::as_str)
                .map(str::to_owned),
        });
    }
    let catalogs: Vec<String> = if let Some(target) = ref_spec.as_str() {
        vec![target.strip_prefix("catalog=")?.to_owned()]
    } else {
        ref_spec
            .as_array()?
            .iter()
            .filter_map(JsonValue::as_str)
            .filter_map(|target| target.strip_prefix("catalog="))
            .map(str::to_owned)
            .collect()
    };
    if catalogs.is_empty() {
        return None;
    }
    let target = value.as_str()?;
    let (entry, entry_pointer) = split_catalog_entry_ref(target)?;
    Some(CollectedReference {
        pointer: pointer.to_owned(),
        catalogs,
        entry: entry.to_owned(),
        entry_pointer: entry_pointer.map(str::to_owned),
    })
}

fn resolve_schema_ref<'a>(
    runtime: &'a RuntimePackage,
    current_catalog: &str,
    reference: &str,
) -> Option<(String, &'a JsonValue)> {
    let (uri, pointer) = reference
        .split_once('#')
        .map_or((reference, ""), |(uri, pointer)| (uri, pointer));
    let catalog = if uri.is_empty() {
        current_catalog.to_owned()
    } else if let Some(catalog) = uri
        .strip_prefix("rototo://catalogs/")
        .and_then(|path| path.strip_suffix(".schema.json"))
    {
        catalog.to_owned()
    } else if let Some(catalog) = runtime
        .catalog_schemas
        .iter()
        .find_map(|(catalog, schema)| {
            (schema.get("$id").and_then(JsonValue::as_str) == Some(uri)).then(|| catalog.clone())
        })
    {
        catalog
    } else {
        // A relative file reference, resolved against the current catalog's
        // base URI exactly as the lint-time schema compiler does:
        // `email_template.schema.json` inside `policy.schema.json` means
        // rototo://catalogs/email_template.schema.json, and a namespaced
        // catalog resolves siblings inside its own namespace.
        resolve_relative_schema_uri(current_catalog, uri)?
    };
    let schema = runtime.catalog_schemas.get(&catalog)?;
    if pointer.is_empty() {
        return Some((catalog, schema));
    }
    let pointer = if pointer.starts_with('/') {
        pointer
    } else {
        return None;
    };
    schema.pointer(pointer).map(|schema| (catalog, schema))
}

/// Resolves a relative schema file reference against the current catalog's
/// base URI (`rototo://catalogs/<current>.schema.json`), mirroring the
/// compiler's RFC 3986 relative resolution. Returns the referenced catalog
/// id, or None when the reference climbs out of the catalogs root or does
/// not name a schema file.
fn resolve_relative_schema_uri(current_catalog: &str, uri: &str) -> Option<String> {
    let target = uri.strip_suffix(".schema.json")?;
    if target.is_empty() || uri.contains("://") || uri.starts_with('/') {
        return None;
    }
    // The base directory is the current catalog's namespace.
    let mut segments: Vec<&str> = match current_catalog.rsplit_once('/') {
        Some((namespace, _)) => namespace.split('/').collect(),
        None => Vec::new(),
    };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::resolve_relative_schema_uri;

    #[test]
    fn relative_schema_refs_resolve_against_the_catalog_base() {
        assert_eq!(
            resolve_relative_schema_uri("policy", "email_template.schema.json").as_deref(),
            Some("email_template")
        );
        // A namespaced catalog resolves siblings inside its namespace.
        assert_eq!(
            resolve_relative_schema_uri("acme/banner", "peer.schema.json").as_deref(),
            Some("acme/peer")
        );
        assert_eq!(
            resolve_relative_schema_uri("acme/banner", "../top.schema.json").as_deref(),
            Some("top")
        );
    }

    #[test]
    fn relative_schema_refs_never_escape_or_misparse() {
        // Climbing out of the catalogs root is not a reference.
        assert_eq!(
            resolve_relative_schema_uri("policy", "../out.schema.json"),
            None
        );
        // Absolute and non-schema references are left to the other arms.
        assert_eq!(
            resolve_relative_schema_uri("policy", "/abs.schema.json"),
            None
        );
        assert_eq!(
            resolve_relative_schema_uri("policy", "https://x/y.schema.json"),
            None
        );
        assert_eq!(resolve_relative_schema_uri("policy", "other.json"), None);
    }
}
