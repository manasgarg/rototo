mod output;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;

use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use regex::Regex;
use serde::Serialize;

use crate::output::{
    print_diagnostic_catalog_entry, print_inspection, print_qualifier_get, print_qualifier_list,
    print_variable_get, print_variable_list, print_workspace_lint,
};
use rototo::diagnostics::{DiagnosticCatalogEntry, EntityId, LintDiagnostic, Severity};
use rototo::model::{
    DiagnosticCatalog, QualifierInspection, QualifierResolution, VariableInspection,
    VariableResolution, WorkspaceInspection, WorkspaceLint,
};
use rototo::workspace::{qualifier_for_id, read_toml, read_variable_toml, variable_for_id};
use rototo::{
    Result, RototoError, SourceAuth, SourceOptions, StagedWorkspace, catalog,
    catalog_for_workspace, diagnostic_for_rule, find_workspace_root, inspect_workspace,
    lint_workspace, resolve_qualifier, resolve_qualifiers, resolve_variable, resolve_variables,
    stage_workspace_source,
};

#[derive(Debug, Parser)]
#[command(
    name = "rototo",
    version,
    about = "Control Git-backed runtime configuration workspaces",
    after_help = TOP_LEVEL_HELP,
    override_usage = "rototo <command> [options]",
    help_template = TOP_LEVEL_HELP_TEMPLATE
)]
struct Cli {
    /// Emit machine-readable JSON.
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    json: bool,

    /// Bearer token for https:// workspace archive downloads.
    #[arg(
        long,
        global = true,
        env = "ROTOTO_WORKSPACE_TOKEN",
        hide_env_values = true,
        value_name = "TOKEN"
    )]
    workspace_token: Option<String>,

    /// Suppress success output from lint commands.
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    quiet: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate a workspace or selected targets.
    Lint(WorkspaceCommandArgs),
    /// Explain how rototo sees workspace data.
    Inspect(WorkspaceCommandArgs),
    /// Display workspace config, variables, qualifiers, and lint metadata.
    Show(WorkspaceCommandArgs),
    /// Evaluate variables or qualifiers with runtime context.
    Resolve(ResolveArgs),
    /// Read bundled documentation.
    Docs(DocsArgs),
    /// Run the rototo Language Server Protocol server over stdio.
    Lsp,
    /// Generate shell completion scripts.
    Completions { shell: CompletionShell },
}

#[derive(Debug, Args)]
struct WorkspaceCommandArgs {
    /// Workspace source. Defaults to the nearest parent with rototo-workspace.toml.
    #[arg(value_name = "WORKSPACE_SOURCE")]
    workspace: Option<String>,

    #[command(flatten)]
    selectors: SelectorArgs,
}

#[derive(Clone, Debug, Default, Args)]
struct SelectorArgs {
    /// Select one variable id. Repeatable.
    #[arg(long = "variable", value_name = "ID")]
    variables: Vec<String>,

    /// Select all variables.
    #[arg(long = "variables", action = ArgAction::SetTrue)]
    all_variables: bool,

    /// Select one qualifier id. Repeatable.
    #[arg(long = "qualifier", value_name = "ID")]
    qualifiers: Vec<String>,

    /// Select all qualifiers.
    #[arg(long = "qualifiers", action = ArgAction::SetTrue)]
    all_qualifiers: bool,

    /// Select one diagnostic rule id. Repeatable.
    #[arg(long = "lint-rule", value_name = "AUTHORITY/RULE")]
    lint_rules: Vec<String>,

    /// Select all diagnostic rules.
    #[arg(long = "lint-rules", action = ArgAction::SetTrue)]
    all_lint_rules: bool,

    /// Select one lint authority. Repeatable.
    #[arg(long = "lint-authority", value_name = "AUTHORITY")]
    lint_authorities: Vec<String>,

    /// Select all lint authorities.
    #[arg(long = "lint-authorities", action = ArgAction::SetTrue)]
    all_lint_authorities: bool,

    /// Select one workspace Lua linter id. Repeatable.
    #[arg(long = "linter", value_name = "ID")]
    linters: Vec<String>,

    /// Select all workspace Lua linters.
    #[arg(long = "linters", action = ArgAction::SetTrue)]
    all_linters: bool,
}

#[derive(Debug, Args)]
struct ResolveArgs {
    /// Workspace source. Defaults to the nearest parent with rototo-workspace.toml.
    #[arg(value_name = "WORKSPACE_SOURCE")]
    workspace: Option<String>,

    #[command(flatten)]
    selectors: SelectorArgs,

    /// Environment name for variable resolution.
    #[arg(long = "env", value_name = "ENV")]
    env: Option<String>,

    /// Evaluation context: JSON object, @file, or path=value. Repeatable; later values override earlier ones.
    #[arg(long = "context", value_name = "CONTEXT")]
    context: Vec<String>,
}

#[derive(Debug, Args)]
struct DocsArgs {
    /// Render the documentation page matching this page id prefix.
    #[arg(
        short = 'p',
        long = "page",
        value_name = "PAGE_PREFIX",
        conflicts_with = "search"
    )]
    page: Option<String>,

    /// Search documentation pages with a regular expression.
    #[arg(short = 's', long = "search", value_name = "REGEX")]
    search: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}

impl From<CompletionShell> for Shell {
    fn from(shell: CompletionShell) -> Self {
        match shell {
            CompletionShell::Bash => Shell::Bash,
            CompletionShell::Elvish => Shell::Elvish,
            CompletionShell::Fish => Shell::Fish,
            CompletionShell::PowerShell => Shell::PowerShell,
            CompletionShell::Zsh => Shell::Zsh,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct TargetSelectors {
    variables: Selection<String>,
    qualifiers: Selection<String>,
    lint_rules: Selection<String>,
    lint_authorities: Selection<String>,
    linters: Selection<String>,
}

#[derive(Clone, Debug, Default)]
enum Selection<T> {
    #[default]
    None,
    Some(BTreeSet<T>),
    All,
}

impl<T> Selection<T> {
    fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    fn is_some_or_all(&self) -> bool {
        !self.is_none()
    }

    fn explicit_values(&self) -> Box<dyn Iterator<Item = &T> + '_> {
        match self {
            Self::Some(values) => Box::new(values.iter()),
            Self::None | Self::All => Box::new(std::iter::empty()),
        }
    }
}

impl TargetSelectors {
    fn from_args(args: &SelectorArgs) -> Self {
        Self {
            variables: selection(args.all_variables, &args.variables),
            qualifiers: selection(args.all_qualifiers, &args.qualifiers),
            lint_rules: selection(args.all_lint_rules, &args.lint_rules),
            lint_authorities: selection(args.all_lint_authorities, &args.lint_authorities),
            linters: selection(args.all_linters, &args.linters),
        }
    }

    fn is_empty(&self) -> bool {
        self.variables.is_none()
            && self.qualifiers.is_none()
            && self.lint_rules.is_none()
            && self.lint_authorities.is_none()
            && self.linters.is_none()
    }

    fn has_resolvable_targets(&self) -> bool {
        self.variables.is_some_or_all() || self.qualifiers.is_some_or_all()
    }

    fn has_lint_metadata_targets(&self) -> bool {
        self.lint_rules.is_some_or_all()
            || self.lint_authorities.is_some_or_all()
            || self.linters.is_some_or_all()
    }

    fn is_global_catalog_query(&self) -> bool {
        self.variables.is_none()
            && self.qualifiers.is_none()
            && self.linters.is_none()
            && (self.lint_rules.is_some_or_all() || self.lint_authorities.is_some_or_all())
    }
}

fn selection(all: bool, values: &[String]) -> Selection<String> {
    if all {
        Selection::All
    } else if values.is_empty() {
        Selection::None
    } else {
        Selection::Some(values.iter().cloned().collect())
    }
}

const TOP_LEVEL_HELP: &str = r#"Examples:
  rototo lint examples/basic
  rototo show examples/basic --variables
  rototo resolve examples/basic --variable checkout-redesign --env prod --context user.tier=premium
  rototo docs -p quickstart

Run `rototo <command> --help` for command details.
Run `rototo docs -p source-uri-reference` for workspace source syntax.
Run `rototo docs -p context-reference` for context input syntax."#;

const TOP_LEVEL_HELP_TEMPLATE: &str = r#"{about}

Usage:
  {usage}

Workspace commands:
  lint       Validate a workspace or selected targets
  inspect    Explain how rototo sees workspace data
  show       Display workspace config, variables, qualifiers, and lint metadata
  resolve    Evaluate variables or qualifiers with runtime context

Utility commands:
  docs       Read bundled documentation
  lsp        Run the language server over stdio
  completions Generate shell completions
  help       Print this message or the help of the given subcommand(s)

Global options:
  --json
  --quiet
  --workspace-token <token>
  -V, --version
  -h, --help

{after-help}
"#;

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    match run().await {
        Ok(status) => status,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let source_options = source_options(&cli);

    match cli.command {
        Command::Lint(args) => run_lint(args, &source_options, cli.json, cli.quiet).await,
        Command::Inspect(args) => run_inspect(args, &source_options, cli.json).await,
        Command::Show(args) => run_show(args, &source_options, cli.json).await,
        Command::Resolve(args) => run_resolve(args, &source_options, cli.json).await,
        Command::Docs(args) => run_docs(args, cli.json),
        Command::Lsp => {
            rototo::lsp::serve_stdio().await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Completions { shell } => {
            let mut command = Cli::command();
            let name = command.get_name().to_owned();
            generate(
                Shell::from(shell),
                &mut command,
                name,
                &mut std::io::stdout(),
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

async fn run_lint(
    args: WorkspaceCommandArgs,
    source_options: &SourceOptions,
    json: bool,
    quiet: bool,
) -> Result<ExitCode> {
    let workspace = workspace_source_or_current(args.workspace, source_options).await?;
    let selectors = TargetSelectors::from_args(&args.selectors);

    if selectors.is_empty() {
        let lint = lint_workspace(workspace.path()).await?;
        let passed = !lint.has_errors();
        print_workspace_lint(&lint, json, quiet)?;
        return Ok(if passed {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }

    let inspection = inspect_workspace(workspace.path()).await?;
    let catalog = catalog_for_workspace(workspace.path()).await?;
    validate_workspace_selectors(&selectors, &inspection, &catalog, workspace.path()).await?;

    let lint = lint_workspace(workspace.path()).await?;
    let lint = filter_lint(lint, &selectors);
    let passed = !lint.has_errors();
    print_workspace_lint(&lint, json, quiet)?;
    Ok(if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn run_inspect(
    args: WorkspaceCommandArgs,
    source_options: &SourceOptions,
    json: bool,
) -> Result<ExitCode> {
    let workspace = workspace_source_or_current(args.workspace, source_options).await?;
    let inspection = inspect_workspace(workspace.path()).await?;
    let selectors = TargetSelectors::from_args(&args.selectors);

    if selectors.is_empty() {
        print_inspection(&inspection, json)?;
        return Ok(ExitCode::SUCCESS);
    }

    let catalog = catalog_for_workspace(workspace.path()).await?;
    validate_workspace_selectors(&selectors, &inspection, &catalog, workspace.path()).await?;
    let view = selected_workspace_view(&inspection, &selectors, &catalog, workspace.path()).await?;
    print_workspace_view("inspect", &view, json)?;
    Ok(ExitCode::SUCCESS)
}

async fn run_show(
    args: WorkspaceCommandArgs,
    source_options: &SourceOptions,
    json: bool,
) -> Result<ExitCode> {
    let selectors = TargetSelectors::from_args(&args.selectors);
    if args.workspace.is_none() && selectors.is_global_catalog_query() {
        let catalog = catalog();
        validate_global_catalog_selectors(&selectors, &catalog)?;
        print_selected_lint_rules(&catalog, &selectors, json)?;
        return Ok(ExitCode::SUCCESS);
    }

    let workspace = workspace_source_or_current(args.workspace, source_options).await?;
    let inspection = inspect_workspace(workspace.path()).await?;

    if selectors.is_empty() {
        print_inspection(&inspection, json)?;
        return Ok(ExitCode::SUCCESS);
    }

    let catalog = catalog_for_workspace(workspace.path()).await?;
    validate_workspace_selectors(&selectors, &inspection, &catalog, workspace.path()).await?;

    if json {
        let view =
            selected_workspace_view(&inspection, &selectors, &catalog, workspace.path()).await?;
        print_workspace_view("show", &view, true)?;
        return Ok(ExitCode::SUCCESS);
    }

    show_selected_targets(&inspection, &selectors, &catalog, workspace.path()).await?;
    Ok(ExitCode::SUCCESS)
}

fn validate_global_catalog_selectors(
    selectors: &TargetSelectors,
    catalog: &DiagnosticCatalog,
) -> Result<()> {
    for rule in selectors.lint_rules.explicit_values() {
        diagnostic_for_rule(catalog, rule)?;
    }
    let authorities = catalog_authorities(catalog);
    for authority in selectors.lint_authorities.explicit_values() {
        if !authorities.contains(authority) {
            return Err(RototoError::new(format!(
                "lint authority not found: {authority}"
            )));
        }
    }
    Ok(())
}

async fn run_resolve(
    args: ResolveArgs,
    source_options: &SourceOptions,
    json: bool,
) -> Result<ExitCode> {
    let selectors = TargetSelectors::from_args(&args.selectors);
    if selectors.has_lint_metadata_targets() {
        return Err(RototoError::new(
            "lint selectors cannot be used with resolve",
        ));
    }
    if !selectors.has_resolvable_targets() {
        return Err(RototoError::new(
            "resolve requires at least one --variable, --variables, --qualifier, or --qualifiers selector",
        ));
    }
    if args.context.is_empty() {
        return Err(RototoError::new(
            "resolve requires at least one --context value",
        ));
    }
    if selectors.variables.is_some_or_all() && args.env.is_none() {
        return Err(RototoError::new(
            "--env is required when resolving variables",
        ));
    }

    let workspace = workspace_source_or_current(args.workspace, source_options).await?;
    let inspection = inspect_workspace(workspace.path()).await?;
    let catalog = catalog_for_workspace(workspace.path()).await?;
    validate_workspace_selectors(&selectors, &inspection, &catalog, workspace.path()).await?;

    let context = parse_context(&args.context).await?;
    let mut variables = Vec::new();
    let mut qualifiers = Vec::new();

    if selectors.variables.is_some_or_all() {
        let env = args.env.as_deref().expect("validated above");
        match selected_variable_ids(&inspection, &selectors.variables) {
            SelectedIds::All => {
                variables.extend(resolve_variables(workspace.path(), env, &context).await?)
            }
            SelectedIds::Some(ids) => {
                for id in ids {
                    variables.push(resolve_variable(workspace.path(), &id, env, &context).await?);
                }
            }
            SelectedIds::None => {}
        }
    }

    if selectors.qualifiers.is_some_or_all() {
        match selected_qualifier_ids(&inspection, &selectors.qualifiers) {
            SelectedIds::All => {
                qualifiers.extend(resolve_qualifiers(workspace.path(), &context).await?)
            }
            SelectedIds::Some(ids) => {
                for id in ids {
                    qualifiers.push(resolve_qualifier(workspace.path(), &id, &context).await?);
                }
            }
            SelectedIds::None => {}
        }
    }

    print_resolutions(workspace.path(), &variables, &qualifiers, json)?;
    Ok(ExitCode::SUCCESS)
}

fn run_docs(args: DocsArgs, json: bool) -> Result<ExitCode> {
    match (args.page, args.search) {
        (Some(page), None) => print_docs_page(&page, json),
        (None, Some(search)) => print_docs_search(&search, json),
        (None, None) => {
            print_docs_index(json)?;
            Ok(ExitCode::SUCCESS)
        }
        (Some(_), Some(_)) => Err(RototoError::new(
            "--page and --search cannot be used together",
        )),
    }
}

#[derive(Debug)]
enum SelectedIds {
    None,
    Some(Vec<String>),
    All,
}

fn selected_variable_ids(
    inspection: &WorkspaceInspection,
    selection: &Selection<String>,
) -> SelectedIds {
    match selection {
        Selection::None => SelectedIds::None,
        Selection::All => SelectedIds::All,
        Selection::Some(ids) => SelectedIds::Some(ordered_selected_ids(
            ids,
            inspection
                .variables
                .iter()
                .map(|variable| variable.id.as_str()),
        )),
    }
}

fn selected_qualifier_ids(
    inspection: &WorkspaceInspection,
    selection: &Selection<String>,
) -> SelectedIds {
    match selection {
        Selection::None => SelectedIds::None,
        Selection::All => SelectedIds::All,
        Selection::Some(ids) => SelectedIds::Some(ordered_selected_ids(
            ids,
            inspection
                .qualifiers
                .iter()
                .map(|qualifier| qualifier.id.as_str()),
        )),
    }
}

fn ordered_selected_ids<'a>(
    ids: &BTreeSet<String>,
    workspace_order: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    let mut ordered = Vec::new();
    for id in workspace_order {
        if ids.contains(id) {
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

async fn validate_workspace_selectors(
    selectors: &TargetSelectors,
    inspection: &WorkspaceInspection,
    catalog: &DiagnosticCatalog,
    workspace: &Path,
) -> Result<()> {
    for id in selectors.variables.explicit_values() {
        if !inspection
            .variables
            .iter()
            .any(|variable| variable.id == *id)
        {
            return Err(RototoError::new(format!(
                "variable not found: variable://{id}"
            )));
        }
    }
    for id in selectors.qualifiers.explicit_values() {
        if !inspection
            .qualifiers
            .iter()
            .any(|qualifier| qualifier.id == *id)
        {
            return Err(RototoError::new(format!(
                "qualifier not found: qualifier://{id}"
            )));
        }
    }
    for rule in selectors.lint_rules.explicit_values() {
        diagnostic_for_rule(catalog, rule)?;
    }
    let authorities = catalog_authorities(catalog);
    for authority in selectors.lint_authorities.explicit_values() {
        if !authorities.contains(authority) {
            return Err(RototoError::new(format!(
                "lint authority not found: {authority}"
            )));
        }
    }
    let linters = discover_linters(workspace).await?;
    for id in selectors.linters.explicit_values() {
        if !linters.iter().any(|linter| linter.id == *id) {
            return Err(RototoError::new(format!("linter not found: {id}")));
        }
    }
    Ok(())
}

fn filter_lint(lint: WorkspaceLint, selectors: &TargetSelectors) -> WorkspaceLint {
    let WorkspaceLint {
        root,
        documents,
        diagnostics,
    } = lint;
    let diagnostics = diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic_matches_selectors(diagnostic, selectors))
        .collect();
    WorkspaceLint {
        root,
        documents,
        diagnostics,
    }
}

fn diagnostic_matches_selectors(diagnostic: &LintDiagnostic, selectors: &TargetSelectors) -> bool {
    selection_matches_variable(&selectors.variables, diagnostic)
        || selection_matches_qualifier(&selectors.qualifiers, diagnostic)
        || selection_matches_lint_rule(&selectors.lint_rules, diagnostic)
        || selection_matches_lint_authority(&selectors.lint_authorities, diagnostic)
        || selection_matches_linter(&selectors.linters, diagnostic)
}

fn selection_matches_variable(selection: &Selection<String>, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        Selection::None => false,
        Selection::All => diagnostic_is_variable_related(diagnostic),
        Selection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_variable(diagnostic, id)),
    }
}

fn selection_matches_qualifier(selection: &Selection<String>, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        Selection::None => false,
        Selection::All => diagnostic_is_qualifier_related(diagnostic),
        Selection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_qualifier(diagnostic, id)),
    }
}

fn selection_matches_lint_rule(selection: &Selection<String>, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        Selection::None => false,
        Selection::All => true,
        Selection::Some(rules) => rules.contains(&diagnostic.rule.as_string()),
    }
}

fn selection_matches_lint_authority(
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

fn selection_matches_linter(selection: &Selection<String>, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        Selection::None => false,
        Selection::All => diagnostic_is_linter_related(diagnostic),
        Selection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_linter(diagnostic, id)),
    }
}

fn diagnostic_is_variable_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.entity,
        EntityId::Variable { .. }
            | EntityId::Value { .. }
            | EntityId::EnvironmentBlock { .. }
            | EntityId::Rule { .. }
    ) || diagnostic.primary.path.starts_with("variables/")
}

fn diagnostic_belongs_to_variable(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let variable_path = format!("variables/{id}.toml");
    let external_values_prefix = format!("variables/{id}-values/");
    matches!(&diagnostic.entity, EntityId::Variable { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.entity, EntityId::Value { variable, .. } if variable == id)
        || matches!(&diagnostic.entity, EntityId::EnvironmentBlock { variable, .. } if variable == id)
        || matches!(&diagnostic.entity, EntityId::Rule { variable, .. } if variable == id)
        || diagnostic.primary.path == variable_path
        || diagnostic.primary.path.starts_with(&external_values_prefix)
}

fn diagnostic_is_qualifier_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.entity,
        EntityId::Qualifier { .. } | EntityId::Predicate { .. }
    ) || diagnostic.primary.path.starts_with("qualifiers/")
}

fn diagnostic_belongs_to_qualifier(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let qualifier_path = format!("qualifiers/{id}.toml");
    matches!(&diagnostic.entity, EntityId::Qualifier { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.entity, EntityId::Predicate { qualifier, .. } if qualifier == id)
        || diagnostic.primary.path == qualifier_path
}

fn diagnostic_is_linter_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(diagnostic.entity, EntityId::CustomLint { .. })
        || diagnostic.primary.path.starts_with("lint/")
        || authority_of(&diagnostic.rule.as_string()).is_some_and(|authority| authority != "rototo")
}

fn diagnostic_belongs_to_linter(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let path = format!("lint/{id}.lua");
    matches!(&diagnostic.entity, EntityId::CustomLint { path: diagnostic_path } if diagnostic_path == &path)
        || diagnostic.primary.path == path
}

fn authority_of(rule: &str) -> Option<&str> {
    rule.split_once('/').map(|(authority, _)| authority)
}

fn catalog_authorities(catalog: &DiagnosticCatalog) -> BTreeSet<String> {
    catalog
        .diagnostics
        .iter()
        .filter_map(|diagnostic| authority_of(&diagnostic.rule).map(str::to_owned))
        .collect()
}

async fn show_selected_targets(
    inspection: &WorkspaceInspection,
    selectors: &TargetSelectors,
    catalog: &DiagnosticCatalog,
    workspace: &Path,
) -> Result<()> {
    match &selectors.variables {
        Selection::All => print_variable_list(inspection, false)?,
        Selection::Some(ids) => {
            for id in ordered_selected_ids(ids, inspection.variables.iter().map(|v| v.id.as_str()))
            {
                print_variable_get(inspection, &id, false).await?;
            }
        }
        Selection::None => {}
    }
    match &selectors.qualifiers {
        Selection::All => print_qualifier_list(inspection, false)?,
        Selection::Some(ids) => {
            for id in ordered_selected_ids(ids, inspection.qualifiers.iter().map(|q| q.id.as_str()))
            {
                print_qualifier_get(inspection, &id, false).await?;
            }
        }
        Selection::None => {}
    }
    print_selected_lint_rules(catalog, selectors, false)?;
    print_selected_linters(workspace, selectors, false).await?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct WorkspaceView {
    command: String,
    workspace: String,
    variables: Vec<WorkspaceFileView>,
    qualifiers: Vec<WorkspaceFileView>,
    lint_rules: Vec<DiagnosticCatalogEntryView>,
    lint_authorities: Vec<LintAuthorityView>,
    linters: Vec<LinterInfo>,
}

#[derive(Debug, Serialize)]
struct WorkspaceFileView {
    id: String,
    uri: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct DiagnosticCatalogEntryView {
    rule: String,
    severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    entity: Option<String>,
    title: String,
    help: String,
}

#[derive(Debug, Serialize)]
struct LintAuthorityView {
    authority: String,
    rules: Vec<DiagnosticCatalogEntryView>,
}

#[derive(Clone, Debug, Serialize)]
struct LinterInfo {
    id: String,
    path: String,
}

async fn selected_workspace_view(
    inspection: &WorkspaceInspection,
    selectors: &TargetSelectors,
    catalog: &DiagnosticCatalog,
    workspace: &Path,
) -> Result<WorkspaceView> {
    let mut variables = Vec::new();
    let mut qualifiers = Vec::new();
    let mut lint_rules = selected_lint_rule_entries(catalog, selectors);
    let mut lint_authorities = selected_lint_authorities(catalog, selectors);
    let mut linters = selected_linters(workspace, selectors).await?;

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
    match &selectors.qualifiers {
        Selection::All => {
            for qualifier in &inspection.qualifiers {
                qualifiers.push(qualifier_view(inspection, qualifier, false).await?);
            }
        }
        Selection::Some(ids) => {
            for id in ordered_selected_ids(ids, inspection.qualifiers.iter().map(|q| q.id.as_str()))
            {
                let qualifier = qualifier_for_id(inspection, &id)?;
                qualifiers.push(qualifier_view(inspection, qualifier, true).await?);
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
        linters = discover_linters(workspace).await?;
    }

    Ok(WorkspaceView {
        command: String::new(),
        workspace: inspection.root.display().to_string(),
        variables,
        qualifiers,
        lint_rules,
        lint_authorities,
        linters,
    })
}

async fn variable_view(
    inspection: &WorkspaceInspection,
    variable: &VariableInspection,
    include_value: bool,
) -> Result<WorkspaceFileView> {
    let value = if include_value {
        Some(
            serde_json::to_value(read_variable_toml(&inspection.root, variable).await?)
                .map_err(|err| RototoError::new(err.to_string()))?,
        )
    } else {
        None
    };
    Ok(WorkspaceFileView {
        id: variable.id.clone(),
        uri: variable.uri.clone(),
        path: variable.path.display().to_string(),
        value,
    })
}

async fn qualifier_view(
    inspection: &WorkspaceInspection,
    qualifier: &QualifierInspection,
    include_value: bool,
) -> Result<WorkspaceFileView> {
    let value = if include_value {
        Some(
            serde_json::to_value(read_toml(&inspection.root.join(&qualifier.path)).await?)
                .map_err(|err| RototoError::new(err.to_string()))?,
        )
    } else {
        None
    };
    Ok(WorkspaceFileView {
        id: qualifier.id.clone(),
        uri: qualifier.uri.clone(),
        path: qualifier.path.display().to_string(),
        value,
    })
}

fn print_workspace_view(command: &str, view: &WorkspaceView, json: bool) -> Result<()> {
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

    if !view.variables.is_empty() {
        println!("variables:");
        for variable in &view.variables {
            println!("  {}  {}  {}", variable.id, variable.uri, variable.path);
        }
    }
    if !view.qualifiers.is_empty() {
        println!("qualifiers:");
        for qualifier in &view.qualifiers {
            println!("  {}  {}  {}", qualifier.id, qualifier.uri, qualifier.path);
        }
    }
    if !view.lint_rules.is_empty() {
        println!("lint rules:");
        for rule in &view.lint_rules {
            println!("  {}  {}  {}", rule.rule, rule.severity_label(), rule.title);
        }
    }
    if !view.lint_authorities.is_empty() {
        println!("lint authorities:");
        for authority in &view.lint_authorities {
            println!("  {}", authority.authority);
            for rule in &authority.rules {
                println!("    {}  {}", rule.rule, rule.title);
            }
        }
    }
    if !view.linters.is_empty() {
        println!("linters:");
        for linter in &view.linters {
            println!("  {}  {}", linter.id, linter.path);
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

fn selected_lint_rule_entries(
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

fn selected_lint_authorities(
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

async fn selected_linters(
    workspace: &Path,
    selectors: &TargetSelectors,
) -> Result<Vec<LinterInfo>> {
    match &selectors.linters {
        Selection::None | Selection::All => Ok(Vec::new()),
        Selection::Some(ids) => Ok(discover_linters(workspace)
            .await?
            .into_iter()
            .filter(|linter| ids.contains(&linter.id))
            .collect()),
    }
}

fn authorities_from_catalog(catalog: &DiagnosticCatalog) -> Vec<LintAuthorityView> {
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

fn catalog_entry_view(entry: &DiagnosticCatalogEntry) -> DiagnosticCatalogEntryView {
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

fn print_selected_lint_rules(
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

fn print_diagnostic_catalog(catalog: &DiagnosticCatalog, json: bool) -> Result<()> {
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

fn print_lint_authorities(authorities: &[LintAuthorityView], json: bool) -> Result<()> {
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

async fn print_selected_linters(
    workspace: &Path,
    selectors: &TargetSelectors,
    json: bool,
) -> Result<()> {
    match &selectors.linters {
        Selection::None => {}
        Selection::All => print_linters(&discover_linters(workspace).await?, json)?,
        Selection::Some(ids) => {
            let selected: Vec<_> = discover_linters(workspace)
                .await?
                .into_iter()
                .filter(|linter| ids.contains(&linter.id))
                .collect();
            print_linters(&selected, json)?;
        }
    }
    Ok(())
}

fn print_linters(linters: &[LinterInfo], json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(linters)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }
    for linter in linters {
        println!("{}  {}", linter.id, linter.path);
    }
    Ok(())
}

async fn discover_linters(workspace: &Path) -> Result<Vec<LinterInfo>> {
    let lint_dir = workspace.join("lint");
    if tokio::fs::metadata(&lint_dir).await.is_err() {
        return Ok(Vec::new());
    }

    let mut entries = tokio::fs::read_dir(&lint_dir)
        .await
        .map_err(|err| RototoError::new(format!("failed to read {}: {err}", lint_dir.display())))?;
    let mut linters = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| RototoError::new(format!("failed to read {}: {err}", lint_dir.display())))?
    {
        let file_type = entry.file_type().await.map_err(|err| {
            RototoError::new(format!(
                "failed to inspect {}: {err}",
                entry.path().display()
            ))
        })?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("lua") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        linters.push(LinterInfo {
            id: id.to_owned(),
            path: format!("lint/{}.lua", id),
        });
    }
    linters.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(linters)
}

#[derive(Debug, Serialize)]
struct ResolveOutput<'a> {
    workspace: String,
    variables: &'a [VariableResolution],
    qualifiers: &'a [QualifierResolution],
}

fn print_resolutions(
    workspace: &Path,
    variables: &[VariableResolution],
    qualifiers: &[QualifierResolution],
    json: bool,
) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ResolveOutput {
                workspace: workspace.display().to_string(),
                variables,
                qualifiers,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    for resolution in variables {
        println!(
            "{}={} ({})",
            resolution.id,
            serde_json::to_string(&resolution.value)
                .map_err(|err| RototoError::new(err.to_string()))?,
            resolution.value_key
        );
    }
    for resolution in qualifiers {
        println!("{}={}", resolution.id, resolution.value);
    }
    Ok(())
}

#[derive(Serialize)]
struct DocsIndexJson {
    sections: Vec<DocsSectionJson>,
}

#[derive(Serialize)]
struct DocsSectionJson {
    title: &'static str,
    pages: Vec<DocsPageSummaryJson>,
}

#[derive(Serialize)]
struct DocsPageSummaryJson {
    id: &'static str,
    title: &'static str,
}

#[derive(Serialize)]
struct DocsPageJson {
    id: &'static str,
    title: &'static str,
    markdown: String,
}

#[derive(Serialize)]
struct DocsSearchJson {
    query: String,
    matches: Vec<DocsSearchMatch>,
}

#[derive(Serialize)]
struct DocsSearchMatch {
    page: &'static str,
    title: &'static str,
    line: usize,
    text: String,
    spans: Vec<DocsSearchSpan>,
}

#[derive(Serialize)]
struct DocsSearchSpan {
    start: usize,
    end: usize,
}

enum PagePrefixMatch {
    One(&'static rototo::docs::DocPage),
    Ambiguous(Vec<&'static rototo::docs::DocPage>),
    None,
}

fn print_docs_index(json: bool) -> Result<()> {
    let sections = rototo::docs::DOC_NAV_SECTIONS
        .iter()
        .map(|section| DocsSectionJson {
            title: section.title,
            pages: section
                .pages
                .iter()
                .filter_map(|id| docs_page_by_id(id))
                .map(|page| DocsPageSummaryJson {
                    id: page.id,
                    title: page.title,
                })
                .collect(),
        })
        .collect::<Vec<_>>();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&DocsIndexJson { sections })
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    for section in sections {
        println!("{}", section.title);
        for page in section.pages {
            println!("  {:<42} {}", page.id, page.title);
        }
        println!();
    }
    Ok(())
}

fn print_docs_page(prefix: &str, json: bool) -> Result<ExitCode> {
    match docs_page_by_prefix(prefix) {
        PagePrefixMatch::One(page) => {
            let markdown = render_cli_markdown(page.markdown)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&DocsPageJson {
                        id: page.id,
                        title: page.title,
                        markdown,
                    })
                    .map_err(|err| RototoError::new(err.to_string()))?
                );
            } else {
                print!("{markdown}");
            }
            Ok(ExitCode::SUCCESS)
        }
        PagePrefixMatch::Ambiguous(pages) => {
            println!("multiple documentation pages match \"{prefix}\":\n");
            for page in &pages {
                println!("  {:<42} {}", page.id, page.title);
            }
            println!("\nrun one of:");
            for page in pages {
                println!("  rototo docs -p {}", page.id);
            }
            Ok(ExitCode::FAILURE)
        }
        PagePrefixMatch::None => Err(RototoError::new(format!(
            "documentation page not found for prefix: {prefix}"
        ))),
    }
}

fn print_docs_search(query: &str, json: bool) -> Result<ExitCode> {
    let regex = Regex::new(query)
        .map_err(|err| RototoError::new(format!("invalid documentation search regex: {err}")))?;
    let matches = search_docs(query, &regex);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&DocsSearchJson {
                query: query.to_owned(),
                matches,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(ExitCode::SUCCESS);
    }

    let mut current_page = None;
    for hit in &matches {
        if current_page != Some(hit.page) {
            if current_page.is_some() {
                println!();
            }
            println!("{} - {}", hit.page, hit.title);
            current_page = Some(hit.page);
        }
        println!("  {}: {}", hit.line, hit.text);
        if let Some(span) = hit.spans.first() {
            println!(
                "      {}{}",
                " ".repeat(span.start),
                "^".repeat(span.end - span.start)
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn search_docs(query: &str, regex: &Regex) -> Vec<DocsSearchMatch> {
    let mut matches = Vec::new();
    for page in rototo::docs::DOCS {
        push_search_match(&mut matches, page, 0, page.id, regex);
        push_search_match(&mut matches, page, 0, page.title, regex);
        for (index, line) in page.markdown.lines().enumerate() {
            push_search_match(&mut matches, page, index + 1, line, regex);
        }
    }
    matches.sort_by_key(|hit| (docs_nav_index(hit.page), hit.line));
    if query.is_empty() {
        matches.clear();
    }
    matches
}

fn push_search_match(
    matches: &mut Vec<DocsSearchMatch>,
    page: &'static rototo::docs::DocPage,
    line: usize,
    text: &str,
    regex: &Regex,
) {
    let spans = regex
        .find_iter(text)
        .map(|found| DocsSearchSpan {
            start: found.start(),
            end: found.end(),
        })
        .collect::<Vec<_>>();
    if spans.is_empty() {
        return;
    }
    matches.push(DocsSearchMatch {
        page: page.id,
        title: page.title,
        line,
        text: text.to_owned(),
        spans,
    });
}

fn docs_page_by_prefix(prefix: &str) -> PagePrefixMatch {
    let prefix = normalize_docs_page_id(prefix);
    if let Some(page) = docs_page_by_id(&prefix) {
        return PagePrefixMatch::One(page);
    }
    let matches = rototo::docs::DOCS
        .iter()
        .filter(|page| page.id.starts_with(&prefix))
        .collect::<Vec<_>>();
    match matches.len() {
        0 => PagePrefixMatch::None,
        1 => PagePrefixMatch::One(matches[0]),
        _ => PagePrefixMatch::Ambiguous(matches),
    }
}

fn docs_page_by_id(id: &str) -> Option<&'static rototo::docs::DocPage> {
    let id = normalize_docs_page_id(id);
    rototo::docs::DOCS.iter().find(|page| page.id == id)
}

fn normalize_docs_page_id(id: &str) -> String {
    match id {
        "" | "/" | "index.html" => "index".to_owned(),
        _ => id.strip_suffix(".html").unwrap_or(id).to_owned(),
    }
}

fn docs_nav_index(page_id: &str) -> usize {
    rototo::docs::DOCS
        .iter()
        .position(|page| page.id == page_id)
        .unwrap_or(usize::MAX)
}

fn render_cli_markdown(markdown: &str) -> Result<String> {
    let link = Regex::new(r"\[([^\]\n]+)\]\(([^)\s]+)\)")
        .map_err(|err| RototoError::new(err.to_string()))?;
    Ok(link
        .replace_all(markdown, |captures: &regex::Captures<'_>| {
            let text = captures.get(1).expect("capture exists").as_str();
            let target = captures.get(2).expect("capture exists").as_str();
            if let Some(page_id) = internal_doc_link_target(target) {
                format!("{text} (rototo docs -p {page_id})")
            } else {
                captures.get(0).expect("capture exists").as_str().to_owned()
            }
        })
        .into_owned())
}

fn internal_doc_link_target(target: &str) -> Option<String> {
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with('#')
    {
        return None;
    }
    let target = target.split('#').next().unwrap_or(target);
    let file_name = Path::new(target).file_name()?.to_str()?;
    let id = file_name
        .strip_suffix(".md")
        .or_else(|| file_name.strip_suffix(".html"))?;
    docs_page_by_id(id).map(|page| page.id.to_owned())
}

async fn parse_context(parts: &[String]) -> Result<serde_json::Value> {
    let mut context = serde_json::Value::Object(serde_json::Map::new());
    for part in parts {
        let value = parse_context_part(part).await?;
        merge_context(&mut context, value)?;
    }
    Ok(context)
}

async fn parse_context_part(part: &str) -> Result<serde_json::Value> {
    if let Some(path) = part.strip_prefix('@') {
        if path.is_empty() {
            return Err(RototoError::new("context file path must not be empty"));
        }
        let text = tokio::fs::read_to_string(path).await.map_err(|err| {
            RototoError::new(format!("failed to read context file {path}: {err}"))
        })?;
        return parse_context_json(&text);
    }

    if part.trim_start().starts_with('{') {
        return parse_context_json(part);
    }

    if let Some((path, value)) = part.split_once('=') {
        if path.is_empty() {
            return Err(RototoError::new(
                "context assignment path must not be empty",
            ));
        }
        return context_assignment(path, value);
    }

    parse_context_json(part)
}

fn parse_context_json(text: &str) -> Result<serde_json::Value> {
    let context: serde_json::Value = serde_json::from_str(text)
        .map_err(|err| RototoError::new(format!("failed to parse context JSON: {err}")))?;
    if !context.is_object() {
        return Err(RototoError::new("context JSON must be an object"));
    }
    Ok(context)
}

fn context_assignment(path: &str, value: &str) -> Result<serde_json::Value> {
    let value =
        serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.to_owned()));
    let mut root = serde_json::Map::new();
    insert_context_path(&mut root, path, value)?;
    Ok(serde_json::Value::Object(root))
}

fn insert_context_path(
    object: &mut serde_json::Map<String, serde_json::Value>,
    path: &str,
    value: serde_json::Value,
) -> Result<()> {
    let mut segments = path.split('.').peekable();
    let mut current = object;
    while let Some(segment) = segments.next() {
        if segment.is_empty() {
            return Err(RototoError::new(format!("invalid context path: {path}")));
        }
        if segments.peek().is_none() {
            current.insert(segment.to_owned(), value);
            return Ok(());
        }
        let entry = current
            .entry(segment.to_owned())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !entry.is_object() {
            *entry = serde_json::Value::Object(serde_json::Map::new());
        }
        current = entry.as_object_mut().expect("object inserted above");
    }
    Err(RototoError::new(
        "context assignment path must not be empty",
    ))
}

fn merge_context(target: &mut serde_json::Value, source: serde_json::Value) -> Result<()> {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return Err(RototoError::new("context must be a JSON object"));
    };
    merge_context_objects(target, source);
    Ok(())
}

fn merge_context_objects(
    target: &mut serde_json::Map<String, serde_json::Value>,
    source: &serde_json::Map<String, serde_json::Value>,
) {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(existing), serde_json::Value::Object(source_object)) if existing.is_object() => {
                merge_context_objects(
                    existing.as_object_mut().expect("checked above"),
                    source_object,
                );
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

async fn workspace_source_or_current(
    workspace: Option<String>,
    source_options: &SourceOptions,
) -> Result<StagedWorkspace> {
    match workspace {
        Some(workspace) => stage_workspace_source(workspace, source_options).await,
        None => {
            let current_dir = tokio::task::spawn_blocking(std::env::current_dir)
                .await
                .map_err(|err| RototoError::new(format!("current directory task failed: {err}")))?
                .map_err(|err| {
                    RototoError::new(format!("failed to read current directory: {err}"))
                })?;
            Ok(StagedWorkspace::local(
                find_workspace_root(&current_dir).await?,
            ))
        }
    }
}

fn source_options(cli: &Cli) -> SourceOptions {
    match &cli.workspace_token {
        Some(token) => SourceOptions::new().with_auth(SourceAuth::Bearer(token.clone())),
        None => SourceOptions::new(),
    }
}

fn severity_label(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}
