use std::collections::BTreeMap;
use std::path::Path;

use toml::Value;

use crate::diagnostics::{
    CustomRuleDefinition, CustomRuleId, DiagnosticCatalogEntry, RototoRuleId,
};
use crate::error::{Result, RototoError};
use crate::model::{DiagnosticCatalog, DiagnosticCatalogScope};
use crate::workspace::{inspect_workspace, read_toml};

pub fn catalog() -> DiagnosticCatalog {
    let mut diagnostics: Vec<_> = RototoRuleId::iter()
        .map(DiagnosticCatalogEntry::from_rototo)
        .collect();
    diagnostics.sort_by(|left, right| left.rule.cmp(&right.rule));

    DiagnosticCatalog {
        scope: DiagnosticCatalogScope::Global,
        subject: "global".to_owned(),
        diagnostics,
    }
}

pub async fn catalog_for_workspace(workspace_root: &Path) -> Result<DiagnosticCatalog> {
    let workspace = inspect_workspace(workspace_root).await?;
    let mut diagnostics: Vec<_> = RototoRuleId::iter()
        .map(DiagnosticCatalogEntry::from_rototo)
        .collect();
    let mut custom_rules = BTreeMap::new();

    for variable in &workspace.variables {
        let path = workspace.root.join(&variable.path);
        let Ok(toml) = read_toml(&path).await else {
            continue;
        };
        for definition in custom_rule_definitions_from_toml(&toml) {
            custom_rules
                .entry(definition.rule.clone())
                .or_insert(definition);
        }
    }

    diagnostics.extend(
        custom_rules
            .values()
            .map(DiagnosticCatalogEntry::from_custom),
    );
    diagnostics.sort_by(|left, right| left.rule.cmp(&right.rule));

    Ok(DiagnosticCatalog {
        scope: DiagnosticCatalogScope::Workspace,
        subject: workspace.root.display().to_string(),
        diagnostics,
    })
}

pub fn diagnostic_for_rule<'a>(
    catalog: &'a DiagnosticCatalog,
    rule: &str,
) -> Result<&'a DiagnosticCatalogEntry> {
    catalog
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule == rule)
        .ok_or_else(|| RototoError::new(format!("diagnostic not found: {rule}")))
}

fn custom_rule_definitions_from_toml(toml: &Value) -> Vec<CustomRuleDefinition> {
    let Some(rules) = toml
        .get("lint")
        .and_then(|lint| lint.get("rule"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut definitions = Vec::new();
    for rule in rules {
        let Some(rule) = rule.as_table() else {
            continue;
        };
        let (Some(id), Some(title), Some(help)) = (
            rule.get("id").and_then(Value::as_str),
            rule.get("title").and_then(Value::as_str),
            rule.get("help").and_then(Value::as_str),
        ) else {
            continue;
        };
        let Ok(rule) = CustomRuleId::parse(id) else {
            continue;
        };
        definitions.push(CustomRuleDefinition::new(rule, title, help));
    }
    definitions
}
