mod output;

use std::process::ExitCode;

use clap::{ArgAction, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};

use crate::output::{
    print_diagnostic_catalog, print_diagnostic_catalog_entry, print_inspection,
    print_qualifier_get, print_qualifier_lint, print_qualifier_list, print_qualifier_resolution,
    print_qualifier_resolutions, print_variable_get, print_variable_lint, print_variable_list,
    print_variable_resolution, print_variable_resolutions, print_workspace_lint,
};
use rototo::{
    Result, RototoError, SourceAuth, SourceOptions, StagedWorkspace, catalog,
    catalog_for_workspace, diagnostic_for_rule, find_workspace_root, inspect_workspace,
    lint_qualifier, lint_variable, lint_workspace, resolve_qualifier, resolve_qualifiers,
    resolve_variable, resolve_variables, stage_workspace_source,
};

#[derive(Debug, Parser)]
#[command(
    name = "rototo",
    version,
    about = "Resolve and validate workspace-defined qualifiers and variables",
    after_help = TOP_LEVEL_HELP,
    args_conflicts_with_subcommands = false
)]
struct Cli {
    /// Emit machine-readable JSON.
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    json: bool,

    /// Load this workspace source instead of discovering one from the current directory.
    #[arg(long, global = true, value_name = "WORKSPACE_SOURCE")]
    workspace: Option<String>,

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
    #[arg(short, long, global = true, action = ArgAction::SetTrue)]
    quiet: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect workspace contents or run full validation.
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    /// Work with boolean context rules used by variables.
    Qualifier {
        #[command(subcommand)]
        command: QualifierCommand,
    },
    /// Work with environment-aware configuration values.
    Variable {
        #[command(subcommand)]
        command: VariableCommand,
    },
    /// Look up rototo diagnostic rules and help text.
    Diagnostics {
        #[command(subcommand)]
        command: DiagnosticsCommand,
    },
    /// Read, export, or serve the bundled rototo documentation.
    Docs {
        #[command(subcommand)]
        command: DocsCommand,
    },
    /// Generate shell completion scripts.
    Completions { shell: CompletionShell },
}

#[derive(Debug, Subcommand)]
enum WorkspaceCommand {
    /// Summarize environments, qualifiers, and variables.
    Inspect {
        /// Workspace source. Defaults to the nearest parent with rototo-workspace.toml.
        #[arg(value_name = "WORKSPACE_SOURCE")]
        workspace_path: Option<String>,
    },
    /// Check workspace files, schemas, references, and custom Lua lint.
    Lint {
        /// Workspace source. Defaults to the nearest parent with rototo-workspace.toml.
        #[arg(value_name = "WORKSPACE_SOURCE")]
        workspace_path: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum QualifierCommand {
    /// Print all qualifier ids available in the workspace.
    List,
    /// Show the TOML-backed definition for one qualifier.
    Get {
        /// Qualifier id.
        id: String,
    },
    /// Check one qualifier's syntax, references, and predicate rules.
    Lint {
        /// Qualifier id.
        id: String,
    },
    /// Evaluate one qualifier against a JSON context.
    Resolve {
        /// Qualifier id.
        id: String,

        /// Evaluation context: JSON object, @file, or path=value. Repeatable; later values override earlier ones.
        #[arg(long = "context", required = true)]
        context: Vec<String>,
    },
    /// Evaluate every qualifier against a JSON context.
    ResolveAll {
        /// Evaluation context: JSON object, @file, or path=value. Repeatable; later values override earlier ones.
        #[arg(long = "context", required = true)]
        context: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum VariableCommand {
    /// Print all variable ids available in the workspace.
    List,
    /// Show the TOML-backed definition and expanded values for one variable.
    Get {
        /// Variable id.
        id: String,
    },
    /// Check one variable's type, values, env rules, schema, and Lua lint.
    Lint {
        /// Variable id.
        id: String,
    },
    /// Select one variable value for an environment and JSON context.
    Resolve {
        /// Variable id.
        id: String,

        /// Environment name.
        #[arg(long = "env")]
        env: String,

        /// Evaluation context: JSON object, @file, or path=value. Repeatable; later values override earlier ones.
        #[arg(long = "context", required = true)]
        context: Vec<String>,
    },
    /// Select values for every variable in an environment and JSON context.
    ResolveAll {
        /// Environment name.
        #[arg(long = "env")]
        env: String,

        /// Evaluation context: JSON object, @file, or path=value. Repeatable; later values override earlier ones.
        #[arg(long = "context", required = true)]
        context: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum DiagnosticsCommand {
    /// Show known diagnostic rules with titles and severity.
    List,
    /// Explain one diagnostic rule and how to address it.
    Get {
        /// Diagnostic rule, such as rototo/workspace-manifest-missing.
        rule: String,
    },
}

#[derive(Debug, Subcommand)]
enum DocsCommand {
    /// List bundled documentation pages.
    List,
    /// Print one bundled documentation page.
    Show {
        /// Documentation page id. Defaults to index.
        #[arg(default_value = "index")]
        page: String,

        /// Output format.
        #[arg(long, value_enum, default_value_t = DocsFormat::Markdown)]
        format: DocsFormat,
    },
    /// Export bundled documentation as a static HTML site.
    Export {
        /// Directory to write HTML files into.
        #[arg(long, value_name = "DIR")]
        out: std::path::PathBuf,
    },
    /// Serve bundled documentation over HTTP.
    Serve {
        /// Address to bind.
        #[arg(long, default_value = "127.0.0.1:8787")]
        addr: std::net::SocketAddr,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DocsFormat {
    Markdown,
    Html,
}

const TOP_LEVEL_HELP: &str = r#"Workspace selection:
  Commands that read a workspace accept --workspace <WORKSPACE_SOURCE>.

  If no workspace source is provided, rototo searches upward from the current
  directory for rototo-workspace.toml.

Workspace sources:
  ./workspace
  file:///abs/path/to/workspace
  git+file:///path/to/repo#main:rototo
  git+https://github.com/org/repo.git#main:rototo
  git+ssh://git@github.com/org/repo.git#main:rototo
  https://example.com/workspace.tar.gz#:rototo

  Git sources use #ref:subdir.
  HTTPS archive sources use #:subdir.
  Plain http:// sources are not supported.

Context values:
  Resolve commands accept one or more --context values:

    --context '{"user":{"tier":"premium"}}'
    --context @context.json
    --context user.tier=premium

  Later context values override earlier ones.

Environment:
  ROTOTO_WORKSPACE_TOKEN
      Bearer token for https:// workspace archive downloads.

Examples:
  rototo workspace inspect --workspace ./examples/basic
  rototo workspace lint --workspace git+https://github.com/org/config.git#main:rototo
  rototo qualifier resolve premium-users --context user.tier=premium
  rototo variable resolve checkout-redesign --env prod --context user.tier=premium"#;

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
    let workspace_flag = cli.workspace.clone();

    match cli.command {
        Command::Workspace {
            command: WorkspaceCommand::Inspect { workspace_path },
        } => {
            let workspace = workspace_source_or_current(
                selected_workspace(workspace_flag.clone(), workspace_path)?,
                &source_options,
            )
            .await?;
            let inspection = inspect_workspace(workspace.path()).await?;
            print_inspection(&inspection, cli.json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Workspace {
            command: WorkspaceCommand::Lint { workspace_path },
        } => {
            let workspace = workspace_source_or_current(
                selected_workspace(workspace_flag.clone(), workspace_path)?,
                &source_options,
            )
            .await?;
            let lint = lint_workspace(workspace.path()).await?;
            let passed = lint.diagnostics.is_empty();
            print_workspace_lint(&lint, cli.json, cli.quiet)?;
            Ok(if passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Qualifier { command } => match command {
            QualifierCommand::List => {
                let workspace =
                    workspace_source_or_current(workspace_flag.clone(), &source_options).await?;
                let inspection = inspect_workspace(workspace.path()).await?;
                print_qualifier_list(&inspection, cli.json)?;
                Ok(ExitCode::SUCCESS)
            }
            QualifierCommand::Get { id } => {
                let workspace =
                    workspace_source_or_current(workspace_flag.clone(), &source_options).await?;
                let inspection = inspect_workspace(workspace.path()).await?;
                print_qualifier_get(&inspection, &id, cli.json).await?;
                Ok(ExitCode::SUCCESS)
            }
            QualifierCommand::Lint { id } => {
                let workspace =
                    workspace_source_or_current(workspace_flag.clone(), &source_options).await?;
                let lint = lint_qualifier(workspace.path(), &id).await?;
                let passed = lint.diagnostics.is_empty();
                print_qualifier_lint(&lint, cli.json, cli.quiet)?;
                Ok(if passed {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                })
            }
            QualifierCommand::Resolve { id, context } => {
                let workspace =
                    workspace_source_or_current(workspace_flag.clone(), &source_options).await?;
                let context = parse_context(&context).await?;
                let resolution = resolve_qualifier(workspace.path(), &id, &context).await?;
                print_qualifier_resolution(workspace.path(), &resolution, cli.json)?;
                Ok(ExitCode::SUCCESS)
            }
            QualifierCommand::ResolveAll { context } => {
                let workspace =
                    workspace_source_or_current(workspace_flag.clone(), &source_options).await?;
                let context = parse_context(&context).await?;
                let resolutions = resolve_qualifiers(workspace.path(), &context).await?;
                print_qualifier_resolutions(workspace.path(), &resolutions, cli.json)?;
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Variable { command } => match command {
            VariableCommand::List => {
                let workspace =
                    workspace_source_or_current(workspace_flag.clone(), &source_options).await?;
                let inspection = inspect_workspace(workspace.path()).await?;
                print_variable_list(&inspection, cli.json)?;
                Ok(ExitCode::SUCCESS)
            }
            VariableCommand::Get { id } => {
                let workspace =
                    workspace_source_or_current(workspace_flag.clone(), &source_options).await?;
                let inspection = inspect_workspace(workspace.path()).await?;
                print_variable_get(&inspection, &id, cli.json).await?;
                Ok(ExitCode::SUCCESS)
            }
            VariableCommand::Lint { id } => {
                let workspace =
                    workspace_source_or_current(workspace_flag.clone(), &source_options).await?;
                let lint = lint_variable(workspace.path(), &id).await?;
                let passed = lint.diagnostics.is_empty();
                print_variable_lint(&lint, cli.json, cli.quiet)?;
                Ok(if passed {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                })
            }
            VariableCommand::Resolve { id, env, context } => {
                let workspace =
                    workspace_source_or_current(workspace_flag.clone(), &source_options).await?;
                let context = parse_context(&context).await?;
                let resolution = resolve_variable(workspace.path(), &id, &env, &context).await?;
                print_variable_resolution(workspace.path(), &resolution, cli.json)?;
                Ok(ExitCode::SUCCESS)
            }
            VariableCommand::ResolveAll { env, context } => {
                let workspace =
                    workspace_source_or_current(workspace_flag.clone(), &source_options).await?;
                let context = parse_context(&context).await?;
                let resolutions = resolve_variables(workspace.path(), &env, &context).await?;
                print_variable_resolutions(workspace.path(), &resolutions, cli.json)?;
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Diagnostics {
            command: DiagnosticsCommand::List,
        } => {
            let catalog = diagnostic_catalog(workspace_flag.clone(), &source_options).await?;
            print_diagnostic_catalog(&catalog, cli.json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Diagnostics {
            command: DiagnosticsCommand::Get { rule },
        } => {
            let catalog = diagnostic_catalog(workspace_flag.clone(), &source_options).await?;
            let diagnostic = diagnostic_for_rule(&catalog, &rule)?;
            print_diagnostic_catalog_entry(diagnostic, cli.json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Docs { command } => match command {
            DocsCommand::List => {
                print_docs_list(cli.json)?;
                Ok(ExitCode::SUCCESS)
            }
            DocsCommand::Show { page, format } => {
                let page = rototo::docs::get_page(&page)?;
                match format {
                    DocsFormat::Markdown => print!("{}", page.markdown),
                    DocsFormat::Html => print!("{}", rototo::docs::render_page_html(page)),
                }
                Ok(ExitCode::SUCCESS)
            }
            DocsCommand::Export { out } => {
                rototo::docs::export_html(&out).await?;
                println!("exported rototo docs to {}", out.display());
                Ok(ExitCode::SUCCESS)
            }
            DocsCommand::Serve { addr } => {
                rototo::docs::serve(addr).await.map(|()| ExitCode::SUCCESS)
            }
        },
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

fn print_docs_list(json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(rototo::docs::DOCS)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    println!("{:<16}  title", "page");
    for page in rototo::docs::DOCS {
        println!("{:<16}  {}", page.id, page.title);
    }
    Ok(())
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

async fn diagnostic_catalog(
    workspace: Option<String>,
    source_options: &SourceOptions,
) -> Result<rototo::model::DiagnosticCatalog> {
    match workspace {
        Some(workspace) => {
            let workspace = workspace_source_or_current(Some(workspace), source_options).await?;
            catalog_for_workspace(workspace.path()).await
        }
        None => Ok(catalog()),
    }
}

fn selected_workspace(
    workspace_flag: Option<String>,
    workspace_path: Option<String>,
) -> Result<Option<String>> {
    match (workspace_flag, workspace_path) {
        (Some(_), Some(_)) => Err(RototoError::new(
            "pass workspace either as --workspace or as a positional argument, not both",
        )),
        (Some(workspace), None) | (None, Some(workspace)) => Ok(Some(workspace)),
        (None, None) => Ok(None),
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

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
