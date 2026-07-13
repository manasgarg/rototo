#![allow(clippy::wildcard_imports)]

use crate::*;

pub(crate) async fn run_lint(
    args: PackageCommandArgs,
    source_options: &SourceOptions,
    json: bool,
    quiet: bool,
) -> Result<ExitCode> {
    let package = package_source_for_lint(args.package, source_options).await?;
    let selectors = TargetSelectors::from_args(&args.selectors);

    if selectors.is_empty() {
        let lint = lint_package(package.path()).await?;
        let passed = !lint.has_errors();
        print_package_lint(&lint, json, quiet)?;
        return Ok(if passed {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }

    let inspection = inspect_package(package.path()).await?;
    let catalog = diagnostics_catalog_for_package(package.path()).await?;
    validate_package_selectors(&selectors, &inspection, &catalog)?;

    let lint = lint_package(package.path()).await?;
    let lint = filter_lint(lint, &selectors);
    let passed = !lint.has_errors();
    print_package_lint(&lint, json, quiet)?;
    Ok(if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

pub(crate) async fn package_source_for_lint(
    package: Option<String>,
    source_options: &SourceOptions,
) -> Result<StagedPackage> {
    match package {
        Some(package) if !package.contains("://") => {
            let path = PathBuf::from(&package);
            if local_package_has_valid_extends(&path).await {
                package_source_or_current(Some(package), source_options).await
            } else {
                Ok(StagedPackage::local(path))
            }
        }
        package => package_source_or_current(package, source_options).await,
    }
}

pub(crate) async fn local_package_has_valid_extends(path: &Path) -> bool {
    let manifest = match read_toml(&path.join("rototo-package.toml")).await {
        Ok(manifest) => manifest,
        Err(_) => return false,
    };
    package_extends_sources(&manifest).is_ok_and(|sources| !sources.is_empty())
}

pub(crate) fn filter_lint(lint: PackageLint, selectors: &TargetSelectors) -> PackageLint {
    let PackageLint {
        root,
        documents,
        diagnostics,
    } = lint;
    let diagnostics = diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic_matches_selectors(diagnostic, selectors))
        .collect();
    PackageLint {
        root,
        documents,
        diagnostics,
    }
}

pub(crate) fn diagnostic_matches_selectors(
    diagnostic: &LintDiagnostic,
    selectors: &TargetSelectors,
) -> bool {
    selection_matches_variable(&selectors.variables, diagnostic)
        || selection_matches_catalog(&selectors.catalogs, diagnostic)
        || selection_matches_lint_rule(&selectors.lint_rules, diagnostic)
        || selection_matches_lint_authority(&selectors.lint_authorities, diagnostic)
        || selection_matches_linter(&selectors.linters, diagnostic)
}

pub(crate) fn selection_matches_variable(
    selection: &Selection<String>,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        Selection::None => false,
        Selection::All => diagnostic_is_variable_related(diagnostic),
        Selection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_variable(diagnostic, id)),
    }
}

pub(crate) fn selection_matches_catalog(
    selection: &Selection<String>,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        Selection::None => false,
        Selection::All => diagnostic_is_catalog_related(diagnostic),
        Selection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_catalog(diagnostic, id)),
    }
}

pub(crate) fn selection_matches_lint_rule(
    selection: &Selection<String>,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        Selection::None => false,
        Selection::All => true,
        Selection::Some(rules) => rules.contains(&diagnostic.rule.as_string()),
    }
}

pub(crate) fn selection_matches_lint_authority(
    selection: &Selection<String>,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        Selection::None => false,
        Selection::All => true,
        Selection::Some(authorities) => authority_of(&diagnostic.rule.as_string())
            .is_some_and(|authority| authorities.contains(authority)),
    }
}

pub(crate) fn selection_matches_linter(
    selection: &Selection<String>,
    diagnostic: &LintDiagnostic,
) -> bool {
    match selection {
        Selection::None => false,
        Selection::All => diagnostic_is_linter_related(diagnostic),
        Selection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_linter(diagnostic, id)),
    }
}

pub(crate) fn diagnostic_is_variable_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.target.entity,
        SemanticEntity::Variable { .. }
            | SemanticEntity::Value { .. }
            | SemanticEntity::Rule { .. }
    ) || diagnostic.primary.path.starts_with("variables/")
}

pub(crate) fn diagnostic_belongs_to_variable(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let variable_path = format!("variables/{id}.toml");
    matches!(&diagnostic.target.entity, SemanticEntity::Variable { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Value { variable, .. } if variable == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Rule { variable, .. } if variable == id)
        || diagnostic.primary.path == variable_path
}

pub(crate) fn diagnostic_is_catalog_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.target.entity,
        SemanticEntity::Catalog { .. } | SemanticEntity::CatalogEntry { .. }
    ) || diagnostic.primary.path.starts_with("model/catalogs/")
        || diagnostic.primary.path.starts_with("data/catalogs/")
}

pub(crate) fn diagnostic_belongs_to_catalog(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let catalog_path = format!("model/catalogs/{id}.schema.json");
    let catalog_entries_prefix = format!("data/catalogs/{id}/");
    matches!(&diagnostic.target.entity, SemanticEntity::Catalog { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::CatalogEntry { catalog, .. } if catalog == id)
        || diagnostic.primary.path == catalog_path
        || diagnostic.primary.path.starts_with(&catalog_entries_prefix)
}

pub(crate) fn diagnostic_is_linter_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(diagnostic.target.entity, SemanticEntity::CustomLint { .. })
        || diagnostic.primary.path.starts_with("lint/")
        || authority_of(&diagnostic.rule.as_string()).is_some_and(|authority| authority != "rototo")
}

pub(crate) fn diagnostic_belongs_to_linter(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let path = format!("lint/{id}.lua");
    matches!(&diagnostic.target.entity, SemanticEntity::CustomLint { path: diagnostic_path } if diagnostic_path == &path)
        || diagnostic.primary.path == path
}

pub(crate) fn authority_of(rule: &str) -> Option<&str> {
    rule.split_once('/').map(|(authority, _)| authority)
}

pub(crate) fn catalog_authorities(catalog: &DiagnosticCatalog) -> BTreeSet<String> {
    catalog
        .diagnostics
        .iter()
        .filter_map(|diagnostic| authority_of(&diagnostic.rule).map(str::to_owned))
        .collect()
}
