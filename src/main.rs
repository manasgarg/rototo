mod output;
mod style;

mod cli;

pub(crate) use cli::context::parse_context;
pub(crate) use cli::lint::package_source_for_lint;
pub(crate) use cli::selectors::{
    SelectedIds, Selection, TargetSelectors, selected_variable_ids, validate_package_selectors,
};
pub(crate) use cli::{
    package_source_or_current, package_source_string_or_current, path_exists, severity_label,
};

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use serde::Serialize;

use crate::output::{
    print_catalog_get, print_catalog_list, print_diagnostic_catalog_entry, print_inspect_report,
    print_package_lint, print_variable_get, print_variable_list,
};
use rototo::diagnostics::{DiagnosticCatalogEntry, LintDiagnostic, SemanticEntity, Severity};
use rototo::model::{
    CatalogInspection, DiagnosticCatalog, EvaluationContextInspection, InspectSelection,
    LinterInspection, PackageInspectRequest, PackageInspection, PackageLint, VariableInspection,
};
use rototo::package::{
    catalog_for_id, package_extends_sources, read_catalog_json, read_toml, read_variable_toml,
    variable_for_id,
};
use rototo::{
    Result, RototoError, SourceAuth, SourceOptions, StagedPackage, diagnostic_for_rule,
    diagnostics_catalog, diagnostics_catalog_for_package, inspect_package, inspect_package_report,
    lint_package,
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

    /// Bearer token for https:// package archive downloads. Repeatable: a
    /// bare TOKEN covers a single archive origin; PREFIX=TOKEN entries scope
    /// tokens to https:// URL prefixes (longest match wins). The
    /// ROTOTO_PACKAGE_TOKEN environment variable takes the same entries,
    /// whitespace-separated.
    #[arg(
        long = "package-token",
        global = true,
        action = ArgAction::Append,
        value_name = "TOKEN|PREFIX=TOKEN"
    )]
    package_token: Vec<String>,

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
    /// Display package config, variables, catalogs, and lint metadata.
    Show(PackageCommandArgs),
    /// Evaluate variables with runtime context.
    Resolve(ResolveArgs),
    /// Build a deterministic, content-addressed distributable archive.
    Package(PackageArgs),
    /// Read bundled documentation.
    Docs(DocsArgs),
    /// Configure shell, editor, and agent integrations.
    Setup(SetupArgs),
    /// Run the rototo Language Server Protocol server over stdio.
    Lsp,
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Local package path to initialize or modify.
    #[arg(value_name = "PACKAGE")]
    package: PathBuf,

    /// Create a variable template with this id.
    #[arg(long = "variable", value_name = "ID")]
    variable: Option<String>,

    /// Create a catalog template with this id.
    #[arg(long = "catalog", value_name = "ID")]
    catalog: Option<String>,

    /// Create or infer an evaluation context schema template.
    #[arg(
        long = "evaluation-context",
        value_name = "ID",
        num_args = 0..=1,
        default_missing_value = "evaluation"
    )]
    evaluation_context: Option<String>,

    /// Overwrite files created by this command.
    #[arg(long = "force", action = ArgAction::SetTrue)]
    force: bool,

    /// Print the planned writes without changing the filesystem.
    #[arg(long = "dry-run", action = ArgAction::SetTrue)]
    dry_run: bool,

    /// Add missing inferred paths to an existing evaluation context schema.
    #[arg(
        long = "update",
        action = ArgAction::SetTrue,
        requires = "evaluation_context",
        conflicts_with = "force"
    )]
    update: bool,
}

#[derive(Debug, Args)]
struct FixturesArgs {
    /// Package source to print resolve commands for. Defaults to the package in
    /// the current directory.
    #[arg(value_name = "PACKAGE_SOURCE")]
    package: Option<String>,

    /// Select one variable id. Repeatable.
    #[arg(long = "variable", value_name = "ID")]
    variables: Vec<String>,

    /// Select all variables.
    #[arg(long = "variables", action = ArgAction::SetTrue)]
    all_variables: bool,

    /// How to render context in printed commands.
    #[arg(long = "context-form", value_enum, default_value_t = ContextForm::Path)]
    context_form: ContextForm,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum ContextForm {
    /// Decompose context into `--context a.b=value` arguments.
    #[default]
    Path,
    /// Emit a single `--context '<json>'` argument.
    Json,
}

impl ContextForm {
    fn to_render(self) -> rototo::fixtures::ContextForm {
        match self {
            Self::Path => rototo::fixtures::ContextForm::Path,
            Self::Json => rototo::fixtures::ContextForm::Json,
        }
    }
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
struct PackageArgs {
    /// Package source. Defaults to the nearest parent with rototo-package.toml.
    #[arg(value_name = "PACKAGE_SOURCE")]
    package: Option<String>,

    /// Directory to write the content-addressed archive into.
    #[arg(short = 'o', long = "output", value_name = "DIR", default_value = ".")]
    output: PathBuf,

    /// Write the flattened projection as a plain directory instead of an
    /// archive. The directory must be empty or absent.
    #[arg(long = "unpacked", value_name = "DIR", conflicts_with = "output")]
    unpacked: Option<PathBuf>,
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
        "init config --variable premium_users",
        "fixtures examples/basic --variable tenant_limits",
        "lint examples/basic",
        "show examples/basic --variables",
        "diff examples/basic --context @examples/basic/model/context/request-samples/premium_enterprise.json",
        "resolve examples/basic --variable checkout_redesign --context lane=prod --context user.tier=premium",
        "package examples/basic --output dist",
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
        "Display package config, variables, catalogs, and lint metadata",
    ));
    out.push_str(&command(
        "resolve",
        "Evaluate variables with runtime context",
    ));
    out.push_str(&command(
        "package",
        "Build a deterministic, content-addressed distributable archive",
    ));
    out.push('\n');
    out.push_str(&style::bold("Utility commands:"));
    out.push('\n');
    out.push_str(&command("docs", "Read bundled documentation"));
    out.push_str(&command(
        "setup",
        "Configure shell, editor, and agent integrations",
    ));
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
    let source_options = source_options(&cli)?;

    match cli.command {
        Command::Init(args) => cli::init::run_init(args, cli.json, cli.quiet).await,
        Command::Fixtures(args) => {
            cli::fixtures::run_fixtures(args, &source_options, cli.json, cli.quiet).await
        }
        Command::Lint(args) => {
            cli::lint::run_lint(args, &source_options, cli.json, cli.quiet).await
        }
        Command::Inspect(args) => cli::inspect::run_inspect(args, &source_options, cli.json).await,
        Command::Diff(args) => cli::diff::run_diff(args, &source_options, cli.json).await,
        Command::Show(args) => cli::inspect::run_show(args, &source_options, cli.json).await,
        Command::Resolve(args) => cli::resolve::run_resolve(args, &source_options, cli.json).await,
        Command::Package(args) => {
            cli::package::run_package(args, &source_options, cli.json, cli.quiet).await
        }
        Command::Docs(args) => cli::docs::run_docs(args, cli.json).await,
        Command::Setup(args) => cli::setup::run_setup(args, cli.json, cli.quiet).await,
        Command::Lsp => {
            rototo::lsp::serve_stdio().await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Combines --package-token occurrences with whitespace-separated
/// ROTOTO_PACKAGE_TOKEN entries; both surfaces share one entry grammar.
fn package_token_entries(cli: &Cli) -> Vec<String> {
    let mut entries = cli.package_token.clone();
    if let Ok(value) = std::env::var("ROTOTO_PACKAGE_TOKEN") {
        entries.extend(value.split_whitespace().map(str::to_owned));
    }
    entries
}

fn source_options(cli: &Cli) -> Result<SourceOptions> {
    let auth = rototo::source_auth_from_package_token_entries(&package_token_entries(cli))?;
    Ok(match auth {
        SourceAuth::None => SourceOptions::new(),
        auth => SourceOptions::new().with_auth(auth),
    })
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
