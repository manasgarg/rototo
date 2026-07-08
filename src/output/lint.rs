use super::*;

#[derive(Debug, Serialize)]
pub(super) struct PackageLintJson<'a> {
    package: String,
    documents: &'a [rototo::model::SourceDocumentSummary],
    diagnostics: &'a [LintDiagnostic],
}

pub(crate) fn print_package_lint(lint: &PackageLint, json: bool, quiet: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&PackageLintJson {
                package: lint.root.display().to_string(),
                documents: &lint.documents,
                diagnostics: &lint.diagnostics,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    if lint.diagnostics.is_empty() {
        if quiet {
            return Ok(());
        }
        println!("{}", style::ok_line(&lint.root.display().to_string()));
        return Ok(());
    }

    print_diagnostics(&lint.diagnostics);
    Ok(())
}

pub(crate) fn print_diagnostic_catalog_entry(
    diagnostic: &DiagnosticCatalogEntry,
    json: bool,
) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(diagnostic)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!("{}", diagnostic.rule);
    if let Some(entity) = &diagnostic.entity {
        println!("  entity: {}", diagnostic_entity_label(entity));
    }
    println!("  severity: {}", severity_label(&diagnostic.severity));
    println!("  title: {}", diagnostic.title);
    println!("  help: {}", diagnostic.help);
    Ok(())
}

pub(super) fn print_diagnostics(diagnostics: &[LintDiagnostic]) {
    for diagnostic in diagnostics {
        println!(
            "{}: {}: {}",
            style::severity_prefix(&diagnostic.severity, &diagnostic.rule.as_string()),
            style::info(&diagnostic_location_label(diagnostic)),
            diagnostic.message
        );
        println!("  {} {}", style::dim("help:"), style::dim(&diagnostic.help));
        for related in &diagnostic.related {
            println!(
                "  {} {}: {}",
                style::dim("note:"),
                style::info(&diagnostic_location_label_for_location(&related.location)),
                related.message
            );
        }
    }
}

pub(super) fn diagnostic_location_label(diagnostic: &LintDiagnostic) -> String {
    diagnostic_location_label_for_location(&diagnostic.primary)
}

pub(super) fn diagnostic_location_label_for_location(location: &DiagnosticLocation) -> String {
    let Some(range) = location.range else {
        return location.path.clone();
    };
    format!(
        "{}:{}:{}",
        location.path,
        range.start.line + 1,
        range.start.character + 1
    )
}

pub(super) fn semantic_target_label(target: &SemanticTarget) -> String {
    match &target.field {
        Some(field) => format!(
            "{}.{}",
            semantic_entity_label(&target.entity),
            semantic_field_label(field)
        ),
        None => semantic_entity_label(&target.entity),
    }
}

pub(super) fn semantic_entity_label(entity: &SemanticEntity) -> String {
    match entity {
        SemanticEntity::Package => "package".to_owned(),
        SemanticEntity::Manifest => "manifest".to_owned(),
        SemanticEntity::List { id } => format!("list:{id}"),
        SemanticEntity::Layer { id } => format!("layer:{id}"),
        SemanticEntity::Governance => "governance".to_owned(),
        SemanticEntity::Variable { id } => format!("variable:{id}"),
        SemanticEntity::EvaluationContext { id } => format!("evaluation-context:{id}"),
        SemanticEntity::EvaluationContextSample {
            evaluation_context,
            key,
        } => {
            format!("evaluation-context:{evaluation_context}.entry:{key}")
        }
        SemanticEntity::Catalog { id } => format!("catalog:{id}"),
        SemanticEntity::CatalogEntry { catalog, key } => format!("catalog:{catalog}.value:{key}"),
        SemanticEntity::Value { variable, key } => format!("variable:{variable}.value:{key}"),
        SemanticEntity::Rule { variable, index } => {
            format!("variable:{variable}.rule[{index}]")
        }
        SemanticEntity::CustomLint { path } => format!("lint:{path}"),
    }
}

pub(super) fn semantic_field_label(field: &SemanticField) -> String {
    match field {
        SemanticField::PackageExtends => "extends".to_owned(),
        SemanticField::SchemaVersion => "schema_version".to_owned(),
        SemanticField::Description => "description".to_owned(),
        SemanticField::VariableType => "type".to_owned(),
        SemanticField::VariableSchema => "schema".to_owned(),
        SemanticField::VariableValues => "values".to_owned(),
        SemanticField::VariableResolve => "resolve".to_owned(),
        SemanticField::VariableResolveDefault => "resolve.default".to_owned(),
        SemanticField::VariableRuleWhen => "when".to_owned(),
        SemanticField::VariableRuleValue => "value".to_owned(),
        SemanticField::VariableQueryFilter => "filter".to_owned(),
        SemanticField::VariableQuerySort => "sort".to_owned(),
        SemanticField::VariableAllocation => "allocation".to_owned(),
        SemanticField::VariableAssignValue => "assign.value".to_owned(),
        SemanticField::Value => "value".to_owned(),
        SemanticField::ValueJsonPath { path } => format!("value.{}", path.join(".")),
        SemanticField::SchemaJson => "json".to_owned(),
        SemanticField::SchemaJsonPath { path } => format!("json.{}", path.join(".")),
        SemanticField::EvaluationContextSample => "entry".to_owned(),
        SemanticField::CatalogEntry => "value".to_owned(),
    }
}

pub(super) fn compact_json_option(value: &Option<serde_json::Value>) -> Result<String> {
    match value {
        Some(value) => compact_json(value),
        None => Ok("<none>".to_owned()),
    }
}

pub(super) fn variable_rule_condition(rule: &rototo::model::RulePathwayInspectReport) -> &str {
    rule.when.as_deref().unwrap_or("<missing>")
}

pub(super) fn severity_label(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

pub(super) fn diagnostic_entity_label(entity: &DiagnosticEntity) -> &'static str {
    match entity {
        DiagnosticEntity::Package => "package",
        DiagnosticEntity::List => "list",
        DiagnosticEntity::Layer => "layer",
        DiagnosticEntity::Governance => "governance",
        DiagnosticEntity::Variable => "variable",
        DiagnosticEntity::EvaluationContext => "evaluation_context",
        DiagnosticEntity::EvaluationContextSample => "evaluation_context_sample",
        DiagnosticEntity::Catalog => "catalog",
        DiagnosticEntity::CatalogEntry => "catalog_entry",
        DiagnosticEntity::Value => "value",
        DiagnosticEntity::Rule => "rule",
    }
}
