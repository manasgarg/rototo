mod output;
mod style;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use regex::Regex;
use serde::Serialize;
use serde_json::Value as JsonValue;
use tempfile::TempDir;

use crate::output::{
    print_catalog_get, print_catalog_list, print_diagnostic_catalog_entry, print_inspect_report,
    print_package_diff, print_package_lint, print_qualifier_get, print_qualifier_list,
    print_variable_get, print_variable_list,
};
use rototo::diagnostics::{DiagnosticCatalogEntry, LintDiagnostic, SemanticEntity, Severity};
use rototo::model::{
    CatalogInspection, DiagnosticCatalog, EvaluationContextInspection, InspectSelection,
    LinterInspection, PackageInspectRequest, PackageInspection, PackageLint, QualifierInspection,
    QualifierResolutionTrace, VariableInspection, VariableResolutionTrace,
};
use rototo::package::{
    catalog_for_id, package_extends_sources, qualifier_for_id, read_catalog_json, read_toml,
    read_variable_toml, variable_for_id,
};
use rototo::{
    Result, RototoError, SourceAuth, SourceOptions, StagedPackage, diagnostic_for_rule,
    diagnostics_catalog, diagnostics_catalog_for_package, diff_packages, find_package_root,
    inspect_package, inspect_package_report, lint_package, stage_package_source,
    trace_qualifier_resolution, trace_variable_resolution,
};

#[derive(Debug, Parser)]
#[command(
    name = "rototo",
    version,
    about = "Control Git-backed runtime configuration packages",
    after_help = top_level_help(),
    override_usage = "rototo <command> [options]",
    help_template = top_level_help_template(),
    styles = help_styles()
)]
struct Cli {
    /// Emit machine-readable JSON.
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    json: bool,

    /// Bearer token for https:// package archive downloads.
    #[arg(
        long,
        global = true,
        env = "ROTOTO_PACKAGE_TOKEN",
        hide_env_values = true,
        value_name = "TOKEN"
    )]
    package_token: Option<String>,

    /// Suppress success output from lint commands.
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    quiet: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create package and entity templates.
    Init(InitArgs),
    /// Generate readable runtime behavior fixtures.
    Fixtures(FixturesArgs),
    /// Validate a package or selected targets.
    Lint(PackageCommandArgs),
    /// Explain how rototo sees package data.
    Inspect(InspectArgs),
    /// Compare package semantics across Git refs or the worktree.
    Diff(DiffArgs),
    /// Display package config, variables, qualifiers, and lint metadata.
    Show(PackageCommandArgs),
    /// Evaluate variables or qualifiers with runtime context.
    Resolve(ResolveArgs),
    /// Read bundled documentation.
    Docs(DocsArgs),
    /// Configure shell, editor, and agent integrations.
    Setup(SetupArgs),
    /// Serve the rototo console: web UI plus JSON API over a package.
    #[cfg(feature = "console")]
    Console(ConsoleArgs),
    /// Run the rototo Language Server Protocol server over stdio.
    Lsp,
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Local package path to initialize or modify.
    #[arg(value_name = "PACKAGE")]
    package: PathBuf,

    /// Create a qualifier template with this id.
    #[arg(long = "qualifier", value_name = "ID")]
    qualifier: Option<String>,

    /// Create a variable template with this id.
    #[arg(long = "variable", value_name = "ID")]
    variable: Option<String>,

    /// Create a catalog template with this id.
    #[arg(long = "catalog", value_name = "ID")]
    catalog: Option<String>,

    /// Create or infer the evaluation context schema template.
    #[arg(long = "evaluation-context", action = ArgAction::SetTrue)]
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
    /// Package source to generate fixtures from.
    #[arg(value_name = "PACKAGE_SOURCE")]
    package: String,

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
struct PackageCommandArgs {
    /// Package source. Defaults to the nearest parent with rototo-package.toml.
    #[arg(value_name = "PACKAGE_SOURCE")]
    package: Option<String>,

    #[command(flatten)]
    selectors: SelectorArgs,
}

#[derive(Debug, Args)]
struct InspectArgs {
    /// Package source. Defaults to the nearest parent with rototo-package.toml.
    #[arg(value_name = "PACKAGE_SOURCE")]
    package: Option<String>,

    #[command(flatten)]
    selectors: SelectorArgs,

    /// Evaluation context: JSON object, @file, or path=value. Repeatable; later values override earlier ones.
    #[arg(long = "context", value_name = "CONTEXT")]
    context: Vec<String>,
}

#[derive(Debug, Args)]
struct DiffArgs {
    /// Local package path. Defaults to the nearest parent with rototo-package.toml.
    #[arg(value_name = "PACKAGE")]
    package: Option<String>,

    /// Git ref used as the before side of the comparison. Defaults to HEAD.
    #[arg(long = "from", value_name = "REF")]
    from: Option<String>,

    /// Git ref used as the after side of the comparison. Defaults to the current worktree.
    #[arg(long = "to", value_name = "REF")]
    to: Option<String>,

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

    /// Select one package Lua linter id. Repeatable.
    #[arg(long = "linter", value_name = "ID")]
    linters: Vec<String>,

    /// Select all package Lua linters.
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
    /// Package source. Defaults to the nearest parent with rototo-package.toml.
    #[arg(value_name = "PACKAGE_SOURCE")]
    package: Option<String>,

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

#[derive(Debug, Args)]
struct SetupArgs {
    /// Set up every supported local integration.
    #[arg(long = "all", action = ArgAction::SetTrue)]
    all: bool,

    /// Shell completion target.
    #[arg(long = "shell", value_name = "SHELL", value_enum)]
    shell: Option<SetupShellArg>,

    /// Editor integration target.
    #[arg(long = "editor", value_name = "EDITOR", value_enum)]
    editor: Option<SetupEditorArg>,

    /// Agent guidance target.
    #[arg(long = "agent", value_name = "AGENT", value_enum)]
    agent: Option<SetupAgentArg>,

    /// Print generated setup content instead of writing files.
    #[arg(long = "print", action = ArgAction::SetTrue)]
    print: bool,

    /// Print planned setup changes without changing the filesystem.
    #[arg(long = "dry-run", action = ArgAction::SetTrue)]
    dry_run: bool,

    /// Overwrite rototo-owned generated setup files when they already exist.
    #[arg(long = "force", action = ArgAction::SetTrue)]
    force: bool,
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

    /// Package source to register at startup.
    #[arg(long = "package", value_name = "PACKAGE_SOURCE")]
    package: Option<String>,

    /// Console state persistence mode. Defaults to ephemeral for local folder
    /// packages and persistent otherwise.
    #[arg(long = "state", value_enum)]
    state: Option<ConsoleStateArg>,

    /// Console deployment mode. Defaults to local with --package, hosted otherwise.
    #[arg(long = "deployment", value_enum)]
    deployment: Option<ConsoleDeploymentArg>,

    /// Write behavior for console branch edits. Defaults to direct-push for local
    /// fixed packages and pull-request otherwise.
    #[arg(long = "write", value_enum)]
    write: Option<ConsoleWriteArg>,
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
enum ConsoleStateArg {
    Ephemeral,
    Persistent,
}

#[cfg(feature = "console")]
impl From<ConsoleStateArg> for rototo::console::ConsoleStateMode {
    fn from(value: ConsoleStateArg) -> Self {
        match value {
            ConsoleStateArg::Ephemeral => Self::Ephemeral,
            ConsoleStateArg::Persistent => Self::Persistent,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum SetupShellArg {
    Auto,
    Bash,
    Elvish,
    Fish,
    #[value(name = "powershell", alias = "power-shell")]
    PowerShell,
    Zsh,
    None,
}

impl SetupShellArg {
    fn completion_shell(self) -> Option<Shell> {
        match self {
            SetupShellArg::Auto | SetupShellArg::None => None,
            SetupShellArg::Bash => Some(Shell::Bash),
            SetupShellArg::Elvish => Some(Shell::Elvish),
            SetupShellArg::Fish => Some(Shell::Fish),
            SetupShellArg::PowerShell => Some(Shell::PowerShell),
            SetupShellArg::Zsh => Some(Shell::Zsh),
        }
    }

    fn label(self) -> &'static str {
        match self {
            SetupShellArg::Auto => "auto",
            SetupShellArg::Bash => "bash",
            SetupShellArg::Elvish => "elvish",
            SetupShellArg::Fish => "fish",
            SetupShellArg::PowerShell => "powershell",
            SetupShellArg::Zsh => "zsh",
            SetupShellArg::None => "none",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum SetupEditorArg {
    All,
    Neovim,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum SetupAgentArg {
    All,
    Claude,
    Codex,
    None,
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
        "diff examples/basic --context @examples/basic/evaluation-contexts/request-samples/premium-enterprise.json",
        "resolve examples/basic --variable checkout-redesign --context lane=prod --context user.tier=premium",
        "docs -p motivation",
        "setup --shell zsh",
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
    out.push_str(&style::bold("Package commands:"));
    out.push('\n');
    out.push_str(&command("init", "Create package and entity templates"));
    out.push_str(&command(
        "fixtures",
        "Generate readable runtime behavior fixtures",
    ));
    out.push_str(&command("lint", "Validate a package or selected targets"));
    out.push_str(&command("inspect", "Explain how rototo sees package data"));
    out.push_str(&command(
        "diff",
        "Compare package semantics across Git refs or the worktree",
    ));
    out.push_str(&command(
        "show",
        "Display package config, variables, qualifiers, and lint metadata",
    ));
    out.push_str(&command(
        "resolve",
        "Evaluate variables or qualifiers with runtime context",
    ));
    out.push('\n');
    out.push_str(&style::bold("Utility commands:"));
    out.push('\n');
    out.push_str(&command("docs", "Read bundled documentation"));
    out.push_str(&command(
        "setup",
        "Configure shell, editor, and agent integrations",
    ));
    #[cfg(feature = "console")]
    out.push_str(&command("console", "Serve the web console and JSON API"));
    out.push_str(&command("lsp", "Run the language server over stdio"));
    out.push_str(&command(
        "help",
        "Print this message or the help of the given subcommand(s)",
    ));
    out.push('\n');
    out.push_str(&style::bold("Global options:"));
    out.push('\n');
    out.push_str(&flag("--json"));
    out.push_str(&flag("--quiet"));
    out.push_str(&flag("--package-token <token>"));
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
        Command::Setup(args) => run_setup(args, cli.json, cli.quiet).await,
        #[cfg(feature = "console")]
        Command::Console(args) => {
            rototo::console::run(rototo::console::ConsoleOptions {
                bind: args.bind,
                public_url: args.public_url,
                data_dir: args.data_dir,
                package: args.package,
                state_mode: args.state.map(Into::into),
                deployment: args.deployment.map(Into::into),
                write_policy: args.write.map(Into::into),
                package_token: cli.package_token.clone(),
            })
            .await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Lsp => {
            rototo::lsp::serve_stdio().await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SetupTarget {
    Shell(SetupShellArg),
    Neovim,
    Claude,
    Codex,
}

#[derive(Debug, Serialize)]
struct SetupReport {
    command: &'static str,
    dry_run: bool,
    changes: Vec<SetupChange>,
}

#[derive(Debug, Serialize)]
struct SetupChange {
    target: &'static str,
    status: SetupStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum SetupStatus {
    Changed,
    Unchanged,
    NeedsManualStep,
}

impl SetupStatus {
    fn label(self, dry_run: bool) -> &'static str {
        match (self, dry_run) {
            (Self::Changed, true) => "would change",
            (Self::Changed, false) => "changed",
            (Self::Unchanged, _) => "unchanged",
            (Self::NeedsManualStep, _) => "needs_manual_step",
        }
    }
}

struct SetupRunOptions {
    dry_run: bool,
    force: bool,
}

const SETUP_GENERATED_MARKER: &str = "Generated by rototo setup. Do not edit manually.";
const AGENT_SETUP_BEGIN: &str = "<!-- BEGIN rototo setup -->";
const AGENT_SETUP_END: &str = "<!-- END rototo setup -->";

async fn run_setup(args: SetupArgs, json: bool, quiet: bool) -> Result<ExitCode> {
    let targets = setup_targets(&args)?;
    if args.print {
        print_setup_targets(&targets)?;
        return Ok(ExitCode::SUCCESS);
    }

    let options = SetupRunOptions {
        dry_run: args.dry_run,
        force: args.force,
    };
    let mut changes = Vec::new();
    for target in targets {
        match target {
            SetupTarget::Shell(shell) => setup_shell(shell, &options, &mut changes).await?,
            SetupTarget::Neovim => setup_neovim(&options, &mut changes).await?,
            SetupTarget::Claude => {
                setup_agent("CLAUDE.md", "claude-guidance", &options, &mut changes).await?
            }
            SetupTarget::Codex => {
                setup_agent("AGENTS.md", "codex-guidance", &options, &mut changes).await?
            }
        }
    }

    print_setup_report(
        &SetupReport {
            command: "setup",
            dry_run: options.dry_run,
            changes,
        },
        json,
        quiet,
    )?;
    Ok(ExitCode::SUCCESS)
}

fn setup_targets(args: &SetupArgs) -> Result<Vec<SetupTarget>> {
    let explicit =
        args.all || args.shell.is_some() || args.editor.is_some() || args.agent.is_some();
    if !explicit {
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return Err(RototoError::new(
                "rototo setup needs a terminal. Use --all or select targets with flags.",
            ));
        }
        return interactive_setup_targets();
    }

    let mut targets = Vec::new();
    if args.all {
        add_target(&mut targets, SetupTarget::Shell(SetupShellArg::Auto));
        add_target(&mut targets, SetupTarget::Neovim);
        add_target(&mut targets, SetupTarget::Claude);
        add_target(&mut targets, SetupTarget::Codex);
    }

    if let Some(shell) = args.shell {
        targets.retain(|target| !matches!(target, SetupTarget::Shell(_)));
        if shell != SetupShellArg::None {
            add_target(&mut targets, SetupTarget::Shell(shell));
        }
    }

    if let Some(editor) = args.editor {
        targets.retain(|target| !matches!(target, SetupTarget::Neovim));
        match editor {
            SetupEditorArg::All => {
                add_target(&mut targets, SetupTarget::Neovim);
            }
            SetupEditorArg::Neovim => add_target(&mut targets, SetupTarget::Neovim),
            SetupEditorArg::None => {}
        }
    }

    if let Some(agent) = args.agent {
        targets.retain(|target| !matches!(target, SetupTarget::Claude | SetupTarget::Codex));
        match agent {
            SetupAgentArg::All => {
                add_target(&mut targets, SetupTarget::Claude);
                add_target(&mut targets, SetupTarget::Codex);
            }
            SetupAgentArg::Claude => add_target(&mut targets, SetupTarget::Claude),
            SetupAgentArg::Codex => add_target(&mut targets, SetupTarget::Codex),
            SetupAgentArg::None => {}
        }
    }

    if args.print && targets.len() != 1 {
        return Err(RototoError::new(
            "rototo setup --print requires exactly one explicit target",
        ));
    }

    Ok(targets)
}

fn interactive_setup_targets() -> Result<Vec<SetupTarget>> {
    let mut targets = Vec::new();
    if prompt_setup_target("Shell completions", true)? {
        add_target(&mut targets, SetupTarget::Shell(SetupShellArg::Auto));
    }
    if prompt_setup_target("Neovim LSP", true)? {
        add_target(&mut targets, SetupTarget::Neovim);
    }
    if prompt_setup_target("Claude agent guidance", true)? {
        add_target(&mut targets, SetupTarget::Claude);
    }
    if prompt_setup_target("Codex agent guidance", true)? {
        add_target(&mut targets, SetupTarget::Codex);
    }
    Ok(targets)
}

fn prompt_setup_target(label: &str, default: bool) -> Result<bool> {
    use std::io::Write;

    let suffix = if default { "Y/n" } else { "y/N" };
    print!("{label}? [{suffix}] ");
    std::io::stdout()
        .flush()
        .map_err(|err| RototoError::new(format!("failed to write setup prompt: {err}")))?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|err| RototoError::new(format!("failed to read setup prompt: {err}")))?;
    let answer = answer.trim().to_ascii_lowercase();
    match answer.as_str() {
        "" => Ok(default),
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => Err(RototoError::new(format!(
            "invalid setup answer for {label}: expected yes or no"
        ))),
    }
}

fn add_target(targets: &mut Vec<SetupTarget>, target: SetupTarget) {
    if !targets.contains(&target) {
        targets.push(target);
    }
}

fn print_setup_targets(targets: &[SetupTarget]) -> Result<()> {
    let target = targets.first().ok_or_else(|| {
        RototoError::new("rototo setup --print requires exactly one explicit target")
    })?;
    match *target {
        SetupTarget::Shell(shell) => {
            let shell = resolve_setup_shell(shell)?;
            let completion_shell = shell.completion_shell().ok_or_else(|| {
                RototoError::new("setup --print requires a concrete shell target")
            })?;
            print!("{}", completion_script(shell, completion_shell)?);
        }
        SetupTarget::Neovim => print!("{}", neovim_lsp_config()),
        SetupTarget::Claude | SetupTarget::Codex => print!("{}", agent_guidance_block()),
    }
    Ok(())
}

async fn setup_shell(
    shell: SetupShellArg,
    options: &SetupRunOptions,
    changes: &mut Vec<SetupChange>,
) -> Result<()> {
    let shell = resolve_setup_shell(shell)?;
    if shell == SetupShellArg::PowerShell {
        changes.push(setup_change(
            "shell-completions",
            SetupStatus::NeedsManualStep,
            None,
            Some(
                "run `rototo setup --shell powershell --print` and add the output to your PowerShell profile",
            ),
        ));
        return Ok(());
    }

    let completion_shell = shell.completion_shell().ok_or_else(|| {
        RototoError::new(format!("unsupported shell setup target: {}", shell.label()))
    })?;
    let path = shell_completion_path(shell)?;
    let content = completion_script(shell, completion_shell)?;
    let status = write_generated_file(&path, &content, options).await?;
    changes.push(setup_change(
        "shell-completions",
        status,
        Some(path.display().to_string()),
        None,
    ));

    if shell == SetupShellArg::Zsh {
        let completion_dir = path.parent().unwrap_or(&path).display();
        let message = format!(
            "add this near the top of your zsh profile: fpath=(\"{completion_dir}\" $fpath), then restart zsh",
        );
        changes.push(setup_change(
            "zsh-profile",
            SetupStatus::NeedsManualStep,
            None,
            Some(&message),
        ));
    }
    Ok(())
}

fn resolve_setup_shell(shell: SetupShellArg) -> Result<SetupShellArg> {
    match shell {
        SetupShellArg::Auto => detect_setup_shell(),
        SetupShellArg::None => Err(RototoError::new("no shell setup target selected")),
        shell => Ok(shell),
    }
}

fn detect_setup_shell() -> Result<SetupShellArg> {
    let shell = std::env::var_os("SHELL")
        .ok_or_else(|| RototoError::new("could not detect shell from SHELL; pass --shell"))?;
    let name = Path::new(&shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    match name {
        "bash" => Ok(SetupShellArg::Bash),
        "elvish" => Ok(SetupShellArg::Elvish),
        "fish" => Ok(SetupShellArg::Fish),
        "powershell" | "pwsh" => Ok(SetupShellArg::PowerShell),
        "zsh" => Ok(SetupShellArg::Zsh),
        _ => Err(RototoError::new(format!(
            "could not detect a supported shell from SHELL={}; pass --shell",
            shell.to_string_lossy()
        ))),
    }
}

fn shell_completion_path(shell: SetupShellArg) -> Result<PathBuf> {
    match shell {
        SetupShellArg::Bash => {
            Ok(home_dir()?.join(".local/share/bash-completion/completions/rototo"))
        }
        SetupShellArg::Elvish => Ok(home_dir()?.join(".elvish/lib/rototo-completions.elv")),
        SetupShellArg::Fish => Ok(config_home()?.join("fish/completions/rototo.fish")),
        SetupShellArg::Zsh => Ok(zdotdir()?.join(".zfunc/_rototo")),
        SetupShellArg::Auto | SetupShellArg::PowerShell | SetupShellArg::None => {
            Err(RototoError::new(format!(
                "unsupported shell completion path: {}",
                shell.label()
            )))
        }
    }
}

fn completion_script(shell: SetupShellArg, completion_shell: Shell) -> Result<String> {
    let mut command = Cli::command();
    let name = command.get_name().to_owned();
    let mut bytes = Vec::new();
    generate(completion_shell, &mut command, name, &mut bytes);
    let script = String::from_utf8(bytes)
        .map_err(|err| RototoError::new(format!("generated completion was not utf-8: {err}")))?;
    Ok(with_generated_marker(shell, script))
}

fn with_generated_marker(shell: SetupShellArg, script: String) -> String {
    let marker = format!("# {SETUP_GENERATED_MARKER}\n");
    if shell == SetupShellArg::Zsh
        && script
            .lines()
            .next()
            .is_some_and(|line| line.starts_with("#compdef "))
        && let Some((first, rest)) = script.split_once('\n')
    {
        return format!("{first}\n{marker}{rest}");
    }
    format!("{marker}{script}")
}

async fn setup_neovim(options: &SetupRunOptions, changes: &mut Vec<SetupChange>) -> Result<()> {
    let nvim = config_home()?.join("nvim");
    let rototo_lua = nvim.join("lua/rototo.lua");
    let status = write_generated_file(&rototo_lua, &neovim_lsp_config(), options).await?;
    changes.push(setup_change(
        "neovim-lsp",
        status,
        Some(rototo_lua.display().to_string()),
        None,
    ));

    let init_lua = nvim.join("init.lua");
    let init_vim = nvim.join("init.vim");
    let (init_path, line, alternatives) =
        if path_exists(&init_lua).await? || !path_exists(&init_vim).await? {
            (
                init_lua,
                "require(\"rototo\")",
                vec!["require(\"rototo\")", "require('rototo')"],
            )
        } else {
            (
                init_vim,
                "lua require(\"rototo\")",
                vec!["lua require(\"rototo\")", "lua require('rototo')"],
            )
        };
    let status = ensure_config_line(&init_path, line, &alternatives, options.dry_run).await?;
    changes.push(setup_change(
        "neovim-init",
        status,
        Some(init_path.display().to_string()),
        None,
    ));
    Ok(())
}

fn neovim_lsp_config() -> String {
    let marker = format!("-- {SETUP_GENERATED_MARKER}");
    format!(
        r#"{marker}
local root_markers = {{ "rototo-package.toml" }}

local function rototo_root(path)
  local marker = vim.fs.find(root_markers, {{ upward = true, path = vim.fs.dirname(path) }})[1]
  if marker == nil then
    return nil
  end
  return vim.fs.dirname(marker)
end

vim.api.nvim_create_autocmd("FileType", {{
  pattern = {{ "toml", "json", "lua" }},
  callback = function(event)
    local root = rototo_root(vim.api.nvim_buf_get_name(event.buf))
    if root == nil then
      return
    end
    vim.lsp.start({{
      name = "rototo",
      cmd = {{ "rototo", "lsp" }},
      root_dir = root,
    }}, {{ bufnr = event.buf }})
  end,
}})
"#,
    )
}

async fn setup_agent(
    file_name: &str,
    target: &'static str,
    options: &SetupRunOptions,
    changes: &mut Vec<SetupChange>,
) -> Result<()> {
    let path = nearest_agent_file(file_name).await?;
    let status =
        upsert_managed_markdown_block(&path, &agent_guidance_block(), options.dry_run).await?;
    changes.push(setup_change(
        target,
        status,
        Some(path.display().to_string()),
        None,
    ));
    Ok(())
}

async fn nearest_agent_file(file_name: &str) -> Result<PathBuf> {
    let start = std::env::current_dir()
        .map_err(|err| RototoError::new(format!("failed to read current directory: {err}")))?;
    let mut dir = start.as_path();
    loop {
        let candidate = dir.join(file_name);
        if path_exists(&candidate).await? {
            return Ok(candidate);
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return Ok(start.join(file_name)),
        }
    }
}

fn agent_guidance_block() -> String {
    format!(
        r#"{AGENT_SETUP_BEGIN}
## rototo

When changing runtime behavior, use rototo package files as the control plane. Use `rototo docs` for concepts, `rototo inspect` to understand package shape, `rototo resolve` with realistic context to verify behavior, and `rototo lint` before finishing. Use `rototo lsp` in editors when available.
{AGENT_SETUP_END}
"#
    )
}

async fn write_generated_file(
    path: &Path,
    content: &str,
    options: &SetupRunOptions,
) -> Result<SetupStatus> {
    match tokio::fs::read_to_string(path).await {
        Ok(existing) if existing == content => Ok(SetupStatus::Unchanged),
        Ok(existing) => {
            if !options.force && !existing.contains(SETUP_GENERATED_MARKER) {
                return Err(RototoError::new(format!(
                    "file already exists with different content: {} (use --force to overwrite)",
                    path.display()
                )));
            }
            if !options.dry_run {
                write_file_creating_parent(path, content).await?;
            }
            Ok(SetupStatus::Changed)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if !options.dry_run {
                write_file_creating_parent(path, content).await?;
            }
            Ok(SetupStatus::Changed)
        }
        Err(err) => Err(RototoError::new(format!(
            "failed to read {}: {err}",
            path.display()
        ))),
    }
}

async fn ensure_config_line(
    path: &Path,
    line: &str,
    alternatives: &[&str],
    dry_run: bool,
) -> Result<SetupStatus> {
    match tokio::fs::read_to_string(path).await {
        Ok(existing) => {
            if alternatives
                .iter()
                .any(|alternative| existing.contains(alternative))
            {
                return Ok(SetupStatus::Unchanged);
            }
            let mut updated = existing;
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(line);
            updated.push('\n');
            if !dry_run {
                write_file_creating_parent(path, &updated).await?;
            }
            Ok(SetupStatus::Changed)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if !dry_run {
                write_file_creating_parent(path, &format!("{line}\n")).await?;
            }
            Ok(SetupStatus::Changed)
        }
        Err(err) => Err(RototoError::new(format!(
            "failed to read {}: {err}",
            path.display()
        ))),
    }
}

async fn upsert_managed_markdown_block(
    path: &Path,
    block: &str,
    dry_run: bool,
) -> Result<SetupStatus> {
    match tokio::fs::read_to_string(path).await {
        Ok(existing) => {
            let updated = replace_or_append_managed_block(&existing, block)?;
            if updated == existing {
                return Ok(SetupStatus::Unchanged);
            }
            if !dry_run {
                write_file_creating_parent(path, &updated).await?;
            }
            Ok(SetupStatus::Changed)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if !dry_run {
                write_file_creating_parent(path, block).await?;
            }
            Ok(SetupStatus::Changed)
        }
        Err(err) => Err(RototoError::new(format!(
            "failed to read {}: {err}",
            path.display()
        ))),
    }
}

fn replace_or_append_managed_block(existing: &str, block: &str) -> Result<String> {
    match (
        existing.find(AGENT_SETUP_BEGIN),
        existing.find(AGENT_SETUP_END),
    ) {
        (Some(begin), Some(end)) if begin <= end => {
            let after_end = end + AGENT_SETUP_END.len();
            let mut updated = String::new();
            updated.push_str(&existing[..begin]);
            updated.push_str(block);
            updated.push_str(existing[after_end..].trim_start_matches('\n'));
            Ok(updated)
        }
        (None, None) => {
            let mut updated = existing.to_owned();
            if !updated.is_empty() {
                if !updated.ends_with('\n') {
                    updated.push('\n');
                }
                updated.push('\n');
            }
            updated.push_str(block);
            Ok(updated)
        }
        _ => Err(RototoError::new(
            "existing agent instructions contain an incomplete rototo setup block",
        )),
    }
}

async fn write_file_creating_parent(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| {
            RototoError::new(format!(
                "failed to create directory {}: {err}",
                parent.display()
            ))
        })?;
    }
    tokio::fs::write(path, content)
        .await
        .map_err(|err| RototoError::new(format!("failed to write {}: {err}", path.display())))
}

fn setup_change(
    target: &'static str,
    status: SetupStatus,
    path: Option<String>,
    message: Option<&str>,
) -> SetupChange {
    SetupChange {
        target,
        status,
        path,
        message: message.map(str::to_owned),
    }
}

fn print_setup_report(report: &SetupReport, json: bool, quiet: bool) -> Result<()> {
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

    println!("rototo setup");
    println!();
    for change in &report.changes {
        let label = change.status.label(report.dry_run);
        match (&change.path, &change.message) {
            (Some(path), Some(message)) => {
                println!("{label:<17} {}: {path} ({message})", change.target)
            }
            (Some(path), None) => println!("{label:<17} {}: {path}", change.target),
            (None, Some(message)) => println!("{label:<17} {}: {message}", change.target),
            (None, None) => println!("{label:<17} {}", change.target),
        }
    }
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    for name in ["HOME", "USERPROFILE"] {
        if let Some(value) = std::env::var_os(name).filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(value));
        }
    }
    Err(RototoError::new(
        "could not find a home directory from HOME or USERPROFILE",
    ))
}

fn config_home() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(value));
    }
    Ok(home_dir()?.join(".config"))
}

fn zdotdir() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("ZDOTDIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(value));
    }
    home_dir()
}

async fn run_init(args: InitArgs, json: bool, quiet: bool) -> Result<ExitCode> {
    let package = local_init_package_path(&args.package)?;
    let target = init_target(&args)?;
    let plan = build_init_plan(&package, target).await?;
    let report = execute_init_plan(&package, &plan, args.force, args.dry_run).await?;
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
        rototo::fixtures::generate_fixture_suite(&args.package, source_options, selection).await?;
    let report = suite.write_to(&args.out).await?;
    print_fixtures_report(&suite.package, &report, json, quiet)?;
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
    package: &'a str,
    out: &'a str,
    files: &'a [String],
}

fn print_fixtures_report(
    package: &str,
    report: &rototo::fixtures::FixtureWriteReport,
    json: bool,
    quiet: bool,
) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&FixturesReport {
                command: "fixtures",
                package,
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
    Package,
    Qualifier(String),
    Variable(String),
    Catalog(String),
    Context,
}

fn init_target(args: &InitArgs) -> Result<InitTarget> {
    let mut count = 0;
    let mut target = InitTarget::Package;

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
            "init accepts one entity flag at a time: --qualifier, --variable, --catalog, or --evaluation-context",
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

fn local_init_package_path(path: &Path) -> Result<PathBuf> {
    let source = path.to_string_lossy();
    if source.contains("://") || source.starts_with("git+") {
        return Err(RototoError::new(
            "init requires a local package path, not a package source URI",
        ));
    }

    std::path::absolute(path)
        .map_err(|err| RototoError::new(format!("failed to resolve package path: {err}")))
}

async fn build_init_plan(package: &Path, target: InitTarget) -> Result<Vec<InitPlanEntry>> {
    let initialized = package_initialized(package).await?;
    match target {
        InitTarget::Package => Ok(package_init_plan(package)),
        InitTarget::Qualifier(id) => {
            let mut plan = implicit_package_init_plan(package, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(package.join("qualifiers")));
            }
            plan.push(InitPlanEntry::file(
                "qualifier",
                package.join("qualifiers").join(format!("{id}.toml")),
                qualifier_template(&id),
            ));
            Ok(plan)
        }
        InitTarget::Variable(id) => {
            let mut plan = implicit_package_init_plan(package, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(package.join("variables")));
            }
            plan.push(InitPlanEntry::file(
                "variable",
                package.join("variables").join(format!("{id}.toml")),
                variable_template(&id),
            ));
            Ok(plan)
        }
        InitTarget::Catalog(id) => {
            let mut plan = implicit_package_init_plan(package, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(package.join("catalogs")));
            }
            plan.extend([
                InitPlanEntry::directory(package.join("catalogs").join(format!("{id}-entries"))),
                InitPlanEntry::file(
                    "catalog",
                    package.join("catalogs").join(format!("{id}.schema.json")),
                    catalog_schema_template()?,
                ),
                InitPlanEntry::file(
                    "catalog_entry",
                    package
                        .join("catalogs")
                        .join(format!("{id}-entries"))
                        .join("default.toml"),
                    catalog_entry_template(),
                ),
            ]);
            Ok(plan)
        }
        InitTarget::Context => {
            let mut plan = implicit_package_init_plan(package, initialized);
            if initialized {
                plan.push(InitPlanEntry::directory(
                    package.join("evaluation-contexts"),
                ));
            }
            let content = if initialized {
                context_schema_template(package).await?
            } else {
                starter_context_schema_template()?
            };
            plan.push(InitPlanEntry::file(
                "evaluation_context",
                package
                    .join("evaluation-contexts")
                    .join("request.schema.json"),
                content,
            ));
            Ok(plan)
        }
    }
}

fn implicit_package_init_plan(package: &Path, initialized: bool) -> Vec<InitPlanEntry> {
    if initialized {
        Vec::new()
    } else {
        package_init_plan(package)
    }
}

fn package_init_plan(package: &Path) -> Vec<InitPlanEntry> {
    vec![
        InitPlanEntry::directory(package.to_path_buf()),
        InitPlanEntry::file(
            "package_manifest",
            package.join("rototo-package.toml"),
            package_manifest_template(),
        ),
        InitPlanEntry::directory(package.join("qualifiers")),
        InitPlanEntry::directory(package.join("variables")),
        InitPlanEntry::directory(package.join("catalogs")),
        InitPlanEntry::directory(package.join("evaluation-contexts")),
        InitPlanEntry::directory(package.join("lint")),
    ]
}

async fn package_initialized(package: &Path) -> Result<bool> {
    path_exists(&package.join("rototo-package.toml")).await
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
    package: String,
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
    package: &Path,
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
        package: package.display().to_string(),
        dry_run,
        files: plan
            .iter()
            .zip(actions)
            .map(|(entry, action)| InitFileReport {
                kind: entry.kind,
                path: init_report_path(package, &entry.path),
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

fn init_report_path(package: &Path, path: &Path) -> String {
    match path.strip_prefix(package) {
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

    println!("package: {}", report.package);
    for file in &report.files {
        println!(
            "  {:<15} {}",
            format!("{}:", file.action.label()),
            file.path
        );
    }
    Ok(())
}

fn package_manifest_template() -> String {
    r#"schema_version = 1

# Optional package layering:
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

when = "context.user.tier == \"premium\""

# Compose multiple conditions with boolean operators.
#
# when = 'context.user.tier == "premium" && context.request.country in ["DE", "FR", "NL"]'
#
# Qualifiers can reference other qualifiers.
#
# when = 'qualifier["beta-rollout"] && context.user.tier == "premium"'
#
# bucket(...) produces stable rollout membership for a context value.
#
# when = 'bucket(context.user.id, "{id}-rollout", 0, 1000)'
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

[resolve]
default = "control"

# Rules are evaluated in order. The first matching condition selects its value.
#
# [[resolve.rule]]
# when = 'qualifier["premium-users"]'
# value = "treatment"
#
# For catalog-backed values, use a catalog type:
#
# type = "catalog:{id}"
#
# Catalog value names become the selectable values.
"#
    )
}

fn catalog_schema_template() -> Result<String> {
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "description": "Edit this description to explain the catalog values",
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

async fn context_schema_template(package: &Path) -> Result<String> {
    let report = inspect_package_report(
        package,
        PackageInspectRequest {
            qualifiers: InspectSelection::All,
            ..PackageInspectRequest::default()
        },
    )
    .await?;

    let mut builder = ContextSchemaBuilder::default();
    for qualifier in &report.qualifiers {
        for path in &qualifier.dependencies.context_paths {
            builder.add_context_path(path);
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
    fn add_context_path(&mut self, path: &str) {
        let segments = path.split('.').collect::<Vec<_>>();
        if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
            return;
        }

        let types = BTreeSet::from([ContextSchemaType::String]);
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
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array,
    Object,
}

impl ContextSchemaType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
        }
    }
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
        "array" => Some(ContextSchemaType::Array),
        "object" => Some(ContextSchemaType::Object),
        "null" => Some(ContextSchemaType::Null),
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

async fn package_source_for_lint(
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

async fn local_package_has_valid_extends(path: &Path) -> bool {
    let manifest = match read_toml(&path.join("rototo-package.toml")).await {
        Ok(manifest) => manifest,
        Err(_) => return false,
    };
    package_extends_sources(&manifest).is_ok_and(|sources| !sources.is_empty())
}

async fn run_inspect(
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

struct DiffPackageSide {
    package: StagedPackage,
    label: String,
    _snapshot: Option<TempDir>,
}

async fn run_diff(args: DiffArgs, source_options: &SourceOptions, json: bool) -> Result<ExitCode> {
    let package_root = local_diff_package_path(args.package).await?;
    let git = local_git_context(&package_root, source_options).await?;
    let from_ref = args.from.unwrap_or_else(|| "HEAD".to_owned());
    validate_diff_git_ref(&from_ref)?;
    let before = stage_git_ref_diff_side(&git, &from_ref, source_options).await?;
    let after = match args.to {
        Some(to_ref) => {
            validate_diff_git_ref(&to_ref)?;
            stage_git_ref_diff_side(&git, &to_ref, source_options).await?
        }
        None => stage_worktree_diff_side(&git, source_options).await?,
    };
    let context = if args.context.is_empty() {
        None
    } else {
        Some(parse_context(&args.context).await?)
    };
    let mut diff = diff_packages(
        before.package.path(),
        after.package.path(),
        context.as_ref(),
    )
    .await?;
    diff.before = before.label;
    diff.after = after.label;
    print_package_diff(&diff, json)?;
    Ok(ExitCode::SUCCESS)
}

struct LocalGitContext {
    repo_root: PathBuf,
    package_root: PathBuf,
    package_relative: PathBuf,
    package_label: String,
}

async fn local_diff_package_path(package: Option<String>) -> Result<PathBuf> {
    match package {
        Some(package) => {
            if package.contains("://") || package.starts_with("git+") {
                return Err(RototoError::new(
                    "diff currently requires a local package path; use --from and --to with local Git refs",
                ));
            }
            tokio::fs::canonicalize(&package)
                .await
                .map_err(|err| RototoError::new(format!("failed to resolve package path: {err}")))
        }
        None => {
            let current_dir = tokio::task::spawn_blocking(std::env::current_dir)
                .await
                .map_err(|err| RototoError::new(format!("current directory task failed: {err}")))?
                .map_err(|err| {
                    RototoError::new(format!("failed to read current directory: {err}"))
                })?;
            find_package_root(&current_dir).await
        }
    }
}

async fn local_git_context(
    package_root: &Path,
    source_options: &SourceOptions,
) -> Result<LocalGitContext> {
    let repo_root = git_stdout(
        package_root,
        &["rev-parse", "--show-toplevel"],
        source_options,
        "find repository root",
    )
    .await?;
    let repo_root = PathBuf::from(String::from_utf8_lossy(&repo_root).trim());
    let repo_root = tokio::fs::canonicalize(&repo_root).await.map_err(|err| {
        RototoError::new(format!(
            "failed to resolve Git repository root {}: {err}",
            repo_root.display()
        ))
    })?;
    let package_root = tokio::fs::canonicalize(package_root).await.map_err(|err| {
        RototoError::new(format!(
            "failed to resolve package path {}: {err}",
            package_root.display()
        ))
    })?;
    let package_relative = package_root.strip_prefix(&repo_root).map_err(|_| {
        RototoError::new(format!(
            "package {} is not inside Git repository {}",
            package_root.display(),
            repo_root.display()
        ))
    })?;
    let package_relative = package_relative.to_path_buf();
    let package_label = git_path_label(&package_relative);
    Ok(LocalGitContext {
        repo_root,
        package_root,
        package_relative,
        package_label,
    })
}

async fn stage_git_ref_diff_side(
    git: &LocalGitContext,
    ref_: &str,
    source_options: &SourceOptions,
) -> Result<DiffPackageSide> {
    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let extract_root = tempdir.path().join("repo");
    tokio::fs::create_dir_all(&extract_root)
        .await
        .map_err(|err| {
            RototoError::new(format!(
                "failed to create Git snapshot directory {}: {err}",
                extract_root.display()
            ))
        })?;

    let mut args = vec![
        "archive".to_owned(),
        "--format=tar".to_owned(),
        ref_.to_owned(),
        "--".to_owned(),
    ];
    if !git.package_relative.as_os_str().is_empty() {
        args.push(git_path_label(&git.package_relative));
    }
    let archive = git_stdout_owned(&git.repo_root, &args, source_options, "archive ref").await?;
    let extract_root_for_task = extract_root.clone();
    tokio::task::spawn_blocking(move || {
        let mut archive = tar::Archive::new(Cursor::new(archive));
        archive.unpack(&extract_root_for_task).map_err(|err| {
            RototoError::new(format!(
                "failed to extract Git snapshot {}: {err}",
                extract_root_for_task.display()
            ))
        })
    })
    .await
    .map_err(|err| RototoError::new(format!("Git snapshot extraction task failed: {err}")))??;

    let package_root = if git.package_relative.as_os_str().is_empty() {
        extract_root
    } else {
        extract_root.join(&git.package_relative)
    };
    if !tokio::fs::metadata(package_root.join("rototo-package.toml"))
        .await
        .is_ok_and(|metadata| metadata.is_file())
    {
        return Err(RototoError::new(format!(
            "package {} was not found at Git ref {ref_}",
            git.package_label
        )));
    }
    let label = format!("{ref_}:{}", git.package_label);
    stage_diff_side_from_path(package_root, label, Some(tempdir), source_options).await
}

async fn stage_worktree_diff_side(
    git: &LocalGitContext,
    source_options: &SourceOptions,
) -> Result<DiffPackageSide> {
    stage_diff_side_from_path(
        git.package_root.clone(),
        format!("worktree:{}", git.package_label),
        None,
        source_options,
    )
    .await
}

async fn stage_diff_side_from_path(
    package_root: PathBuf,
    label: String,
    snapshot: Option<TempDir>,
    source_options: &SourceOptions,
) -> Result<DiffPackageSide> {
    let package = package_source_for_lint(
        Some(package_root.to_string_lossy().into_owned()),
        source_options,
    )
    .await?;
    Ok(DiffPackageSide {
        package,
        label,
        _snapshot: snapshot,
    })
}

async fn git_stdout(
    current_dir: &Path,
    args: &[&str],
    source_options: &SourceOptions,
    operation: &str,
) -> Result<Vec<u8>> {
    let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    git_stdout_owned(current_dir, &args, source_options, operation).await
}

async fn git_stdout_owned(
    current_dir: &Path,
    args: &[String],
    source_options: &SourceOptions,
    operation: &str,
) -> Result<Vec<u8>> {
    let mut command = tokio::process::Command::new("git");
    command.kill_on_drop(true);
    command.current_dir(current_dir);
    for arg in args {
        command.arg(arg);
    }
    scrub_git_process_variables(&mut command);
    let output = tokio::time::timeout(source_options.git_timeout(), command.output())
        .await
        .map_err(|_| RototoError::new(format!("git {operation} timed out")))?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                RototoError::new("required tool `git` was not found on PATH")
            } else {
                RototoError::new(format!("failed to run git: {err}"))
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git {operation} failed: {}",
            stderr.trim()
        )));
    }
    Ok(output.stdout)
}

fn validate_diff_git_ref(ref_: &str) -> Result<()> {
    if ref_.starts_with('-') {
        return Err(RototoError::new(format!(
            "diff git ref must not begin with '-': {ref_}"
        )));
    }
    Ok(())
}

fn git_path_label(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

fn scrub_git_process_variables(command: &mut tokio::process::Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    for key in [
        "GIT_INDEX_FILE",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
}

async fn run_show(
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
    let package = package_source_or_current(args.package, source_options).await?;
    let inspection = inspect_package(package.path()).await?;
    let catalog = diagnostics_catalog_for_package(package.path()).await?;
    validate_package_selectors(&selectors, &inspection, &catalog)?;

    if args.context.is_empty() {
        let model = rototo::lint::package_semantic_model(package.path()).await?;
        let contexts =
            trace_sample_resolutions(package.path(), &inspection, &selectors, &model).await?;
        print_resolutions(package.path(), &[], &[], &contexts, &[], json)?;
        return Ok(ExitCode::SUCCESS);
    }

    let context = parse_context(&args.context).await?;
    let context_gaps =
        resolve_context_gaps(package.path(), &inspection, &selectors, &context).await?;
    match trace_selected_resolutions(package.path(), &inspection, &selectors, &context).await {
        Ok((variables, qualifiers)) => {
            print_resolutions(
                package.path(),
                &variables,
                &qualifiers,
                &[],
                &context_gaps,
                json,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            // Resolution evaluates strictly and fails on the first missing or
            // mistyped attribute. Surface the full set of invocation gaps so the
            // caller can fix the context in one pass rather than one path at a time.
            if !context_gaps.is_empty() {
                print_resolutions(package.path(), &[], &[], &[], &context_gaps, json)?;
            }
            Err(err)
        }
    }
}

async fn trace_selected_resolutions(
    package: &Path,
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
    context: &JsonValue,
) -> Result<(Vec<VariableResolutionTrace>, Vec<QualifierResolutionTrace>)> {
    let mut variables = Vec::new();
    for id in selected_variable_id_list(inspection, &selectors.variables) {
        variables.push(trace_variable_resolution(package, &id, context).await?);
    }

    let mut qualifiers = Vec::new();
    for id in selected_qualifier_id_list(inspection, &selectors.qualifiers) {
        qualifiers.push(trace_qualifier_resolution(package, &id, context).await?);
    }

    Ok((variables, qualifiers))
}

async fn trace_sample_resolutions(
    package: &Path,
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
    model: &rototo::lint::PackageSemanticModel,
) -> Result<Vec<ContextResolveOutput>> {
    let variable_ids = selected_variable_id_list(inspection, &selectors.variables);
    let qualifier_ids = selected_qualifier_id_list(inspection, &selectors.qualifiers);
    let variable_contexts = variable_evaluation_contexts(model);
    let qualifier_contexts = qualifier_evaluation_contexts(model);
    let variable_has_rules = variable_rule_presence(model);
    let samples = stored_evaluation_contexts(model);

    let mut requested_contexts = BTreeSet::new();
    let mut context_independent_variables = BTreeSet::new();
    for variable in &variable_ids {
        let contexts = variable_contexts.get(variable).cloned().unwrap_or_default();
        if contexts.is_empty() && !variable_has_rules.get(variable).copied().unwrap_or(false) {
            context_independent_variables.insert(variable.clone());
        } else {
            requested_contexts.extend(contexts);
        }
    }
    for qualifier in &qualifier_ids {
        requested_contexts.extend(
            qualifier_contexts
                .get(qualifier)
                .cloned()
                .unwrap_or_default(),
        );
    }

    let mut runs = Vec::new();
    let mut resolved_variables = BTreeSet::new();
    let mut resolved_qualifiers = BTreeSet::new();
    for sample in samples
        .iter()
        .filter(|sample| requested_contexts.contains(&sample.evaluation_context))
    {
        let mut variables = Vec::new();
        for variable in &variable_ids {
            let contexts = variable_contexts.get(variable).cloned().unwrap_or_default();
            if contexts.contains(&sample.evaluation_context)
                || context_independent_variables.contains(variable)
            {
                variables.push(trace_variable_resolution(package, variable, &sample.value).await?);
                resolved_variables.insert(variable.clone());
            }
        }

        let mut qualifiers = Vec::new();
        for qualifier in &qualifier_ids {
            if qualifier_contexts
                .get(qualifier)
                .is_some_and(|contexts| contexts.contains(&sample.evaluation_context))
            {
                qualifiers
                    .push(trace_qualifier_resolution(package, qualifier, &sample.value).await?);
                resolved_qualifiers.insert(qualifier.clone());
            }
        }

        if !variables.is_empty() || !qualifiers.is_empty() {
            runs.push(ContextResolveOutput {
                evaluation_context: Some(sample.evaluation_context.clone()),
                sample: Some(sample.key.clone()),
                variables,
                qualifiers,
            });
        }
    }

    let unresolved_context_independent = context_independent_variables
        .iter()
        .filter(|variable| !resolved_variables.contains(*variable))
        .cloned()
        .collect::<Vec<_>>();
    if !unresolved_context_independent.is_empty() {
        let empty_context = JsonValue::Object(serde_json::Map::new());
        let mut variables = Vec::new();
        for variable in unresolved_context_independent {
            variables.push(trace_variable_resolution(package, &variable, &empty_context).await?);
            resolved_variables.insert(variable);
        }
        runs.push(ContextResolveOutput {
            evaluation_context: None,
            sample: None,
            variables,
            qualifiers: Vec::new(),
        });
    }

    let unresolved = unresolved_resolution_targets(
        &variable_ids,
        &qualifier_ids,
        &resolved_variables,
        &resolved_qualifiers,
    );
    if !unresolved.is_empty() {
        return Err(RototoError::new(format!(
            "no stored evaluation context sample matched selected target(s): {}",
            unresolved.join(", ")
        )));
    }

    Ok(runs)
}

#[derive(Debug)]
struct StoredEvaluationContext {
    evaluation_context: String,
    key: String,
    value: JsonValue,
}

fn stored_evaluation_contexts(
    model: &rototo::lint::PackageSemanticModel,
) -> Vec<StoredEvaluationContext> {
    model
        .evaluation_context_samples
        .iter()
        .filter_map(|entry| {
            entry.value.as_ref().map(|value| StoredEvaluationContext {
                evaluation_context: entry.evaluation_context.clone(),
                key: entry.key.clone(),
                value: value.clone(),
            })
        })
        .collect()
}

fn variable_evaluation_contexts(
    model: &rototo::lint::PackageSemanticModel,
) -> BTreeMap<String, BTreeSet<String>> {
    model
        .variable_evaluation_contexts
        .iter()
        .map(|compatibility| {
            (
                compatibility.variable.clone(),
                compatibility.evaluation_contexts.iter().cloned().collect(),
            )
        })
        .collect()
}

fn qualifier_evaluation_contexts(
    model: &rototo::lint::PackageSemanticModel,
) -> BTreeMap<String, BTreeSet<String>> {
    model
        .qualifier_evaluation_contexts
        .iter()
        .map(|compatibility| {
            (
                compatibility.qualifier.clone(),
                compatibility.evaluation_contexts.iter().cloned().collect(),
            )
        })
        .collect()
}

fn variable_rule_presence(model: &rototo::lint::PackageSemanticModel) -> BTreeMap<String, bool> {
    model
        .variables
        .iter()
        .map(|variable| {
            (
                variable.id.clone(),
                variable
                    .resolve
                    .as_ref()
                    .is_some_and(|resolve| !resolve.rules.is_empty()),
            )
        })
        .collect()
}

fn unresolved_resolution_targets(
    variable_ids: &[String],
    qualifier_ids: &[String],
    resolved_variables: &BTreeSet<String>,
    resolved_qualifiers: &BTreeSet<String>,
) -> Vec<String> {
    let mut unresolved = Vec::new();
    for variable in variable_ids {
        if !resolved_variables.contains(variable) {
            unresolved.push(format!("variable://{variable}"));
        }
    }
    for qualifier in qualifier_ids {
        if !resolved_qualifiers.contains(qualifier) {
            unresolved.push(format!("qualifier://{qualifier}"));
        }
    }
    unresolved
}

fn selected_variable_id_list(
    inspection: &PackageInspection,
    selection: &Selection<String>,
) -> Vec<String> {
    match selected_variable_ids(inspection, selection) {
        SelectedIds::None => Vec::new(),
        SelectedIds::Some(ids) => ids,
        SelectedIds::All => inspection
            .variables
            .iter()
            .map(|variable| variable.id.clone())
            .collect(),
    }
}

fn selected_qualifier_id_list(
    inspection: &PackageInspection,
    selection: &Selection<String>,
) -> Vec<String> {
    match selected_qualifier_ids(inspection, selection) {
        SelectedIds::None => Vec::new(),
        SelectedIds::Some(ids) => ids,
        SelectedIds::All => inspection
            .qualifiers
            .iter()
            .map(|qualifier| qualifier.id.clone())
            .collect(),
    }
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
    inspection: &PackageInspection,
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
    inspection: &PackageInspection,
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
    package_order: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    let mut ordered = Vec::new();
    for id in package_order {
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

fn validate_package_selectors(
    selectors: &TargetSelectors,
    inspection: &PackageInspection,
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

fn filter_lint(lint: PackageLint, selectors: &TargetSelectors) -> PackageLint {
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
    let catalog_path = format!("catalogs/{id}.schema.json");
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
    match &selectors.qualifiers {
        Selection::All => print_qualifier_list(inspection, false).await?,
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
struct PackageView {
    command: String,
    package: String,
    evaluation_contexts: Vec<PackageFileView>,
    catalogs: Vec<PackageFileView>,
    variables: Vec<PackageFileView>,
    qualifiers: Vec<PackageFileView>,
    lint_rules: Vec<DiagnosticCatalogEntryView>,
    lint_authorities: Vec<LintAuthorityView>,
    linters: Vec<LinterInspection>,
}

#[derive(Debug, Serialize)]
struct PackageFileView {
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

async fn package_inventory_view(
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

    let mut qualifiers = Vec::new();
    for qualifier in &inspection.qualifiers {
        qualifiers.push(qualifier_view(inspection, qualifier, false).await?);
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
        qualifiers,
        lint_rules: Vec::new(),
        lint_authorities: package_lint_authorities(catalog),
        linters: inspection.linters.clone(),
    })
}

async fn selected_package_view(
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
    catalog: &DiagnosticCatalog,
) -> Result<PackageView> {
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

    Ok(PackageView {
        command: String::new(),
        package: inspection.root.display().to_string(),
        evaluation_contexts: Vec::new(),
        catalogs,
        variables,
        qualifiers,
        lint_rules,
        lint_authorities,
        linters,
    })
}

async fn variable_view(
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

async fn catalog_view(
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

async fn qualifier_view(
    inspection: &PackageInspection,
    qualifier: &QualifierInspection,
    include_value: bool,
) -> Result<PackageFileView> {
    let value = if include_value {
        Some(
            serde_json::to_value(read_toml(&inspection.root.join(&qualifier.path)).await?)
                .map_err(|err| RototoError::new(err.to_string()))?,
        )
    } else {
        None
    };
    Ok(PackageFileView {
        id: qualifier.id.clone(),
        uri: qualifier.uri.clone(),
        path: qualifier.path.display().to_string(),
        value,
    })
}

fn evaluation_context_view(evaluation_context: &EvaluationContextInspection) -> PackageFileView {
    PackageFileView {
        id: evaluation_context.id.clone(),
        uri: evaluation_context.uri.clone(),
        path: evaluation_context.path.display().to_string(),
        value: None,
    }
}

fn print_package_view(command: &str, view: &PackageView, json: bool) -> Result<()> {
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
    if !view.qualifiers.is_empty() {
        println!(
            "{} {}",
            style::label("qualifiers"),
            style::bold(&view.qualifiers.len().to_string())
        );
        for qualifier in &view.qualifiers {
            println!(
                "  {}  {}",
                style::sea(&qualifier.id),
                style::dim(&qualifier.path)
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

fn package_lint_authorities(catalog: &DiagnosticCatalog) -> Vec<LintAuthorityView> {
    authorities_from_catalog(catalog)
        .into_iter()
        .filter(|authority| authority.authority != "rototo")
        .collect()
}

fn selected_linters(
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
    package: String,
    variables: &'a [VariableResolutionTrace],
    qualifiers: &'a [QualifierResolutionTrace],
    #[serde(skip_serializing_if = "is_empty_slice")]
    contexts: &'a [ContextResolveOutput],
    #[serde(skip_serializing_if = "is_empty_slice")]
    context_gaps: &'a [ContextResolveGap],
}

/// What a supplied `--context` is missing relative to what a resolved target's
/// expressions actually read. This is an invocation-time observation, distinct
/// from the package-static gaps that lint reports.
#[derive(Debug, Serialize)]
struct ContextResolveGap {
    target: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    missing_paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    mismatched_paths: Vec<ContextResolveMismatch>,
}

#[derive(Debug, Serialize)]
struct ContextResolveMismatch {
    path: String,
    expected_types: Vec<String>,
    actual_type: String,
}

#[derive(Debug, Serialize)]
struct ContextResolveOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    evaluation_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample: Option<String>,
    variables: Vec<VariableResolutionTrace>,
    qualifiers: Vec<QualifierResolutionTrace>,
}

fn is_empty_slice<T>(value: &&[T]) -> bool {
    value.is_empty()
}

async fn resolve_context_gaps(
    package: &Path,
    inspection: &PackageInspection,
    selectors: &TargetSelectors,
    context: &JsonValue,
) -> Result<Vec<ContextResolveGap>> {
    let variable_ids = selected_variable_id_list(inspection, &selectors.variables);
    let qualifier_ids = selected_qualifier_id_list(inspection, &selectors.qualifiers);
    let report = inspect_package_report(
        package,
        PackageInspectRequest {
            variables: id_selection(variable_ids),
            qualifiers: id_selection(qualifier_ids),
            ..PackageInspectRequest::default()
        },
    )
    .await?;

    let mut gaps = Vec::new();
    for qualifier in &report.qualifiers {
        if let Some(gap) = target_context_gap(
            &format!("qualifier://{}", qualifier.id),
            &qualifier.context_attributes,
            context,
        ) {
            gaps.push(gap);
        }
    }
    for variable in &report.variables {
        if let Some(gap) = target_context_gap(
            &format!("variable://{}", variable.id),
            &variable.context_attributes,
            context,
        ) {
            gaps.push(gap);
        }
    }
    Ok(gaps)
}

fn id_selection(ids: Vec<String>) -> InspectSelection {
    if ids.is_empty() {
        InspectSelection::None
    } else {
        InspectSelection::Some(ids)
    }
}

fn target_context_gap(
    target: &str,
    attributes: &[rototo::model::ContextAttributeInspectReport],
    context: &JsonValue,
) -> Option<ContextResolveGap> {
    let mut missing_paths = Vec::new();
    let mut mismatched_paths = Vec::new();
    for attribute in attributes {
        let pointer = format!("/{}", attribute.path.replace('.', "/"));
        match context.pointer(&pointer) {
            None => missing_paths.push(attribute.path.clone()),
            Some(value) => {
                let actual = json_value_type_label(value);
                if !attribute.expected_types.is_empty()
                    && !attribute
                        .expected_types
                        .iter()
                        .any(|expected| expected == actual)
                {
                    mismatched_paths.push(ContextResolveMismatch {
                        path: attribute.path.clone(),
                        expected_types: attribute.expected_types.clone(),
                        actual_type: actual.to_owned(),
                    });
                }
            }
        }
    }
    if missing_paths.is_empty() && mismatched_paths.is_empty() {
        None
    } else {
        Some(ContextResolveGap {
            target: target.to_owned(),
            missing_paths,
            mismatched_paths,
        })
    }
}

fn json_value_type_label(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

fn print_resolutions(
    package: &Path,
    variables: &[VariableResolutionTrace],
    qualifiers: &[QualifierResolutionTrace],
    contexts: &[ContextResolveOutput],
    context_gaps: &[ContextResolveGap],
    json: bool,
) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ResolveOutput {
                package: package.display().to_string(),
                variables,
                qualifiers,
                contexts,
                context_gaps,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!(
        "{} {}",
        style::label("package"),
        style::bold(&package.display().to_string())
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
    if !contexts.is_empty() {
        let count = contexts.len();
        for (index, context) in contexts.iter().enumerate() {
            print_resolve_separator(index, count);
            print_context_resolution_trace(context)?;
        }
    }
    if !context_gaps.is_empty() {
        println!("{}", style::label("context gaps"));
        for gap in context_gaps {
            println!("  {}", style::sea(&gap.target));
            for path in &gap.missing_paths {
                println!(
                    "    {} {}",
                    style::warn("missing"),
                    style::info(&format!("context.{path}"))
                );
            }
            for mismatch in &gap.mismatched_paths {
                println!(
                    "    {} {} {}",
                    style::warn("type"),
                    style::info(&format!("context.{}", mismatch.path)),
                    style::dim(&format!(
                        "expected {}, got {}",
                        mismatch.expected_types.join(" or "),
                        mismatch.actual_type
                    ))
                );
            }
        }
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
            style::sea(&rule.condition),
            style::arrow(),
            compact_json(&rule.value)?,
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
        compact_json(&trace.default_value)?
    );
    println!("  {}", style::subhead("result"));
    println!(
        "    source: {}",
        style::sea_bold(&resolution_source_label(&trace.resolution.source))
    );
    println!("    value: {}", compact_json(&trace.resolution.value)?);
    Ok(())
}

fn resolution_source_label(source: &rototo::model::VariableResolutionSource) -> String {
    match source {
        rototo::model::VariableResolutionSource::Literal => "literal".to_owned(),
        rototo::model::VariableResolutionSource::Catalog { catalog, value } => {
            format!("{catalog}:{value}")
        }
        rototo::model::VariableResolutionSource::CatalogList { catalog, values } => {
            format!("{catalog}:[{}]", values.join(","))
        }
    }
}

fn print_qualifier_resolution_trace(trace: &QualifierResolutionTrace) -> Result<()> {
    println!("qualifier: {}", style::sea(&trace.id));
    println!("  {} {}", style::subhead("when"), style::info(&trace.when));
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

fn print_context_resolution_trace(context: &ContextResolveOutput) -> Result<()> {
    match (&context.evaluation_context, &context.sample) {
        (Some(evaluation_context), Some(sample)) => {
            println!("evaluation context: {}", style::sea(evaluation_context));
            println!("sample: {}", style::info(sample));
        }
        _ => {
            println!("evaluation context: {}", style::dim("<none>"));
        }
    }

    let count = context.variables.len() + context.qualifiers.len();
    let mut index = 0;
    for trace in &context.variables {
        print_resolve_separator(index, count);
        index += 1;
        print_variable_resolution_trace(trace)?;
    }
    for trace in &context.qualifiers {
        print_resolve_separator(index, count);
        index += 1;
        print_qualifier_resolution_trace(trace)?;
    }
    Ok(())
}

fn print_nested_qualifier_resolution_trace(trace: &QualifierResolutionTrace) -> Result<()> {
    println!("    qualifier: {}", style::sea(&trace.id));
    println!(
        "      {} {}",
        style::subhead("when"),
        style::info(&trace.when)
    );
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

async fn package_source_or_current(
    package: Option<String>,
    source_options: &SourceOptions,
) -> Result<StagedPackage> {
    match package {
        Some(package) => stage_package_source(package, source_options).await,
        None => {
            let current_dir = tokio::task::spawn_blocking(std::env::current_dir)
                .await
                .map_err(|err| RototoError::new(format!("current directory task failed: {err}")))?
                .map_err(|err| {
                    RototoError::new(format!("failed to read current directory: {err}"))
                })?;
            Ok(StagedPackage::local(find_package_root(&current_dir).await?))
        }
    }
}

fn source_options(cli: &Cli) -> SourceOptions {
    match &cli.package_token {
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
