use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};

use jsonschema::Validator;
use toml::Value;

use crate::diagnostics::{CustomRuleDefinition, CustomRuleId, Diagnostic, RototoRuleId};
use crate::error::{Result, RototoError};
use crate::model::{
    QualifierInspection, QualifierLint, VariableInspection, VariableLint, WorkspaceLint,
};
use crate::workspace::{
    VariableTomlReadErrorKind, inspect_workspace, read_variable_toml_detailed,
    workspace_environments,
};

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";

pub async fn lint_workspace(workspace_root: &Path) -> Result<WorkspaceLint> {
    let root = match tokio::fs::canonicalize(workspace_root).await {
        Ok(root) => root,
        Err(err) => {
            return Ok(WorkspaceLint {
                root: workspace_root.to_path_buf(),
                diagnostics: vec![Diagnostic::rototo(
                    RototoRuleId::WorkspaceNotFound,
                    workspace_root.display().to_string(),
                    err.to_string(),
                )],
            });
        }
    };

    let mut diagnostics = Vec::new();
    let Some(manifest) = read_toml_diagnostic(
        &root.join(WORKSPACE_MANIFEST),
        RototoRuleId::WorkspaceManifestMissing,
        RototoRuleId::WorkspaceManifestParseFailed,
        &mut diagnostics,
    )
    .await
    else {
        return Ok(WorkspaceLint { root, diagnostics });
    };

    let environments = match workspace_environments(&manifest) {
        Ok(environments) => environments,
        Err(err) => {
            diagnostics.push(Diagnostic::rototo(
                RototoRuleId::WorkspaceManifestSchemaFailed,
                WORKSPACE_MANIFEST,
                err.to_string(),
            ));
            return Ok(WorkspaceLint { root, diagnostics });
        }
    };

    let inspection = match inspect_workspace(&root).await {
        Ok(inspection) => inspection,
        Err(err) => {
            diagnostics.push(Diagnostic::rototo(
                RototoRuleId::WorkspaceManifestSchemaFailed,
                WORKSPACE_MANIFEST,
                err.to_string(),
            ));
            return Ok(WorkspaceLint { root, diagnostics });
        }
    };

    let qualifier_ids: HashSet<&str> = inspection
        .qualifiers
        .iter()
        .map(|qualifier| qualifier.id.as_str())
        .collect();

    for qualifier in &inspection.qualifiers {
        lint_qualifier_file(&root, qualifier, &qualifier_ids, &mut diagnostics).await;
    }
    let mut custom_rules = BTreeMap::new();
    for variable in &inspection.variables {
        lint_variable_file(
            &root,
            variable,
            &environments,
            &qualifier_ids,
            &mut custom_rules,
            &mut diagnostics,
        )
        .await;
    }

    if let Some(context_schema) = context_schema_validator(&root, &manifest, &mut diagnostics).await
    {
        lint_context_schema_references(
            &root,
            &inspection.qualifiers,
            &context_schema,
            &mut diagnostics,
        )
        .await;
    }

    lint_schemas(&root, &mut diagnostics).await;
    sort_diagnostics(&mut diagnostics);

    Ok(WorkspaceLint { root, diagnostics })
}

pub async fn lint_qualifier(workspace_root: &Path, id: &str) -> Result<QualifierLint> {
    let root = tokio::fs::canonicalize(workspace_root)
        .await
        .map_err(|err| RototoError::new(format!("workspace not found: {err}")))?;
    let lint = lint_workspace(&root).await?;
    let inspection = inspect_workspace(&root).await?;
    let qualifier = crate::workspace::qualifier_for_id(&inspection, id)?;

    Ok(QualifierLint {
        root,
        id: id.to_owned(),
        diagnostics: lint
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.path == qualifier.path.display().to_string())
            .collect(),
    })
}

pub async fn lint_variable(workspace_root: &Path, id: &str) -> Result<VariableLint> {
    let root = tokio::fs::canonicalize(workspace_root)
        .await
        .map_err(|err| RototoError::new(format!("workspace not found: {err}")))?;
    let lint = lint_workspace(&root).await?;
    let inspection = inspect_workspace(&root).await?;
    let variable = crate::workspace::variable_for_id(&inspection, id)?;

    Ok(VariableLint {
        root,
        id: id.to_owned(),
        diagnostics: lint
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.path == variable.path.display().to_string())
            .collect(),
    })
}

async fn lint_qualifier_file(
    root: &Path,
    qualifier: &QualifierInspection,
    qualifier_ids: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = root.join(&qualifier.path);
    let Some(toml) = read_toml_diagnostic(
        &path,
        RototoRuleId::QualifierParseFailed,
        RototoRuleId::QualifierParseFailed,
        diagnostics,
    )
    .await
    else {
        return;
    };

    validate_qualifier_toml(qualifier, &toml, qualifier_ids, diagnostics);
}

fn validate_qualifier_toml(
    qualifier: &QualifierInspection,
    toml: &Value,
    qualifier_ids: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if toml.get("schema_version").and_then(Value::as_integer) != Some(1) {
        push_qualifier_error(
            qualifier,
            RototoRuleId::QualifierSchemaVersion,
            "qualifier must declare schema_version = 1",
            diagnostics,
        );
    }
    if toml.get("qualifier").and_then(Value::as_table).is_none() {
        push_qualifier_error(
            qualifier,
            RototoRuleId::QualifierMissingTable,
            "qualifier must contain a [qualifier] table",
            diagnostics,
        );
    }

    if let Some(predicates) = toml
        .get("qualifier")
        .and_then(|qualifier| qualifier.get("predicate"))
        .and_then(Value::as_array)
    {
        for predicate in predicates {
            lint_predicate(qualifier, predicate, qualifier_ids, diagnostics);
        }
    } else {
        push_qualifier_error(
            qualifier,
            RototoRuleId::QualifierPredicateMissing,
            "qualifier must contain at least one [[qualifier.predicate]]",
            diagnostics,
        );
    }
}

async fn lint_variable_file(
    root: &Path,
    variable: &VariableInspection,
    environments: &[String],
    qualifier_ids: &HashSet<&str>,
    custom_rules: &mut BTreeMap<CustomRuleId, CustomRuleDefinition>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = root.join(&variable.path);
    let Some(_) = read_toml_diagnostic(
        &path,
        RototoRuleId::VariableParseFailed,
        RototoRuleId::VariableParseFailed,
        diagnostics,
    )
    .await
    else {
        return;
    };
    let toml = match read_variable_toml_detailed(root, variable).await {
        Ok(toml) => toml,
        Err(err) => {
            push_variable_error(
                variable,
                variable_toml_error_rule(err.kind),
                format!("variable values could not be loaded: {err}"),
                diagnostics,
            );
            return;
        }
    };

    let custom_rule_definitions = validate_variable_toml(
        root,
        variable,
        &toml,
        environments,
        qualifier_ids,
        diagnostics,
    )
    .await;
    record_custom_rule_definitions(
        variable,
        &custom_rule_definitions,
        custom_rules,
        diagnostics,
    );

    match crate::lua_lint::lint_variable(
        root,
        variable,
        &toml,
        environments,
        &custom_rule_definitions,
    )
    .await
    {
        Ok(custom_diagnostics) => diagnostics.extend(custom_diagnostics),
        Err(err) => diagnostics.push(Diagnostic::rototo(
            RototoRuleId::CustomLintFailed,
            variable.path.display().to_string(),
            err.to_string(),
        )),
    }
}

async fn validate_variable_toml(
    root: &Path,
    variable_inspection: &VariableInspection,
    toml: &Value,
    environments: &[String],
    qualifier_ids: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<CustomRuleDefinition> {
    let mut custom_rule_definitions = Vec::new();

    if toml.get("schema_version").and_then(Value::as_integer) != Some(1) {
        push_variable_error(
            variable_inspection,
            RototoRuleId::VariableSchemaVersion,
            "variable must declare schema_version = 1",
            diagnostics,
        );
    }

    let Some(variable) = toml.get("variable").and_then(Value::as_table) else {
        push_variable_error(
            variable_inspection,
            RototoRuleId::VariableMissingTable,
            "variable must contain a [variable] table",
            diagnostics,
        );
        return custom_rule_definitions;
    };

    let has_type = variable.get("type").and_then(Value::as_str).is_some();
    let schema = variable.get("schema").and_then(Value::as_str);
    if has_type == schema.is_some() {
        push_variable_error(
            variable_inspection,
            RototoRuleId::VariableTypeOrSchema,
            "variable must declare exactly one of type or schema",
            diagnostics,
        );
    }
    if let Some(lint) = variable.get("lint") {
        match lint.as_table() {
            Some(lint) if lint.get("path").and_then(Value::as_str).is_some() => {
                custom_rule_definitions =
                    lint_custom_rule_definitions(variable_inspection, lint, diagnostics);
            }
            Some(_) => push_variable_error(
                variable_inspection,
                RototoRuleId::VariableLintShape,
                "variable lint must contain path",
                diagnostics,
            ),
            None => push_variable_error(
                variable_inspection,
                RototoRuleId::VariableLintShape,
                "variable lint must be a table",
                diagnostics,
            ),
        }
    }

    let Some(values) = variable.get("values").and_then(Value::as_table) else {
        push_variable_error(
            variable_inspection,
            RototoRuleId::VariableValuesMissing,
            "variable must contain [variable.values]",
            diagnostics,
        );
        return custom_rule_definitions;
    };

    match schema {
        Some(schema) => {
            if let Some(validator) =
                schema_validator(root, variable_inspection, schema, diagnostics).await
            {
                lint_schema_values(variable_inspection, values, &validator, diagnostics);
            }
        }
        None => {
            if let Some(type_name) = variable.get("type").and_then(Value::as_str) {
                lint_typed_values(variable_inspection, values, type_name, diagnostics);
            }
        }
    }

    let Some(env) = variable.get("env").and_then(Value::as_table) else {
        push_variable_error(
            variable_inspection,
            RototoRuleId::VariableEnvMissingDefault,
            "variable must contain [variable.env._]",
            diagnostics,
        );
        return custom_rule_definitions;
    };
    if !env.contains_key("_") {
        push_variable_error(
            variable_inspection,
            RototoRuleId::VariableEnvMissingDefault,
            "variable must contain [variable.env._]",
            diagnostics,
        );
    }

    for (environment, block) in env {
        if environment != "_" && !environments.iter().any(|known| known == environment) {
            push_variable_error(
                variable_inspection,
                RototoRuleId::VariableUnknownEnvironment,
                format!("variable references undeclared environment: {environment}"),
                diagnostics,
            );
        }
        lint_environment_block(
            variable_inspection,
            block,
            values,
            qualifier_ids,
            diagnostics,
        );
    }

    custom_rule_definitions
}

fn lint_custom_rule_definitions(
    variable: &VariableInspection,
    lint: &toml::map::Map<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<CustomRuleDefinition> {
    let Some(rules) = lint.get("rule") else {
        return Vec::new();
    };
    let Some(rules) = rules.as_array() else {
        push_variable_error(
            variable,
            RototoRuleId::VariableLintShape,
            "variable lint rules must use [[variable.lint.rule]] tables",
            diagnostics,
        );
        return Vec::new();
    };

    let mut definitions = Vec::new();
    for rule in rules {
        let Some(rule) = rule.as_table() else {
            push_variable_error(
                variable,
                RototoRuleId::VariableLintShape,
                "variable lint rule must be a table",
                diagnostics,
            );
            continue;
        };
        let Some(id) = rule.get("id").and_then(Value::as_str) else {
            push_variable_error(
                variable,
                RototoRuleId::VariableLintShape,
                "variable lint rule must contain id",
                diagnostics,
            );
            continue;
        };
        let Some(title) = rule.get("title").and_then(Value::as_str) else {
            push_variable_error(
                variable,
                RototoRuleId::VariableLintShape,
                "variable lint rule must contain title",
                diagnostics,
            );
            continue;
        };
        let Some(help) = rule.get("help").and_then(Value::as_str) else {
            push_variable_error(
                variable,
                RototoRuleId::VariableLintShape,
                "variable lint rule must contain help",
                diagnostics,
            );
            continue;
        };

        match CustomRuleId::parse(id) {
            Ok(rule) => definitions.push(CustomRuleDefinition::new(rule, title, help)),
            Err(err) => push_variable_error(
                variable,
                RototoRuleId::CustomLintInvalidRule,
                format!("custom lint rule id is invalid: {id}: {err}"),
                diagnostics,
            ),
        }
    }
    definitions
}

fn record_custom_rule_definitions(
    variable: &VariableInspection,
    definitions: &[CustomRuleDefinition],
    custom_rules: &mut BTreeMap<CustomRuleId, CustomRuleDefinition>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for definition in definitions {
        match custom_rules.get(&definition.rule) {
            Some(existing) if existing.same_metadata(definition) => {}
            Some(_) => push_variable_error(
                variable,
                RototoRuleId::CustomLintRuleConflict,
                format!("custom lint rule metadata conflicts: {}", definition.rule),
                diagnostics,
            ),
            None => {
                custom_rules.insert(definition.rule.clone(), definition.clone());
            }
        }
    }
}

fn variable_toml_error_rule(kind: VariableTomlReadErrorKind) -> RototoRuleId {
    match kind {
        VariableTomlReadErrorKind::Read | VariableTomlReadErrorKind::Parse => {
            RototoRuleId::VariableParseFailed
        }
        VariableTomlReadErrorKind::ExternalValuesLoad => {
            RototoRuleId::VariableExternalValuesLoadFailed
        }
        VariableTomlReadErrorKind::ExternalValueParse => {
            RototoRuleId::VariableExternalValueParseFailed
        }
        VariableTomlReadErrorKind::ExternalValueDuplicate => {
            RototoRuleId::VariableExternalValueDuplicate
        }
    }
}

fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|left, right| {
        (
            left.path.as_str(),
            left.rule.as_string(),
            left.message.as_str(),
        )
            .cmp(&(
                right.path.as_str(),
                right.rule.as_string(),
                right.message.as_str(),
            ))
    });
}

fn lint_environment_block(
    variable: &VariableInspection,
    block: &Value,
    values: &toml::map::Map<String, Value>,
    qualifier_ids: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(table) = block.as_table() else {
        push_variable_error(
            variable,
            RototoRuleId::VariableEnvShape,
            "environment block must be a table",
            diagnostics,
        );
        return;
    };

    let Some(value) = table.get("value").and_then(Value::as_str) else {
        push_variable_error(
            variable,
            RototoRuleId::VariableEnvShape,
            "environment block must reference a value",
            diagnostics,
        );
        return;
    };
    if !values.contains_key(value) {
        push_variable_error(
            variable,
            RototoRuleId::VariableUnknownValue,
            format!("environment references unknown value: {value}"),
            diagnostics,
        );
    }

    if let Some(rules) = table.get("rule").and_then(Value::as_array) {
        for rule in rules {
            let Some(rule) = rule.as_table() else {
                push_variable_error(
                    variable,
                    RototoRuleId::VariableRuleShape,
                    "rule must be a table",
                    diagnostics,
                );
                continue;
            };

            match rule.get("qualifier").and_then(Value::as_str) {
                Some(qualifier) if qualifier_ids.contains(qualifier) => {}
                Some(qualifier) => push_variable_error(
                    variable,
                    RototoRuleId::VariableRuleUnknownQualifier,
                    format!("rule references unknown qualifier: {qualifier}"),
                    diagnostics,
                ),
                None => push_variable_error(
                    variable,
                    RototoRuleId::VariableRuleShape,
                    "rule must reference a qualifier",
                    diagnostics,
                ),
            }

            match rule.get("value").and_then(Value::as_str) {
                Some(value) if values.contains_key(value) => {}
                Some(value) => push_variable_error(
                    variable,
                    RototoRuleId::VariableUnknownValue,
                    format!("rule references unknown value: {value}"),
                    diagnostics,
                ),
                None => push_variable_error(
                    variable,
                    RototoRuleId::VariableRuleShape,
                    "rule must reference a value",
                    diagnostics,
                ),
            }
        }
    }
}

fn lint_predicate(
    qualifier: &QualifierInspection,
    predicate: &Value,
    qualifier_ids: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(predicate) = predicate.as_table() else {
        push_qualifier_error(
            qualifier,
            RototoRuleId::QualifierPredicateShape,
            "predicate must be a table",
            diagnostics,
        );
        return;
    };

    let Some(attribute) = predicate.get("attribute").and_then(Value::as_str) else {
        push_qualifier_error(
            qualifier,
            RototoRuleId::QualifierPredicateShape,
            "predicate must contain attribute",
            diagnostics,
        );
        return;
    };
    let Some(op) = predicate.get("op").and_then(Value::as_str) else {
        push_qualifier_error(
            qualifier,
            RototoRuleId::QualifierPredicateShape,
            "predicate must contain op",
            diagnostics,
        );
        return;
    };
    if !matches!(
        op,
        "eq" | "neq" | "in" | "not_in" | "gt" | "gte" | "lt" | "lte" | "bucket"
    ) {
        push_qualifier_error(
            qualifier,
            RototoRuleId::QualifierPredicateUnknownOp,
            format!("predicate has unknown op: {op}"),
            diagnostics,
        );
    }

    if let Some(referenced_qualifier) = attribute.strip_prefix("qualifier.")
        && !qualifier_ids.contains(referenced_qualifier)
    {
        push_qualifier_error(
            qualifier,
            RototoRuleId::QualifierPredicateUnknownQualifier,
            format!("predicate references unknown qualifier: {referenced_qualifier}"),
            diagnostics,
        );
    }

    if op == "bucket" {
        if predicate.get("salt").and_then(Value::as_str).is_none() {
            push_qualifier_error(
                qualifier,
                RototoRuleId::QualifierPredicateBucket,
                "bucket predicate must contain salt",
                diagnostics,
            );
        }
        let Some(range) = predicate.get("range").and_then(Value::as_array) else {
            push_qualifier_error(
                qualifier,
                RototoRuleId::QualifierPredicateBucket,
                "bucket predicate must contain range",
                diagnostics,
            );
            return;
        };
        if range.len() != 2 {
            push_qualifier_error(
                qualifier,
                RototoRuleId::QualifierPredicateBucket,
                "bucket range must contain two integers",
                diagnostics,
            );
            return;
        }
        let start = range[0].as_integer();
        let end = range[1].as_integer();
        match (start, end) {
            (Some(start), Some(end)) if 0 <= start && start < end && end <= 10_000 => {}
            _ => push_qualifier_error(
                qualifier,
                RototoRuleId::QualifierPredicateBucket,
                "bucket range must satisfy 0 <= start < end <= 10000",
                diagnostics,
            ),
        }
        if predicate.contains_key("value") {
            push_qualifier_error(
                qualifier,
                RototoRuleId::QualifierPredicateBucket,
                "bucket predicate must not contain value",
                diagnostics,
            );
        }
    } else {
        let Some(value) = predicate.get("value") else {
            push_qualifier_error(
                qualifier,
                RototoRuleId::QualifierPredicateValue,
                "predicate must contain value",
                diagnostics,
            );
            return;
        };
        match op {
            "in" | "not_in" if !value.is_array() => {
                push_qualifier_error(
                    qualifier,
                    RototoRuleId::QualifierPredicateValue,
                    format!("{op} predicate value must be a list"),
                    diagnostics,
                );
            }
            "gt" | "gte" | "lt" | "lte" if !value_is_number(value) => {
                push_qualifier_error(
                    qualifier,
                    RototoRuleId::QualifierPredicateValue,
                    format!("{op} predicate value must be a number"),
                    diagnostics,
                );
            }
            _ => {}
        }
    }
}

async fn context_schema_validator(
    root: &Path,
    manifest: &Value,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<serde_json::Value> {
    let context = manifest.get("context")?;
    let Some(context) = context.as_table() else {
        diagnostics.push(Diagnostic::rototo(
            RototoRuleId::WorkspaceContextSchemaRef,
            WORKSPACE_MANIFEST,
            "[context] must be a table",
        ));
        return None;
    };
    let Some(schema_ref) = context.get("schema").and_then(Value::as_str) else {
        diagnostics.push(Diagnostic::rototo(
            RototoRuleId::WorkspaceContextSchemaRef,
            WORKSPACE_MANIFEST,
            "[context] must declare schema",
        ));
        return None;
    };
    if !context_schema_ref_is_safe(schema_ref) {
        diagnostics.push(Diagnostic::rototo(
            RototoRuleId::WorkspaceContextSchemaRef,
            WORKSPACE_MANIFEST,
            "context schema path must be a relative path inside the workspace",
        ));
        return None;
    }

    let schema_path = root.join(schema_ref);
    let text = match tokio::fs::read_to_string(&schema_path).await {
        Ok(text) => text,
        Err(err) => {
            diagnostics.push(Diagnostic::rototo(
                RototoRuleId::WorkspaceContextSchemaRef,
                schema_ref,
                format!("context schema could not be read: {err}"),
            ));
            return None;
        }
    };
    let schema = match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(schema) => schema,
        Err(err) => {
            diagnostics.push(Diagnostic::rototo(
                RototoRuleId::WorkspaceContextSchemaRef,
                schema_ref,
                format!("context schema could not be parsed: {err}"),
            ));
            return None;
        }
    };
    if let Err(err) = jsonschema::validator_for(&schema) {
        diagnostics.push(Diagnostic::rototo(
            RototoRuleId::WorkspaceContextSchemaRef,
            schema_ref,
            format!("context schema is invalid: {err}"),
        ));
        return None;
    }
    Some(schema)
}

async fn lint_context_schema_references(
    root: &Path,
    qualifiers: &[QualifierInspection],
    schema: &serde_json::Value,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for qualifier in qualifiers {
        let path = root.join(&qualifier.path);
        let Ok(toml) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        let Ok(toml) = toml.parse::<Value>() else {
            continue;
        };
        let Some(predicates) = toml
            .get("qualifier")
            .and_then(|qualifier| qualifier.get("predicate"))
            .and_then(Value::as_array)
        else {
            continue;
        };

        for predicate in predicates {
            let Some(attribute) = predicate
                .as_table()
                .and_then(|predicate| predicate.get("attribute"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if attribute.starts_with("qualifier.") || schema_declares_path(schema, attribute) {
                continue;
            }
            diagnostics.push(Diagnostic::rototo(
                RototoRuleId::WorkspaceContextSchemaAttribute,
                qualifier.path.display().to_string(),
                format!("context schema does not declare attribute: {attribute}"),
            ));
        }
    }
}

fn schema_declares_path(schema: &serde_json::Value, path: &str) -> bool {
    let mut current = schema;
    for segment in path.split('.') {
        if accepts_any_object_property(current) {
            return true;
        }
        let Some(properties) = current
            .get("properties")
            .and_then(serde_json::Value::as_object)
        else {
            return false;
        };
        let Some(next) = properties.get(segment) else {
            return false;
        };
        current = next;
    }
    true
}

fn accepts_any_object_property(schema: &serde_json::Value) -> bool {
    schema.get("type").and_then(serde_json::Value::as_str) == Some("object")
        && schema.get("properties").is_none()
        && schema.get("additionalProperties") != Some(&serde_json::Value::Bool(false))
}

fn context_schema_ref_is_safe(schema_ref: &str) -> bool {
    let schema_ref = Path::new(schema_ref);
    !schema_ref.as_os_str().is_empty()
        && !schema_ref.is_absolute()
        && schema_ref
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}

fn lint_typed_values(
    variable: &VariableInspection,
    values: &toml::map::Map<String, Value>,
    type_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !matches!(type_name, "bool" | "int" | "number" | "string" | "list") {
        push_variable_error(
            variable,
            RototoRuleId::VariableUnknownType,
            format!("variable declares unknown type: {type_name}"),
            diagnostics,
        );
        return;
    }

    for (name, value) in values {
        let matches_type = match type_name {
            "bool" => value.as_bool().is_some(),
            "int" => value.as_integer().is_some(),
            "number" => value_is_number(value),
            "string" => value.as_str().is_some(),
            "list" => value.as_array().is_some(),
            _ => unreachable!("unknown type checked above"),
        };
        if !matches_type {
            push_variable_error(
                variable,
                RototoRuleId::VariableValueTypeMismatch,
                format!("value {name} does not match type {type_name}"),
                diagnostics,
            );
        }
    }
}

fn lint_schema_values(
    variable: &VariableInspection,
    values: &toml::map::Map<String, Value>,
    validator: &Validator,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (name, value) in values {
        let Ok(json) = serde_json::to_value(value) else {
            push_variable_error(
                variable,
                RototoRuleId::VariableValueSchemaMismatch,
                format!("value {name} could not be converted to JSON"),
                diagnostics,
            );
            continue;
        };

        if let Err(error) = validator.validate(&json) {
            push_variable_error(
                variable,
                RototoRuleId::VariableValueSchemaMismatch,
                format!("value {name} does not match schema: {error}"),
                diagnostics,
            );
        }
    }
}

async fn schema_validator(
    root: &Path,
    variable: &VariableInspection,
    schema_ref: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Validator> {
    let schema_path = root
        .join(&variable.path)
        .parent()
        .unwrap_or(root)
        .join(schema_ref);
    match tokio::fs::read_to_string(&schema_path).await {
        Ok(text) => {
            let schema: serde_json::Value = match serde_json::from_str(&text) {
                Ok(schema) => schema,
                Err(err) => {
                    push_variable_error(
                        variable,
                        RototoRuleId::VariableSchemaRef,
                        format!("schema could not be parsed: {err}"),
                        diagnostics,
                    );
                    return None;
                }
            };
            match jsonschema::validator_for(&schema) {
                Ok(validator) => Some(validator),
                Err(err) => {
                    push_variable_error(
                        variable,
                        RototoRuleId::VariableSchemaRef,
                        format!("schema is invalid: {err}"),
                        diagnostics,
                    );
                    None
                }
            }
        }
        Err(err) => {
            push_variable_error(
                variable,
                RototoRuleId::VariableSchemaRef,
                format!("schema could not be read: {err}"),
                diagnostics,
            );
            None
        }
    }
}

fn value_is_number(value: &Value) -> bool {
    value.as_integer().is_some() || value.as_float().is_some()
}

async fn lint_schemas(root: &Path, diagnostics: &mut Vec<Diagnostic>) {
    let schemas_dir = root.join("schemas");
    let Ok(mut entries) = tokio::fs::read_dir(&schemas_dir).await else {
        return;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let entry = entry.path();
        if entry.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let text = match tokio::fs::read_to_string(&entry).await {
            Ok(text) => text,
            Err(err) => {
                diagnostics.push(Diagnostic::rototo(
                    RototoRuleId::SchemaParseFailed,
                    display_relative(root, &entry),
                    err.to_string(),
                ));
                continue;
            }
        };
        let schema = match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(schema) => schema,
            Err(err) => {
                diagnostics.push(Diagnostic::rototo(
                    RototoRuleId::SchemaParseFailed,
                    display_relative(root, &entry),
                    err.to_string(),
                ));
                continue;
            }
        };
        if let Err(err) = jsonschema::validator_for(&schema) {
            diagnostics.push(Diagnostic::rototo(
                RototoRuleId::SchemaInvalid,
                display_relative(root, &entry),
                err.to_string(),
            ));
        }
    }
}

async fn read_toml_diagnostic(
    path: &Path,
    missing_rule: RototoRuleId,
    parse_rule: RototoRuleId,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Value> {
    let text = match tokio::fs::read_to_string(path).await {
        Ok(text) => text,
        Err(err) => {
            diagnostics.push(Diagnostic::rototo(
                missing_rule,
                display_path(path),
                err.to_string(),
            ));
            return None;
        }
    };
    match text.parse::<Value>() {
        Ok(value) => Some(value),
        Err(err) => {
            diagnostics.push(Diagnostic::rototo(
                parse_rule,
                display_path(path),
                err.to_string(),
            ));
            None
        }
    }
}

fn push_qualifier_error(
    qualifier: &QualifierInspection,
    rule: RototoRuleId,
    message: impl Into<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    push_workspace_file_error(&qualifier.path, rule, message, diagnostics);
}

fn push_variable_error(
    variable: &VariableInspection,
    rule: RototoRuleId,
    message: impl Into<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    push_workspace_file_error(&variable.path, rule, message, diagnostics);
}

fn push_workspace_file_error(
    path: &Path,
    rule: RototoRuleId,
    message: impl Into<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(Diagnostic::rototo(
        rule,
        path.display().to_string(),
        message,
    ));
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
