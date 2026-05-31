use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use jsonschema::Validator;
use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};

use super::index::*;
use super::input::LintInput;
use super::source::resolve_workspace_root_path;
use super::{WorkspaceLintSnapshot, lint_workspace_snapshot};

#[derive(Debug)]
pub(crate) struct RuntimeWorkspace {
    pub(crate) environments: Vec<String>,
    pub(crate) context_schema: Option<JsonValue>,
    pub(crate) context_validator: Option<Arc<Validator>>,
    pub(crate) qualifiers: BTreeMap<String, RuntimeQualifier>,
    pub(crate) variables: BTreeMap<String, RuntimeVariable>,
}

impl RuntimeWorkspace {
    pub(crate) fn validate_environment(&self, environment: &str) -> Result<()> {
        if self.environments.iter().any(|known| known == environment) {
            Ok(())
        } else {
            Err(RototoError::new(format!(
                "unknown environment: {environment}"
            )))
        }
    }

    pub(crate) fn validate_context(&self, context: &JsonValue) -> Result<()> {
        let Some(validator) = &self.context_validator else {
            return Ok(());
        };
        validator.validate(context).map_err(|err| {
            RototoError::new(format!("resolve context does not match schema: {err}"))
        })
    }
}

#[derive(Debug)]
pub(crate) struct RuntimeQualifier {
    pub(crate) predicates: Vec<RuntimePredicate>,
}

#[derive(Debug)]
pub(crate) enum RuntimePredicate {
    Compare {
        attribute: RuntimeAttribute,
        op: RuntimeCompareOp,
        value: JsonValue,
    },
    Bucket {
        attribute: String,
        salt: String,
        start: i64,
        end: i64,
    },
}

#[derive(Debug)]
pub(crate) enum RuntimeAttribute {
    ContextPath(String),
    Qualifier(String),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RuntimeCompareOp {
    Eq,
    Neq,
    In,
    NotIn,
    Gt,
    Gte,
    Lt,
    Lte,
}

#[derive(Debug)]
pub(crate) struct RuntimeVariable {
    pub(crate) values: BTreeMap<String, JsonValue>,
    pub(crate) environments: BTreeMap<String, RuntimeEnvironmentBlock>,
}

#[derive(Debug)]
pub(crate) struct RuntimeEnvironmentBlock {
    pub(crate) value: String,
    pub(crate) rules: Vec<RuntimeRule>,
}

#[derive(Debug)]
pub(crate) struct RuntimeRule {
    pub(crate) qualifier: String,
    pub(crate) value: String,
}

pub(crate) async fn compile_runtime_workspace(root: &Path) -> Result<RuntimeWorkspace> {
    let snapshot = lint_workspace_snapshot(LintInput::new(root.to_path_buf())).await?;
    compile_runtime_workspace_from_snapshot(&snapshot)
}

pub(crate) fn compile_runtime_workspace_from_snapshot(
    snapshot: &WorkspaceLintSnapshot,
) -> Result<RuntimeWorkspace> {
    RuntimeCompiler::new(snapshot).compile()
}

struct RuntimeCompiler<'a> {
    snapshot: &'a WorkspaceLintSnapshot,
}

impl<'a> RuntimeCompiler<'a> {
    fn new(snapshot: &'a WorkspaceLintSnapshot) -> Self {
        Self { snapshot }
    }

    fn compile(&self) -> Result<RuntimeWorkspace> {
        let index = &self.snapshot.index;
        let manifest = index
            .manifest
            .as_ref()
            .ok_or_else(|| RototoError::new("workspace manifest is missing"))?;
        let environments = self.compile_environments(manifest)?;
        let (context_schema, context_validator) = self.compile_context_schema(index, manifest)?;
        let qualifiers = self.compile_qualifiers(index)?;
        let variables = self.compile_variables(index, &environments, &qualifiers)?;

        Ok(RuntimeWorkspace {
            environments,
            context_schema,
            context_validator,
            qualifiers,
            variables,
        })
    }

    fn compile_environments(&self, manifest: &ManifestNode) -> Result<Vec<String>> {
        let WorkspaceEnvironmentCollection::Environments { values, .. } = &manifest.environments
        else {
            return Err(RototoError::new(
                "workspace manifest must declare [environments].values",
            ));
        };

        let mut environments = Vec::new();
        let mut seen = BTreeSet::new();
        for environment in values {
            if environment.name == "_" {
                return Err(RototoError::new(
                    "_ is reserved as the catch-all environment",
                ));
            }
            if !seen.insert(environment.name.clone()) {
                return Err(RototoError::new(format!(
                    "duplicate environment: {}",
                    environment.name
                )));
            }
            environments.push(environment.name.clone());
        }

        if environments.is_empty() {
            return Err(RototoError::new(
                "workspace must declare at least one environment",
            ));
        }
        Ok(environments)
    }

    fn compile_context_schema(
        &self,
        index: &SemanticIndex,
        manifest: &ManifestNode,
    ) -> Result<(Option<JsonValue>, Option<Arc<Validator>>)> {
        let Some(context) = &manifest.context_schema else {
            return Ok((None, None));
        };
        if context.invalid_shape {
            return Err(RototoError::new("[context] must be a table"));
        }

        let ProjectField::Present(schema_ref) = &context.schema else {
            return Err(RototoError::new("[context] must declare schema"));
        };
        let schema_path = resolve_workspace_root_path(&schema_ref.value).ok_or_else(|| {
            RototoError::new("context schema path must be a relative path inside the workspace")
        })?;
        let schema = index.schemas.get(&schema_path).ok_or_else(|| {
            RototoError::new(format!("context schema file not found: {schema_path}"))
        })?;
        let json = schema.json.clone().ok_or_else(|| {
            RototoError::new(format!(
                "context schema file could not be parsed: {schema_path}"
            ))
        })?;
        let validator = schema.validator.clone().ok_or_else(|| {
            RototoError::new(format!(
                "context schema is invalid: {}",
                schema
                    .invalid_message
                    .as_deref()
                    .unwrap_or("schema did not compile")
            ))
        })?;

        Ok((Some(json), Some(validator)))
    }

    fn compile_qualifiers(
        &self,
        index: &SemanticIndex,
    ) -> Result<BTreeMap<String, RuntimeQualifier>> {
        let mut qualifiers = BTreeMap::new();
        for qualifier in index.qualifiers.values() {
            if !integer_field_is(&qualifier.schema_version, 1) {
                return Err(RototoError::new(format!(
                    "qualifier must declare schema_version = 1: {}",
                    qualifier.id
                )));
            }

            let PredicateCollection::Predicates(predicates) = &qualifier.predicates else {
                return Err(RototoError::new(format!(
                    "qualifier must contain at least one predicate: {}",
                    qualifier.id
                )));
            };
            if predicates.is_empty() {
                return Err(RototoError::new(format!(
                    "qualifier must contain at least one predicate: {}",
                    qualifier.id
                )));
            }

            let predicates = predicates
                .iter()
                .map(|predicate| self.compile_predicate(index, qualifier, predicate))
                .collect::<Result<Vec<_>>>()?;
            qualifiers.insert(qualifier.id.clone(), RuntimeQualifier { predicates });
        }
        Ok(qualifiers)
    }

    fn compile_predicate(
        &self,
        index: &SemanticIndex,
        _qualifier: &QualifierNode,
        predicate: &PredicateNode,
    ) -> Result<RuntimePredicate> {
        let attribute = present_string(&predicate.attribute, "predicate must contain attribute")?;
        let op = present_predicate_op(&predicate.op)?;

        if matches!(op, PredicateOp::Bucket) {
            let salt =
                present_optional_string(&predicate.salt, "bucket predicate must contain salt")?;
            let range = predicate
                .range
                .as_ref()
                .ok_or_else(|| RototoError::new("bucket predicate must contain range"))?;
            let (Some(start), Some(end)) = (range.start, range.end) else {
                return Err(RototoError::new(
                    "bucket predicate range must contain two integers",
                ));
            };
            if !range.is_array || range.len != 2 || !(0 <= start && start < end && end <= 10_000) {
                return Err(RototoError::new(
                    "bucket range must satisfy 0 <= start < end <= 10000",
                ));
            }
            if predicate.has_bucket_value {
                return Err(RototoError::new("bucket predicate must not contain value"));
            }
            return Ok(RuntimePredicate::Bucket {
                attribute: attribute.value.clone(),
                salt,
                start,
                end,
            });
        }

        let compare_op = compile_compare_op(op)?;
        let value = predicate
            .value
            .as_ref()
            .ok_or_else(|| RototoError::new("predicate must contain value"))?;
        validate_compare_value(compare_op, value)?;
        let attribute = if let Some(qualifier_id) = attribute.value.strip_prefix("qualifier.") {
            if !index.qualifiers.contains_key(qualifier_id) {
                return Err(RototoError::new(format!(
                    "predicate references unknown qualifier: {qualifier_id}"
                )));
            }
            RuntimeAttribute::Qualifier(qualifier_id.to_owned())
        } else {
            RuntimeAttribute::ContextPath(attribute.value.clone())
        };

        Ok(RuntimePredicate::Compare {
            attribute,
            op: compare_op,
            value: value.value.clone(),
        })
    }

    fn compile_variables(
        &self,
        index: &SemanticIndex,
        environments: &[String],
        qualifiers: &BTreeMap<String, RuntimeQualifier>,
    ) -> Result<BTreeMap<String, RuntimeVariable>> {
        let known_environments = environments.iter().cloned().collect::<BTreeSet<_>>();
        let mut variables = BTreeMap::new();
        for variable in index.variables.values() {
            if !integer_field_is(&variable.schema_version, 1) {
                return Err(RototoError::new(format!(
                    "variable must declare schema_version = 1: {}",
                    variable.id
                )));
            }
            self.validate_variable_type_source(index, variable)?;
            let values = self.compile_variable_values(index, variable)?;
            let environments = self.compile_variable_environments(
                variable,
                &values,
                &known_environments,
                qualifiers,
            )?;

            variables.insert(
                variable.id.clone(),
                RuntimeVariable {
                    values,
                    environments,
                },
            );
        }
        Ok(variables)
    }

    fn validate_variable_type_source(
        &self,
        index: &SemanticIndex,
        variable: &VariableNode,
    ) -> Result<()> {
        match &variable.type_source {
            TypeSourceNode::Primitive(type_name) if is_known_primitive(&type_name.value) => Ok(()),
            TypeSourceNode::Primitive(type_name) => Err(RototoError::new(format!(
                "variable declares unknown type: {}",
                type_name.value
            ))),
            TypeSourceNode::Schema(schema_ref) => {
                let schema_path = super::source::resolve_workspace_relative_path(
                    &variable.location.path,
                    &schema_ref.value,
                )
                .ok_or_else(|| {
                    RototoError::new(format!(
                        "variable schema reference is invalid: {} is not a relative path inside the workspace",
                        schema_ref.value
                    ))
                })?;
                let schema = index.schemas.get(&schema_path).ok_or_else(|| {
                    RototoError::new(format!(
                        "variable schema reference is invalid: schema file not found: {schema_path}"
                    ))
                })?;
                if schema.validator.is_none() {
                    return Err(RototoError::new(format!(
                        "variable schema is invalid: {}",
                        schema
                            .invalid_message
                            .as_deref()
                            .unwrap_or("schema did not compile")
                    )));
                }
                Ok(())
            }
            TypeSourceNode::Missing { .. }
            | TypeSourceNode::Conflict { .. }
            | TypeSourceNode::Invalid { .. } => Err(RototoError::new(format!(
                "variable must declare exactly one of type or schema: {}",
                variable.id
            ))),
        }
    }

    fn compile_variable_values(
        &self,
        index: &SemanticIndex,
        variable: &VariableNode,
    ) -> Result<BTreeMap<String, JsonValue>> {
        if variable.values.invalid_shape {
            return Err(RototoError::new(format!(
                "variable values must be a table: {}",
                variable.id
            )));
        }

        let mut values = BTreeMap::new();
        for (key, value) in &variable.values.inline_values {
            values.insert(key.clone(), value.value.clone());
        }

        let external_values = index.external_values.get(&variable.id);
        for key in &variable.values.external_keys {
            let value = external_values
                .and_then(|values| values.get(key))
                .ok_or_else(|| {
                    RototoError::new(format!("external value could not be loaded: {key}"))
                })?;
            if values
                .insert(key.clone(), runtime_external_value(&value.value))
                .is_some()
            {
                return Err(RototoError::new(format!(
                    "variable value is declared more than once: {key}"
                )));
            }
        }

        if values.is_empty() {
            return Err(RototoError::new(format!(
                "variable must contain values: {}",
                variable.id
            )));
        }
        Ok(values)
    }

    fn compile_variable_environments(
        &self,
        variable: &VariableNode,
        values: &BTreeMap<String, JsonValue>,
        known_environments: &BTreeSet<String>,
        qualifiers: &BTreeMap<String, RuntimeQualifier>,
    ) -> Result<BTreeMap<String, RuntimeEnvironmentBlock>> {
        let EnvironmentCollection::Environments(environments) = &variable.environments else {
            return Err(RototoError::new(format!(
                "variable must contain [env._]: {}",
                variable.id
            )));
        };
        if !environments.contains_key("_") {
            return Err(RototoError::new(format!(
                "variable must contain [env._]: {}",
                variable.id
            )));
        }

        let mut compiled = BTreeMap::new();
        for (environment, block) in environments {
            if environment != "_" && !known_environments.contains(environment) {
                return Err(RototoError::new(format!(
                    "variable references undeclared environment: {environment}"
                )));
            }

            let value = present_string(&block.value, "environment block must reference a value")?
                .value
                .clone();
            if !values.contains_key(&value) {
                return Err(RototoError::new(format!(
                    "environment references unknown value: {value}"
                )));
            }

            let RuleCollection::Rules(rules) = &block.rules else {
                return Err(RototoError::new(
                    "rule must use [[env.<id>.rule]] tables or inline rule tables",
                ));
            };
            let rules = rules
                .iter()
                .map(|rule| self.compile_variable_rule(rule, values, qualifiers))
                .collect::<Result<Vec<_>>>()?;
            compiled.insert(
                environment.clone(),
                RuntimeEnvironmentBlock { value, rules },
            );
        }

        Ok(compiled)
    }

    fn compile_variable_rule(
        &self,
        rule: &VariableRuleNode,
        values: &BTreeMap<String, JsonValue>,
        qualifiers: &BTreeMap<String, RuntimeQualifier>,
    ) -> Result<RuntimeRule> {
        if rule.invalid_shape {
            return Err(RototoError::new("rule must be a table"));
        }
        let qualifier = present_string(&rule.qualifier, "rule must reference a qualifier")?
            .value
            .clone();
        if !qualifiers.contains_key(&qualifier) {
            return Err(RototoError::new(format!(
                "rule references unknown qualifier: {qualifier}"
            )));
        }
        let value = present_string(&rule.value, "rule must reference a value")?
            .value
            .clone();
        if !values.contains_key(&value) {
            return Err(RototoError::new(format!(
                "rule references unknown value: {value}"
            )));
        }
        Ok(RuntimeRule { qualifier, value })
    }
}

fn integer_field_is(field: &ProjectField<i64>, expected: i64) -> bool {
    matches!(field, ProjectField::Present(value) if value.value == expected)
}

fn present_string<'a>(
    field: &'a ProjectField<String>,
    message: &'static str,
) -> Result<&'a Spanned<String>> {
    match field {
        ProjectField::Present(value) => Ok(value),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => {
            Err(RototoError::new(message))
        }
    }
}

fn present_optional_string(
    field: &Option<ProjectField<String>>,
    message: &'static str,
) -> Result<String> {
    let Some(field) = field else {
        return Err(RototoError::new(message));
    };
    Ok(present_string(field, message)?.value.clone())
}

fn present_predicate_op(field: &ProjectField<PredicateOp>) -> Result<&PredicateOp> {
    match field {
        ProjectField::Present(value) => match &value.value {
            PredicateOp::Unknown(op) => Err(RototoError::new(format!(
                "unknown predicate operator: {op}"
            ))),
            op => Ok(op),
        },
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => {
            Err(RototoError::new("predicate must contain op"))
        }
    }
}

fn compile_compare_op(op: &PredicateOp) -> Result<RuntimeCompareOp> {
    Ok(match op {
        PredicateOp::Eq => RuntimeCompareOp::Eq,
        PredicateOp::Neq => RuntimeCompareOp::Neq,
        PredicateOp::In => RuntimeCompareOp::In,
        PredicateOp::NotIn => RuntimeCompareOp::NotIn,
        PredicateOp::Gt => RuntimeCompareOp::Gt,
        PredicateOp::Gte => RuntimeCompareOp::Gte,
        PredicateOp::Lt => RuntimeCompareOp::Lt,
        PredicateOp::Lte => RuntimeCompareOp::Lte,
        PredicateOp::Bucket | PredicateOp::Unknown(_) => {
            return Err(RototoError::new("predicate operator is not comparable"));
        }
    })
}

fn validate_compare_value(op: RuntimeCompareOp, value: &ValueShapeNode) -> Result<()> {
    match op {
        RuntimeCompareOp::In | RuntimeCompareOp::NotIn if value.shape != ValueShape::Array => {
            Err(RototoError::new("in/not_in predicate value must be a list"))
        }
        RuntimeCompareOp::Gt
        | RuntimeCompareOp::Gte
        | RuntimeCompareOp::Lt
        | RuntimeCompareOp::Lte
            if !matches!(value.shape, ValueShape::Integer | ValueShape::Float) =>
        {
            Err(RototoError::new(
                "comparison predicate value must be a number",
            ))
        }
        _ => Ok(()),
    }
}

fn is_known_primitive(value: &str) -> bool {
    matches!(value, "bool" | "int" | "number" | "string" | "list")
}

fn runtime_external_value(value: &JsonValue) -> JsonValue {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    if object.len() == 1
        && let Some(value) = object.get("value")
    {
        return value.clone();
    }
    value.clone()
}
