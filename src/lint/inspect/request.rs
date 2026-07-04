use super::*;

pub(super) fn validate_context_request(request: &PackageInspectRequest) -> Result<()> {
    if request.context.is_none() {
        return Ok(());
    }
    if !request.variables.is_some_or_all() {
        return Err(RototoError::new(
            "inspect --context requires at least one --variable or --variables selector",
        ));
    }
    Ok(())
}

pub(super) fn validate_request(
    snapshot: &PackageLintSnapshot,
    request: &PackageInspectRequest,
    catalog: &[DiagnosticCatalogEntry],
) -> Result<()> {
    for id in request.variables.explicit_values() {
        if !snapshot.index.variables.contains_key(id) {
            return Err(RototoError::new(format!(
                "variable not found: variable://{id}"
            )));
        }
    }
    for id in request.catalogs.explicit_values() {
        if !snapshot.index.catalogs.contains_key(id) {
            return Err(RototoError::new(format!(
                "catalog not found: catalog://{id}"
            )));
        }
    }
    for rule in request.lint_rules.explicit_values() {
        diagnostic_for_rule_in_entries(catalog, rule)?;
    }

    let authorities = catalog
        .iter()
        .filter_map(|entry| authority_of(&entry.rule).map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for authority in request.lint_authorities.explicit_values() {
        if !authorities.contains(authority) {
            return Err(RototoError::new(format!(
                "lint authority not found: {authority}"
            )));
        }
    }

    let linters = snapshot
        .index
        .custom_lints
        .files
        .keys()
        .filter_map(|path| linter_id(path))
        .collect::<BTreeSet<_>>();
    for id in request.linters.explicit_values() {
        if !linters.contains(id) {
            return Err(RototoError::new(format!("linter not found: {id}")));
        }
    }

    Ok(())
}

pub(super) fn selected_evaluation_contexts(
    snapshot: &PackageLintSnapshot,
    include_all_for_none: bool,
) -> Vec<EvaluationContextInspectReport> {
    if !include_all_for_none {
        return Vec::new();
    }
    snapshot
        .index
        .evaluation_contexts
        .values()
        .map(|evaluation_context| evaluation_context_report(snapshot, evaluation_context))
        .collect()
}

pub(super) fn selected_ids<'a>(
    selection: &'a InspectSelection,
    all_ids: impl Iterator<Item = &'a str>,
    include_all_for_none: bool,
) -> Vec<String> {
    match selection {
        InspectSelection::All => all_ids.map(str::to_owned).collect(),
        InspectSelection::Some(ids) => {
            let mut ordered = Vec::new();
            let requested = ids.iter().cloned().collect::<BTreeSet<_>>();
            for id in all_ids {
                if requested.contains(id) {
                    ordered.push(id.to_owned());
                }
            }
            for id in ids {
                if !ordered.iter().any(|ordered_id| ordered_id == id) {
                    ordered.push(id.clone());
                }
            }
            ordered
        }
        InspectSelection::None if include_all_for_none => all_ids.map(str::to_owned).collect(),
        InspectSelection::None => Vec::new(),
    }
}

pub(super) fn selected_diagnostics(
    snapshot: &PackageLintSnapshot,
    request: &PackageInspectRequest,
    inventory: bool,
) -> Vec<LintDiagnostic> {
    if inventory {
        return snapshot.lint.diagnostics.clone();
    }
    snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_matches_request(diagnostic, request))
        .cloned()
        .collect()
}

pub(super) fn diagnostic_matches_request(
    diagnostic: &LintDiagnostic,
    request: &PackageInspectRequest,
) -> bool {
    selection_matches_variable(&request.variables, diagnostic)
        || selection_matches_catalog(&request.catalogs, diagnostic)
        || selection_matches_lint_rule(&request.lint_rules, diagnostic)
        || selection_matches_lint_authority(&request.lint_authorities, diagnostic)
        || selection_matches_linter(&request.linters, diagnostic)
}

pub(super) fn selection_matches_variable(
    selection: &InspectSelection,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => diagnostic_is_variable_related(diagnostic),
        InspectSelection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_variable(diagnostic, id)),
    }
}

pub(super) fn selection_matches_catalog(
    selection: &InspectSelection,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => diagnostic_is_catalog_related(diagnostic),
        InspectSelection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_catalog(diagnostic, id)),
    }
}

pub(super) fn selection_matches_lint_rule(
    selection: &InspectSelection,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => true,
        InspectSelection::Some(rules) => rules.contains(&diagnostic.rule.as_string()),
    }
}

pub(super) fn selection_matches_lint_authority(
    selection: &InspectSelection,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => true,
        InspectSelection::Some(authorities) => authority_of(&diagnostic.rule.as_string())
            .is_some_and(|authority| authorities.iter().any(|selected| selected == authority)),
    }
}

pub(super) fn selection_matches_linter(
    selection: &InspectSelection,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        InspectSelection::None => false,
        InspectSelection::All => diagnostic_is_linter_related(diagnostic),
        InspectSelection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_linter(diagnostic, id)),
    }
}

pub(super) fn diagnostic_is_variable_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.target.entity,
        SemanticEntity::Variable { .. }
            | SemanticEntity::Value { .. }
            | SemanticEntity::Rule { .. }
    ) || diagnostic.primary.path.starts_with("variables/")
}

pub(super) fn diagnostic_belongs_to_variable(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let variable_path = format!("variables/{id}.toml");
    matches!(&diagnostic.target.entity, SemanticEntity::Variable { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Value { variable, .. } if variable == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Rule { variable, .. } if variable == id)
        || diagnostic.primary.path == variable_path
}

pub(super) fn diagnostic_is_catalog_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.target.entity,
        SemanticEntity::Catalog { .. } | SemanticEntity::CatalogEntry { .. }
    ) || diagnostic.primary.path.starts_with("model/catalogs/")
        || diagnostic.primary.path.starts_with("data/catalogs/")
}

pub(super) fn diagnostic_belongs_to_catalog(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let catalog_path = format!("model/catalogs/{id}.schema.json");
    let catalog_entries_prefix = format!("data/catalogs/{id}/");
    matches!(&diagnostic.target.entity, SemanticEntity::Catalog { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::CatalogEntry { catalog, .. } if catalog == id)
        || diagnostic.primary.path == catalog_path
        || diagnostic.primary.path.starts_with(&catalog_entries_prefix)
}

pub(super) fn diagnostic_is_linter_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(diagnostic.target.entity, SemanticEntity::CustomLint { .. })
        || diagnostic.primary.path.starts_with("lint/")
        || authority_of(&diagnostic.rule.as_string()).is_some_and(|authority| authority != "rototo")
}

pub(super) fn diagnostic_belongs_to_linter(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let path = format!("lint/{id}.lua");
    matches!(&diagnostic.target.entity, SemanticEntity::CustomLint { path: diagnostic_path } if diagnostic_path == &path)
        || diagnostic.primary.path == path
}

pub(super) fn diagnostic_belongs_to_evaluation_context(
    diagnostic: &LintDiagnostic,
    id: &str,
) -> bool {
    let schema_path = format!("model/context/{id}.schema.json");
    let samples_prefix = format!("model/context/{id}-samples/");
    matches!(&diagnostic.target.entity, SemanticEntity::EvaluationContext { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::EvaluationContextSample { evaluation_context, .. } if evaluation_context == id)
        || diagnostic.primary.path == schema_path
        || diagnostic.primary.path.starts_with(&samples_prefix)
}

pub(super) fn selected_lint_rules(
    snapshot: &PackageLintSnapshot,
    request: &PackageInspectRequest,
    catalog: &[DiagnosticCatalogEntry],
) -> Vec<LintRuleInspectReport> {
    let entries = match &request.lint_rules {
        InspectSelection::None => Vec::new(),
        InspectSelection::All => catalog.iter().collect(),
        InspectSelection::Some(rules) => catalog
            .iter()
            .filter(|entry| rules.contains(&entry.rule))
            .collect(),
    };
    entries
        .into_iter()
        .map(|entry| lint_rule_report(snapshot, entry))
        .collect()
}

pub(super) fn selected_lint_authorities(
    snapshot: &PackageLintSnapshot,
    request: &PackageInspectRequest,
    catalog: &[DiagnosticCatalogEntry],
    include_package_rules_for_none: bool,
) -> Vec<LintAuthorityInspectReport> {
    let (selected, package_rules_only) = match &request.lint_authorities {
        InspectSelection::None if include_package_rules_for_none => (None, true),
        InspectSelection::None => return Vec::new(),
        InspectSelection::All => (None, false),
        InspectSelection::Some(authorities) => (
            Some(authorities.iter().cloned().collect::<BTreeSet<_>>()),
            false,
        ),
    };
    let mut grouped: BTreeMap<String, Vec<LintRuleInspectReport>> = BTreeMap::new();
    for entry in catalog {
        let Some(authority) = authority_of(&entry.rule) else {
            continue;
        };
        if package_rules_only && authority == "rototo" {
            continue;
        }
        if selected
            .as_ref()
            .is_some_and(|authorities| !authorities.contains(authority))
        {
            continue;
        }
        grouped
            .entry(authority.to_owned())
            .or_default()
            .push(lint_rule_report(snapshot, entry));
    }
    grouped
        .into_iter()
        .map(|(authority, rules)| LintAuthorityInspectReport { authority, rules })
        .collect()
}

pub(super) fn lint_rule_report(
    snapshot: &PackageLintSnapshot,
    entry: &DiagnosticCatalogEntry,
) -> LintRuleInspectReport {
    let diagnostics = snapshot
        .lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule.as_string() == entry.rule)
        .cloned()
        .collect();
    LintRuleInspectReport {
        rule: entry.rule.clone(),
        severity: entry.severity,
        entity: entry
            .entity
            .map(|entity| format!("{entity:?}").to_lowercase()),
        title: entry.title.clone(),
        help: entry.help.clone(),
        diagnostics,
    }
}

pub(super) fn selected_linters(
    snapshot: &PackageLintSnapshot,
    request: &PackageInspectRequest,
    include_all_for_none: bool,
) -> Vec<LinterInspectReport> {
    let selected = match &request.linters {
        InspectSelection::None if include_all_for_none => None,
        InspectSelection::None => return Vec::new(),
        InspectSelection::All => None,
        InspectSelection::Some(ids) => Some(ids.iter().cloned().collect::<BTreeSet<_>>()),
    };
    snapshot
        .index
        .custom_lints
        .files
        .values()
        .filter_map(|file| {
            let id = linter_id(&file.path)?;
            if selected
                .as_ref()
                .is_some_and(|selected| !selected.contains(&id))
            {
                return None;
            }
            let registrations = snapshot
                .index
                .custom_lints
                .registrations
                .iter()
                .filter(|registration| registration.file_path == file.path)
                .map(|registration| LinterRegistrationInspectReport {
                    stage: format!("{:?}", registration.stage).to_lowercase(),
                    target: registered_address_label(&registration.selector.address),
                    rule: registration.rule.as_str().to_owned(),
                    handler: registration.handler.clone(),
                })
                .collect();
            let diagnostics = snapshot
                .lint
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic_belongs_to_linter(diagnostic, &id))
                .cloned()
                .collect();
            Some(LinterInspectReport {
                id,
                path: file.path.clone(),
                registrations,
                diagnostics,
            })
        })
        .collect()
}

pub(super) fn catalog_from_snapshot(snapshot: &PackageLintSnapshot) -> Vec<DiagnosticCatalogEntry> {
    let mut entries = RototoRuleId::iter()
        .map(DiagnosticCatalogEntry::from_rototo)
        .collect::<Vec<_>>();
    entries.extend(
        snapshot
            .index
            .custom_lints
            .rules
            .values()
            .map(|rule| DiagnosticCatalogEntry::from_custom(&rule.definition)),
    );
    entries.sort_by(|left, right| left.rule.cmp(&right.rule));
    entries
}

pub(super) fn diagnostic_for_rule_in_entries<'a>(
    entries: &'a [DiagnosticCatalogEntry],
    rule: &str,
) -> Result<&'a DiagnosticCatalogEntry> {
    entries
        .iter()
        .find(|entry| entry.rule == rule)
        .ok_or_else(|| RototoError::new(format!("diagnostic not found: {rule}")))
}

pub(super) fn authority_of(rule: &str) -> Option<&str> {
    rule.split_once('/').map(|(authority, _)| authority)
}

pub(super) fn linter_id(path: &str) -> Option<String> {
    path.strip_prefix("lint/")
        .and_then(|path| path.strip_suffix(".lua"))
        .map(str::to_owned)
}

pub(super) fn registered_address_label(address: &RegisteredLintAddress) -> String {
    match address {
        RegisteredLintAddress::Package => "/".to_owned(),
        RegisteredLintAddress::Variables => "/variables".to_owned(),
        RegisteredLintAddress::Variable { id } => format!("/variables/{id}"),
        RegisteredLintAddress::VariableValues { variable } => {
            format!("/variables/{variable}/values")
        }
        RegisteredLintAddress::VariableValue { variable, key } => {
            format!("/variables/{variable}/values/{key}")
        }
        RegisteredLintAddress::VariableRules { variable } => {
            format!("/variables/{variable}/rules")
        }
        RegisteredLintAddress::VariableRule { variable, index } => {
            format!("/variables/{variable}/rules/{index}")
        }
        RegisteredLintAddress::Catalogs => "/catalogs".to_owned(),
        RegisteredLintAddress::Catalog { id } => format!("/catalogs/{id}"),
        RegisteredLintAddress::CatalogEntries { catalog } => {
            format!("/catalogs/{catalog}/entries")
        }
        RegisteredLintAddress::CatalogEntry { catalog, key } => {
            format!("/catalogs/{catalog}/entries/{key}")
        }
        RegisteredLintAddress::EvaluationContexts => "/evaluation-contexts".to_owned(),
        RegisteredLintAddress::EvaluationContext { id } => format!("/evaluation-contexts/{id}"),
        RegisteredLintAddress::EvaluationContextSamples { evaluation_context } => {
            format!("/evaluation-contexts/{evaluation_context}/samples")
        }
        RegisteredLintAddress::EvaluationContextSample {
            evaluation_context,
            key,
        } => format!("/evaluation-contexts/{evaluation_context}/samples/{key}"),
    }
}
