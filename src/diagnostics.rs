use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct DiagnosticSpec {
    pub code: &'static str,
    pub title: &'static str,
    pub help: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct LintRule {
    pub id: &'static str,
    pub title: &'static str,
    pub help: &'static str,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticCatalogEntry {
    pub code: String,
    pub title: String,
    pub help: String,
    pub source: DiagnosticSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

pub const WORKSPACE_TOML_FILE_PARSE_FAILED: DiagnosticSpec = DiagnosticSpec {
    code: "rototo/workspace-toml-file-parse-failed",
    title: "Workspace TOML file could not be parsed",
    help: "Fix the TOML syntax in the referenced workspace file.",
};

pub const WORKSPACE_TOML_FILE_INVALID: DiagnosticSpec = DiagnosticSpec {
    code: "rototo/workspace-toml-file-invalid",
    title: "Workspace TOML file is invalid",
    help: "Update the referenced qualifier or variable TOML file so it matches rototo workspace rules.",
};

pub const JSON_SCHEMA_FILE_PARSE_FAILED: DiagnosticSpec = DiagnosticSpec {
    code: "rototo/json-schema-file-parse-failed",
    title: "JSON Schema file could not be parsed",
    help: "Fix the JSON syntax in the referenced schema file.",
};

pub const JSON_SCHEMA_FILE_INVALID: DiagnosticSpec = DiagnosticSpec {
    code: "rototo/json-schema-file-invalid",
    title: "JSON Schema file is invalid",
    help: "Update the referenced schema file so it is valid JSON Schema.",
};

pub const VARIABLE_CUSTOM_LINT_FAILED: DiagnosticSpec = DiagnosticSpec {
    code: "rototo/variable-custom-lint-failed",
    title: "Variable custom lint failed",
    help: "Update the variable or its Lua lint rule so the custom lint passes.",
};

pub const WORKSPACE_NOT_FOUND: DiagnosticSpec = DiagnosticSpec {
    code: "rototo/workspace-not-found",
    title: "Workspace directory was not found",
    help: "Pass a path to an existing rototo workspace directory.",
};

pub const WORKSPACE_MANIFEST_MISSING: DiagnosticSpec = DiagnosticSpec {
    code: "rototo/workspace-manifest-missing",
    title: "Workspace manifest is missing",
    help: "Create rototo-workspace.toml at the workspace root.",
};

pub const WORKSPACE_MANIFEST_PARSE_FAILED: DiagnosticSpec = DiagnosticSpec {
    code: "rototo/workspace-manifest-parse-failed",
    title: "Workspace manifest could not be parsed",
    help: "Fix the TOML syntax in rototo-workspace.toml.",
};

pub const WORKSPACE_MANIFEST_SCHEMA_FAILED: DiagnosticSpec = DiagnosticSpec {
    code: "rototo/workspace-manifest-schema-failed",
    title: "Workspace manifest does not match the rototo workspace schema",
    help: "Declare schema_version = 1 and [environments].values in rototo-workspace.toml.",
};

pub const WORKSPACE_CONTEXT_SCHEMA_FAILED: DiagnosticSpec = DiagnosticSpec {
    code: "rototo/workspace-context-schema-failed",
    title: "Resolve context schema contract failed",
    help: "Update the workspace context schema or qualifier context references.",
};

pub fn kernel_catalog_entries() -> Vec<DiagnosticCatalogEntry> {
    [
        (WORKSPACE_TOML_FILE_PARSE_FAILED, DiagnosticSource::Kernel),
        (WORKSPACE_TOML_FILE_INVALID, DiagnosticSource::Kernel),
        (JSON_SCHEMA_FILE_PARSE_FAILED, DiagnosticSource::Schema),
        (JSON_SCHEMA_FILE_INVALID, DiagnosticSource::Schema),
        (VARIABLE_CUSTOM_LINT_FAILED, DiagnosticSource::Custom),
        (WORKSPACE_CONTEXT_SCHEMA_FAILED, DiagnosticSource::Custom),
        (WORKSPACE_NOT_FOUND, DiagnosticSource::Kernel),
        (WORKSPACE_MANIFEST_MISSING, DiagnosticSource::Kernel),
        (WORKSPACE_MANIFEST_PARSE_FAILED, DiagnosticSource::Kernel),
        (WORKSPACE_MANIFEST_SCHEMA_FAILED, DiagnosticSource::Kernel),
    ]
    .into_iter()
    .map(|(spec, source)| DiagnosticCatalogEntry {
        code: spec.code.to_owned(),
        title: spec.title.to_owned(),
        help: spec.help.to_owned(),
        source,
        kind: None,
    })
    .collect()
}

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub source: DiagnosticSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub path: String,
    pub message: String,
    pub help: String,
    pub details: serde_json::Value,
}

impl Diagnostic {
    pub fn new(
        spec: DiagnosticSpec,
        source: DiagnosticSource,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: spec.code.to_owned(),
            severity: Severity::Error,
            source,
            rule: None,
            kind: None,
            path: path.into(),
            message: message.into(),
            help: spec.help.to_owned(),
            details: serde_json::json!({ "title": spec.title }),
        }
    }

    pub fn new_rule(
        spec: DiagnosticSpec,
        source: DiagnosticSource,
        path: impl Into<String>,
        message: impl Into<String>,
        rule: LintRule,
    ) -> Self {
        Self {
            code: spec.code.to_owned(),
            severity: Severity::Error,
            source,
            rule: Some(rule.id.to_owned()),
            kind: None,
            path: path.into(),
            message: message.into(),
            help: rule.help.to_owned(),
            details: serde_json::json!({ "title": rule.title }),
        }
    }

    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSource {
    Kernel,
    Schema,
    Custom,
}
