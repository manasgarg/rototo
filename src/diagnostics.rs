use std::fmt;

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticEntity {
    Workspace,
    Qualifier,
    Variable,
    Value,
    Rule,
    Schema,
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
                Self::ALL.iter().copied()
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
    WorkspaceNotFound => {
        id: "workspace-not-found",
        entity: Workspace,
        title: "Workspace was not found",
        help: "Pass a path to an existing rototo workspace directory.",
    },
    WorkspaceManifestMissing => {
        id: "workspace-manifest-missing",
        entity: Workspace,
        title: "Workspace manifest is missing",
        help: "Create rototo-workspace.toml at the workspace root.",
    },
    WorkspaceManifestParseFailed => {
        id: "workspace-manifest-parse-failed",
        entity: Workspace,
        title: "Workspace manifest could not be parsed",
        help: "Fix the TOML syntax in rototo-workspace.toml.",
    },
    WorkspaceManifestSchemaFailed => {
        id: "workspace-manifest-schema-failed",
        entity: Workspace,
        title: "Workspace manifest does not match schema",
        help: "Declare schema_version = 1 and [environments].values in rototo-workspace.toml.",
    },
    WorkspaceContextSchemaRef => {
        id: "workspace-context-schema-ref",
        entity: Workspace,
        title: "Resolve context schema reference is invalid",
        help: "Point [context].schema to a readable valid JSON Schema file.",
    },
    WorkspaceContextSchemaAttribute => {
        id: "workspace-context-schema-attribute",
        entity: Workspace,
        title: "Qualifier context attribute is not declared by the resolve context schema",
        help: "Declare the context path in the workspace context schema or update the qualifier.",
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
    QualifierPredicateMissing => {
        id: "qualifier-predicate-missing",
        entity: Qualifier,
        title: "Qualifier predicate is missing",
        help: "Add at least one [[predicate]] table.",
    },
    QualifierPredicateShape => {
        id: "qualifier-predicate-shape",
        entity: Qualifier,
        title: "Qualifier predicate has the wrong shape",
        help: "Use [[predicate]] tables with attribute, op, and value fields.",
    },
    QualifierPredicateUnknownOp => {
        id: "qualifier-predicate-unknown-op",
        entity: Qualifier,
        title: "Qualifier predicate uses an unknown operator",
        help: "Use one of eq, neq, in, not_in, gt, gte, lt, lte, or bucket.",
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
    VariableTypeOrSchema => {
        id: "variable-type-or-schema",
        entity: Variable,
        title: "Variable must declare exactly one type source",
        help: "Declare exactly one of type or schema.",
    },
    VariableUnknownType => {
        id: "variable-unknown-type",
        entity: Variable,
        title: "Variable type is unknown",
        help: "Use one of bool, int, number, string, or list.",
    },
    VariableValuesMissing => {
        id: "variable-values-missing",
        entity: Variable,
        title: "Variable values are missing",
        help: "Add [values] entries or external value files.",
    },
    VariableUnknownValue => {
        id: "variable-unknown-value",
        entity: Variable,
        title: "Variable references an unknown value",
        help: "Create the referenced value under [values] or update the reference.",
    },
    VariableValueTypeMismatch => {
        id: "variable-value-type-mismatch",
        entity: Variable,
        title: "Variable value does not match type",
        help: "Update the value so it matches the declared primitive type.",
    },
    VariableValueSchemaMismatch => {
        id: "variable-value-schema-mismatch",
        entity: Variable,
        title: "Variable value does not match schema",
        help: "Update the value so it matches the variable JSON Schema.",
    },
    VariableSchemaRef => {
        id: "variable-schema-ref",
        entity: Variable,
        title: "Variable schema reference is invalid",
        help: "Point schema to a readable valid JSON Schema file.",
    },
    VariableEnvMissingDefault => {
        id: "variable-env-missing-default",
        entity: Variable,
        title: "Variable default environment is missing",
        help: "Add [env._] with a value reference.",
    },
    VariableUnknownEnvironment => {
        id: "variable-unknown-environment",
        entity: Variable,
        title: "Variable references an undeclared environment",
        help: "Declare the environment in [environments].values or remove the environment block.",
    },
    VariableEnvShape => {
        id: "variable-env-shape",
        entity: Variable,
        title: "Variable environment block is invalid",
        help: "Environment blocks must be tables with a value reference.",
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
    VariableRuleShadowed => {
        id: "variable-rule-shadowed",
        entity: Rule,
        title: "Variable rule is shadowed",
        help: "Remove the later duplicate qualifier rule or reorder the environment rules.",
        severity: Warning,
    },
    VariableValueUnused => {
        id: "variable-value-unused",
        entity: Value,
        title: "Variable value is not used",
        help: "Reference the value from an environment default or rule, or remove it.",
        severity: Warning,
    },
    VariableExternalValuesLoadFailed => {
        id: "variable-external-values-load-failed",
        entity: Variable,
        title: "Variable external values could not be loaded",
        help: "Fix the external values directory or the referenced value files.",
    },
    VariableExternalValueParseFailed => {
        id: "variable-external-value-parse-failed",
        entity: Variable,
        title: "Variable external value file could not be parsed",
        help: "Fix the TOML syntax in the external variable value file.",
    },
    VariableExternalValueDuplicate => {
        id: "variable-external-value-duplicate",
        entity: Variable,
        title: "Variable external value duplicates an existing value",
        help: "Ensure each variable value key is declared exactly once.",
    },
    CustomLintFailed => {
        id: "custom-lint-failed",
        entity: Workspace,
        title: "Custom lint execution failed",
        help: "Update the Lua lint file or target data so custom lint can run.",
    },
    CustomLintRegistrationInvalid => {
        id: "custom-lint-registration-invalid",
        entity: Workspace,
        title: "Custom lint registration is invalid",
        help: "Register custom lint with an allowed stage, entity, field, rule, and handler.",
    },
    CustomLintRuleShape => {
        id: "custom-lint-rule-shape",
        entity: Workspace,
        title: "Custom lint rule declaration is invalid",
        help: "Declare custom lint rules with id, title, and help.",
    },
    CustomLintInvalidRule => {
        id: "custom-lint-invalid-rule",
        entity: Workspace,
        title: "Custom lint rule id is invalid",
        help: "Declare and emit rule ids as <authority>/<rule-id>; rototo is reserved.",
    },
    CustomLintUnknownRule => {
        id: "custom-lint-unknown-rule",
        entity: Workspace,
        title: "Custom lint registration references an undeclared rule",
        help: "Declare the custom rule in the workspace manifest or update the Lua registration.",
    },
    CustomLintRuleConflict => {
        id: "custom-lint-rule-conflict",
        entity: Workspace,
        title: "Custom lint rule metadata conflicts",
        help: "Use identical title and help text for repeated custom rule declarations.",
    },
    SchemaParseFailed => {
        id: "schema-parse-failed",
        entity: Schema,
        title: "JSON Schema file could not be parsed",
        help: "Fix the JSON syntax so rototo can parse the schema file.",
    },
    SchemaInvalid => {
        id: "schema-invalid",
        entity: Schema,
        title: "JSON Schema is invalid",
        help: "Update the schema file so it is valid JSON Schema.",
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
pub enum EntityId {
    Workspace,
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
    Value {
        variable: String,
        key: String,
    },
    EnvironmentBlock {
        variable: String,
        environment: String,
    },
    Rule {
        variable: String,
        environment: String,
        index: usize,
    },
    CustomLint {
        path: String,
    },
    Schema {
        path: String,
    },
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
    WorkspaceRoot,
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

    pub fn workspace_root(path: impl Into<String>) -> Self {
        Self {
            kind: DiagnosticLocationKind::WorkspaceRoot,
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
    pub entity: EntityId,
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
        entity: EntityId,
        primary: DiagnosticLocation,
        message: impl Into<String>,
    ) -> Self {
        let meta = rule.meta();
        Self {
            rule: DiagnosticRule::Rototo(rule),
            severity: meta.severity,
            stage,
            entity,
            message: message.into(),
            help: meta.help.to_owned(),
            primary,
            related: Vec::new(),
        }
    }

    pub fn custom(
        definition: &CustomRuleDefinition,
        stage: LintStage,
        entity: EntityId,
        primary: DiagnosticLocation,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule: DiagnosticRule::Custom(definition.rule.clone()),
            severity: definition.severity,
            stage,
            entity,
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
