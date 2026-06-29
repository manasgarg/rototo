use std::fmt;

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticEntity {
    Package,
    Qualifier,
    Variable,
    Catalog,
    CatalogEntry,
    EvaluationContext,
    EvaluationContextSample,
    Value,
    Rule,
}

#[derive(Debug, Clone, Copy)]
pub struct RuleMeta {
    pub rule: &'static str,
    pub severity: Severity,
    pub entity: DiagnosticEntity,
    pub title: &'static str,
    pub help: &'static str,
}

macro_rules! rototo_rule_severity {
    () => {
        Severity::Error
    };
    ($severity:ident) => {
        Severity::$severity
    };
}

macro_rules! rototo_rules {
    ($($variant:ident => {
        id: $id:literal,
        entity: $entity:ident,
        title: $title:literal,
        help: $help:literal $(,
        severity: $severity:ident)? $(,)?
    }),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum RototoRuleId {
            $($variant),+
        }

        impl RototoRuleId {
            pub const ALL: &'static [Self] = &[
                $(Self::$variant),+
            ];

            pub fn iter() -> impl Iterator<Item = Self> {
                Self::ALL
                    .iter()
                    .copied()
                    .filter(|rule| !rule.is_retired())
            }

            pub fn is_retired(self) -> bool {
                matches!(
                    self,
                    Self::PackageContextSchemaRef
                        | Self::PackageContextSchemaAttribute
                        | Self::PackageContextSchemaReservedField
                        | Self::PackageContextSchemaMissing
                        | Self::QualifierPredicateMissing
                        | Self::QualifierPredicateShape
                        | Self::QualifierPredicateUnknownOp
                        | Self::QualifierPredicateUnknownQualifier
                        | Self::QualifierPredicateBucket
                        | Self::QualifierPredicateValue
                        | Self::QualifierPredicateContextTypeMismatch
                        | Self::QualifierPredicateDuplicate
                        | Self::CatalogSchemaVersion
                        | Self::CatalogSchemaRef
                )
            }

            pub fn meta(self) -> RuleMeta {
                match self {
                    $(Self::$variant => RuleMeta {
                        rule: concat!("rototo/", $id),
                        severity: rototo_rule_severity!($($severity)?),
                        entity: DiagnosticEntity::$entity,
                        title: $title,
                        help: $help,
                    }),+
                }
            }
        }
    };
}

rototo_rules! {
    PackageNotFound => {
        id: "package-not-found",
        entity: Package,
        title: "Package was not found",
        help: "Pass a path to an existing rototo package directory.",
    },
    PackageManifestMissing => {
        id: "package-manifest-missing",
        entity: Package,
        title: "Package manifest is missing",
        help: "Create rototo-package.toml at the package root.",
    },
    PackageManifestParseFailed => {
        id: "package-manifest-parse-failed",
        entity: Package,
        title: "Package manifest could not be parsed",
        help: "Fix the TOML syntax in rototo-package.toml.",
    },
    PackageManifestSchemaFailed => {
        id: "package-manifest-schema-failed",
        entity: Package,
        title: "Package manifest does not match schema",
        help: "Declare schema_version = 1 and optional extends in rototo-package.toml.",
    },
    TraceWhenMissing => {
        id: "trace-when-missing",
        entity: Package,
        title: "Trace policy is missing when",
        help: "Each [[trace]] policy must declare when = \"<expression>\".",
    },
    TraceWhenShape => {
        id: "trace-when-shape",
        entity: Package,
        title: "Trace policy when expression is invalid",
        help: "A [[trace]] when must be a string holding a valid boolean expression.",
    },
    TraceWhenInvalidReference => {
        id: "trace-when-invalid-reference",
        entity: Package,
        title: "Trace policy when references an unknown identifier",
        help: "Trace when reads context.<path>, env.qualifier[\"<id>\"], env.now, and env.resolving.variable / env.resolving.qualifier.",
    },
    PackageContextSchemaRef => {
        id: "package-context-schema-ref",
        entity: Package,
        title: "Evaluation context schema is invalid",
        help: "Retired. Use evaluation-contexts/<id>.schema.json for evaluation context validation.",
    },
    PackageContextSchemaAttribute => {
        id: "package-context-schema-attribute",
        entity: Package,
        title: "Qualifier context attribute is not declared by the evaluation context schema",
        help: "Declare the context path in the package context schema or update the qualifier.",
    },
    PackageContextSchemaReservedField => {
        id: "package-context-schema-reserved-field",
        entity: Package,
        title: "Evaluation context schema declares a reserved field",
        help: "Rename the evaluation context field; qualifier is reserved for qualifier.<id> predicate references.",
    },
    PackageContextSchemaMissing => {
        id: "package-context-schema-missing",
        entity: Package,
        title: "Evaluation context schema is missing",
        help: "Retired. Add evaluation-contexts/<id>.schema.json for evaluation context validation.",
        severity: Warning,
    },
    EvaluationContextSchemaInvalid => {
        id: "evaluation-context-schema-invalid",
        entity: EvaluationContext,
        title: "Evaluation context schema is invalid",
        help: "Fix the evaluation-contexts/<id>.schema.json file so it parses and compiles as JSON Schema.",
    },
    EvaluationContextReservedField => {
        id: "evaluation-context-reserved-field",
        entity: EvaluationContext,
        title: "Evaluation context schema declares a reserved field",
        help: "Rename the evaluation context field; qualifier is reserved for qualifier.<id> predicate references.",
    },
    EvaluationContextSampleSchemaMismatch => {
        id: "evaluation-context-sample-schema-mismatch",
        entity: EvaluationContextSample,
        title: "Evaluation context sample does not match its schema",
        help: "Update the evaluation context sample so it validates against the owning evaluation context schema.",
    },
    EvaluationContextSampleShape => {
        id: "evaluation-context-sample-shape",
        entity: EvaluationContextSample,
        title: "Evaluation context sample is invalid",
        help: "Evaluation context samples must parse as JSON objects.",
    },
    QualifierParseFailed => {
        id: "qualifier-parse-failed",
        entity: Qualifier,
        title: "Qualifier TOML file could not be parsed",
        help: "Fix the TOML syntax so rototo can parse the qualifier file.",
    },
    QualifierSchemaVersion => {
        id: "qualifier-schema-version",
        entity: Qualifier,
        title: "Qualifier schema version is missing or unsupported",
        help: "Declare schema_version = 1 in the qualifier file.",
    },
    QualifierWhenMissing => {
        id: "qualifier-when-missing",
        entity: Qualifier,
        title: "Qualifier condition is missing",
        help: "Add an expression with when = \"...\".",
    },
    QualifierWhenShape => {
        id: "qualifier-when-shape",
        entity: Qualifier,
        title: "Qualifier condition is invalid",
        help: "Use when = \"...\" with a valid expression.",
    },
    QualifierWhenUnknownQualifier => {
        id: "qualifier-when-unknown-qualifier",
        entity: Qualifier,
        title: "Qualifier condition references an unknown qualifier",
        help: "Create the referenced qualifier or update the qualifier reference in the when expression.",
    },
    QualifierWhenUndeclaredContextPath => {
        id: "qualifier-when-undeclared-context-path",
        entity: Qualifier,
        title: "Qualifier when expression references an undeclared context path",
        help: "Declare the attribute in an evaluation context schema under evaluation-contexts/<id>.schema.json, or fix the path in the when expression.",
    },
    QualifierWhenInvalidReference => {
        id: "qualifier-when-invalid-reference",
        entity: Qualifier,
        title: "Qualifier when expression references an identifier rototo does not provide",
        help: "Expressions read context.<path>, env.qualifier[\"<id>\"], and env.now. Reference qualifiers as env.qualifier[\"<id>\"].",
    },
    QualifierWhenContextPathTypeMismatch => {
        id: "qualifier-when-context-path-type-mismatch",
        entity: Qualifier,
        title: "Qualifier when expression uses a context path with the wrong type",
        help: "Declare the context attribute with a type that matches how the when expression uses it, or change the comparison to match the declared type.",
    },
    QualifierPredicateMissing => {
        id: "qualifier-predicate-missing",
        entity: Qualifier,
        title: "Qualifier predicate is missing",
        help: "Retired. Use when = \"...\" with a valid expression.",
    },
    QualifierPredicateShape => {
        id: "qualifier-predicate-shape",
        entity: Qualifier,
        title: "Qualifier predicate has the wrong shape",
        help: "Retired. Use when = \"...\" with a valid expression.",
    },
    QualifierPredicateUnknownOp => {
        id: "qualifier-predicate-unknown-op",
        entity: Qualifier,
        title: "Qualifier predicate uses an unknown operator",
        help: "Use a supported predicate operator such as eq, in, gte, prefix, regex, semver, time_between, exists, between, contains_any, cidr, or bucket.",
    },
    QualifierPredicateUnknownQualifier => {
        id: "qualifier-predicate-unknown-qualifier",
        entity: Qualifier,
        title: "Qualifier predicate references an unknown qualifier",
        help: "Create the referenced qualifier or update the qualifier.<id> reference.",
    },
    QualifierPredicateBucket => {
        id: "qualifier-predicate-bucket",
        entity: Qualifier,
        title: "Bucket predicate is invalid",
        help: "Bucket predicates need salt and range = [start, end] with 0 <= start < end <= 10000.",
    },
    QualifierPredicateValue => {
        id: "qualifier-predicate-value",
        entity: Qualifier,
        title: "Qualifier predicate value is invalid",
        help: "Add a value with the shape required by the predicate operator.",
    },
    QualifierPredicateContextTypeMismatch => {
        id: "qualifier-predicate-context-type-mismatch",
        entity: Qualifier,
        title: "Qualifier predicate does not match the evaluation context schema type",
        help: "Update the predicate operator or value so it matches the context schema field type.",
    },
    QualifierNoCompatibleEvaluationContext => {
        id: "qualifier-no-compatible-evaluation-context",
        entity: Qualifier,
        title: "Qualifier has no compatible evaluation context",
        help: "Add an evaluation context schema under evaluation-contexts/<id>.schema.json that declares the qualifier's context attributes, or update the qualifier predicates.",
    },
    QualifierPredicateDuplicate => {
        id: "qualifier-predicate-duplicate",
        entity: Qualifier,
        title: "Qualifier predicate is duplicated",
        help: "Remove duplicate predicates that do not change qualifier behavior.",
        severity: Warning,
    },
    QualifierCycle => {
        id: "qualifier-cycle",
        entity: Qualifier,
        title: "Qualifier references form a cycle",
        help: "Remove the qualifier reference cycle so qualifier evaluation can terminate.",
    },
    QualifierUnreferenced => {
        id: "qualifier-unreferenced",
        entity: Qualifier,
        title: "Qualifier is not referenced",
        help: "Reference the qualifier from another qualifier or variable rule, or remove it.",
        severity: Warning,
    },
    QualifierUnreachable => {
        id: "qualifier-unreachable",
        entity: Qualifier,
        title: "Qualifier cannot affect resolution",
        help: "Reference the qualifier from a reachable variable rule path, or remove it.",
        severity: Warning,
    },
    VariableParseFailed => {
        id: "variable-parse-failed",
        entity: Variable,
        title: "Variable TOML file could not be parsed",
        help: "Fix the TOML syntax so rototo can parse the variable file.",
    },
    VariableSchemaVersion => {
        id: "variable-schema-version",
        entity: Variable,
        title: "Variable schema version is missing or unsupported",
        help: "Declare schema_version = 1 in the variable file.",
    },
    VariableTypeSource => {
        id: "variable-type-source",
        entity: Variable,
        title: "Variable type source is invalid",
        help: "Declare type as a primitive type or catalog:<catalog-id>.",
    },
    VariableUnknownType => {
        id: "variable-unknown-type",
        entity: Variable,
        title: "Variable type is unknown",
        help: "Use one of bool, int, number, string, list, or catalog:<catalog-id>.",
    },
    VariableUnknownCatalog => {
        id: "variable-unknown-catalog",
        entity: Variable,
        title: "Variable references an unknown catalog",
        help: "Create the referenced catalog or update the catalog type.",
    },
    VariableValuesDisallowed => {
        id: "variable-values-disallowed",
        entity: Variable,
        title: "Variable values are not allowed",
        help: "Remove [values] and put literal values directly under [resolve].",
    },
    VariableUnknownValue => {
        id: "variable-unknown-value",
        entity: Variable,
        title: "Variable references an unknown catalog value",
        help: "Create the referenced catalog value or update the reference.",
    },
    VariableValueTypeMismatch => {
        id: "variable-value-type-mismatch",
        entity: Variable,
        title: "Variable value does not match type",
        help: "Update the value so it matches the declared primitive type.",
    },
    CatalogParseFailed => {
        id: "catalog-parse-failed",
        entity: Catalog,
        title: "Catalog schema file could not be parsed",
        help: "Fix the JSON syntax so rototo can parse the catalog schema file.",
    },
    CatalogEntryParseFailed => {
        id: "catalog-entry-parse-failed",
        entity: CatalogEntry,
        title: "Catalog value TOML file could not be parsed",
        help: "Fix the TOML syntax so rototo can parse the catalog value file.",
    },
    CatalogSchemaVersion => {
        id: "catalog-schema-version",
        entity: Catalog,
        title: "Catalog schema version is missing or unsupported",
        help: "Declare schema_version = 1 in the catalog file.",
    },
    CatalogSchemaRef => {
        id: "catalog-schema-ref",
        entity: Catalog,
        title: "Catalog schema reference is invalid",
        help: "Point schema to a readable valid JSON Schema file.",
    },
    CatalogSchemaInvalid => {
        id: "catalog-schema-invalid",
        entity: Catalog,
        title: "Catalog schema is invalid",
        help: "Update catalogs/<id>.schema.json so it is a valid JSON Schema.",
    },
    CatalogEntrySchemaMismatch => {
        id: "catalog-entry-schema-mismatch",
        entity: CatalogEntry,
        title: "Catalog value does not match schema",
        help: "Update the catalog value so it matches the catalog JSON Schema.",
    },
    CatalogEntryUnknownReference => {
        id: "catalog-entry-unknown-reference",
        entity: CatalogEntry,
        title: "Catalog value references an invalid catalog entry",
        help: "Create the referenced catalog entry, fix the pointer, or update the x-rototo-catalog-ref field.",
    },
    VariableResolveMissingDefault => {
        id: "variable-resolve-missing-default",
        entity: Variable,
        title: "Variable resolve default is missing",
        help: "Add [resolve].default with a value reference.",
    },
    VariableResolveShape => {
        id: "variable-resolve-shape",
        entity: Variable,
        title: "Variable resolve block is invalid",
        help: "Resolve blocks must be tables with default and optional rule references.",
    },
    VariableRuleShape => {
        id: "variable-rule-shape",
        entity: Variable,
        title: "Variable rule is invalid",
        help: "Rules must be tables with qualifier and value references.",
    },
    VariableRuleUnknownQualifier => {
        id: "variable-rule-unknown-qualifier",
        entity: Variable,
        title: "Variable rule references an unknown qualifier",
        help: "Create the referenced qualifier or update the rule.",
    },
    VariableRuleUndeclaredContextPath => {
        id: "variable-rule-undeclared-context-path",
        entity: Rule,
        title: "Variable rule references an undeclared context path",
        help: "Declare the attribute in an evaluation context schema under evaluation-contexts/<id>.schema.json, or fix the path in the rule when/query expression.",
    },
    VariableRuleInvalidReference => {
        id: "variable-rule-invalid-reference",
        entity: Rule,
        title: "Variable rule references an identifier rototo does not provide",
        help: "Expressions read context.<path>, entry.<path> (in queries), env.qualifier[\"<id>\"], and env.now. Reference qualifiers as env.qualifier[\"<id>\"].",
    },
    VariableRuleContextPathTypeMismatch => {
        id: "variable-rule-context-path-type-mismatch",
        entity: Rule,
        title: "Variable rule uses a context path with the wrong type",
        help: "Declare the context attribute with a type that matches how the rule uses it, or change the comparison to match the declared type.",
    },
    VariableRuleShadowed => {
        id: "variable-rule-shadowed",
        entity: Rule,
        title: "Variable rule is shadowed",
        help: "Remove the later duplicate qualifier rule or reorder the resolve rules.",
        severity: Warning,
    },
    VariableRuleSelectsDefaultValue => {
        id: "variable-rule-selects-default-value",
        entity: Rule,
        title: "Variable rule selects the default value",
        help: "Remove the rule or update it to select a value that differs from the resolve default.",
        severity: Warning,
    },
    VariableEvaluationContextConflict => {
        id: "variable-evaluation-context-conflict",
        entity: Variable,
        title: "Variable rules require incompatible evaluation contexts",
        help: "Use rule conditions that share at least one compatible evaluation context, or split the behavior into separate variables.",
    },
    EvaluationContextParseFailed => {
        id: "evaluation-context-parse-failed",
        entity: EvaluationContext,
        title: "Evaluation context schema JSON file could not be parsed",
        help: "Fix the JSON syntax so rototo can parse the evaluation context schema file.",
    },
    EvaluationContextSampleParseFailed => {
        id: "evaluation-context-sample-parse-failed",
        entity: EvaluationContextSample,
        title: "Evaluation context sample JSON file could not be parsed",
        help: "Fix the JSON syntax so rototo can parse the evaluation context sample file.",
    },
    CustomLintFailed => {
        id: "custom-lint-failed",
        entity: Package,
        title: "Custom lint execution failed",
        help: "Update the Lua lint file or target data so custom lint can run.",
    },
    CustomLintRegistrationInvalid => {
        id: "custom-lint-registration-invalid",
        entity: Package,
        title: "Custom lint registration is invalid",
        help: "Register custom lint with an allowed stage, entity, field, rule metadata, and handler.",
    },
    CustomLintRuleConflict => {
        id: "custom-lint-rule-conflict",
        entity: Package,
        title: "Custom lint rule metadata conflicts",
        help: "Use identical title and help text for repeated custom rule declarations.",
    },
    CustomLintFileUnregistered => {
        id: "custom-lint-file-unregistered",
        entity: Package,
        title: "Custom lint file registers no handlers",
        help: "Register at least one handler from the Lua file or remove the file.",
        severity: Warning,
    },
    CustomLintRegistrationDuplicate => {
        id: "custom-lint-registration-duplicate",
        entity: Package,
        title: "Custom lint registration is duplicated",
        help: "Remove duplicate custom lint registrations so handlers run once per target.",
        severity: Warning,
    },
    SchemaUiUnknownWidget => {
        id: "schema-ui-unknown-widget",
        entity: Catalog,
        title: "UI widget hint names an unknown widget",
        help: "Use a widget from the x-rototo-ui vocabulary: color, slider, textarea.",
        severity: Warning,
    },
    SchemaUiWidgetTypeMismatch => {
        id: "schema-ui-widget-type-mismatch",
        entity: Catalog,
        title: "UI widget hint does not fit the property type",
        help: "Pick a widget that supports the property's declared type, or change the type.",
        severity: Warning,
    },
    SchemaUiWidgetParams => {
        id: "schema-ui-widget-params",
        entity: Catalog,
        title: "UI widget hint parameters are invalid",
        help: "Fix the x-rototo-ui object: declare a widget string, use only the widget's parameters, and give sliders bounds.",
        severity: Warning,
    },
}

impl Serialize for RototoRuleId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.meta().rule)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CustomRuleId(String);

impl CustomRuleId {
    pub fn parse(rule: impl AsRef<str>) -> std::result::Result<Self, CustomRuleIdError> {
        let rule = rule.as_ref();
        let Some((authority, id)) = rule.split_once('/') else {
            return Err(CustomRuleIdError::new(
                "custom rule id must use <authority>/<rule-id>",
            ));
        };
        if id.contains('/') {
            return Err(CustomRuleIdError::new(
                "custom rule id must contain exactly one slash",
            ));
        }
        if authority == "rototo" {
            return Err(CustomRuleIdError::new(
                "rototo is reserved for built-in diagnostic rules",
            ));
        }
        if !valid_rule_segment(authority) || !valid_rule_segment(id) {
            return Err(CustomRuleIdError::new(
                "rule id segments must use lowercase ASCII letters, digits, and hyphen",
            ));
        }
        Ok(Self(rule.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CustomRuleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for CustomRuleId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct CustomRuleIdError {
    message: String,
}

impl CustomRuleIdError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CustomRuleIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CustomRuleIdError {}

fn valid_rule_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomRuleDefinition {
    pub rule: CustomRuleId,
    pub severity: Severity,
    pub title: String,
    pub help: String,
}

impl CustomRuleDefinition {
    pub fn new(rule: CustomRuleId, title: impl Into<String>, help: impl Into<String>) -> Self {
        Self::with_severity(rule, Severity::Error, title, help)
    }

    pub fn with_severity(
        rule: CustomRuleId,
        severity: Severity,
        title: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        Self {
            rule,
            severity,
            title: title.into(),
            help: help.into(),
        }
    }

    pub fn same_metadata(&self, other: &Self) -> bool {
        self.severity == other.severity && self.title == other.title && self.help == other.help
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticRule {
    Rototo(RototoRuleId),
    Custom(CustomRuleId),
}

impl DiagnosticRule {
    pub fn as_string(&self) -> String {
        match self {
            Self::Rototo(rule) => rule.meta().rule.to_owned(),
            Self::Custom(rule) => rule.as_str().to_owned(),
        }
    }
}

impl Serialize for DiagnosticRule {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Rototo(rule) => rule.serialize(serializer),
            Self::Custom(rule) => rule.serialize(serializer),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DiagnosticCatalogEntry {
    pub rule: String,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<DiagnosticEntity>,
    pub title: String,
    pub help: String,
}

impl DiagnosticCatalogEntry {
    pub fn from_rototo(rule: RototoRuleId) -> Self {
        let meta = rule.meta();
        Self {
            rule: meta.rule.to_owned(),
            severity: meta.severity,
            entity: Some(meta.entity),
            title: meta.title.to_owned(),
            help: meta.help.to_owned(),
        }
    }

    pub fn from_custom(definition: &CustomRuleDefinition) -> Self {
        Self {
            rule: definition.rule.as_str().to_owned(),
            severity: definition.severity,
            entity: None,
            title: definition.title.clone(),
            help: definition.help.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DocId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LintStage {
    Discover,
    Parse,
    Project,
    Register,
    Reference,
    Value,
    Graph,
    Policy,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticEntity {
    Package,
    Manifest,
    Qualifier {
        id: String,
    },
    Predicate {
        qualifier: String,
        index: usize,
    },
    Variable {
        id: String,
    },
    Catalog {
        id: String,
    },
    CatalogEntry {
        catalog: String,
        key: String,
    },
    EvaluationContext {
        id: String,
    },
    EvaluationContextSample {
        evaluation_context: String,
        key: String,
    },
    Value {
        variable: String,
        key: String,
    },
    Rule {
        variable: String,
        index: usize,
    },
    CustomLint {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticField {
    PackageExtends,
    SchemaVersion,
    Description,
    QualifierWhen,
    QualifierPredicates,
    PredicateAttribute,
    PredicateOp,
    PredicateNot,
    PredicateValue,
    PredicateSalt,
    PredicateRange,
    VariableType,
    VariableSchema,
    VariableValues,
    VariableResolve,
    VariableResolveDefault,
    VariableRuleWhen,
    VariableRuleQuery,
    VariableRuleValue,
    Value,
    ValueJsonPath { path: Vec<String> },
    SchemaJson,
    SchemaJsonPath { path: Vec<String> },
    EvaluationContextSample,
    CatalogEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SemanticTarget {
    pub entity: SemanticEntity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<SemanticField>,
}

impl SemanticTarget {
    pub fn entity(entity: SemanticEntity) -> Self {
        Self {
            entity,
            field: None,
        }
    }

    pub fn field(entity: SemanticEntity, field: SemanticField) -> Self {
        Self {
            entity,
            field: Some(field),
        }
    }
}

impl From<SemanticEntity> for SemanticTarget {
    fn from(entity: SemanticEntity) -> Self {
        Self::entity(entity)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SourcePosition {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl TextRange {
    pub(crate) fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceSpan {
    pub(crate) doc: DocId,
    pub(crate) range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLocationKind {
    Span,
    Document,
    PackageRoot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticLocation {
    #[serde(skip)]
    pub kind: DiagnosticLocationKind,
    #[serde(skip)]
    pub doc: Option<DocId>,
    #[serde(skip)]
    pub(crate) span: Option<SourceSpan>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<SourceRange>,
}

impl DiagnosticLocation {
    pub fn span(doc: DocId, path: impl Into<String>, range: SourceRange) -> Self {
        Self {
            kind: DiagnosticLocationKind::Span,
            doc: Some(doc),
            span: None,
            path: path.into(),
            range: Some(range),
        }
    }

    pub(crate) fn source_span(
        doc: DocId,
        path: impl Into<String>,
        text_range: TextRange,
        rendered_range: SourceRange,
    ) -> Self {
        Self {
            kind: DiagnosticLocationKind::Span,
            doc: Some(doc),
            span: Some(SourceSpan {
                doc,
                range: text_range,
            }),
            path: path.into(),
            range: Some(rendered_range),
        }
    }

    pub fn document(doc: DocId, path: impl Into<String>) -> Self {
        Self {
            kind: DiagnosticLocationKind::Document,
            doc: Some(doc),
            span: None,
            path: path.into(),
            range: None,
        }
    }

    pub fn package_root(path: impl Into<String>) -> Self {
        Self {
            kind: DiagnosticLocationKind::PackageRoot,
            doc: None,
            span: None,
            path: path.into(),
            range: None,
        }
    }

    pub fn doc(&self) -> Option<DocId> {
        self.doc
    }

    pub(crate) fn byte_start(&self) -> Option<usize> {
        self.span.map(|span| span.range.start)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelatedLocation {
    pub location: DiagnosticLocation,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LintDiagnostic {
    pub rule: DiagnosticRule,
    pub severity: Severity,
    pub stage: LintStage,
    pub target: SemanticTarget,
    pub message: String,
    pub help: String,
    #[serde(rename = "location")]
    pub primary: DiagnosticLocation,
    pub related: Vec<RelatedLocation>,
}

impl LintDiagnostic {
    pub fn rototo(
        rule: RototoRuleId,
        stage: LintStage,
        target: impl Into<SemanticTarget>,
        primary: DiagnosticLocation,
        message: impl Into<String>,
    ) -> Self {
        let meta = rule.meta();
        Self {
            rule: DiagnosticRule::Rototo(rule),
            severity: meta.severity,
            stage,
            target: target.into(),
            message: message.into(),
            help: meta.help.to_owned(),
            primary,
            related: Vec::new(),
        }
    }

    pub fn custom(
        definition: &CustomRuleDefinition,
        stage: LintStage,
        target: impl Into<SemanticTarget>,
        primary: DiagnosticLocation,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule: DiagnosticRule::Custom(definition.rule.clone()),
            severity: definition.severity,
            stage,
            target: target.into(),
            message: message.into(),
            help: definition.help.clone(),
            primary,
            related: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub rule: DiagnosticRule,
    pub severity: Severity,
    pub path: String,
    pub message: String,
    pub help: String,
}

impl Diagnostic {
    pub fn rototo(rule: RototoRuleId, path: impl Into<String>, message: impl Into<String>) -> Self {
        let meta = rule.meta();
        Self {
            rule: DiagnosticRule::Rototo(rule),
            severity: meta.severity,
            path: path.into(),
            message: message.into(),
            help: meta.help.to_owned(),
        }
    }

    pub fn custom(
        definition: &CustomRuleDefinition,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule: DiagnosticRule::Custom(definition.rule.clone()),
            severity: definition.severity,
            path: path.into(),
            message: message.into(),
            help: definition.help.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "error" => Some(Self::Error),
            "warning" => Some(Self::Warning),
            _ => None,
        }
    }
}
