mod output;
mod style;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use regex::Regex;
use serde::Serialize;

use crate::output::{
    print_catalog_get, print_catalog_list, print_diagnostic_catalog_entry, print_inspect_report,
    print_qualifier_get, print_qualifier_list, print_variable_get, print_variable_list,
    print_workspace_diff, print_workspace_lint,
};
use rototo::diagnostics::{DiagnosticCatalogEntry, LintDiagnostic, SemanticEntity, Severity};
use rototo::model::{
    CatalogInspection, DiagnosticCatalog, InspectSelection, LinterInspection,
    PredicateInspectReport, QualifierInspection, QualifierResolutionTrace, SchemaInspection,
    VariableInspection, VariableResolutionTrace, WorkspaceInspectRequest, WorkspaceInspection,
    WorkspaceLint,
};
use rototo::workspace::{
    catalog_for_id, qualifier_for_id, read_catalog_toml, read_toml, read_variable_toml,
    variable_for_id, workspace_extends_sources,
};
use rototo::{
    Result, RototoError, SourceAuth, SourceOptions, StagedWorkspace, diagnostic_for_rule,
    diagnostics_catalog, diagnostics_catalog_for_workspace, diff_workspaces, find_workspace_root,
    inspect_workspace, inspect_workspace_report, lint_workspace, stage_workspace_source,
    trace_qualifier_resolution, trace_qualifier_resolutions, trace_variable_resolution,
    trace_variable_resolutions,
};

#[derive(Debug, Parser)]
#[command(
    name = "rototo",
    version,
    about = "Control Git-backed runtime configuration workspaces",
    after_help = top_level_help(),
    override_usage = "rototo <command> [options]",
    help_template = top_level_help_template(),
    styles = help_styles()
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
    /// Create workspace and entity templates.
    Init(InitArgs),
    /// Generate readable runtime behavior fixtures.
    Fixtures(FixturesArgs),
    /// Validate a workspace or selected targets.
    Lint(WorkspaceCommandArgs),
    /// Explain how rototo sees workspace data.
    Inspect(InspectArgs),
    /// Compare two workspaces by rototo concepts.
    Diff(DiffArgs),
    /// Display workspace config, variables, qualifiers, and lint metadata.
    Show(WorkspaceCommandArgs),
    /// Evaluate variables or qualifiers with runtime context.
    Resolve(ResolveArgs),
    /// Read bundled documentation.
    Docs(DocsArgs),
    /// Serve the rototo console: web UI plus JSON API over a workspace.
    #[cfg(feature = "console")]
    Console(ConsoleArgs),
    /// Run the rototo Language Server Protocol server over stdio.
    Lsp,
    /// Generate shell completion scripts.
    Completions { shell: CompletionShell },
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Local workspace path to initialize or modify.
    #[arg(value_name = "WORKSPACE")]
    workspace: PathBuf,

    /// Create a qualifier template with this id.
    #[arg(long = "qualifier", value_name = "ID")]
    qualifier: Option<String>,

    /// Create a variable template with this id.
    #[arg(long = "variable", value_name = "ID")]
    variable: Option<String>,

    /// Create a catalog template with this id.
    #[arg(long = "catalog", value_name = "ID")]
    catalog: Option<String>,

    /// Create or infer the request context schema template.
    #[arg(long = "context", action = ArgAction::SetTrue)]
    context: bool,

    /// Overwrite files created by this command.
    #[arg(long = "force", action = ArgAction::SetTrue)]
    force: bool,

    /// Print the planned writes without changing the filesystem.
    #[arg(long = "dry-run", action = ArgAction::SetTrue)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct FixturesArgs {
    /// Workspace source to generate fixtures from.
    #[arg(value_name = "WORKSPACE_SOURCE")]
    workspace: String,

    /// Directory where rototo fixture TOML files will be written.
    #[arg(long = "out", value_name = "DIR")]
    out: PathBuf,

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
}

#[derive(Debug, Args)]
struct WorkspaceCommandArgs {
    /// Workspace source. Defaults to the nearest parent with rototo-workspace.toml.
    #[arg(value_name = "WORKSPACE_SOURCE")]
    workspace: Option<String>,

    #[command(flatten)]
    selectors: SelectorArgs,
}

#[derive(Debug, Args)]
struct InspectArgs {
    /// Workspace source. Defaults to the nearest parent with rototo-workspace.toml.
    #[arg(value_name = "WORKSPACE_SOURCE")]
    workspace: Option<String>,

    #[command(flatten)]
    selectors: SelectorArgs,

    /// Evaluation context: JSON object, @file, or path=value. Repeatable; later values override earlier ones.
    #[arg(long = "context", value_name = "CONTEXT")]
    context: Vec<String>,
}

#[derive(Debug, Args)]
struct DiffArgs {
    /// Workspace source used as the before side of the comparison.
    #[arg(value_name = "BEFORE_WORKSPACE_SOURCE")]
    before: String,

    /// Workspace source used as the after side of the comparison.
    #[arg(value_name = "AFTER_WORKSPACE_SOURCE")]
    after: String,

    /// Evaluation context used to report resolution impact: JSON object, @file, or path=value.
    #[arg(long = "context", value_name = "CONTEXT")]
    context: Vec<String>,
}

#[derive(Clone, Debug, Default, Args)]
struct SelectorArgs {
    /// Select one variable id. Repeatable.
    #[arg(long = "variable", value_name = "ID")]
    variables: Vec<String>,

    /// Select all variables.
    #[arg(long = "variables", action = ArgAction::SetTrue)]
    all_variables: bool,

    /// Select one catalog id. Repeatable.
    #[arg(long = "catalog", value_name = "ID")]
    catalogs: Vec<String>,

    /// Select all catalogs.
    #[arg(long = "catalogs", action = ArgAction::SetTrue)]
    all_catalogs: bool,

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

#[derive(Clone, Debug, Default, Args)]
struct ResolveSelectorArgs {
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
}

#[derive(Debug, Args)]
struct ResolveArgs {
    /// Workspace source. Defaults to the nearest parent with rototo-workspace.toml.
    #[arg(value_name = "WORKSPACE_SOURCE")]
    workspace: Option<String>,

    #[command(flatten)]
    selectors: ResolveSelectorArgs,

    /// Evaluation context: JSON object, @file, or path=value. Repeatable; later values override earlier ones. Defaults to {}.
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
        conflicts_with_all = ["search", "export"]
    )]
    page: Option<String>,

    /// Search documentation pages with a regular expression.
    #[arg(
        short = 's',
        long = "search",
        value_name = "REGEX",
        conflicts_with = "export"
    )]
    search: Option<String>,

    /// Export documentation pages as a static HTML site. Defaults to ./site when no path is given.
    #[arg(
        long = "export",
        value_name = "OUT_DIR",
        num_args = 0..=1,
        default_missing_value = "site"
    )]
    export: Option<PathBuf>,

    /// Generate a package README from the SDK reference docs.
    #[arg(
        long = "package-readme",
        value_name = "SDK",
        value_enum,
        conflicts_with_all = ["page", "search", "export"]
    )]
    package_readme: Option<PackageReadmeTarget>,

    /// Output file for --package-readme.
    #[arg(long = "out", value_name = "FILE", requires = "package_readme")]
    out: Option<PathBuf>,

    /// Base URL used for internal docs links in generated package READMEs.
    #[arg(
        long = "docs-base-url",
        value_name = "URL",
        requires = "package_readme"
    )]
    docs_base_url: Option<String>,
}

#[cfg(feature = "console")]
#[derive(Debug, Args)]
struct ConsoleArgs {
    /// Address to listen on.
    #[arg(long = "bind", value_name = "ADDR", default_value = rototo::console::DEFAULT_BIND)]
    bind: String,

    /// Public origin for OAuth redirects and cookies, for deployments behind
    /// a reverse proxy.
    #[arg(
        long = "public-url",
        value_name = "URL",
        env = "ROTOTO_CONSOLE_PUBLIC_URL"
    )]
    public_url: Option<String>,

    /// Directory for console state (sessions, selected branches, credentials).
    #[arg(long = "data-dir", value_name = "DIR", env = "ROTOTO_CONSOLE_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Workspace source to register at startup.
    #[arg(long = "workspace", value_name = "WORKSPACE_SOURCE")]
    workspace: Option<String>,

    /// Console deployment mode. Defaults to local with --workspace, hosted otherwise.
    #[arg(long = "deployment", value_enum)]
    deployment: Option<ConsoleDeploymentArg>,

    /// Write behavior for console branch edits.
    #[arg(long = "write", value_enum, default_value_t = ConsoleWriteArg::PullRequest)]
    write: ConsoleWriteArg,
}

#[cfg(feature = "console")]
#[derive(Clone, Copy, Debug, ValueEnum)]
enum ConsoleDeploymentArg {
    Local,
    Hosted,
}

#[cfg(feature = "console")]
impl From<ConsoleDeploymentArg> for rototo::console::ConsoleDeployment {
    fn from(value: ConsoleDeploymentArg) -> Self {
        match value {
            ConsoleDeploymentArg::Local => Self::Local,
            ConsoleDeploymentArg::Hosted => Self::Hosted,
        }
    }
}

#[cfg(feature = "console")]
#[derive(Clone, Copy, Debug, ValueEnum)]
enum ConsoleWriteArg {
    Disabled,
    PullRequest,
    DirectPush,
}

#[cfg(feature = "console")]
impl From<ConsoleWriteArg> for rototo::console::ConsoleWritePolicy {
    fn from(value: ConsoleWriteArg) -> Self {
        match value {
            ConsoleWriteArg::Disabled => Self::Disabled,
            ConsoleWriteArg::PullRequest => Self::PullRequest,
            ConsoleWriteArg::DirectPush => Self::DirectPush,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PackageReadmeTarget {
    Python,
    #[value(name = "typescript")]
    TypeScript,
    Java,
    Go,
}

impl PackageReadmeTarget {
    fn id(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::TypeScript => "typescript",
            Self::Java => "java",
            Self::Go => "go",
        }
    }
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
    catalogs: Selection<String>,
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
            catalogs: selection(args.all_catalogs, &args.catalogs),
            qualifiers: selection(args.all_qualifiers, &args.qualifiers),
            lint_rules: selection(args.all_lint_rules, &args.lint_rules),
            lint_authorities: selection(args.all_lint_authorities, &args.lint_authorities),
            linters: selection(args.all_linters, &args.linters),
        }
    }

    fn from_resolve_args(args: &ResolveSelectorArgs) -> Self {
        Self {
            variables: selection(args.all_variables, &args.variables),
            catalogs: Selection::None,
            qualifiers: selection(args.all_qualifiers, &args.qualifiers),
            lint_rules: Selection::None,
            lint_authorities: Selection::None,
            linters: Selection::None,
        }
    }

    fn is_empty(&self) -> bool {
        self.variables.is_none()
            && self.catalogs.is_none()
            && self.qualifiers.is_none()
            && self.lint_rules.is_none()
            && self.lint_authorities.is_none()
            && self.linters.is_none()
    }

    fn has_resolvable_targets(&self) -> bool {
        self.variables.is_some_or_all() || self.qualifiers.is_some_or_all()
    }

    fn is_global_catalog_query(&self) -> bool {
        self.variables.is_none()
            && self.catalogs.is_none()
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

fn inspect_selection(selection: &Selection<String>) -> InspectSelection {
    match selection {
        Selection::None => InspectSelection::None,
        Selection::All => InspectSelection::All,
        Selection::Some(values) => InspectSelection::Some(values.iter().cloned().collect()),
    }
}

fn help_styles() -> clap::builder::Styles {
    use clap::builder::styling::{Ansi256Color, Style};
    clap::builder::Styles::styled()
        .header(Style::new().bold())
        .usage(Style::new().bold())
        .literal(Style::new().fg_color(Some(Ansi256Color(43).into())))
        .placeholder(Style::new().fg_color(Some(Ansi256Color(245).into())))
}

fn top_level_help() -> String {
    let mut out = String::new();
    out.push_str(&style::bold("Examples:"));
    out.push('\n');
    for example in [
        "init config",
        "init config --qualifier premium-users",
        "fixtures examples/basic --variable tenant-limits --out tests/fixtures/rototo",
        "lint examples/basic",
        "show examples/basic --variables",
        "resolve examples/basic --variable checkout-redesign --context lane=prod --context user.tier=premium",
        "docs -p index",
    ] {
        out.push_str(&format!("  {} {}\n", style::sea("rototo"), example));
    }
    out.push('\n');
    out.push_str(&format!(
        "Run {} for command details.\n",
        style::cyan("rototo <command> --help")
    ));
    out.push_str(&format!(
        "Run {} to list bundled documentation.",
        style::cyan("rototo docs")
    ));
    out
}

fn top_level_help_template() -> String {
    // Pad before coloring so ANSI escapes do not break column alignment.
    let command =
        |name: &str, help: &str| format!("  {} {help}\n", style::sea(&format!("{name:<11}")));
    let flag = |name: &str| format!("  {}\n", style::sea(name));
    let mut out = String::new();
    out.push_str("{about}\n\n");
    out.push_str(&style::bold("Usage:"));
    out.push_str("\n  {usage}\n\n");
    out.push_str(&style::bold("Workspace commands:"));
    out.push('\n');
    out.push_str(&command("init", "Create workspace and entity templates"));
    out.push_str(&command(
        "fixtures",
        "Generate readable runtime behavior fixtures",
    ));
    out.push_str(&command("lint", "Validate a workspace or selected targets"));
    out.push_str(&command(
        "inspect",
        "Explain how rototo sees workspace data",
    ));
    out.push_str(&command(
        "show",
        "Display workspace config, variables, qualifiers, and lint metadata",
    ));
    out.push_str(&command(
        "resolve",
        "Evaluate variables or qualifiers with runtime context",
    ));
    out.push('\n');
    out.push_str(&style::bold("Utility commands:"));
    out.push('\n');
    out.push_str(&command("docs", "Read bundled documentation"));
    #[cfg(feature = "console")]
    out.push_str(&command("console", "Serve the web console and JSON API"));
    out.push_str(&command("lsp", "Run the language server over stdio"));
    out.push_str(&command("completions", "Generate shell completions"));
    out.push_str(&command(
        "help",
        "Print this message or the help of the given subcommand(s)",
    ));
    out.push('\n');
    out.push_str(&style::bold("Global options:"));
    out.push('\n');
    out.push_str(&flag("--json"));
    out.push_str(&flag("--quiet"));
    out.push_str(&flag("--workspace-token <token>"));
    out.push_str(&flag("-V, --version"));
    out.push_str(&flag("-h, --help"));
    out.push('\n');
    out.push_str("{after-help}\n");
    out
}

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
    style::init();

    match run().await {
        Ok(status) => status,
        Err(err) => {
            eprintln!("{}", style::err_line(&err.to_string()));
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let source_options = source_options(&cli);

    match cli.command {
        Command::Init(args) => run_init(args, cli.json, cli.quiet).await,
        Command::Fixtures(args) => run_fixtures(args, &source_options, cli.json, cli.quiet).await,
        Command::Lint(args) => run_lint(args, &source_options, cli.json, cli.quiet).await,
        Command::Inspect(args) => run_inspect(args, &source_options, cli.json).await,
        Command::Diff(args) => run_diff(args, &source_options, cli.json).await,
        Command::Show(args) => run_show(args, &source_options, cli.json).await,
        Command::Resolve(args) => run_resolve(args, &source_options, cli.json).await,
        Command::Docs(args) => run_docs(args, cli.json).await,
        #[cfg(feature = "console")]
        Command::Console(args) => {
            rototo::console::run(rototo::console::ConsoleOptions {
                bind: args.bind,
                public_url: args.public_url,
                data_dir: args.data_dir,
                workspace: args.workspace,
                deployment: args.deployment.map(Into::into),
                write_policy: args.write.into(),
                workspace_token: cli.workspace_token.clone(),
            })
            .await?;
            Ok(ExitCode::SUCCESS)
        }
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

async fn run_init(args: InitArgs, json: bool, quiet: bool) -> Result<ExitCode> {
    let workspace = local_init_workspace_path(&args.workspace)?;
    let target = init_target(&args)?;
    let plan = build_init_plan(&workspace, target).await?;
    let report = execute_init_plan(&workspace, &plan, args.force, args.dry_run).await?;
    print_init_report(&report, json, quiet)?;
    Ok(ExitCode::SUCCESS)
}

async fn run_fixtures(
    args: FixturesArgs,
    source_options: &SourceOptions,
    json: bool,
    quiet: bool,
) -> Result<ExitCode> {
    let selection = fixture_generate_selection(&args);
    let suite =
        rototo::fixtures::generate_fixture_suite(&args.workspace, source_options, selection)
            .await?;
    let report = suite.write_to(&args.out).await?;
    print_fixtures_report(&suite.workspace, &report, json, quiet)?;
    Ok(ExitCode::SUCCESS)
}

fn fixture_generate_selection(args: &FixturesArgs) -> rototo::fixtures::FixtureGenerateSelection {
    rototo::fixtures::FixtureGenerateSelection {
        variables: fixture_target_selection(args.all_variables, &args.variables),
        qualifiers: fixture_target_selection(args.all_qualifiers, &args.qualifiers),
    }
}

fn fixture_target_selection(
    all: bool,
    values: &[String],
) -> rototo::fixtures::FixtureTargetSelection {
    if all {
        rototo::fixtures::FixtureTargetSelection::All
    } else if values.is_empty() {
        rototo::fixtures::FixtureTargetSelection::None
    } else {
        rototo::fixtures::FixtureTargetSelection::some(values.iter().cloned())
    }
}

#[derive(Serialize)]
struct FixturesReport<'a> {
    command: &'static str,
    workspace: &'a str,
    out: &'a str,
    files: &'a [String],
}

fn print_fixtures_report(
    workspace: &str,
    report: &rototo::fixtures::FixtureWriteReport,
    json: bool,
    quiet: bool,
) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&FixturesReport {
                command: "fixtures",
                workspace,
                out: &report.out,
                files: &report.files,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    if quiet {
        return Ok(());
    }

    println!("{} {}", style::label("fixtures"), style::bold(&report.out));
    for file in &report.files {
        println!("  {} {}", style::ok("wrote"), file);
    }
    Ok(())
}

enum InitTarget {
    Workspace,
    Qualifier(String),
    Variable(String),
    Catalog(String),
    Context,
}

fn init_target(args: &InitArgs) -> Result<InitTarget> {
    let mut count = 0;
    let mut target = InitTarget::Workspace;

    if let Some(id) = &args.qualifier {
        count += 1;
        validate_template_id("qualifier", id)?;
        target = InitTarget::Qualifier(id.clone());
    }
    if let Some(id) = &args.variable {
        count += 1;
        validate_template_id("variable", id)?;
        target = InitTarget::Variable(id.clone());
    }
    if let Some(id) = &args.catalog {
        count += 1;
        validate_template_id("catalog", id)?;
        target = InitTarget::Catalog(id.clone());
    }
    if args.context {
        count += 1;
        target = InitTarget::Context;
    }

    if count > 1 {
        return Err(RototoError::new(
            "init accepts one entity flag at a time: --qualifier, --variable, --catalog, or --context",
        ));
    }

    Ok(target)
}

fn validate_template_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(RototoError::new(format!("{kind} id must not be empty")));
    }
    if id.starts_with('.') || id.split('.').any(str::is_empty) {
        return Err(RototoError::new(format!(
            "{kind} id must not start with '.', end with '.', or contain empty '.' segments"
        )));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(RototoError::new(format!(
            "{kind} id must use only ASCII letters, digits, '.', '_', or '-'"
        )));
    }
    Ok(())
}

fn local_init_workspace_path(path: &Path) -> Result<PathBuf> {
    let source = path.to_string_lossy();
    if source.contains("://") || source.starts_with("git+") {
        return Err(RototoError::new(
            "init requires a local workspace path, not a workspace source URI",
        ));
    }

    std::path::absolute(path)
        .map_err(|err| RototoError::new(format!("failed to resolve workspace path: {err}")))
}

async fn build_init_plan(workspace: &Path, target: InitTarget) -> Result<Vec<InitPlanEntry>> {
    let initialized = workspace_initialized(workspace).await?;
    match target {
        InitTarget::Workspace => Ok(workspace_init_plan(workspace)),
        InitTarget::Qualifier(id) => {
            let mut plan = implicit_workspace_init_plan(workspace, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(workspace.join("qualifiers")));
            }
            plan.push(InitPlanEntry::file(
                "qualifier",
                workspace.join("qualifiers").join(format!("{id}.toml")),
                qualifier_template(&id),
            ));
            Ok(plan)
        }
        InitTarget::Variable(id) => {
            let mut plan = implicit_workspace_init_plan(workspace, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(workspace.join("variables")));
            }
            plan.push(InitPlanEntry::file(
                "variable",
                workspace.join("variables").join(format!("{id}.toml")),
                variable_template(&id),
            ));
            Ok(plan)
        }
        InitTarget::Catalog(id) => {
            let mut plan = implicit_workspace_init_plan(workspace, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(workspace.join("catalogs")));
                plan.push(InitPlanEntry::directory(workspace.join("schemas")));
            }
            plan.extend([
                InitPlanEntry::directory(workspace.join("catalogs").join(format!("{id}-entries"))),
                InitPlanEntry::file(
                    "catalog",
                    workspace.join("catalogs").join(format!("{id}.toml")),
                    catalog_template(&id),
                ),
                InitPlanEntry::file(
                    "schema",
                    workspace.join("schemas").join(format!("{id}.schema.json")),
                    catalog_schema_template()?,
                ),
                InitPlanEntry::file(
                    "catalog_entry",
                    workspace
                        .join("catalogs")
                        .join(format!("{id}-entries"))
                        .join("default.toml"),
                    catalog_entry_template(),
                ),
            ]);
            Ok(plan)
        }
        InitTarget::Context => {
            let mut plan = implicit_workspace_init_plan(workspace, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(workspace.join("schemas")));
            }
            let content = if initialized {
                context_schema_template(workspace).await?
            } else {
                starter_context_schema_template()?
            };
            plan.push(InitPlanEntry::file(
                "context_schema",
                workspace.join("schemas").join("context.schema.json"),
                content,
            ));
            Ok(plan)
        }
    }
}

fn implicit_workspace_init_plan(workspace: &Path, initialized: bool) -> Vec<InitPlanEntry> {
    if initialized {
        Vec::new()
    } else {
        workspace_init_plan(workspace)
    }
}

fn workspace_init_plan(workspace: &Path) -> Vec<InitPlanEntry> {
    vec![
        InitPlanEntry::directory(workspace.to_path_buf()),
        InitPlanEntry::file(
            "workspace_manifest",
            workspace.join("rototo-workspace.toml"),
            workspace_manifest_template(),
        ),
        InitPlanEntry::directory(workspace.join("qualifiers")),
        InitPlanEntry::directory(workspace.join("variables")),
        InitPlanEntry::directory(workspace.join("catalogs")),
        InitPlanEntry::directory(workspace.join("schemas")),
        InitPlanEntry::directory(workspace.join("lint")),
    ]
}

async fn workspace_initialized(workspace: &Path) -> Result<bool> {
    path_exists(&workspace.join("rototo-workspace.toml")).await
}

async fn path_exists(path: &Path) -> Result<bool> {
    match tokio::fs::metadata(path).await {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(RototoError::new(format!(
            "failed to inspect {}: {err}",
            path.display()
        ))),
    }
}

#[derive(Debug)]
struct InitPlanEntry {
    kind: &'static str,
    path: PathBuf,
    content: Option<String>,
}

impl InitPlanEntry {
    fn directory(path: PathBuf) -> Self {
        Self {
            kind: "directory",
            path,
            content: None,
        }
    }

    fn file(kind: &'static str, path: PathBuf, content: String) -> Self {
        Self {
            kind,
            path,
            content: Some(content),
        }
    }

    fn is_directory(&self) -> bool {
        self.content.is_none()
    }
}

#[derive(Debug, Serialize)]
struct InitReport {
    command: &'static str,
    workspace: String,
    dry_run: bool,
    files: Vec<InitFileReport>,
}

#[derive(Debug, Serialize)]
struct InitFileReport {
    kind: &'static str,
    path: String,
    action: InitAction,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum InitAction {
    Exists,
    Created,
    Overwritten,
    WouldCreate,
    WouldOverwrite,
}

impl InitAction {
    fn label(self) -> &'static str {
        match self {
            Self::Exists => "exists",
            Self::Created => "created",
            Self::Overwritten => "overwritten",
            Self::WouldCreate => "would create",
            Self::WouldOverwrite => "would overwrite",
        }
    }
}

async fn execute_init_plan(
    workspace: &Path,
    plan: &[InitPlanEntry],
    force: bool,
    dry_run: bool,
) -> Result<InitReport> {
    let mut actions = Vec::with_capacity(plan.len());
    for entry in plan {
        actions.push(planned_init_action(entry, force, dry_run).await?);
    }

    if !dry_run {
        for entry in plan {
            if entry.is_directory() {
                tokio::fs::create_dir_all(&entry.path)
                    .await
                    .map_err(|err| {
                        RototoError::new(format!(
                            "failed to create directory {}: {err}",
                            entry.path.display()
                        ))
                    })?;
            } else if let Some(content) = &entry.content {
                if let Some(parent) = entry.path.parent() {
                    tokio::fs::create_dir_all(parent).await.map_err(|err| {
                        RototoError::new(format!(
                            "failed to create directory {}: {err}",
                            parent.display()
                        ))
                    })?;
                }
                tokio::fs::write(&entry.path, content)
                    .await
                    .map_err(|err| {
                        RototoError::new(format!("failed to write {}: {err}", entry.path.display()))
                    })?;
            }
        }
    }

    Ok(InitReport {
        command: "init",
        workspace: workspace.display().to_string(),
        dry_run,
        files: plan
            .iter()
            .zip(actions)
            .map(|(entry, action)| InitFileReport {
                kind: entry.kind,
                path: init_report_path(workspace, &entry.path),
                action,
            })
            .collect(),
    })
}

async fn planned_init_action(
    entry: &InitPlanEntry,
    force: bool,
    dry_run: bool,
) -> Result<InitAction> {
    let metadata = match tokio::fs::metadata(&entry.path).await {
        Ok(metadata) => Some(metadata),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(RototoError::new(format!(
                "failed to inspect {}: {err}",
                entry.path.display()
            )));
        }
    };

    if entry.is_directory() {
        if let Some(metadata) = metadata {
            if !metadata.is_dir() {
                return Err(RototoError::new(format!(
                    "path exists and is not a directory: {}",
                    entry.path.display()
                )));
            }
            return Ok(InitAction::Exists);
        }
        return Ok(if dry_run {
            InitAction::WouldCreate
        } else {
            InitAction::Created
        });
    }

    if let Some(metadata) = metadata {
        if metadata.is_dir() {
            return Err(RototoError::new(format!(
                "path exists and is a directory: {}",
                entry.path.display()
            )));
        }
        if !force {
            return Err(RototoError::new(format!(
                "file already exists: {} (use --force to overwrite)",
                entry.path.display()
            )));
        }
        return Ok(if dry_run {
            InitAction::WouldOverwrite
        } else {
            InitAction::Overwritten
        });
    }

    Ok(if dry_run {
        InitAction::WouldCreate
    } else {
        InitAction::Created
    })
}

fn init_report_path(workspace: &Path, path: &Path) -> String {
    match path.strip_prefix(workspace) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
        Ok(relative) => relative.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

fn print_init_report(report: &InitReport, json: bool, quiet: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    if quiet {
        return Ok(());
    }

    println!("workspace: {}", report.workspace);
    for file in &report.files {
        println!(
            "  {:<15} {}",
            format!("{}:", file.action.label()),
            file.path
        );
    }
    Ok(())
}

fn workspace_manifest_template() -> String {
    r#"schema_version = 1

# Optional workspace layering:
#
# extends = ["../shared-config"]
#
# Custom lint handlers live in lint/*.lua and register their rule metadata there.
"#
    .to_owned()
}

fn qualifier_template(id: &str) -> String {
    let description = toml_string(&format!(
        "Edit this description to explain when {id} should match"
    ));
    format!(
        r#"schema_version = 1

description = {description}

[[predicate]]
attribute = "user.tier"
op = "eq"
value = "premium"

# Additional predicates are ANDed with the predicate above.
#
# [[predicate]]
# attribute = "request.country"
# op = "in"
# value = ["DE", "FR", "NL"]
#
# Qualifiers can reference other qualifiers.
#
# [[predicate]]
# attribute = "qualifier.beta-rollout"
# op = "eq"
# value = true
#
# Bucket predicates produce stable rollout membership for a context value.
#
# [[predicate]]
# attribute = "user.id"
# op = "bucket"
# salt = "{id}-rollout"
# range = [0, 1000]
"#
    )
}

fn variable_template(id: &str) -> String {
    let description = toml_string(&format!(
        "Edit this description to explain what {id} controls"
    ));
    format!(
        r#"schema_version = 1

description = {description}
type = "string"

[values]
control = "control"
# treatment = "treatment"

[resolve]
default = "control"

# Rules are evaluated in order. The first matching qualifier selects its value.
#
# [[resolve.rule]]
# qualifier = "premium-users"
# value = "treatment"
#
# For catalog-backed values, remove [values] and use a catalog type:
#
# type = "catalog:{id}"
#
# Catalog entry keys become the selectable value keys.
"#
    )
}

fn catalog_template(id: &str) -> String {
    let description = toml_string(&format!(
        "Edit this description to explain the {id} catalog entries"
    ));
    let schema = toml_string(&format!("../schemas/{id}.schema.json"));
    format!(
        r#"schema_version = 1

description = {description}
schema = {schema}
"#
    )
}

fn catalog_schema_template() -> Result<String> {
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "heading": { "type": "string" },
            "enabled": { "type": "boolean" }
        },
        "required": ["heading", "enabled"]
    });
    pretty_json(&schema)
}

fn catalog_entry_template() -> String {
    r#"heading = "Edit this heading"
enabled = false
"#
    .to_owned()
}

async fn context_schema_template(workspace: &Path) -> Result<String> {
    let report = inspect_workspace_report(
        workspace,
        WorkspaceInspectRequest {
            qualifiers: InspectSelection::All,
            ..WorkspaceInspectRequest::default()
        },
    )
    .await?;

    let mut builder = ContextSchemaBuilder::default();
    for qualifier in &report.qualifiers {
        for predicate in &qualifier.predicates {
            builder.add_predicate(predicate);
        }
    }

    if builder.is_empty() {
        return starter_context_schema_template();
    }

    pretty_json(&builder.into_schema())
}

fn starter_context_schema_template() -> Result<String> {
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "user": {
                "type": "object",
                "additionalProperties": true,
                "properties": {
                    "tier": { "type": "string" },
                    "id": { "type": ["string", "number"] }
                }
            }
        }
    });
    pretty_json(&schema)
}

#[derive(Default)]
struct ContextSchemaBuilder {
    properties: serde_json::Map<String, serde_json::Value>,
}

impl ContextSchemaBuilder {
    fn add_predicate(&mut self, predicate: &PredicateInspectReport) {
        let Some(attribute) = predicate.attribute.as_deref() else {
            return;
        };
        if attribute.starts_with("qualifier.") {
            return;
        }

        let types = infer_context_schema_types(predicate);
        if types.is_empty() {
            return;
        }

        let segments = attribute.split('.').collect::<Vec<_>>();
        if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
            return;
        }

        insert_context_schema_path(&mut self.properties, &segments, &types);
    }

    fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    fn into_schema(self) -> serde_json::Value {
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": true,
            "properties": self.properties
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ContextSchemaType {
    Boolean,
    Integer,
    Number,
    String,
}

impl ContextSchemaType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
        }
    }
}

fn infer_context_schema_types(predicate: &PredicateInspectReport) -> BTreeSet<ContextSchemaType> {
    let mut types = match predicate.op.as_deref() {
        Some("eq" | "neq") => predicate
            .value
            .as_ref()
            .map(context_schema_types_from_json)
            .unwrap_or_default(),
        Some("in" | "not_in") => {
            let mut types = BTreeSet::new();
            if let Some(values) = predicate
                .value
                .as_ref()
                .and_then(serde_json::Value::as_array)
            {
                for value in values {
                    types.extend(context_schema_types_from_json(value));
                }
            }
            types
        }
        Some("gt" | "gte" | "lt" | "lte") => BTreeSet::from([ContextSchemaType::Number]),
        Some("bucket") => BTreeSet::from([
            ContextSchemaType::Boolean,
            ContextSchemaType::Integer,
            ContextSchemaType::Number,
            ContextSchemaType::String,
        ]),
        _ => BTreeSet::new(),
    };
    normalize_context_schema_types(&mut types);
    types
}

fn context_schema_types_from_json(value: &serde_json::Value) -> BTreeSet<ContextSchemaType> {
    let mut types = BTreeSet::new();
    match value {
        serde_json::Value::Bool(_) => {
            types.insert(ContextSchemaType::Boolean);
        }
        serde_json::Value::Number(number) => {
            types.insert(if number.is_i64() || number.is_u64() {
                ContextSchemaType::Integer
            } else {
                ContextSchemaType::Number
            });
        }
        serde_json::Value::String(_) => {
            types.insert(ContextSchemaType::String);
        }
        serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_) => {}
    }
    types
}

fn insert_context_schema_path(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    segments: &[&str],
    types: &BTreeSet<ContextSchemaType>,
) {
    let segment = segments[0];
    if segments.len() == 1 {
        merge_context_schema_leaf(properties, segment, types);
        return;
    }

    let entry = properties
        .entry(segment.to_owned())
        .or_insert_with(empty_context_object_schema);
    ensure_context_object_schema(entry);
    let child_properties = entry
        .as_object_mut()
        .expect("object schema ensured above")
        .entry("properties")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .expect("properties object inserted above");
    insert_context_schema_path(child_properties, &segments[1..], types);
}

fn merge_context_schema_leaf(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    segment: &str,
    types: &BTreeSet<ContextSchemaType>,
) {
    let entry = properties
        .entry(segment.to_owned())
        .or_insert_with(|| context_schema_leaf(types));
    if entry
        .as_object()
        .is_some_and(|object| object.contains_key("properties"))
    {
        return;
    }

    let mut merged = context_schema_types_from_schema(entry);
    merged.extend(types.iter().copied());
    normalize_context_schema_types(&mut merged);
    *entry = context_schema_leaf(&merged);
}

fn empty_context_object_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": true,
        "properties": {}
    })
}

fn ensure_context_object_schema(value: &mut serde_json::Value) {
    if !value.is_object() {
        *value = empty_context_object_schema();
        return;
    }

    let object = value.as_object_mut().expect("object checked above");
    object.insert(
        "type".to_owned(),
        serde_json::Value::String("object".to_owned()),
    );
    object.insert(
        "additionalProperties".to_owned(),
        serde_json::Value::Bool(true),
    );
    let properties = object
        .entry("properties")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !properties.is_object() {
        *properties = serde_json::Value::Object(serde_json::Map::new());
    }
}

fn context_schema_leaf(types: &BTreeSet<ContextSchemaType>) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    object.insert("type".to_owned(), context_schema_type_value(types));
    serde_json::Value::Object(object)
}

fn context_schema_type_value(types: &BTreeSet<ContextSchemaType>) -> serde_json::Value {
    let mut types = types.clone();
    normalize_context_schema_types(&mut types);
    if types.len() == 1 {
        return serde_json::Value::String(
            types.iter().next().expect("one type").as_str().to_owned(),
        );
    }
    serde_json::Value::Array(
        types
            .iter()
            .map(|ty| serde_json::Value::String(ty.as_str().to_owned()))
            .collect(),
    )
}

fn context_schema_types_from_schema(schema: &serde_json::Value) -> BTreeSet<ContextSchemaType> {
    let mut types = BTreeSet::new();
    match schema.as_object().and_then(|object| object.get("type")) {
        Some(serde_json::Value::String(value)) => {
            if let Some(ty) = context_schema_type_from_str(value) {
                types.insert(ty);
            }
        }
        Some(serde_json::Value::Array(values)) => {
            for value in values {
                if let Some(ty) = value.as_str().and_then(context_schema_type_from_str) {
                    types.insert(ty);
                }
            }
        }
        _ => {}
    }
    types
}

fn context_schema_type_from_str(value: &str) -> Option<ContextSchemaType> {
    match value {
        "boolean" => Some(ContextSchemaType::Boolean),
        "integer" => Some(ContextSchemaType::Integer),
        "number" => Some(ContextSchemaType::Number),
        "string" => Some(ContextSchemaType::String),
        _ => None,
    }
}

fn normalize_context_schema_types(types: &mut BTreeSet<ContextSchemaType>) {
    if types.contains(&ContextSchemaType::Number) {
        types.remove(&ContextSchemaType::Integer);
    }
}

fn pretty_json(value: &serde_json::Value) -> Result<String> {
    let mut text =
        serde_json::to_string_pretty(value).map_err(|err| RototoError::new(err.to_string()))?;
    text.push('\n');
    Ok(text)
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

async fn run_lint(
    args: WorkspaceCommandArgs,
    source_options: &SourceOptions,
    json: bool,
    quiet: bool,
) -> Result<ExitCode> {
    let workspace = workspace_source_for_lint(args.workspace, source_options).await?;
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
    let catalog = diagnostics_catalog_for_workspace(workspace.path()).await?;
    validate_workspace_selectors(&selectors, &inspection, &catalog)?;

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

async fn workspace_source_for_lint(
    workspace: Option<String>,
    source_options: &SourceOptions,
) -> Result<StagedWorkspace> {
    match workspace {
        Some(workspace) if !workspace.contains("://") => {
            let path = PathBuf::from(&workspace);
            if local_workspace_has_valid_extends(&path).await {
                workspace_source_or_current(Some(workspace), source_options).await
            } else {
                Ok(StagedWorkspace::local(path))
            }
        }
        workspace => workspace_source_or_current(workspace, source_options).await,
    }
}

async fn local_workspace_has_valid_extends(path: &Path) -> bool {
    let manifest = match read_toml(&path.join("rototo-workspace.toml")).await {
        Ok(manifest) => manifest,
        Err(_) => return false,
    };
    workspace_extends_sources(&manifest).is_ok_and(|sources| !sources.is_empty())
}

async fn run_inspect(
    args: InspectArgs,
    source_options: &SourceOptions,
    json: bool,
) -> Result<ExitCode> {
    let workspace = workspace_source_or_current(args.workspace, source_options).await?;
    let selectors = TargetSelectors::from_args(&args.selectors);
    let context = if args.context.is_empty() {
        None
    } else {
        Some(parse_context(&args.context).await?)
    };
    let report = inspect_workspace_report(
        workspace.path(),
        WorkspaceInspectRequest {
            variables: inspect_selection(&selectors.variables),
            catalogs: inspect_selection(&selectors.catalogs),
            qualifiers: inspect_selection(&selectors.qualifiers),
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

async fn run_diff(args: DiffArgs, source_options: &SourceOptions, json: bool) -> Result<ExitCode> {
    let before = workspace_source_for_lint(Some(args.before), source_options).await?;
    let after = workspace_source_for_lint(Some(args.after), source_options).await?;
    let context = if args.context.is_empty() {
        None
    } else {
        Some(parse_context(&args.context).await?)
    };
    let diff = diff_workspaces(before.path(), after.path(), context.as_ref()).await?;
    print_workspace_diff(&diff, json)?;
    Ok(ExitCode::SUCCESS)
}

async fn run_show(
    args: WorkspaceCommandArgs,
    source_options: &SourceOptions,
    json: bool,
) -> Result<ExitCode> {
    let selectors = TargetSelectors::from_args(&args.selectors);
    if args.workspace.is_none() && selectors.is_global_catalog_query() {
        let catalog = diagnostics_catalog();
        validate_global_catalog_selectors(&selectors, &catalog)?;
        print_selected_lint_rules(&catalog, &selectors, json)?;
        return Ok(ExitCode::SUCCESS);
    }

    let workspace = workspace_source_or_current(args.workspace, source_options).await?;
    let inspection = inspect_workspace(workspace.path()).await?;
    let catalog = diagnostics_catalog_for_workspace(workspace.path()).await?;

    if selectors.is_empty() {
        let view = workspace_inventory_view(&inspection, &catalog).await?;
        print_workspace_view("show", &view, json)?;
        return Ok(ExitCode::SUCCESS);
    }

    validate_workspace_selectors(&selectors, &inspection, &catalog)?;

    if json {
        let view = selected_workspace_view(&inspection, &selectors, &catalog).await?;
        print_workspace_view("show", &view, true)?;
        return Ok(ExitCode::SUCCESS);
    }

    show_selected_targets(&inspection, &selectors, &catalog).await?;
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
    let selectors = TargetSelectors::from_resolve_args(&args.selectors);
    if !selectors.has_resolvable_targets() {
        return Err(RototoError::new(
            "resolve requires at least one --variable, --variables, --qualifier, or --qualifiers selector",
        ));
    }
    let workspace = workspace_source_or_current(args.workspace, source_options).await?;
    let inspection = inspect_workspace(workspace.path()).await?;
    let catalog = diagnostics_catalog_for_workspace(workspace.path()).await?;
    validate_workspace_selectors(&selectors, &inspection, &catalog)?;

    let context = parse_context(&args.context).await?;
    let mut variables = Vec::new();
    let mut qualifiers = Vec::new();

    if selectors.variables.is_some_or_all() {
        match selected_variable_ids(&inspection, &selectors.variables) {
            SelectedIds::All => {
                variables.extend(trace_variable_resolutions(workspace.path(), &context).await?)
            }
            SelectedIds::Some(ids) => {
                for id in ids {
                    variables
                        .push(trace_variable_resolution(workspace.path(), &id, &context).await?);
                }
            }
            SelectedIds::None => {}
        }
    }

    if selectors.qualifiers.is_some_or_all() {
        match selected_qualifier_ids(&inspection, &selectors.qualifiers) {
            SelectedIds::All => {
                qualifiers.extend(trace_qualifier_resolutions(workspace.path(), &context).await?)
            }
            SelectedIds::Some(ids) => {
                for id in ids {
                    qualifiers
                        .push(trace_qualifier_resolution(workspace.path(), &id, &context).await?);
                }
            }
            SelectedIds::None => {}
        }
    }

    print_resolutions(workspace.path(), &variables, &qualifiers, json)?;
    Ok(ExitCode::SUCCESS)
}

async fn run_docs(args: DocsArgs, json: bool) -> Result<ExitCode> {
    let docs_base_url = args.docs_base_url;
    match (
        args.export,
        args.page,
        args.search,
        args.package_readme,
        args.out,
    ) {
        (Some(out), None, None, None, None) => {
            rototo::docs::export_html(&out).await?;
            print_docs_export(&out, json)?;
            Ok(ExitCode::SUCCESS)
        }
        (None, Some(page), None, None, None) => print_docs_page(&page, json),
        (None, None, Some(search), None, None) => print_docs_search(&search, json),
        (None, None, None, Some(target), Some(out)) => {
            let docs_base_url = docs_base_url
                .as_deref()
                .unwrap_or(rototo::docs::DEFAULT_DOCS_BASE_URL);
            write_package_readme(target, &out, docs_base_url).await?;
            print_package_readme_export(target, &out, json)?;
            Ok(ExitCode::SUCCESS)
        }
        (None, None, None, None, None) => {
            print_docs_index(json)?;
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(RototoError::new(
            "--export, --page, --search, and --package-readme cannot be used together",
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

fn validate_workspace_selectors(
    selectors: &TargetSelectors,
    inspection: &WorkspaceInspection,
    catalog: &DiagnosticCatalog,
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
    for id in selectors.catalogs.explicit_values() {
        if !inspection.catalogs.iter().any(|catalog| catalog.id == *id) {
            return Err(RototoError::new(format!(
                "catalog not found: catalog://{id}"
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
    for id in selectors.linters.explicit_values() {
        if !inspection.linters.iter().any(|linter| linter.id == *id) {
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
        || selection_matches_catalog(&selectors.catalogs, diagnostic)
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

fn selection_matches_catalog(selection: &Selection<String>, diagnostic: &LintDiagnostic) -> bool {
    match selection {
        Selection::None => false,
        Selection::All => diagnostic_is_catalog_related(diagnostic),
        Selection::Some(ids) => ids
            .iter()
            .any(|id| diagnostic_belongs_to_catalog(diagnostic, id)),
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
        diagnostic.target.entity,
        SemanticEntity::Variable { .. }
            | SemanticEntity::Value { .. }
            | SemanticEntity::Rule { .. }
    ) || diagnostic.primary.path.starts_with("variables/")
}

fn diagnostic_belongs_to_variable(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let variable_path = format!("variables/{id}.toml");
    matches!(&diagnostic.target.entity, SemanticEntity::Variable { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Value { variable, .. } if variable == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Rule { variable, .. } if variable == id)
        || diagnostic.primary.path == variable_path
}

fn diagnostic_is_catalog_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.target.entity,
        SemanticEntity::Catalog { .. } | SemanticEntity::CatalogEntry { .. }
    ) || diagnostic.primary.path.starts_with("catalogs/")
}

fn diagnostic_belongs_to_catalog(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let catalog_path = format!("catalogs/{id}.toml");
    let catalog_entries_prefix = format!("catalogs/{id}-entries/");
    matches!(&diagnostic.target.entity, SemanticEntity::Catalog { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::CatalogEntry { catalog, .. } if catalog == id)
        || diagnostic.primary.path == catalog_path
        || diagnostic.primary.path.starts_with(&catalog_entries_prefix)
}

fn diagnostic_is_qualifier_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(
        diagnostic.target.entity,
        SemanticEntity::Qualifier { .. } | SemanticEntity::Predicate { .. }
    ) || diagnostic.primary.path.starts_with("qualifiers/")
}

fn diagnostic_belongs_to_qualifier(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let qualifier_path = format!("qualifiers/{id}.toml");
    matches!(&diagnostic.target.entity, SemanticEntity::Qualifier { id: diagnostic_id } if diagnostic_id == id)
        || matches!(&diagnostic.target.entity, SemanticEntity::Predicate { qualifier, .. } if qualifier == id)
        || diagnostic.primary.path == qualifier_path
}

fn diagnostic_is_linter_related(diagnostic: &LintDiagnostic) -> bool {
    matches!(diagnostic.target.entity, SemanticEntity::CustomLint { .. })
        || diagnostic.primary.path.starts_with("lint/")
        || authority_of(&diagnostic.rule.as_string()).is_some_and(|authority| authority != "rototo")
}

fn diagnostic_belongs_to_linter(diagnostic: &LintDiagnostic, id: &str) -> bool {
    let path = format!("lint/{id}.lua");
    matches!(&diagnostic.target.entity, SemanticEntity::CustomLint { path: diagnostic_path } if diagnostic_path == &path)
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
    match &selectors.catalogs {
        Selection::All => print_catalog_list(inspection, false)?,
        Selection::Some(ids) => {
            for id in ordered_selected_ids(ids, inspection.catalogs.iter().map(|r| r.id.as_str())) {
                print_catalog_get(inspection, &id, false).await?;
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
    print_selected_linters(inspection, selectors, false)?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct WorkspaceView {
    command: String,
    workspace: String,
    schemas: Vec<SchemaInspection>,
    catalogs: Vec<WorkspaceFileView>,
    variables: Vec<WorkspaceFileView>,
    qualifiers: Vec<WorkspaceFileView>,
    lint_rules: Vec<DiagnosticCatalogEntryView>,
    lint_authorities: Vec<LintAuthorityView>,
    linters: Vec<LinterInspection>,
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

async fn workspace_inventory_view(
    inspection: &WorkspaceInspection,
    catalog: &DiagnosticCatalog,
) -> Result<WorkspaceView> {
    let mut variables = Vec::new();
    for variable in &inspection.variables {
        variables.push(variable_view(inspection, variable, false).await?);
    }

    let mut catalogs = Vec::new();
    for catalog in &inspection.catalogs {
        catalogs.push(catalog_view(inspection, catalog, false).await?);
    }

    let mut qualifiers = Vec::new();
    for qualifier in &inspection.qualifiers {
        qualifiers.push(qualifier_view(inspection, qualifier, false).await?);
    }

    Ok(WorkspaceView {
        command: String::new(),
        workspace: inspection.root.display().to_string(),
        schemas: inspection.schemas.clone(),
        catalogs,
        variables,
        qualifiers,
        lint_rules: Vec::new(),
        lint_authorities: workspace_lint_authorities(catalog),
        linters: inspection.linters.clone(),
    })
}

async fn selected_workspace_view(
    inspection: &WorkspaceInspection,
    selectors: &TargetSelectors,
    catalog: &DiagnosticCatalog,
) -> Result<WorkspaceView> {
    let mut variables = Vec::new();
    let mut catalogs = Vec::new();
    let mut qualifiers = Vec::new();
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
        linters = inspection.linters.clone();
    }

    Ok(WorkspaceView {
        command: String::new(),
        workspace: inspection.root.display().to_string(),
        schemas: Vec::new(),
        catalogs,
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

async fn catalog_view(
    inspection: &WorkspaceInspection,
    catalog: &CatalogInspection,
    include_value: bool,
) -> Result<WorkspaceFileView> {
    let value = if include_value {
        Some(
            serde_json::to_value(read_catalog_toml(&inspection.root, catalog).await?)
                .map_err(|err| RototoError::new(err.to_string()))?,
        )
    } else {
        None
    };
    Ok(WorkspaceFileView {
        id: catalog.id.clone(),
        uri: catalog.uri.clone(),
        path: catalog.path.display().to_string(),
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

    println!(
        "{} {}",
        style::label("workspace"),
        style::bold(&view.workspace)
    );
    if !view.schemas.is_empty() {
        println!("{}", style::label("schemas"));
        for schema in &view.schemas {
            println!(
                "  {}  {}",
                style::sea(&schema.id),
                style::dim(&schema.path.display().to_string())
            );
        }
    }
    if !view.qualifiers.is_empty() {
        println!("{}", style::label("qualifiers"));
        for qualifier in &view.qualifiers {
            println!(
                "  {}  {}  {}",
                style::sea(&qualifier.id),
                style::dim(&qualifier.uri),
                style::dim(&qualifier.path)
            );
        }
    }
    if !view.catalogs.is_empty() {
        println!("{}", style::label("catalogs"));
        for catalog in &view.catalogs {
            println!(
                "  {}  {}  {}",
                style::sea(&catalog.id),
                style::dim(&catalog.uri),
                style::dim(&catalog.path)
            );
        }
    }
    if !view.variables.is_empty() {
        println!("{}", style::label("variables"));
        for variable in &view.variables {
            println!(
                "  {}  {}  {}",
                style::sea(&variable.id),
                style::dim(&variable.uri),
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

fn workspace_lint_authorities(catalog: &DiagnosticCatalog) -> Vec<LintAuthorityView> {
    authorities_from_catalog(catalog)
        .into_iter()
        .filter(|authority| authority.authority != "rototo")
        .collect()
}

fn selected_linters(
    inspection: &WorkspaceInspection,
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

fn print_selected_linters(
    inspection: &WorkspaceInspection,
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

fn print_linters(linters: &[LinterInspection], json: bool) -> Result<()> {
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

#[derive(Debug, Serialize)]
struct ResolveOutput<'a> {
    workspace: String,
    variables: &'a [VariableResolutionTrace],
    qualifiers: &'a [QualifierResolutionTrace],
}

fn print_resolutions(
    workspace: &Path,
    variables: &[VariableResolutionTrace],
    qualifiers: &[QualifierResolutionTrace],
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

    println!(
        "{} {}",
        style::label("workspace"),
        style::bold(&workspace.display().to_string())
    );
    let count = variables.len() + qualifiers.len();
    let mut index = 0;
    for trace in variables {
        print_resolve_separator(index, count);
        index += 1;
        print_variable_resolution_trace(trace)?;
    }
    for trace in qualifiers {
        print_resolve_separator(index, count);
        index += 1;
        print_qualifier_resolution_trace(trace)?;
    }
    Ok(())
}

fn print_resolve_separator(index: usize, count: usize) {
    if count > 1 && index > 0 {
        println!("{}", style::hairline());
    }
}

fn print_variable_resolution_trace(trace: &VariableResolutionTrace) -> Result<()> {
    println!("variable: {}", style::sea(&trace.resolution.id));
    if !trace.qualifier_traces.is_empty() {
        println!("  {}", style::subhead("qualifiers"));
        for qualifier in &trace.qualifier_traces {
            print_nested_qualifier_resolution_trace(qualifier)?;
        }
    }
    println!("  {}", style::subhead("pathway"));
    for rule in &trace.rules {
        println!(
            "    {} if {} {} {} ({})",
            style::dim(&format!("rule[{}]", rule.index)),
            style::sea(&rule.qualifier),
            style::arrow(),
            rule.value,
            if rule.matched {
                style::ok("matched")
            } else {
                style::dim("skipped")
            }
        );
    }
    println!(
        "    {} {} {}",
        style::dim("default"),
        style::arrow(),
        trace.default_value
    );
    println!("  {}", style::subhead("result"));
    println!(
        "    value key: {}",
        style::sea_bold(&trace.resolution.value_key)
    );
    println!("    value: {}", compact_json(&trace.resolution.value)?);
    Ok(())
}

fn print_qualifier_resolution_trace(trace: &QualifierResolutionTrace) -> Result<()> {
    println!("qualifier: {}", style::sea(&trace.id));
    if !trace.predicates.is_empty() {
        println!("  {}", style::subhead("predicates"));
        for predicate in &trace.predicates {
            print_predicate_resolution(predicate, "    ")?;
        }
    }
    println!(
        "  result: {}",
        if trace.value {
            style::ok("true")
        } else {
            style::dim("false")
        }
    );
    Ok(())
}

fn print_nested_qualifier_resolution_trace(trace: &QualifierResolutionTrace) -> Result<()> {
    println!("    qualifier: {}", style::sea(&trace.id));
    if !trace.predicates.is_empty() {
        println!("      {}", style::subhead("predicates"));
        for predicate in &trace.predicates {
            print_predicate_resolution(predicate, "        ")?;
        }
    }
    println!(
        "      result: {}",
        if trace.value {
            style::ok("true")
        } else {
            style::dim("false")
        }
    );
    Ok(())
}

fn print_predicate_resolution(
    predicate: &rototo::model::PredicateResolutionTrace,
    indent: &str,
) -> Result<()> {
    println!(
        "{indent}[{}] {}",
        predicate.index,
        predicate_source_label(predicate)?
    );
    match &predicate.bucket {
        Some(bucket) => {
            let bucket_value = bucket
                .value
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<missing>".to_owned());
            println!(
                "{indent}    test: bucket salt={} range=[{},{}] bucket={}",
                bucket.salt, bucket.start, bucket.end, bucket_value
            );
        }
        None => {
            let op = predicate.op.as_deref().unwrap_or("<op>");
            let expected = predicate
                .expected
                .as_ref()
                .map(compact_json)
                .transpose()?
                .unwrap_or_else(|| "<missing>".to_owned());
            println!("{indent}    test: {op} {expected}");
        }
    }
    println!(
        "{indent}    matched: {}",
        if predicate.result {
            style::ok("true")
        } else {
            style::dim("false")
        }
    );
    Ok(())
}

fn predicate_source_label(predicate: &rototo::model::PredicateResolutionTrace) -> Result<String> {
    let actual = predicate
        .actual
        .as_ref()
        .map(compact_json)
        .transpose()?
        .unwrap_or_else(|| "<missing>".to_owned());
    if let Some(qualifier) = &predicate.qualifier {
        Ok(format!("qualifier {qualifier} = {actual}"))
    } else {
        Ok(format!("context {} = {actual}", predicate.attribute))
    }
}

fn compact_json(value: &serde_json::Value) -> Result<String> {
    serde_json::to_string(value).map_err(|err| RototoError::new(err.to_string()))
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
struct DocsExportJson {
    out: String,
}

#[derive(Serialize)]
struct PackageReadmeExportJson {
    sdk: &'static str,
    out: String,
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
        println!("{}", style::label(section.title));
        for page in section.pages {
            // Pad before coloring so ANSI escapes do not break alignment.
            println!(
                "  {} {}",
                style::sea(&format!("{:<42}", page.id)),
                page.title
            );
        }
        println!();
    }
    Ok(())
}

fn print_docs_export(out: &Path, json: bool) -> Result<()> {
    let out = out.display().to_string();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&DocsExportJson { out })
                .map_err(|err| RototoError::new(err.to_string()))?
        );
    } else {
        println!(
            "{}",
            style::ok_line(&format!("exported documentation to {out}"))
        );
    }
    Ok(())
}

async fn write_package_readme(
    target: PackageReadmeTarget,
    out: &Path,
    docs_base_url: &str,
) -> Result<()> {
    let readme = rototo::docs::render_package_readme_with_base_url(target.id(), docs_base_url)?;
    if let Some(parent) = out.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| {
            RototoError::new(format!(
                "failed to create package README directory {}: {err}",
                parent.display()
            ))
        })?;
    }
    tokio::fs::write(out, readme).await.map_err(|err| {
        RototoError::new(format!(
            "failed to write package README {}: {err}",
            out.display()
        ))
    })
}

fn print_package_readme_export(target: PackageReadmeTarget, out: &Path, json: bool) -> Result<()> {
    let out = out.display().to_string();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&PackageReadmeExportJson {
                sdk: target.id(),
                out
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
    } else {
        println!("generated {} package README at {out}", target.id());
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
                print!("{}", style::render_markdown(&markdown));
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
        println!(
            "  {}: {}",
            hit.line,
            highlight_docs_search(&hit.text, &hit.spans)
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn highlight_docs_search(text: &str, spans: &[DocsSearchSpan]) -> String {
    let mut highlighted = String::new();
    let mut cursor = 0;
    for span in spans {
        if span.start < cursor || span.start == span.end {
            continue;
        }
        highlighted.push_str(&text[cursor..span.start]);
        highlighted.push_str("\x1b[7m");
        highlighted.push_str(&text[span.start..span.end]);
        highlighted.push_str("\x1b[0m");
        cursor = span.end;
    }
    highlighted.push_str(&text[cursor..]);
    highlighted
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
        .filter(|found| found.start() < found.end())
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

    #[cfg(feature = "console")]
    {
        use tracing_subscriber::prelude::*;

        let (filter, handle) = tracing_subscriber::reload::Layer::new(filter);
        rototo::console::set_tracing_filter_reload_handle(handle);
        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(false)
                    .with_writer(std::io::stderr),
            )
            .init();
    }

    #[cfg(not(feature = "console"))]
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}
