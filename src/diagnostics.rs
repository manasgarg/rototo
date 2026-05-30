use std::fmt;

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticEntity {
    Workspace,
    Qualifier,
    Variable,
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

macro_rules! rototo_rules {
    ($($variant:ident => {
        id: $id:literal,
        entity: $entity:ident,
        title: $title:literal,
        help: $help:literal $(,)?
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
                        severity: Severity::Error,
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
    QualifierMissingTable => {
        id: "qualifier-missing-table",
        entity: Qualifier,
        title: "Qualifier table is missing",
        help: "Add a [qualifier] table.",
    },
    QualifierPredicateMissing => {
        id: "qualifier-predicate-missing",
        entity: Qualifier,
        title: "Qualifier predicate is missing",
        help: "Add at least one [[qualifier.predicate]] table.",
    },
    QualifierPredicateShape => {
        id: "qualifier-predicate-shape",
        entity: Qualifier,
        title: "Qualifier predicate has the wrong shape",
        help: "Use [[qualifier.predicate]] tables with attribute, op, and value fields.",
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
    VariableMissingTable => {
        id: "variable-missing-table",
        entity: Variable,
        title: "Variable table is missing",
        help: "Add a [variable] table.",
    },
    VariableTypeOrSchema => {
        id: "variable-type-or-schema",
        entity: Variable,
        title: "Variable must declare exactly one type source",
        help: "Declare exactly one of type or schema under [variable].",
    },
    VariableUnknownType => {
        id: "variable-unknown-type",
        entity: Variable,
        title: "Variable type is unknown",
        help: "Use one of bool, int, number, string, or list.",
    },
    VariableLintShape => {
        id: "variable-lint-shape",
        entity: Variable,
        title: "Variable custom lint declaration is invalid",
        help: "Use [variable.lint] with a string path field and declared custom rules.",
    },
    VariableValuesMissing => {
        id: "variable-values-missing",
        entity: Variable,
        title: "Variable values are missing",
        help: "Add [variable.values] entries or external value files.",
    },
    VariableUnknownValue => {
        id: "variable-unknown-value",
        entity: Variable,
        title: "Variable references an unknown value",
        help: "Create the referenced value under [variable.values] or update the reference.",
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
        help: "Add [variable.env._] with a value reference.",
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
        entity: Variable,
        title: "Variable custom lint execution failed",
        help: "Update the variable or its Lua lint rule so custom lint can run.",
    },
    CustomLintInvalidRule => {
        id: "custom-lint-invalid-rule",
        entity: Variable,
        title: "Custom lint rule id is invalid",
        help: "Declare and emit rule ids as <authority>/<rule-id>; rototo is reserved.",
    },
    CustomLintUnknownRule => {
        id: "custom-lint-unknown-rule",
        entity: Variable,
        title: "Custom lint emitted an undeclared rule",
        help: "Declare the custom rule in [variable.lint] or update the Lua diagnostic.",
    },
    CustomLintRuleConflict => {
        id: "custom-lint-rule-conflict",
        entity: Variable,
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
    pub title: String,
    pub help: String,
}

impl CustomRuleDefinition {
    pub fn new(rule: CustomRuleId, title: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            rule,
            title: title.into(),
            help: help.into(),
        }
    }

    pub fn same_metadata(&self, other: &Self) -> bool {
        self.title == other.title && self.help == other.help
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
    pub entity: DiagnosticEntity,
    pub title: String,
    pub help: String,
}

impl DiagnosticCatalogEntry {
    pub fn from_rototo(rule: RototoRuleId) -> Self {
        let meta = rule.meta();
        Self {
            rule: meta.rule.to_owned(),
            severity: meta.severity,
            entity: meta.entity,
            title: meta.title.to_owned(),
            help: meta.help.to_owned(),
        }
    }

    pub fn from_custom(definition: &CustomRuleDefinition) -> Self {
        Self {
            rule: definition.rule.as_str().to_owned(),
            severity: Severity::Error,
            entity: DiagnosticEntity::Variable,
            title: definition.title.clone(),
            help: definition.help.clone(),
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
            severity: Severity::Error,
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
}
