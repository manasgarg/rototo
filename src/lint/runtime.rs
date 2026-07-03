use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use jsonschema::Validator;
use serde_json::Value as JsonValue;

use crate::error::{Result, RototoError};
use crate::expression::Expression;

use super::index::*;
use super::input::LintInput;
use super::{PackageLintSnapshot, lint_package_snapshot};

#[derive(Debug)]
pub(crate) struct RuntimePackage {
    pub(crate) evaluation_contexts: BTreeMap<String, RuntimeEvaluationContext>,
    pub(crate) variable_evaluation_contexts: BTreeMap<String, BTreeSet<String>>,
    pub(crate) catalog_schemas: BTreeMap<String, JsonValue>,
    pub(crate) catalog_entries: BTreeMap<String, BTreeMap<String, JsonValue>>,
    pub(crate) variables: BTreeMap<String, RuntimeVariable>,
    pub(crate) trace_policies: Vec<RuntimeTracePolicy>,
}

impl RuntimePackage {
    pub(crate) fn validate_context(&self, context: &JsonValue) -> Result<()> {
        self.validate_context_against(context, None)
    }

    pub(crate) fn validate_context_for_variable(
        &self,
        variable: &str,
        context: &JsonValue,
    ) -> Result<()> {
        let allowed = self
            .variable_evaluation_contexts
            .get(variable)
            .ok_or_else(|| {
                RototoError::new(format!("variable not found: variable://{variable}"))
            })?;
        if allowed.is_empty()
            && self
                .variables
                .get(variable)
                .is_some_and(|variable| match &variable.resolution {
                    RuntimeResolution::Rules { rules, .. } => rules.is_empty(),
                    RuntimeResolution::Query(query) => !query.uses_context,
                })
        {
            return Ok(());
        }
        self.validate_context_against(context, Some(allowed))
    }

    fn validate_context_against(
        &self,
        context: &JsonValue,
        allowed: Option<&BTreeSet<String>>,
    ) -> Result<()> {
        if self.evaluation_contexts.is_empty() {
            return Ok(());
        }
        let mut saw_candidate = false;
        let mut errors = Vec::new();
        for (id, evaluation_context) in &self.evaluation_contexts {
            if allowed.is_some_and(|allowed| !allowed.contains(id)) {
                continue;
            }
            saw_candidate = true;
            match evaluation_context.validator.validate(context) {
                Ok(()) => return Ok(()),
                Err(err) => errors.push(format!("{id}: {err}")),
            }
        }
        if !saw_candidate {
            return Err(RototoError::new(
                "evaluation context does not match any compatible evaluation context",
            ));
        }
        Err(RototoError::new(format!(
            "evaluation context does not match any compatible evaluation context: {}",
            errors.join("; ")
        )))
    }
}

#[derive(Debug)]
pub(crate) struct RuntimeEvaluationContext {
    pub(crate) schema: JsonValue,
    pub(crate) validator: Arc<Validator>,
}

/// A compiled `[[trace]]` policy. Its `when` is evaluated against each
/// resolution to decide whether to emit a trace event; it may read
/// `env.resolving.*`.
#[derive(Debug)]
pub(crate) struct RuntimeTracePolicy {
    pub(crate) when: Expression,
}

#[derive(Debug)]
pub(crate) struct RuntimeVariable {
    pub(crate) resolution: RuntimeResolution,
}

#[derive(Debug)]
pub(crate) enum RuntimeResolution {
    Rules {
        default: RuntimeSelectedValue,
        rules: Vec<RuntimeRule>,
    },
    Query(Box<RuntimeQuery>),
}

#[derive(Debug)]
pub(crate) struct RuntimeRule {
    pub(crate) index: usize,
    pub(crate) when: Expression,
    pub(crate) value: RuntimeSelectedValue,
}

/// A compiled `method = "query"` pipeline over one catalog's entries.
#[derive(Debug)]
pub(crate) struct RuntimeQuery {
    pub(crate) catalog: String,
    /// Whether the variable's type is `catalog:<id>` (the top entry wins)
    /// rather than `list<catalog:<id>>` (every match is the value).
    pub(crate) single: bool,
    pub(crate) filter: Option<Expression>,
    pub(crate) sort: Option<Expression>,
    pub(crate) descending: bool,
    pub(crate) limit: Option<usize>,
    pub(crate) default: Option<RuntimeSelectedValue>,
    /// Whether filter/sort read `context` or other variables, i.e. whether
    /// resolution needs a validated evaluation context at all.
    pub(crate) uses_context: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum RuntimeSelectedValue {
    Literal(JsonValue),
    Catalog {
        catalog: String,
        name: String,
        value: JsonValue,
    },
    CatalogList {
        catalog: String,
        names: Vec<String>,
        value: JsonValue,
    },
}

impl RuntimeSelectedValue {
    pub(crate) fn value(&self) -> &JsonValue {
        match self {
            Self::Literal(value) => value,
            Self::Catalog { value, .. } => value,
            Self::CatalogList { value, .. } => value,
        }
    }
}

pub(crate) async fn compile_runtime_package(root: &Path) -> Result<RuntimePackage> {
    let snapshot = lint_package_snapshot(LintInput::new(root.to_path_buf())).await?;
    compile_runtime_package_from_snapshot(&snapshot)
}

pub(crate) fn compile_runtime_package_from_snapshot(
    snapshot: &PackageLintSnapshot,
) -> Result<RuntimePackage> {
    RuntimeCompiler::new(snapshot).compile()
}

struct RuntimeCompiler<'a> {
    snapshot: &'a PackageLintSnapshot,
}

impl<'a> RuntimeCompiler<'a> {
    fn new(snapshot: &'a PackageLintSnapshot) -> Self {
        Self { snapshot }
    }

    fn compile(&self) -> Result<RuntimePackage> {
        let index = &self.snapshot.index;
        let manifest = index
            .manifest
            .as_ref()
            .ok_or_else(|| RototoError::new("package manifest is missing"))?;
        let evaluation_contexts = self.compile_evaluation_contexts(index)?;
        let compatibility = self.snapshot.evaluation_context_compatibility();
        let catalog_schemas = self.compile_catalog_schemas(index);
        let catalog_entries = self.compile_catalog_entries(index);
        let variables = self.compile_variables(index)?;
        let trace_policies = Self::compile_trace_policies(manifest)?;

        Ok(RuntimePackage {
            evaluation_contexts,
            variable_evaluation_contexts: compatibility.variables,
            catalog_schemas,
            catalog_entries,
            variables,
            trace_policies,
        })
    }

    fn compile_trace_policies(manifest: &ManifestNode) -> Result<Vec<RuntimeTracePolicy>> {
        manifest
            .trace
            .iter()
            .map(|policy| match &policy.when {
                ProjectField::Present(when) => Ok(RuntimeTracePolicy {
                    when: when.value.clone(),
                }),
                ProjectField::Invalid { .. } => Err(RototoError::new(format!(
                    "trace policy {} when expression is invalid",
                    policy.index
                ))),
                ProjectField::Missing { .. } => Err(RototoError::new(format!(
                    "trace policy {} must declare when",
                    policy.index
                ))),
            })
            .collect()
    }

    fn compile_catalog_schemas(&self, index: &SemanticIndex) -> BTreeMap<String, JsonValue> {
        index
            .catalogs
            .iter()
            .filter_map(|(id, catalog)| catalog.json.clone().map(|json| (id.clone(), json)))
            .collect()
    }

    fn compile_catalog_entries(
        &self,
        index: &SemanticIndex,
    ) -> BTreeMap<String, BTreeMap<String, JsonValue>> {
        index
            .catalog_entries
            .iter()
            .map(|(catalog, entries)| {
                (
                    catalog.clone(),
                    entries
                        .iter()
                        .map(|(key, entry)| (key.clone(), entry.value.clone()))
                        .collect(),
                )
            })
            .collect()
    }

    fn compile_evaluation_contexts(
        &self,
        index: &SemanticIndex,
    ) -> Result<BTreeMap<String, RuntimeEvaluationContext>> {
        let mut evaluation_contexts = BTreeMap::new();
        for context in index.evaluation_contexts.values() {
            let json = context.json.clone().ok_or_else(|| {
                RototoError::new(format!(
                    "evaluation context schema file could not be parsed: {}",
                    context.path
                ))
            })?;
            let validator = context.validator.clone().ok_or_else(|| {
                RototoError::new(format!(
                    "evaluation context schema is invalid: {}",
                    context
                        .invalid_message
                        .as_deref()
                        .unwrap_or("schema did not compile")
                ))
            })?;
            evaluation_contexts.insert(
                context.id.clone(),
                RuntimeEvaluationContext {
                    schema: json,
                    validator,
                },
            );
        }
        Ok(evaluation_contexts)
    }

    fn compile_variables(
        &self,
        index: &SemanticIndex,
    ) -> Result<BTreeMap<String, RuntimeVariable>> {
        let mut variables = BTreeMap::new();
        for variable in index.variables.values() {
            if !integer_field_is(&variable.schema_version, 1) {
                return Err(RototoError::new(format!(
                    "variable must declare schema_version = 1: {}",
                    variable.id
                )));
            }
            let type_kind = self.validate_variable_type_source(index, variable)?;
            let resolution = self.compile_variable_resolve(index, variable, &type_kind)?;

            variables.insert(variable.id.clone(), RuntimeVariable { resolution });
        }
        Ok(variables)
    }

    fn validate_variable_type_source(
        &self,
        index: &SemanticIndex,
        variable: &VariableNode,
    ) -> Result<VariableTypeKind> {
        let type_kind = variable_type_kind(&variable.type_source).ok_or_else(|| {
            RototoError::new(format!("variable must declare type: {}", variable.id))
        })?;
        validate_variable_type_kind(index, &type_kind.value)?;
        Ok(type_kind.value)
    }
}

fn validate_variable_type_kind(index: &SemanticIndex, type_kind: &VariableTypeKind) -> Result<()> {
    match type_kind {
        VariableTypeKind::Primitive(type_name) if is_known_primitive(type_name) => Ok(()),
        VariableTypeKind::Primitive(type_name) => Err(RototoError::new(format!(
            "variable declares unknown type: {type_name}"
        ))),
        VariableTypeKind::Catalog(catalog) if index.catalogs.contains_key(catalog) => Ok(()),
        VariableTypeKind::Catalog(catalog) => Err(RototoError::new(format!(
            "variable references unknown catalog: {catalog}"
        ))),
        VariableTypeKind::Enum(id) if index.enums.contains_key(id) => Ok(()),
        VariableTypeKind::Enum(id) => Err(RototoError::new(format!(
            "variable references unknown enum: {id}"
        ))),
        VariableTypeKind::List(item) => validate_variable_type_kind(index, item),
    }
}

impl<'a> RuntimeCompiler<'a> {
    fn compile_variable_resolve(
        &self,
        index: &SemanticIndex,
        variable: &VariableNode,
        type_kind: &VariableTypeKind,
    ) -> Result<RuntimeResolution> {
        let ResolveNode::Resolve {
            method,
            default,
            rules,
            query,
            ..
        } = &variable.resolve
        else {
            return Err(RototoError::new(format!(
                "variable must contain [resolve]: {}",
                variable.id
            )));
        };

        let method_name = method
            .as_ref()
            .map(|method| method.value.as_str())
            .unwrap_or("rules");
        match method_name {
            "rules" => {
                if query.is_some() {
                    return Err(RototoError::new(
                        "query parameters (from, filter, sort, order, limit) are only valid with method = \"query\"",
                    ));
                }
                let default = present_json(default, "resolve must declare a default value")?;
                let default =
                    self.compile_variable_value(index, variable, type_kind, &default.value)?;
                let RuleCollection::Rules(rules) = rules else {
                    return Err(RototoError::new("rule must use [[resolve.rule]] tables"));
                };
                let rules = rules
                    .iter()
                    .map(|rule| self.compile_variable_rule(index, variable, type_kind, rule))
                    .collect::<Result<Vec<_>>>()?;
                Ok(RuntimeResolution::Rules { default, rules })
            }
            "query" => {
                self.compile_variable_query(index, variable, type_kind, default, rules, query)
            }
            other => Err(RototoError::new(format!(
                "unknown resolution method: {other}; supported methods are rules and query"
            ))),
        }
    }

    fn compile_variable_query(
        &self,
        index: &SemanticIndex,
        variable: &VariableNode,
        type_kind: &VariableTypeKind,
        default: &ProjectField<JsonValue>,
        rules: &RuleCollection,
        query: &Option<Box<QueryNode>>,
    ) -> Result<RuntimeResolution> {
        if !matches!(rules, RuleCollection::Rules(rules) if rules.is_empty()) {
            return Err(RototoError::new(
                "method = \"query\" must not declare [[resolve.rule]] tables",
            ));
        }
        let (catalog, single) = match type_kind {
            VariableTypeKind::Catalog(catalog) => (catalog.as_str(), true),
            VariableTypeKind::List(item) => match item.as_ref() {
                VariableTypeKind::Catalog(catalog) => (catalog.as_str(), false),
                _ => {
                    return Err(RototoError::new(
                        "method = \"query\" requires a catalog:<id> or list<catalog:<id>> type",
                    ));
                }
            },
            _ => {
                return Err(RototoError::new(
                    "method = \"query\" requires a catalog:<id> or list<catalog:<id>> type",
                ));
            }
        };
        let Some(query) = query else {
            return Err(RototoError::new(
                "method = \"query\" must declare from = \"<catalog-id>\"",
            ));
        };
        let from = match &query.from {
            ProjectField::Present(from) => from.value.as_str(),
            _ => {
                return Err(RototoError::new(
                    "method = \"query\" must declare from = \"<catalog-id>\"",
                ));
            }
        };
        if from != catalog {
            return Err(RototoError::new(format!(
                "query from ({from}) must match the variable's catalog type ({catalog})"
            )));
        }
        let filter = match &query.filter {
            Some(ProjectField::Present(filter)) => Some(filter.value.clone()),
            Some(_) => return Err(RototoError::new("query filter expression is invalid")),
            None => None,
        };
        let sort = match &query.sort {
            Some(ProjectField::Present(sort)) => Some(sort.value.clone()),
            Some(_) => return Err(RototoError::new("query sort expression is invalid")),
            None => None,
        };
        let descending = match &query.order {
            Some(ProjectField::Present(order)) => match order.value.as_str() {
                "asc" => false,
                "desc" => true,
                other => {
                    return Err(RototoError::new(format!(
                        "query order must be asc or desc, not {other}"
                    )));
                }
            },
            Some(_) => return Err(RototoError::new("query order must be a string")),
            None => false,
        };
        if descending && sort.is_none() {
            return Err(RototoError::new("query order requires a sort expression"));
        }
        let limit = match &query.limit {
            Some(ProjectField::Present(limit)) if limit.value >= 1 => Some(limit.value as usize),
            Some(_) => return Err(RototoError::new("query limit must be a positive integer")),
            None => None,
        };
        let default = match default {
            ProjectField::Present(default) => {
                Some(self.compile_variable_value(index, variable, type_kind, &default.value)?)
            }
            ProjectField::Invalid { .. } => {
                return Err(RototoError::new("resolve default is invalid"));
            }
            ProjectField::Missing { .. } => None,
        };
        let uses_context = [&filter, &sort].into_iter().flatten().any(|expression| {
            let references = expression.references();
            !references.variables.is_empty()
                || references.context_paths.iter().any(|path| !path.is_empty())
        });
        Ok(RuntimeResolution::Query(Box::new(RuntimeQuery {
            catalog: catalog.to_owned(),
            single,
            filter,
            sort,
            descending,
            limit,
            default,
            uses_context,
        })))
    }

    fn compile_variable_rule(
        &self,
        index: &SemanticIndex,
        variable: &VariableNode,
        type_kind: &VariableTypeKind,
        rule: &VariableRuleNode,
    ) -> Result<RuntimeRule> {
        if rule.invalid_shape {
            return Err(RototoError::new("rule must be a table"));
        }

        let when = match &rule.when {
            Some(ProjectField::Present(when)) => when.value.clone(),
            Some(ProjectField::Invalid { .. } | ProjectField::Missing { .. }) => {
                return Err(RototoError::new("rule when expression is invalid"));
            }
            None => return Err(RototoError::new("rule must declare when")),
        };

        let value = present_json(&rule.value, "rule must declare a value")?;
        let value = self.compile_variable_value(index, variable, type_kind, &value.value)?;

        Ok(RuntimeRule {
            index: rule.index,
            when,
            value,
        })
    }

    fn compile_variable_value(
        &self,
        index: &SemanticIndex,
        variable: &VariableNode,
        type_kind: &VariableTypeKind,
        value: &JsonValue,
    ) -> Result<RuntimeSelectedValue> {
        match type_kind {
            VariableTypeKind::Enum(_) => Ok(RuntimeSelectedValue::Literal(value.clone())),
            VariableTypeKind::Catalog(catalog) => {
                let name = value.as_str().ok_or_else(|| {
                    RototoError::new(format!(
                        "catalog-backed variable value must be a string: {}",
                        variable.id
                    ))
                })?;
                let entry = catalog_entry_value(index, catalog, name)?;
                Ok(RuntimeSelectedValue::Catalog {
                    catalog: catalog.clone(),
                    name: name.to_owned(),
                    value: entry.clone(),
                })
            }
            VariableTypeKind::List(item) => {
                if let VariableTypeKind::Catalog(catalog) = item.as_ref() {
                    let values = value.as_array().ok_or_else(|| {
                        RototoError::new(format!(
                            "list<catalog> variable value must be a list: {}",
                            variable.id
                        ))
                    })?;
                    let mut names = Vec::new();
                    let mut entries = Vec::new();
                    for value in values {
                        let name = value.as_str().ok_or_else(|| {
                            RototoError::new(format!(
                                "list<catalog> variable entries must be strings: {}",
                                variable.id
                            ))
                        })?;
                        names.push(name.to_owned());
                        entries.push(catalog_entry_value(index, catalog, name)?.clone());
                    }
                    return Ok(RuntimeSelectedValue::CatalogList {
                        catalog: catalog.clone(),
                        names,
                        value: JsonValue::Array(entries),
                    });
                }
                Ok(RuntimeSelectedValue::Literal(value.clone()))
            }
            VariableTypeKind::Primitive(_) => Ok(RuntimeSelectedValue::Literal(value.clone())),
        }
    }
}

fn catalog_entry_value<'a>(
    index: &'a SemanticIndex,
    catalog: &str,
    name: &str,
) -> Result<&'a JsonValue> {
    let entries = index
        .catalog_entries
        .get(catalog)
        .ok_or_else(|| RototoError::new(format!("catalog has no values: {catalog}")))?;
    let entry = entries.get(name).ok_or_else(|| {
        RototoError::new(format!("variable references unknown catalog value: {name}"))
    })?;
    Ok(&entry.value)
}

fn integer_field_is(field: &ProjectField<i64>, expected: i64) -> bool {
    matches!(field, ProjectField::Present(value) if value.value == expected)
}

fn present_json<'a>(
    field: &'a ProjectField<JsonValue>,
    message: &'static str,
) -> Result<&'a Spanned<JsonValue>> {
    match field {
        ProjectField::Present(value) => Ok(value),
        ProjectField::Invalid { .. } | ProjectField::Missing { .. } => {
            Err(RototoError::new(message))
        }
    }
}

fn is_known_primitive(value: &str) -> bool {
    matches!(value, "bool" | "int" | "number" | "string" | "list")
}
