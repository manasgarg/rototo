use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use jsonschema::Validator;
use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};

use super::index::*;
use super::input::LintInput;
use super::{WorkspaceLintSnapshot, lint_workspace_snapshot};

const CONTEXT_SCHEMA_PATH: &str = "schemas/context.schema.json";

#[derive(Debug)]
pub(crate) struct RuntimeWorkspace {
    pub(crate) context_schema: Option<JsonValue>,
    pub(crate) context_validator: Option<Arc<Validator>>,
    pub(crate) qualifiers: BTreeMap<String, RuntimeQualifier>,
    pub(crate) variables: BTreeMap<String, RuntimeVariable>,
}

impl RuntimeWorkspace {
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
        index: usize,
        attribute: RuntimeAttribute,
        op: RuntimeCompareOp,
        value: JsonValue,
    },
    Bucket {
        index: usize,
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
    pub(crate) default: String,
    pub(crate) rules: Vec<RuntimeRule>,
}

#[derive(Debug)]
pub(crate) struct RuntimeRule {
    pub(crate) index: usize,
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
        let _manifest = index
            .manifest
            .as_ref()
            .ok_or_else(|| RototoError::new("workspace manifest is missing"))?;
        let (context_schema, context_validator) = self.compile_context_schema(index)?;
        let qualifiers = self.compile_qualifiers(index)?;
        let variables = self.compile_variables(index, &qualifiers)?;

        Ok(RuntimeWorkspace {
            context_schema,
            context_validator,
            qualifiers,
            variables,
        })
    }

    fn compile_context_schema(
        &self,
        index: &SemanticIndex,
    ) -> Result<(Option<JsonValue>, Option<Arc<Validator>>)> {
        let Some(schema) = index.schemas.get(CONTEXT_SCHEMA_PATH) else {
            return Ok((None, None));
        };
        let json = schema.json.clone().ok_or_else(|| {
            RototoError::new(format!(
                "context schema file could not be parsed: {CONTEXT_SCHEMA_PATH}"
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
                index: predicate.index,
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
            index: predicate.index,
            attribute,
            op: compare_op,
            value: value.value.clone(),
        })
    }

    fn compile_variables(
        &self,
        index: &SemanticIndex,
        qualifiers: &BTreeMap<String, RuntimeQualifier>,
    ) -> Result<BTreeMap<String, RuntimeVariable>> {
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
            let (default, rules) = self.compile_variable_resolve(variable, &values, qualifiers)?;

            variables.insert(
                variable.id.clone(),
                RuntimeVariable {
                    values,
                    default,
                    rules,
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
            TypeSourceNode::Catalog(catalog) if index.catalogs.contains_key(&catalog.value) => {
                Ok(())
            }
            TypeSourceNode::Catalog(catalog) => Err(RototoError::new(format!(
                "variable references unknown catalog: {}",
                catalog.value
            ))),
            TypeSourceNode::Schema(_) => Err(RototoError::new(format!(
                "variable schemas are no longer supported: {}",
                variable.id
            ))),
            TypeSourceNode::Missing { .. }
            | TypeSourceNode::Conflict { .. }
            | TypeSourceNode::Invalid { .. } => Err(RototoError::new(format!(
                "variable must declare type: {}",
                variable.id
            ))),
        }
    }

    fn compile_variable_values(
        &self,
        index: &SemanticIndex,
        variable: &VariableNode,
    ) -> Result<BTreeMap<String, JsonValue>> {
        if let TypeSourceNode::Catalog(catalog) = &variable.type_source {
            if variable.values.invalid_shape || !variable.values.inline_values.is_empty() {
                return Err(RototoError::new(format!(
                    "catalog-backed variable must not contain values: {}",
                    variable.id
                )));
            }

            let entries = index.catalog_entries.get(&catalog.value).ok_or_else(|| {
                RototoError::new(format!(
                    "catalog has no entries for variable {}: {}",
                    variable.id, catalog.value
                ))
            })?;
            if entries.is_empty() {
                return Err(RototoError::new(format!(
                    "catalog has no entries for variable {}: {}",
                    variable.id, catalog.value
                )));
            }
            return Ok(entries
                .iter()
                .map(|(key, entry)| (key.clone(), entry.value.clone()))
                .collect());
        }

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

        if values.is_empty() {
            return Err(RototoError::new(format!(
                "variable must contain values: {}",
                variable.id
            )));
        }
        Ok(values)
    }

    fn compile_variable_resolve(
        &self,
        variable: &VariableNode,
        values: &BTreeMap<String, JsonValue>,
        qualifiers: &BTreeMap<String, RuntimeQualifier>,
    ) -> Result<(String, Vec<RuntimeRule>)> {
        let ResolveNode::Resolve { default, rules, .. } = &variable.resolve else {
            return Err(RototoError::new(format!(
                "variable must contain [resolve]: {}",
                variable.id
            )));
        };
        let default = present_string(default, "resolve must reference a default value")?
            .value
            .clone();
        if !values.contains_key(&default) {
            return Err(RototoError::new(format!(
                "resolve default references unknown value: {default}"
            )));
        }
        let RuleCollection::Rules(rules) = rules else {
            return Err(RototoError::new("rule must use [[resolve.rule]] tables"));
        };
        let rules = rules
            .iter()
            .map(|rule| self.compile_variable_rule(rule, values, qualifiers))
            .collect::<Result<Vec<_>>>()?;
        Ok((default, rules))
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
        Ok(RuntimeRule {
            index: rule.index,
            qualifier,
            value,
        })
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
