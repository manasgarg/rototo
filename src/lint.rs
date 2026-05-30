use std::collections::HashSet;
use std::path::{Component, Path};

use jsonschema::Validator;
use toml::Value;

use crate::diagnostics::{
    Diagnostic, DiagnosticSource, JSON_SCHEMA_FILE_INVALID, JSON_SCHEMA_FILE_PARSE_FAILED,
    LintRule, VARIABLE_CUSTOM_LINT_FAILED, WORKSPACE_CONTEXT_SCHEMA_FAILED,
    WORKSPACE_MANIFEST_MISSING, WORKSPACE_MANIFEST_PARSE_FAILED, WORKSPACE_MANIFEST_SCHEMA_FAILED,
    WORKSPACE_NOT_FOUND, WORKSPACE_TOML_FILE_INVALID, WORKSPACE_TOML_FILE_PARSE_FAILED,
};
use crate::error::{Result, RototoError};
use crate::model::{
    QualifierInspection, QualifierLint, VariableInspection, VariableLint, WorkspaceLint,
};
use crate::workspace::{inspect_workspace, read_variable_toml, workspace_environments};

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";

const RULE_WORKSPACE_NOT_FOUND: LintRule = LintRule {
    id: "rototo/workspace/not-found",
    title: "Workspace was not found",
    help: "Pass a path to an existing rototo workspace directory.",
};
const RULE_WORKSPACE_MANIFEST_MISSING: LintRule = LintRule {
    id: "rototo/workspace/manifest/missing",
    title: "Workspace manifest is missing",
    help: "Create rototo-workspace.toml at the workspace root.",
};
const RULE_WORKSPACE_MANIFEST_PARSE_FAILED: LintRule = LintRule {
    id: "rototo/workspace/manifest/parse-failed",
    title: "Workspace manifest could not be parsed",
    help: "Fix the TOML syntax in rototo-workspace.toml.",
};
const RULE_WORKSPACE_MANIFEST_SCHEMA_FAILED: LintRule = LintRule {
    id: "rototo/workspace/manifest/schema-failed",
    title: "Workspace manifest does not match schema",
    help: "Declare schema_version = 1 and [environments].values in rototo-workspace.toml.",
};
const RULE_WORKSPACE_TOML_FILE_PARSE_FAILED: LintRule = LintRule {
    id: "rototo/workspace-file/toml-parse-failed",
    title: "Workspace TOML file could not be parsed",
    help: "Fix the TOML syntax so rototo can parse the workspace file.",
};
const RULE_JSON_SCHEMA_FILE_PARSE_FAILED: LintRule = LintRule {
    id: "rototo/json-schema-file/parse-failed",
    title: "JSON Schema file could not be parsed",
    help: "Fix the JSON syntax so rototo can parse the schema file.",
};
const RULE_SCHEMA_INVALID: LintRule = LintRule {
    id: "rototo/schema/invalid",
    title: "JSON Schema is invalid",
    help: "Update the schema file so it is valid JSON Schema.",
};
const RULE_CONTEXT_SCHEMA_REF: LintRule = LintRule {
    id: "rototo/workspace/context-schema/ref",
    title: "Resolve context schema reference is invalid",
    help: "Point [context].schema to a readable valid JSON Schema file.",
};
const RULE_CONTEXT_SCHEMA_ATTRIBUTE: LintRule = LintRule {
    id: "rototo/workspace/context-schema/attribute",
    title: "Qualifier context attribute is not declared by the resolve context schema",
    help: "Declare the context path in the workspace context schema or update the qualifier.",
};
const RULE_QUALIFIER_SCHEMA_VERSION: LintRule = LintRule {
    id: "rototo/qualifier/schema-version",
    title: "Qualifier schema version is missing or unsupported",
    help: "Declare schema_version = 1 in the qualifier file.",
};
const RULE_QUALIFIER_MISSING_TABLE: LintRule = LintRule {
    id: "rototo/qualifier/missing-table",
    title: "Qualifier table is missing",
    help: "Add a [qualifier] table.",
};
const RULE_QUALIFIER_MISSING_PREDICATE: LintRule = LintRule {
    id: "rototo/qualifier/predicate/missing",
    title: "Qualifier predicate is missing",
    help: "Add at least one [[qualifier.predicate]] table.",
};
const RULE_QUALIFIER_PREDICATE_SHAPE: LintRule = LintRule {
    id: "rototo/qualifier/predicate/shape",
    title: "Qualifier predicate has the wrong shape",
    help: "Use [[qualifier.predicate]] tables with attribute, op, and value fields.",
};
const RULE_QUALIFIER_PREDICATE_UNKNOWN_OP: LintRule = LintRule {
    id: "rototo/qualifier/predicate/unknown-op",
    title: "Qualifier predicate uses an unknown operator",
    help: "Use one of eq, neq, in, not_in, gt, gte, lt, lte, or bucket.",
};
const RULE_QUALIFIER_PREDICATE_UNKNOWN_QUALIFIER: LintRule = LintRule {
    id: "rototo/qualifier/predicate/unknown-qualifier",
    title: "Qualifier predicate references an unknown qualifier",
    help: "Create the referenced qualifier or update the qualifier.<id> reference.",
};
const RULE_QUALIFIER_BUCKET: LintRule = LintRule {
    id: "rototo/qualifier/predicate/bucket",
    title: "Bucket predicate is invalid",
    help: "Bucket predicates need salt and range = [start, end] with 0 <= start < end <= 10000.",
};
const RULE_QUALIFIER_PREDICATE_VALUE: LintRule = LintRule {
    id: "rototo/qualifier/predicate/value",
    title: "Qualifier predicate value is invalid",
    help: "Add a value with the shape required by the predicate operator.",
};
const RULE_VARIABLE_SCHEMA_VERSION: LintRule = LintRule {
    id: "rototo/variable/schema-version",
    title: "Variable schema version is missing or unsupported",
    help: "Declare schema_version = 1 in the variable file.",
};
const RULE_VARIABLE_MISSING_TABLE: LintRule = LintRule {
    id: "rototo/variable/missing-table",
    title: "Variable table is missing",
    help: "Add a [variable] table.",
};
const RULE_VARIABLE_TYPE_OR_SCHEMA: LintRule = LintRule {
    id: "rototo/variable/type-or-schema",
    title: "Variable must declare exactly one type source",
    help: "Declare exactly one of type or schema under [variable].",
};
const RULE_VARIABLE_LINT_SHAPE: LintRule = LintRule {
    id: "rototo/variable/lint/shape",
    title: "Variable custom lint declaration is invalid",
    help: "Use [variable.lint] with a string path field.",
};
const RULE_VARIABLE_MISSING_VALUES: LintRule = LintRule {
    id: "rototo/variable/values/missing",
    title: "Variable values are missing",
    help: "Add [variable.values] entries.",
};
const RULE_VARIABLE_MISSING_ENV: LintRule = LintRule {
    id: "rototo/variable/env/missing-default",
    title: "Variable default environment is missing",
    help: "Add [variable.env._] with a value reference.",
};
const RULE_VARIABLE_UNKNOWN_ENVIRONMENT: LintRule = LintRule {
    id: "rototo/variable/env/unknown-environment",
    title: "Variable references an undeclared environment",
    help: "Declare the environment in [environments].values or remove the environment block.",
};
const RULE_VARIABLE_ENV_SHAPE: LintRule = LintRule {
    id: "rototo/variable/env/shape",
    title: "Variable environment block is invalid",
    help: "Environment blocks must be tables with a value reference.",
};
const RULE_VARIABLE_UNKNOWN_VALUE: LintRule = LintRule {
    id: "rototo/variable/value/unknown",
    title: "Variable references an unknown value",
    help: "Create the referenced value under [variable.values] or update the reference.",
};
const RULE_VARIABLE_RULE_SHAPE: LintRule = LintRule {
    id: "rototo/variable/rule/shape",
    title: "Variable rule is invalid",
    help: "Rules must be tables with qualifier and value references.",
};
const RULE_VARIABLE_RULE_UNKNOWN_QUALIFIER: LintRule = LintRule {
    id: "rototo/variable/rule/unknown-qualifier",
    title: "Variable rule references an unknown qualifier",
    help: "Create the referenced qualifier or update the rule.",
};
const RULE_VARIABLE_UNKNOWN_TYPE: LintRule = LintRule {
    id: "rototo/variable/type/unknown",
    title: "Variable type is unknown",
    help: "Use one of bool, int, number, string, or list.",
};
const RULE_VARIABLE_VALUE_TYPE_MISMATCH: LintRule = LintRule {
    id: "rototo/variable/value/type-mismatch",
    title: "Variable value does not match type",
    help: "Update the value so it matches the declared primitive type.",
};
const RULE_VARIABLE_SCHEMA_REF: LintRule = LintRule {
    id: "rototo/variable/schema/ref",
    title: "Variable schema reference is invalid",
    help: "Point schema to a readable valid JSON Schema file.",
};
const RULE_VARIABLE_VALUE_SCHEMA_MISMATCH: LintRule = LintRule {
    id: "rototo/variable/value/schema-mismatch",
    title: "Variable value does not match schema",
    help: "Update the value so it matches the variable JSON Schema.",
};
const RULE_VARIABLE_CUSTOM_LINT: LintRule = LintRule {
    id: "rototo/variable/custom-lint/failed",
    title: "Variable custom lint failed",
    help: "Update the variable or its Lua lint rule so custom lint passes.",
};

pub async fn lint_workspace(workspace_root: &Path) -> Result<WorkspaceLint> {
    let root = match tokio::fs::canonicalize(workspace_root).await {
        Ok(root) => root,
        Err(err) => {
            return Ok(WorkspaceLint {
                root: workspace_root.to_path_buf(),
                diagnostics: vec![Diagnostic::new_rule(
                    WORKSPACE_NOT_FOUND,
                    DiagnosticSource::Kernel,
                    workspace_root.display().to_string(),
                    err.to_string(),
                    RULE_WORKSPACE_NOT_FOUND,
                )],
            });
        }
    };

    let mut diagnostics = Vec::new();
    let Some(manifest) = read_toml_diagnostic(
        &root.join(WORKSPACE_MANIFEST),
        WORKSPACE_MANIFEST_MISSING,
        WORKSPACE_MANIFEST_PARSE_FAILED,
        RULE_WORKSPACE_MANIFEST_MISSING,
        RULE_WORKSPACE_MANIFEST_PARSE_FAILED,
        &mut diagnostics,
    )
    .await
    else {
        return Ok(WorkspaceLint { root, diagnostics });
    };

    let environments = match workspace_environments(&manifest) {
        Ok(environments) => environments,
        Err(err) => {
            diagnostics.push(Diagnostic::new_rule(
                WORKSPACE_MANIFEST_SCHEMA_FAILED,
                DiagnosticSource::Kernel,
                WORKSPACE_MANIFEST,
                err.to_string(),
                RULE_WORKSPACE_MANIFEST_SCHEMA_FAILED,
            ));
            return Ok(WorkspaceLint { root, diagnostics });
        }
    };

    let inspection = match inspect_workspace(&root).await {
        Ok(inspection) => inspection,
        Err(err) => {
            diagnostics.push(Diagnostic::new_rule(
                WORKSPACE_MANIFEST_SCHEMA_FAILED,
                DiagnosticSource::Kernel,
                WORKSPACE_MANIFEST,
                err.to_string(),
                RULE_WORKSPACE_MANIFEST_SCHEMA_FAILED,
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
    for variable in &inspection.variables {
        lint_variable_file(
            &root,
            variable,
            &environments,
            &qualifier_ids,
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
        WORKSPACE_TOML_FILE_PARSE_FAILED,
        WORKSPACE_TOML_FILE_PARSE_FAILED,
        RULE_WORKSPACE_TOML_FILE_PARSE_FAILED,
        RULE_WORKSPACE_TOML_FILE_PARSE_FAILED,
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
            RULE_QUALIFIER_SCHEMA_VERSION,
            "qualifier must declare schema_version = 1",
            diagnostics,
        );
    }
    if toml.get("qualifier").and_then(Value::as_table).is_none() {
        push_qualifier_error(
            qualifier,
            RULE_QUALIFIER_MISSING_TABLE,
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
            RULE_QUALIFIER_MISSING_PREDICATE,
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = root.join(&variable.path);
    let Some(_) = read_toml_diagnostic(
        &path,
        WORKSPACE_TOML_FILE_PARSE_FAILED,
        WORKSPACE_TOML_FILE_PARSE_FAILED,
        RULE_WORKSPACE_TOML_FILE_PARSE_FAILED,
        RULE_WORKSPACE_TOML_FILE_PARSE_FAILED,
        diagnostics,
    )
    .await
    else {
        return;
    };
    let toml = match read_variable_toml(root, variable).await {
        Ok(toml) => toml,
        Err(err) => {
            push_variable_error(
                variable,
                RULE_VARIABLE_MISSING_VALUES,
                format!("variable values could not be loaded: {err}"),
                diagnostics,
            );
            return;
        }
    };

    validate_variable_toml(
        root,
        variable,
        &toml,
        environments,
        qualifier_ids,
        diagnostics,
    )
    .await;
    match crate::lua_lint::lint_variable(root, variable, &toml, environments).await {
        Ok(custom_diagnostics) => diagnostics.extend(custom_diagnostics),
        Err(err) => diagnostics.push(
            Diagnostic::new_rule(
                VARIABLE_CUSTOM_LINT_FAILED,
                DiagnosticSource::Custom,
                variable.path.display().to_string(),
                err.to_string(),
                RULE_VARIABLE_CUSTOM_LINT,
            )
            .with_kind("variable"),
        ),
    }
}

async fn validate_variable_toml(
    root: &Path,
    variable_inspection: &VariableInspection,
    toml: &Value,
    environments: &[String],
    qualifier_ids: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if toml.get("schema_version").and_then(Value::as_integer) != Some(1) {
        push_variable_error(
            variable_inspection,
            RULE_VARIABLE_SCHEMA_VERSION,
            "variable must declare schema_version = 1",
            diagnostics,
        );
    }

    let Some(variable) = toml.get("variable").and_then(Value::as_table) else {
        push_variable_error(
            variable_inspection,
            RULE_VARIABLE_MISSING_TABLE,
            "variable must contain a [variable] table",
            diagnostics,
        );
        return;
    };

    let has_type = variable.get("type").and_then(Value::as_str).is_some();
    let schema = variable.get("schema").and_then(Value::as_str);
    if has_type == schema.is_some() {
        push_variable_error(
            variable_inspection,
            RULE_VARIABLE_TYPE_OR_SCHEMA,
            "variable must declare exactly one of type or schema",
            diagnostics,
        );
    }
    if let Some(lint) = variable.get("lint") {
        match lint.as_table() {
            Some(lint) if lint.get("path").and_then(Value::as_str).is_some() => {}
            Some(_) => push_variable_error(
                variable_inspection,
                RULE_VARIABLE_LINT_SHAPE,
                "variable lint must contain path",
                diagnostics,
            ),
            None => push_variable_error(
                variable_inspection,
                RULE_VARIABLE_LINT_SHAPE,
                "variable lint must be a table",
                diagnostics,
            ),
        }
    }

    let Some(values) = variable.get("values").and_then(Value::as_table) else {
        push_variable_error(
            variable_inspection,
            RULE_VARIABLE_MISSING_VALUES,
            "variable must contain [variable.values]",
            diagnostics,
        );
        return;
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
            RULE_VARIABLE_MISSING_ENV,
            "variable must contain [variable.env._]",
            diagnostics,
        );
        return;
    };
    if !env.contains_key("_") {
        push_variable_error(
            variable_inspection,
            RULE_VARIABLE_MISSING_ENV,
            "variable must contain [variable.env._]",
            diagnostics,
        );
    }

    for (environment, block) in env {
        if environment != "_" && !environments.iter().any(|known| known == environment) {
            push_variable_error(
                variable_inspection,
                RULE_VARIABLE_UNKNOWN_ENVIRONMENT,
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
            RULE_VARIABLE_ENV_SHAPE,
            "environment block must be a table",
            diagnostics,
        );
        return;
    };

    let Some(value) = table.get("value").and_then(Value::as_str) else {
        push_variable_error(
            variable,
            RULE_VARIABLE_ENV_SHAPE,
            "environment block must reference a value",
            diagnostics,
        );
        return;
    };
    if !values.contains_key(value) {
        push_variable_error(
            variable,
            RULE_VARIABLE_UNKNOWN_VALUE,
            format!("environment references unknown value: {value}"),
            diagnostics,
        );
    }

    if let Some(rules) = table.get("rule").and_then(Value::as_array) {
        for rule in rules {
            let Some(rule) = rule.as_table() else {
                push_variable_error(
                    variable,
                    RULE_VARIABLE_RULE_SHAPE,
                    "rule must be a table",
                    diagnostics,
                );
                continue;
            };

            match rule.get("qualifier").and_then(Value::as_str) {
                Some(qualifier) if qualifier_ids.contains(qualifier) => {}
                Some(qualifier) => push_variable_error(
                    variable,
                    RULE_VARIABLE_RULE_UNKNOWN_QUALIFIER,
                    format!("rule references unknown qualifier: {qualifier}"),
                    diagnostics,
                ),
                None => push_variable_error(
                    variable,
                    RULE_VARIABLE_RULE_SHAPE,
                    "rule must reference a qualifier",
                    diagnostics,
                ),
            }

            match rule.get("value").and_then(Value::as_str) {
                Some(value) if values.contains_key(value) => {}
                Some(value) => push_variable_error(
                    variable,
                    RULE_VARIABLE_UNKNOWN_VALUE,
                    format!("rule references unknown value: {value}"),
                    diagnostics,
                ),
                None => push_variable_error(
                    variable,
                    RULE_VARIABLE_RULE_SHAPE,
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
            RULE_QUALIFIER_PREDICATE_SHAPE,
            "predicate must be a table",
            diagnostics,
        );
        return;
    };

    let Some(attribute) = predicate.get("attribute").and_then(Value::as_str) else {
        push_qualifier_error(
            qualifier,
            RULE_QUALIFIER_PREDICATE_SHAPE,
            "predicate must contain attribute",
            diagnostics,
        );
        return;
    };
    let Some(op) = predicate.get("op").and_then(Value::as_str) else {
        push_qualifier_error(
            qualifier,
            RULE_QUALIFIER_PREDICATE_SHAPE,
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
            RULE_QUALIFIER_PREDICATE_UNKNOWN_OP,
            format!("predicate has unknown op: {op}"),
            diagnostics,
        );
    }

    if let Some(referenced_qualifier) = attribute.strip_prefix("qualifier.")
        && !qualifier_ids.contains(referenced_qualifier)
    {
        push_qualifier_error(
            qualifier,
            RULE_QUALIFIER_PREDICATE_UNKNOWN_QUALIFIER,
            format!("predicate references unknown qualifier: {referenced_qualifier}"),
            diagnostics,
        );
    }

    if op == "bucket" {
        if predicate.get("salt").and_then(Value::as_str).is_none() {
            push_qualifier_error(
                qualifier,
                RULE_QUALIFIER_BUCKET,
                "bucket predicate must contain salt",
                diagnostics,
            );
        }
        let Some(range) = predicate.get("range").and_then(Value::as_array) else {
            push_qualifier_error(
                qualifier,
                RULE_QUALIFIER_BUCKET,
                "bucket predicate must contain range",
                diagnostics,
            );
            return;
        };
        if range.len() != 2 {
            push_qualifier_error(
                qualifier,
                RULE_QUALIFIER_BUCKET,
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
                RULE_QUALIFIER_BUCKET,
                "bucket range must satisfy 0 <= start < end <= 10000",
                diagnostics,
            ),
        }
        if predicate.contains_key("value") {
            push_qualifier_error(
                qualifier,
                RULE_QUALIFIER_BUCKET,
                "bucket predicate must not contain value",
                diagnostics,
            );
        }
    } else {
        let Some(value) = predicate.get("value") else {
            push_qualifier_error(
                qualifier,
                RULE_QUALIFIER_PREDICATE_VALUE,
                "predicate must contain value",
                diagnostics,
            );
            return;
        };
        match op {
            "in" | "not_in" if !value.is_array() => {
                push_qualifier_error(
                    qualifier,
                    RULE_QUALIFIER_PREDICATE_VALUE,
                    format!("{op} predicate value must be a list"),
                    diagnostics,
                );
            }
            "gt" | "gte" | "lt" | "lte" if !value_is_number(value) => {
                push_qualifier_error(
                    qualifier,
                    RULE_QUALIFIER_PREDICATE_VALUE,
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
        diagnostics.push(Diagnostic::new_rule(
            WORKSPACE_CONTEXT_SCHEMA_FAILED,
            DiagnosticSource::Custom,
            WORKSPACE_MANIFEST,
            "[context] must be a table",
            RULE_CONTEXT_SCHEMA_REF,
        ));
        return None;
    };
    let Some(schema_ref) = context.get("schema").and_then(Value::as_str) else {
        diagnostics.push(Diagnostic::new_rule(
            WORKSPACE_CONTEXT_SCHEMA_FAILED,
            DiagnosticSource::Custom,
            WORKSPACE_MANIFEST,
            "[context] must declare schema",
            RULE_CONTEXT_SCHEMA_REF,
        ));
        return None;
    };
    if !context_schema_ref_is_safe(schema_ref) {
        diagnostics.push(Diagnostic::new_rule(
            WORKSPACE_CONTEXT_SCHEMA_FAILED,
            DiagnosticSource::Custom,
            WORKSPACE_MANIFEST,
            "context schema path must be a relative path inside the workspace",
            RULE_CONTEXT_SCHEMA_REF,
        ));
        return None;
    }

    let schema_path = root.join(schema_ref);
    let text = match tokio::fs::read_to_string(&schema_path).await {
        Ok(text) => text,
        Err(err) => {
            diagnostics.push(Diagnostic::new_rule(
                WORKSPACE_CONTEXT_SCHEMA_FAILED,
                DiagnosticSource::Custom,
                schema_ref,
                format!("context schema could not be read: {err}"),
                RULE_CONTEXT_SCHEMA_REF,
            ));
            return None;
        }
    };
    let schema = match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(schema) => schema,
        Err(err) => {
            diagnostics.push(Diagnostic::new_rule(
                WORKSPACE_CONTEXT_SCHEMA_FAILED,
                DiagnosticSource::Custom,
                schema_ref,
                format!("context schema could not be parsed: {err}"),
                RULE_CONTEXT_SCHEMA_REF,
            ));
            return None;
        }
    };
    if let Err(err) = jsonschema::validator_for(&schema) {
        diagnostics.push(Diagnostic::new_rule(
            WORKSPACE_CONTEXT_SCHEMA_FAILED,
            DiagnosticSource::Custom,
            schema_ref,
            format!("context schema is invalid: {err}"),
            RULE_CONTEXT_SCHEMA_REF,
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
            diagnostics.push(
                Diagnostic::new_rule(
                    WORKSPACE_CONTEXT_SCHEMA_FAILED,
                    DiagnosticSource::Custom,
                    qualifier.path.display().to_string(),
                    format!("context schema does not declare attribute: {attribute}"),
                    RULE_CONTEXT_SCHEMA_ATTRIBUTE,
                )
                .with_kind("qualifier"),
            );
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
            RULE_VARIABLE_UNKNOWN_TYPE,
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
                RULE_VARIABLE_VALUE_TYPE_MISMATCH,
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
                RULE_VARIABLE_VALUE_SCHEMA_MISMATCH,
                format!("value {name} could not be converted to JSON"),
                diagnostics,
            );
            continue;
        };

        if let Err(error) = validator.validate(&json) {
            push_variable_error(
                variable,
                RULE_VARIABLE_VALUE_SCHEMA_MISMATCH,
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
                        RULE_VARIABLE_SCHEMA_REF,
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
                        RULE_VARIABLE_SCHEMA_REF,
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
                RULE_VARIABLE_SCHEMA_REF,
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
                diagnostics.push(Diagnostic::new_rule(
                    JSON_SCHEMA_FILE_PARSE_FAILED,
                    DiagnosticSource::Schema,
                    display_relative(root, &entry),
                    err.to_string(),
                    RULE_JSON_SCHEMA_FILE_PARSE_FAILED,
                ));
                continue;
            }
        };
        let schema = match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(schema) => schema,
            Err(err) => {
                diagnostics.push(Diagnostic::new_rule(
                    JSON_SCHEMA_FILE_PARSE_FAILED,
                    DiagnosticSource::Schema,
                    display_relative(root, &entry),
                    err.to_string(),
                    RULE_JSON_SCHEMA_FILE_PARSE_FAILED,
                ));
                continue;
            }
        };
        if let Err(err) = jsonschema::validator_for(&schema) {
            diagnostics.push(Diagnostic::new_rule(
                JSON_SCHEMA_FILE_INVALID,
                DiagnosticSource::Schema,
                display_relative(root, &entry),
                err.to_string(),
                RULE_SCHEMA_INVALID,
            ));
        }
    }
}

async fn read_toml_diagnostic(
    path: &Path,
    missing: crate::diagnostics::DiagnosticSpec,
    parse_failed: crate::diagnostics::DiagnosticSpec,
    missing_rule: LintRule,
    parse_rule: LintRule,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Value> {
    let text = match tokio::fs::read_to_string(path).await {
        Ok(text) => text,
        Err(err) => {
            diagnostics.push(Diagnostic::new_rule(
                missing,
                DiagnosticSource::Kernel,
                display_path(path),
                err.to_string(),
                missing_rule,
            ));
            return None;
        }
    };
    match text.parse::<Value>() {
        Ok(value) => Some(value),
        Err(err) => {
            diagnostics.push(Diagnostic::new_rule(
                parse_failed,
                DiagnosticSource::Kernel,
                display_path(path),
                err.to_string(),
                parse_rule,
            ));
            None
        }
    }
}

fn push_qualifier_error(
    qualifier: &QualifierInspection,
    rule: LintRule,
    message: impl Into<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    push_workspace_file_error("qualifier", &qualifier.path, rule, message, diagnostics);
}

fn push_variable_error(
    variable: &VariableInspection,
    rule: LintRule,
    message: impl Into<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    push_workspace_file_error("variable", &variable.path, rule, message, diagnostics);
}

fn push_workspace_file_error(
    kind: &str,
    path: &Path,
    rule: LintRule,
    message: impl Into<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        Diagnostic::new_rule(
            WORKSPACE_TOML_FILE_INVALID,
            DiagnosticSource::Kernel,
            path.display().to_string(),
            message,
            rule,
        )
        .with_kind(kind),
    );
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
