use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as JsonValue;

use crate::diagnostics::{LintDiagnostic, RototoRuleId, SemanticField};

use super::super::catalog_schema::{CATALOG_SCHEMA_URI_PREFIX, catalog_schema_uri};
use super::super::engine::LintContext;
use super::super::index::*;
use super::super::stages::{push_project_diagnostic, push_value_diagnostic};
use super::schema::lint_schema_ui_hints_for_target;

const CATALOG_REF_KEY: &str = "x-rototo-catalog-ref";

pub(super) fn lint_catalog_shapes(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for catalog in ctx.index.catalogs.values() {
        if let Some(message) = &catalog.invalid_message {
            push_project_diagnostic(
                &mut diagnostics,
                RototoRuleId::CatalogSchemaInvalid,
                catalog.field_target(SemanticField::SchemaJson),
                catalog.location.clone(),
                format!("catalog schema is invalid: {message}"),
            );
        }

        let Some(json) = catalog.json.as_ref() else {
            continue;
        };
        lint_schema_ui_hints_for_target(
            &mut diagnostics,
            catalog.field_target(SemanticField::SchemaJson),
            &catalog.location,
            json,
        );
        lint_catalog_ref_schema_shapes(&mut diagnostics, ctx, catalog, json);
    }
    ctx.diagnostics.extend(diagnostics);
}

pub(super) fn lint_catalog_entries(ctx: &mut LintContext) {
    let mut diagnostics = Vec::new();
    for catalog in ctx.index.catalogs.values() {
        let Some(validator) = &catalog.validator else {
            continue;
        };
        let Some(schema_json) = &catalog.json else {
            continue;
        };

        for entry in ctx
            .index
            .catalog_entries
            .get(&catalog.id)
            .into_iter()
            .flat_map(|entries| entries.values())
        {
            if let Err(err) = validator.validate(&entry.value) {
                push_value_diagnostic(
                    &mut diagnostics,
                    RototoRuleId::CatalogEntrySchemaMismatch,
                    entry.field_target(SemanticField::CatalogEntry),
                    entry.location.clone(),
                    format!("catalog value {} does not match schema: {err}", entry.key),
                );
            }

            let resolver = CatalogSchemaResolver::new(ctx);
            let mut ref_lint = CatalogReferenceLint {
                diagnostics: &mut diagnostics,
                ctx,
                resolver: &resolver,
                entry,
                visited_refs: BTreeSet::new(),
            };
            ref_lint.lint(
                schema_json,
                &catalog_schema_uri(&catalog.id),
                &entry.value,
                "$",
            );
        }
    }
    ctx.diagnostics.extend(diagnostics);
}

fn lint_catalog_ref_schema_shapes(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    catalog: &CatalogNode,
    schema: &JsonValue,
) {
    lint_catalog_ref_schema_node(diagnostics, ctx, catalog, schema, "$");
}

fn lint_catalog_ref_schema_node(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    catalog: &CatalogNode,
    schema: &JsonValue,
    pointer: &str,
) {
    if let Some(ref_value) = schema.get(CATALOG_REF_KEY) {
        match catalog_ref_spec(ref_value) {
            Ok(CatalogRefSpec::Compact(catalogs)) => {
                for target_catalog in catalogs {
                    if !ctx.index.catalogs.contains_key(&target_catalog) {
                        push_project_diagnostic(
                            diagnostics,
                            RototoRuleId::CatalogSchemaInvalid,
                            catalog.field_target(SemanticField::SchemaJson),
                            catalog.location.clone(),
                            format!(
                                "{CATALOG_REF_KEY} references unknown catalog {target_catalog} at {pointer}"
                            ),
                        );
                    }
                }
            }
            Ok(CatalogRefSpec::Object) => {}
            Err(message) => {
                push_project_diagnostic(
                    diagnostics,
                    RototoRuleId::CatalogSchemaInvalid,
                    catalog.field_target(SemanticField::SchemaJson),
                    catalog.location.clone(),
                    format!("{message} at {pointer}"),
                );
            }
        }
    }

    let Some(object) = schema.as_object() else {
        return;
    };

    for keyword in ["properties", "$defs", "definitions", "dependentSchemas"] {
        let Some(children) = object.get(keyword).and_then(JsonValue::as_object) else {
            continue;
        };
        for (key, child) in children {
            let child_pointer = format!(
                "{pointer}/{}/{}",
                escape_json_pointer_token(keyword),
                escape_json_pointer_token(key)
            );
            lint_catalog_ref_schema_node(diagnostics, ctx, catalog, child, &child_pointer);
        }
    }

    if let Some(items) = object.get("items") {
        lint_catalog_ref_schema_node(
            diagnostics,
            ctx,
            catalog,
            items,
            &format!("{pointer}/items"),
        );
    }

    for keyword in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        let Some(children) = object.get(keyword).and_then(JsonValue::as_array) else {
            continue;
        };
        for (index, child) in children.iter().enumerate() {
            lint_catalog_ref_schema_node(
                diagnostics,
                ctx,
                catalog,
                child,
                &format!("{pointer}/{keyword}/{index}"),
            );
        }
    }
}

struct CatalogReferenceLint<'a> {
    diagnostics: &'a mut Vec<LintDiagnostic>,
    ctx: &'a LintContext,
    resolver: &'a CatalogSchemaResolver<'a>,
    entry: &'a CatalogEntryNode,
    visited_refs: BTreeSet<(String, String, String)>,
}

impl CatalogReferenceLint<'_> {
    fn lint(&mut self, schema: &JsonValue, schema_base_uri: &str, value: &JsonValue, path: &str) {
        if let Some(reference) = schema.get("$ref").and_then(JsonValue::as_str) {
            let visit_key = (
                schema_base_uri.to_owned(),
                reference.to_owned(),
                path.to_owned(),
            );
            if self.visited_refs.insert(visit_key)
                && let Some(resolved) = self.resolver.resolve_ref(schema_base_uri, reference)
            {
                self.lint(resolved.schema, &resolved.base_uri, value, path);
            }
        }

        if let Some(ref_value) = schema.get(CATALOG_REF_KEY)
            && let Ok(spec) = catalog_ref_spec(ref_value)
        {
            match spec {
                CatalogRefSpec::Compact(catalogs) => {
                    if let Some(target) = value.as_str() {
                        lint_compact_catalog_ref(
                            self.diagnostics,
                            self.ctx,
                            self.entry,
                            &catalogs,
                            target,
                            path,
                        );
                    }
                }
                CatalogRefSpec::Object => {
                    lint_object_catalog_ref(self.diagnostics, self.ctx, self.entry, value, path);
                }
            }
        }

        if let (Some(properties), Some(object_value)) = (
            schema.get("properties").and_then(JsonValue::as_object),
            value.as_object(),
        ) {
            for (key, subschema) in properties {
                let Some(child) = object_value.get(key) else {
                    continue;
                };
                let child_path = format!("{path}.{key}");
                self.lint(subschema, schema_base_uri, child, &child_path);
            }
        }

        if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
            for (index, child) in array.iter().enumerate() {
                let child_path = format!("{path}[{index}]");
                self.lint(items, schema_base_uri, child, &child_path);
            }
        }

        if let (Some(prefix_items), Some(array)) = (
            schema.get("prefixItems").and_then(JsonValue::as_array),
            value.as_array(),
        ) {
            for (index, subschema) in prefix_items.iter().enumerate() {
                let Some(child) = array.get(index) else {
                    continue;
                };
                let child_path = format!("{path}[{index}]");
                self.lint(subschema, schema_base_uri, child, &child_path);
            }
        }

        for keyword in ["allOf", "anyOf", "oneOf"] {
            let Some(schemas) = schema.get(keyword).and_then(JsonValue::as_array) else {
                continue;
            };
            for subschema in applicable_subschemas(keyword, schemas, value) {
                self.lint(subschema, schema_base_uri, value, path);
            }
        }
    }
}

#[derive(Debug)]
enum CatalogRefSpec {
    Compact(Vec<String>),
    Object,
}

fn catalog_ref_spec(value: &JsonValue) -> Result<CatalogRefSpec, String> {
    if value == &JsonValue::Bool(true) {
        return Ok(CatalogRefSpec::Object);
    }

    if let Some(catalog) = value.as_str() {
        if catalog.is_empty() {
            return Err(format!("{CATALOG_REF_KEY} catalog id must not be empty"));
        }
        return Ok(CatalogRefSpec::Compact(vec![catalog.to_owned()]));
    }

    if let Some(catalogs) = value.as_array() {
        if catalogs.is_empty() {
            return Err(format!("{CATALOG_REF_KEY} catalog array must not be empty"));
        }
        let mut parsed = Vec::with_capacity(catalogs.len());
        for catalog in catalogs {
            let Some(catalog) = catalog.as_str() else {
                return Err(format!(
                    "{CATALOG_REF_KEY} catalog array must contain only strings"
                ));
            };
            if catalog.is_empty() {
                return Err(format!("{CATALOG_REF_KEY} catalog id must not be empty"));
            }
            parsed.push(catalog.to_owned());
        }
        return Ok(CatalogRefSpec::Compact(parsed));
    }

    Err(format!(
        "{CATALOG_REF_KEY} must be a catalog id string, catalog id array, or true"
    ))
}

fn lint_compact_catalog_ref(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    entry: &CatalogEntryNode,
    target_catalogs: &[String],
    target: &str,
    path: &str,
) {
    let target = match parse_compact_catalog_ref(target) {
        Ok(target) => target,
        Err(message) => {
            push_catalog_reference_diagnostic(diagnostics, entry, format!("{path} {message}"));
            return;
        }
    };

    let mut existing_catalogs = Vec::new();
    for target_catalog in target_catalogs {
        if !ctx.index.catalogs.contains_key(target_catalog) {
            push_catalog_reference_diagnostic(
                diagnostics,
                entry,
                format!("{path} references unknown catalog: {target_catalog}"),
            );
            continue;
        }
        if ctx
            .index
            .catalog_entries
            .get(target_catalog)
            .is_some_and(|entries| entries.contains_key(&target.entry))
        {
            existing_catalogs.push(target_catalog.clone());
        }
    }

    match existing_catalogs.as_slice() {
        [] => {
            let known_catalogs = target_catalogs
                .iter()
                .filter(|catalog| ctx.index.catalogs.contains_key(*catalog))
                .cloned()
                .collect::<Vec<_>>();
            if known_catalogs.len() == 1 {
                push_catalog_reference_diagnostic(
                    diagnostics,
                    entry,
                    format!(
                        "{path} references unknown {} entry: {}",
                        known_catalogs[0], target.entry
                    ),
                );
            } else if !known_catalogs.is_empty() {
                push_catalog_reference_diagnostic(
                    diagnostics,
                    entry,
                    format!(
                        "{path} references unknown catalog entry {}; checked catalogs: {}",
                        target.entry,
                        known_catalogs.join(", ")
                    ),
                );
            }
        }
        [target_catalog] => {
            lint_catalog_ref_pointer(
                diagnostics,
                ctx,
                entry,
                target_catalog,
                &target.entry,
                target.pointer.as_deref(),
                path,
            );
        }
        catalogs => {
            push_catalog_reference_diagnostic(
                diagnostics,
                entry,
                format!(
                    "{path} references ambiguous catalog entry {}; found in catalogs: {}",
                    target.entry,
                    catalogs.join(", ")
                ),
            );
        }
    }
}

fn lint_object_catalog_ref(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    entry: &CatalogEntryNode,
    value: &JsonValue,
    path: &str,
) {
    let Some(object) = value.as_object() else {
        return;
    };
    let Some(target_catalog) = object.get("catalog").and_then(JsonValue::as_str) else {
        return;
    };
    let Some(target_entry) = object.get("entry").and_then(JsonValue::as_str) else {
        return;
    };
    let Some(pointer) = object
        .get("pointer")
        .map(JsonValue::as_str)
        .unwrap_or(Some(""))
    else {
        return;
    };
    let pointer = (!pointer.is_empty()).then_some(pointer);

    if !ctx.index.catalogs.contains_key(target_catalog) {
        push_catalog_reference_diagnostic(
            diagnostics,
            entry,
            format!("{path} references unknown catalog: {target_catalog}"),
        );
        return;
    }

    if !ctx
        .index
        .catalog_entries
        .get(target_catalog)
        .is_some_and(|entries| entries.contains_key(target_entry))
    {
        push_catalog_reference_diagnostic(
            diagnostics,
            entry,
            format!("{path} references unknown {target_catalog} entry: {target_entry}"),
        );
        return;
    }

    if let Some(pointer) = pointer
        && !is_valid_json_pointer(pointer)
    {
        push_catalog_reference_diagnostic(
            diagnostics,
            entry,
            format!("{path} references invalid JSON Pointer: {pointer}"),
        );
        return;
    }

    lint_catalog_ref_pointer(
        diagnostics,
        ctx,
        entry,
        target_catalog,
        target_entry,
        pointer,
        path,
    );
}

fn lint_catalog_ref_pointer(
    diagnostics: &mut Vec<LintDiagnostic>,
    ctx: &LintContext,
    entry: &CatalogEntryNode,
    target_catalog: &str,
    target_entry: &str,
    pointer: Option<&str>,
    path: &str,
) {
    let Some(pointer) = pointer else {
        return;
    };
    let Some(target_value) = ctx
        .index
        .catalog_entries
        .get(target_catalog)
        .and_then(|entries| entries.get(target_entry))
        .map(|entry| &entry.value)
    else {
        return;
    };

    if target_value.pointer(pointer).is_none() {
        push_catalog_reference_diagnostic(
            diagnostics,
            entry,
            format!(
                "{path} references missing path {pointer} in {target_catalog} entry: {target_entry}"
            ),
        );
    }
}

fn push_catalog_reference_diagnostic(
    diagnostics: &mut Vec<LintDiagnostic>,
    entry: &CatalogEntryNode,
    message: String,
) {
    push_value_diagnostic(
        diagnostics,
        RototoRuleId::CatalogEntryUnknownReference,
        entry.field_target(SemanticField::CatalogEntry),
        entry.location.clone(),
        message,
    );
}

#[derive(Debug)]
struct CompactCatalogRef {
    entry: String,
    pointer: Option<String>,
}

fn parse_compact_catalog_ref(value: &str) -> Result<CompactCatalogRef, String> {
    let Some((entry, pointer)) = value.split_once('#') else {
        if value.is_empty() {
            return Err("references empty catalog entry".to_owned());
        }
        return Ok(CompactCatalogRef {
            entry: value.to_owned(),
            pointer: None,
        });
    };

    if entry.is_empty() {
        return Err("references empty catalog entry".to_owned());
    }
    if !is_valid_json_pointer(pointer) {
        return Err(format!("references invalid JSON Pointer: {pointer}"));
    }

    Ok(CompactCatalogRef {
        entry: entry.to_owned(),
        pointer: (!pointer.is_empty()).then(|| pointer.to_owned()),
    })
}

fn is_valid_json_pointer(pointer: &str) -> bool {
    if pointer.is_empty() {
        return true;
    }
    if !pointer.starts_with('/') {
        return false;
    }

    pointer.split('/').skip(1).all(|token| {
        let bytes = token.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'~' {
                if !matches!(bytes.get(index + 1), Some(b'0' | b'1')) {
                    return false;
                }
                index += 2;
            } else {
                index += 1;
            }
        }
        true
    })
}

fn applicable_subschemas<'a>(
    keyword: &str,
    schemas: &'a [JsonValue],
    value: &JsonValue,
) -> Vec<&'a JsonValue> {
    if keyword == "allOf" {
        return schemas.iter().collect();
    }

    let matching = schemas
        .iter()
        .filter(|schema| schema_obviously_matches(schema, value))
        .collect::<Vec<_>>();
    if matching.is_empty() {
        schemas.iter().collect()
    } else {
        matching
    }
}

fn schema_obviously_matches(schema: &JsonValue, value: &JsonValue) -> bool {
    let Some(object) = schema.as_object() else {
        return true;
    };

    if let Some(type_name) = object.get("type").and_then(JsonValue::as_str)
        && !json_type_matches(type_name, value)
    {
        return false;
    }

    if let Some(expected) = object.get("const")
        && expected != value
    {
        return false;
    }

    if let Some(enum_values) = object.get("enum").and_then(JsonValue::as_array)
        && !enum_values.iter().any(|candidate| candidate == value)
    {
        return false;
    }

    if let (Some(required), Some(value_object)) = (
        object.get("required").and_then(JsonValue::as_array),
        value.as_object(),
    ) {
        for field in required.iter().filter_map(JsonValue::as_str) {
            if !value_object.contains_key(field) {
                return false;
            }
        }
    }

    if let (Some(properties), Some(value_object)) = (
        object.get("properties").and_then(JsonValue::as_object),
        value.as_object(),
    ) {
        for (key, property_schema) in properties {
            let Some(child) = value_object.get(key) else {
                continue;
            };
            let Some(property_object) = property_schema.as_object() else {
                continue;
            };
            if let Some(expected) = property_object.get("const")
                && expected != child
            {
                return false;
            }
            if let Some(enum_values) = property_object.get("enum").and_then(JsonValue::as_array)
                && !enum_values.iter().any(|candidate| candidate == child)
            {
                return false;
            }
        }
    }

    true
}

fn json_type_matches(type_name: &str, value: &JsonValue) -> bool {
    match type_name {
        "array" => value.is_array(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "object" => value.is_object(),
        "string" => value.is_string(),
        _ => true,
    }
}

struct CatalogSchemaResolver<'a> {
    schemas: BTreeMap<String, &'a JsonValue>,
    aliases: BTreeMap<String, String>,
}

struct ResolvedSchema<'a> {
    schema: &'a JsonValue,
    base_uri: String,
}

impl<'a> CatalogSchemaResolver<'a> {
    fn new(ctx: &'a LintContext) -> Self {
        let mut schemas = BTreeMap::new();
        let mut aliases = BTreeMap::new();

        for catalog in ctx.index.catalogs.values() {
            let Some(json) = catalog.json.as_ref() else {
                continue;
            };
            let canonical = catalog_schema_uri(&catalog.id);
            schemas.insert(canonical.clone(), json);
            aliases.insert(canonical.clone(), canonical.clone());
            if let Some(id) = json
                .get("$id")
                .and_then(JsonValue::as_str)
                .map(normalize_schema_uri)
                .filter(|id| !id.is_empty())
            {
                aliases.insert(id, canonical);
            }
        }

        Self { schemas, aliases }
    }

    fn resolve_ref(&self, base_uri: &str, reference: &str) -> Option<ResolvedSchema<'a>> {
        let (uri, fragment) = split_ref(reference);
        let uri = if uri.is_empty() {
            base_uri.to_owned()
        } else {
            resolve_schema_uri(base_uri, uri)?
        };
        let canonical = self
            .aliases
            .get(&normalize_schema_uri(&uri))
            .unwrap_or(&uri);
        let root = *self.schemas.get(canonical)?;
        let schema = resolve_schema_fragment(root, fragment)?;
        Some(ResolvedSchema {
            schema,
            base_uri: canonical.clone(),
        })
    }
}

fn split_ref(reference: &str) -> (&str, &str) {
    reference.split_once('#').unwrap_or((reference, ""))
}

fn resolve_schema_uri(base_uri: &str, reference: &str) -> Option<String> {
    if reference.contains("://") {
        return Some(reference.to_owned());
    }
    if reference.starts_with('/') {
        return Some(format!("rototo://catalogs{reference}"));
    }

    let base_tail = base_uri.strip_prefix(CATALOG_SCHEMA_URI_PREFIX)?;
    let base_dir = base_tail
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_default();
    let mut segments = Vec::new();
    let combined = format!("{base_dir}{reference}");
    for segment in combined.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }

    let resolved = segments.join("/");
    Some(format!("{CATALOG_SCHEMA_URI_PREFIX}{resolved}"))
}

fn resolve_schema_fragment<'a>(schema: &'a JsonValue, fragment: &str) -> Option<&'a JsonValue> {
    if fragment.is_empty() {
        return Some(schema);
    }
    if fragment.starts_with('/') && is_valid_json_pointer(fragment) {
        return schema.pointer(fragment);
    }
    find_schema_anchor(schema, fragment)
}

fn find_schema_anchor<'a>(schema: &'a JsonValue, anchor: &str) -> Option<&'a JsonValue> {
    let object = schema.as_object()?;
    if object.get("$anchor").and_then(JsonValue::as_str) == Some(anchor) {
        return Some(schema);
    }

    for value in object.values() {
        if let Some(found) = match value {
            JsonValue::Array(items) => items
                .iter()
                .find_map(|item| find_schema_anchor(item, anchor)),
            JsonValue::Object(_) => find_schema_anchor(value, anchor),
            _ => None,
        } {
            return Some(found);
        }
    }
    None
}

fn normalize_schema_uri(uri: &str) -> String {
    uri.trim_end_matches('#').to_owned()
}

fn escape_json_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}
