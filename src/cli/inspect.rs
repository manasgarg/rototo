#![allow(clippy::wildcard_imports)]

use crate::*;

use crate::cli::lint::authority_of;
use crate::cli::selectors::{
    inspect_selection, ordered_selected_ids, validate_global_catalog_selectors,
};

pub(crate) async fn run_inspect(
    args: InspectArgs,
    source_options: &SourceOptions,
    json: bool,
) -> Result<ExitCode> {
    let package = package_source_or_current(args.package, source_options).await?;
    let selectors = TargetSelectors::from_args(&args.selectors);
    let context = if args.context.is_empty() {
        None
    } else {
        Some(parse_context(&args.context).await?)
    };
    let report = inspect_package_report(
        package.path(),
        PackageInspectRequest {
            variables: inspect_selection(&selectors.variables),
            catalogs: inspect_selection(&selectors.catalogs),
            lint_rules: inspect_selection(&selectors.lint_rules),
            lint_authorities: inspect_selection(&selectors.lint_authorities),
            linters: inspect_selection(&selectors.linters),
            context,
        },
    )
    .await?;
    print_inspect_report(&report, json)?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) async fn run_show(
    args: PackageCommandArgs,
    source_options: &SourceOptions,
    json: bool,
) -> Result<ExitCode> {
    let selectors = TargetSelectors::from_args(&args.selectors);
    if args.package.is_none() && selectors.is_global_catalog_query() {
        let catalog = diagnostics_catalog();
        validate_global_catalog_selectors(&selectors, &catalog)?;
        print_selected_lint_rules(&catalog, &selectors, json)?;
        return Ok(ExitCode::SUCCESS);
    }

    let package = package_source_or_current(args.package, source_options).await?;
    let inspection = inspect_package(package.path()).await?;
    let catalog = diagnostics_catalog_for_package(package.path()).await?;

    if selectors.is_empty() {
        let view = package_inventory_view(&inspection, &catalog).await?;
        print_package_view("show", &view, json)?;
        return Ok(ExitCode::SUCCESS);
    }

    validate_package_selectors(&selectors, &inspection, &catalog)?;

    if json {
        let view = selected_package_view(&inspection, &selectors, &catalog).await?;
        print_package_view("show", &view, true)?;
        return Ok(ExitCode::SUCCESS);
    }

    show_selected_targets(&inspection, &selectors, &catalog).await?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) async fn show_selected_targets(
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
    catalog: &DiagnosticCatalog,
) -> Result<()> {
    match &selectors.variables {
        Selection::All => print_variable_list(inspection, false).await?,
        Selection::Some(ids) => {
            for id in ordered_selected_ids(ids, inspection.variables.iter().map(|v| v.id.as_str()))
            {
                print_variable_get(inspection, &id, false).await?;
            }
        }
        Selection::None => {}
    }
    match &selectors.catalogs {
        Selection::All => print_catalog_list(inspection, false).await?,
        Selection::Some(ids) => {
            for id in ordered_selected_ids(ids, inspection.catalogs.iter().map(|r| r.id.as_str())) {
                print_catalog_get(inspection, &id, false).await?;
            }
        }
        Selection::None => {}
    }
    print_selected_lint_rules(catalog, selectors, false)?;
    print_selected_linters(inspection, selectors, false)?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub(crate) struct PackageView {
    command: String,
    package: String,
    evaluation_contexts: Vec<PackageFileView>,
    catalogs: Vec<PackageFileView>,
    variables: Vec<PackageFileView>,
    lint_rules: Vec<DiagnosticCatalogEntryView>,
    lint_authorities: Vec<LintAuthorityView>,
    linters: Vec<LinterInspection>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PackageFileView {
    id: String,
    uri: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DiagnosticCatalogEntryView {
    rule: String,
    severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    entity: Option<String>,
    title: String,
    help: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct LintAuthorityView {
    authority: String,
    rules: Vec<DiagnosticCatalogEntryView>,
}

pub(crate) async fn package_inventory_view(
    inspection: &PackageInspection,
    catalog: &DiagnosticCatalog,
) -> Result<PackageView> {
    let mut variables = Vec::new();
    for variable in &inspection.variables {
        variables.push(variable_view(inspection, variable, false).await?);
    }

    let mut catalogs = Vec::new();
    for catalog in &inspection.catalogs {
        catalogs.push(catalog_view(inspection, catalog, false).await?);
    }

    let evaluation_contexts = inspection
        .evaluation_contexts
        .iter()
        .map(evaluation_context_view)
        .collect();

    Ok(PackageView {
        command: String::new(),
        package: inspection.root.display().to_string(),
        evaluation_contexts,
        catalogs,
        variables,
        lint_rules: Vec::new(),
        lint_authorities: package_lint_authorities(catalog),
        linters: inspection.linters.clone(),
    })
}

pub(crate) async fn selected_package_view(
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
    catalog: &DiagnosticCatalog,
) -> Result<PackageView> {
    let mut variables = Vec::new();
    let mut catalogs = Vec::new();
    let mut lint_rules = selected_lint_rule_entries(catalog, selectors);
    let mut lint_authorities = selected_lint_authorities(catalog, selectors);
    let mut linters = selected_linters(inspection, selectors);

    match &selectors.variables {
        Selection::All => {
            for variable in &inspection.variables {
                variables.push(variable_view(inspection, variable, false).await?);
            }
        }
        Selection::Some(ids) => {
            for id in ordered_selected_ids(ids, inspection.variables.iter().map(|v| v.id.as_str()))
            {
                let variable = variable_for_id(inspection, &id)?;
                variables.push(variable_view(inspection, variable, true).await?);
            }
        }
        Selection::None => {}
    }
    match &selectors.catalogs {
        Selection::All => {
            for catalog in &inspection.catalogs {
                catalogs.push(catalog_view(inspection, catalog, false).await?);
            }
        }
        Selection::Some(ids) => {
            for id in ordered_selected_ids(ids, inspection.catalogs.iter().map(|r| r.id.as_str())) {
                let catalog = catalog_for_id(inspection, &id)?;
                catalogs.push(catalog_view(inspection, catalog, true).await?);
            }
        }
        Selection::None => {}
    }
    if matches!(selectors.lint_rules, Selection::All) {
        lint_rules = catalog.diagnostics.iter().map(catalog_entry_view).collect();
    }
    if matches!(selectors.lint_authorities, Selection::All) {
        lint_authorities = authorities_from_catalog(catalog);
    }
    if matches!(selectors.linters, Selection::All) {
        linters = inspection.linters.clone();
    }

    Ok(PackageView {
        command: String::new(),
        package: inspection.root.display().to_string(),
        evaluation_contexts: Vec::new(),
        catalogs,
        variables,
        lint_rules,
        lint_authorities,
        linters,
    })
}

pub(crate) async fn variable_view(
    inspection: &PackageInspection,
    variable: &VariableInspection,
    include_value: bool,
) -> Result<PackageFileView> {
    let value = if include_value {
        Some(
            serde_json::to_value(read_variable_toml(&inspection.root, variable).await?)
                .map_err(|err| RototoError::new(err.to_string()))?,
        )
    } else {
        None
    };
    Ok(PackageFileView {
        id: variable.id.clone(),
        uri: variable.uri.clone(),
        path: variable.path.display().to_string(),
        value,
    })
}

pub(crate) async fn catalog_view(
    inspection: &PackageInspection,
    catalog: &CatalogInspection,
    include_value: bool,
) -> Result<PackageFileView> {
    let value = if include_value {
        Some(read_catalog_json(&inspection.root, catalog).await?)
    } else {
        None
    };
    Ok(PackageFileView {
        id: catalog.id.clone(),
        uri: catalog.uri.clone(),
        path: catalog.path.display().to_string(),
        value,
    })
}

pub(crate) fn evaluation_context_view(
    evaluation_context: &EvaluationContextInspection,
) -> PackageFileView {
    PackageFileView {
        id: evaluation_context.id.clone(),
        uri: evaluation_context.uri.clone(),
        path: evaluation_context.path.display().to_string(),
        value: None,
    }
}

pub(crate) fn print_package_view(command: &str, view: &PackageView, json: bool) -> Result<()> {
    if json {
        let mut view =
            serde_json::to_value(view).map_err(|err| RototoError::new(err.to_string()))?;
        if let Some(object) = view.as_object_mut() {
            object.insert(
                "command".to_owned(),
                serde_json::Value::String(command.to_owned()),
            );
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&view).map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!("{} {}", style::label("package"), style::bold(&view.package));
    if !view.evaluation_contexts.is_empty() {
        println!(
            "{} {}",
            style::label("evaluation contexts"),
            style::bold(&view.evaluation_contexts.len().to_string())
        );
        for evaluation_context in &view.evaluation_contexts {
            println!(
                "  {}  {}",
                style::sea(&evaluation_context.id),
                style::dim(&evaluation_context.path)
            );
        }
    }
    if !view.catalogs.is_empty() {
        println!(
            "{} {}",
            style::label("catalogs"),
            style::bold(&view.catalogs.len().to_string())
        );
        for catalog in &view.catalogs {
            println!(
                "  {}  {}",
                style::sea(&catalog.id),
                style::dim(&catalog.path)
            );
        }
    }
    if !view.variables.is_empty() {
        println!(
            "{} {}",
            style::label("variables"),
            style::bold(&view.variables.len().to_string())
        );
        for variable in &view.variables {
            println!(
                "  {}  {}",
                style::sea(&variable.id),
                style::dim(&variable.path)
            );
        }
    }
    if !view.lint_rules.is_empty() {
        println!("{}", style::label("lint rules"));
        for rule in &view.lint_rules {
            println!(
                "  {}  {}  {}",
                style::sea(&rule.rule),
                rule.severity_label(),
                rule.title
            );
        }
    }
    if !view.lint_authorities.is_empty() {
        println!("{}", style::label("lint authorities"));
        for authority in &view.lint_authorities {
            println!("  {}", style::sea(&authority.authority));
            for rule in &authority.rules {
                println!("    {}  {}", style::sea(&rule.rule), rule.title);
            }
        }
    }
    if !view.linters.is_empty() {
        println!("linters:");
        for linter in &view.linters {
            println!("  {}  {}", linter.id, linter.path.display());
        }
    }
    Ok(())
}

impl DiagnosticCatalogEntryView {
    fn severity_label(&self) -> &'static str {
        match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

pub(crate) fn selected_lint_rule_entries(
    catalog: &DiagnosticCatalog,
    selectors: &TargetSelectors,
) -> Vec<DiagnosticCatalogEntryView> {
    match &selectors.lint_rules {
        Selection::None | Selection::All => Vec::new(),
        Selection::Some(rules) => catalog
            .diagnostics
            .iter()
            .filter(|entry| rules.contains(&entry.rule))
            .map(catalog_entry_view)
            .collect(),
    }
}

pub(crate) fn selected_lint_authorities(
    catalog: &DiagnosticCatalog,
    selectors: &TargetSelectors,
) -> Vec<LintAuthorityView> {
    match &selectors.lint_authorities {
        Selection::None | Selection::All => Vec::new(),
        Selection::Some(authorities) => authorities_from_catalog(catalog)
            .into_iter()
            .filter(|authority| authorities.contains(&authority.authority))
            .collect(),
    }
}

pub(crate) fn package_lint_authorities(catalog: &DiagnosticCatalog) -> Vec<LintAuthorityView> {
    authorities_from_catalog(catalog)
        .into_iter()
        .filter(|authority| authority.authority != "rototo")
        .collect()
}

pub(crate) fn selected_linters(
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
) -> Vec<LinterInspection> {
    match &selectors.linters {
        Selection::None | Selection::All => Vec::new(),
        Selection::Some(ids) => inspection
            .linters
            .iter()
            .filter(|linter| ids.contains(&linter.id))
            .cloned()
            .collect(),
    }
}

pub(crate) fn authorities_from_catalog(catalog: &DiagnosticCatalog) -> Vec<LintAuthorityView> {
    let mut grouped: BTreeMap<String, Vec<DiagnosticCatalogEntryView>> = BTreeMap::new();
    for entry in &catalog.diagnostics {
        if let Some(authority) = authority_of(&entry.rule) {
            grouped
                .entry(authority.to_owned())
                .or_default()
                .push(catalog_entry_view(entry));
        }
    }
    grouped
        .into_iter()
        .map(|(authority, rules)| LintAuthorityView { authority, rules })
        .collect()
}

pub(crate) fn catalog_entry_view(entry: &DiagnosticCatalogEntry) -> DiagnosticCatalogEntryView {
    DiagnosticCatalogEntryView {
        rule: entry.rule.clone(),
        severity: entry.severity,
        entity: entry
            .entity
            .map(|entity| format!("{entity:?}").to_lowercase()),
        title: entry.title.clone(),
        help: entry.help.clone(),
    }
}

pub(crate) fn print_selected_lint_rules(
    catalog: &DiagnosticCatalog,
    selectors: &TargetSelectors,
    json: bool,
) -> Result<()> {
    match &selectors.lint_rules {
        Selection::None => {}
        Selection::All => print_diagnostic_catalog(catalog, json)?,
        Selection::Some(rules) => {
            for rule in rules {
                let entry = diagnostic_for_rule(catalog, rule)?;
                print_diagnostic_catalog_entry(entry, json)?;
            }
        }
    }
    match &selectors.lint_authorities {
        Selection::None => {}
        Selection::All => print_lint_authorities(&authorities_from_catalog(catalog), json)?,
        Selection::Some(authorities) => {
            let selected: Vec<_> = authorities_from_catalog(catalog)
                .into_iter()
                .filter(|authority| authorities.contains(&authority.authority))
                .collect();
            print_lint_authorities(&selected, json)?;
        }
    }
    Ok(())
}

pub(crate) fn print_diagnostic_catalog(catalog: &DiagnosticCatalog, json: bool) -> Result<()> {
    if json {
        #[derive(Serialize)]
        struct CatalogJson<'a> {
            scope: &'a rototo::model::DiagnosticCatalogScope,
            subject: &'a str,
            diagnostics: &'a [DiagnosticCatalogEntry],
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&CatalogJson {
                scope: &catalog.scope,
                subject: &catalog.subject,
                diagnostics: &catalog.diagnostics,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }
    println!("{:<48}  {:<9}  {:<8}  title", "rule", "entity", "severity");
    for entry in &catalog.diagnostics {
        println!(
            "{:<48}  {:<9}  {:<8}  {}",
            entry.rule,
            entry
                .entity
                .map(|entity| format!("{entity:?}").to_lowercase())
                .unwrap_or_default(),
            severity_label(&entry.severity),
            entry.title
        );
    }
    Ok(())
}

pub(crate) fn print_lint_authorities(authorities: &[LintAuthorityView], json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(authorities)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }
    for authority in authorities {
        println!("{}", authority.authority);
        for rule in &authority.rules {
            println!("  {}  {}", rule.rule, rule.title);
        }
    }
    Ok(())
}

pub(crate) fn print_selected_linters(
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
    json: bool,
) -> Result<()> {
    match &selectors.linters {
        Selection::None => {}
        Selection::All => print_linters(&inspection.linters, json)?,
        Selection::Some(ids) => {
            let selected: Vec<_> = inspection
                .linters
                .iter()
                .filter(|linter| ids.contains(&linter.id))
                .cloned()
                .collect();
            print_linters(&selected, json)?;
        }
    }
    Ok(())
}

pub(crate) fn print_linters(linters: &[LinterInspection], json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(linters)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }
    for linter in linters {
        println!("{}  {}", linter.id, linter.path.display());
    }
    Ok(())
}
