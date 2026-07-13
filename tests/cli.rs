use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

use assert_cmd::Command;
use predicates::prelude::*;
use rototo::docs::DOCS;

#[test]
fn prints_version() {
    Command::cargo_bin("rototo")
        .unwrap()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "rototo {}",
            env!("CARGO_PKG_VERSION")
        )));
}

#[test]
fn top_level_help_is_task_oriented() {
    Command::cargo_bin("rototo")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Package commands"))
        .stdout(predicate::str::contains("Utility commands"))
        .stdout(predicate::str::contains("lint"))
        .stdout(predicate::str::contains("diff"))
        .stdout(predicate::str::contains("setup"))
        .stdout(predicate::str::contains("completions").not())
        .stdout(predicate::str::contains("rototo docs -p motivation"))
        .stdout(predicate::str::contains("rototo help package-sources").not())
        .stdout(predicate::str::contains("git+https://").not());
}

#[test]
fn custom_help_topics_are_not_supported() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["help", "context"])
        .assert()
        .failure();
}

#[test]
fn quiet_suppresses_successful_lint_output() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["--quiet", "lint", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::eq(""));
}

#[test]
fn quiet_keeps_lint_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .env("NO_COLOR", "1")
        .args(["--quiet", "lint", "tests/fixtures/packages/lint-failures"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("error[rototo/"));
}

#[test]
fn resolve_reports_missing_context_attributes() {
    Command::cargo_bin("rototo")
        .unwrap()
        .env("NO_COLOR", "1")
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "premium_users",
            "--context",
            "lane=dev",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("context gaps"))
        .stdout(predicate::str::contains("missing"))
        .stdout(predicate::str::contains("context.user.tier"));
}

#[test]
fn resolve_succeeds_without_context_gaps() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "premium_users",
            "--context",
            "user.tier=premium",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("context gaps").not());
}

#[test]
fn old_noun_commands_are_removed() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["qualifier", "lint", "examples/basic"])
        .assert()
        .failure();
}

#[test]
fn prints_zsh_completions_through_setup() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["setup", "--shell", "zsh", "--print"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef rototo"));
}

#[test]
fn setup_zsh_reports_plain_profile_instruction() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .env("HOME", &home)
        .env_remove("ZDOTDIR")
        .args(["setup", "--shell", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("zsh-profile"))
        .stdout(predicate::str::contains(
            "add this near the top of your zsh profile: fpath=(",
        ))
        .stdout(predicate::str::contains(".zfunc"))
        .stdout(predicate::str::contains("compinit").not());

    assert!(home.join(".zfunc/_rototo").exists());
}

#[test]
fn setup_bash_honors_xdg_data_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&data).unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data)
        .args(["setup", "--shell", "bash"])
        .assert()
        .success();

    assert!(data.join("bash-completion/completions/rototo").exists());
    assert!(!home.join(".local/share").exists());
}

#[test]
fn setup_elvish_defaults_to_xdg_data_dir() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .env("HOME", &home)
        .env_remove("XDG_DATA_HOME")
        .args(["setup", "--shell", "elvish"])
        .assert()
        .success();

    assert!(
        home.join(".local/share/elvish/lib/rototo-completions.elv")
            .exists()
    );
    assert!(!home.join(".elvish").exists());
}

#[test]
fn setup_editor_help_omits_vscode_until_supported() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["setup", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "[possible values: all, neovim, none]",
        ))
        .stdout(predicate::str::contains("vscode").not())
        .stdout(predicate::str::contains("vs-code").not());
}

#[test]
fn setup_editor_vscode_is_rejected_until_supported() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["setup", "--editor", "vscode"])
        .assert()
        .failure();
}

#[test]
fn old_completions_command_is_removed() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["completions", "zsh"])
        .assert()
        .failure();
}

#[test]
fn diff_defaults_to_head_vs_worktree_for_local_package() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let package = repo.join("config");
    write_basic_package(&package, 1800);
    init_git_repo(&repo);

    fs::write(
        package.join("variables/summary_token_budget.toml"),
        variable_toml(2400),
    )
    .unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir(&repo)
        .env("NO_COLOR", "1")
        .args(["diff", "config", "--context", "{}"])
        .assert()
        .success()
        .stdout(predicate::str::contains("before: HEAD:config"))
        .stdout(predicate::str::contains("after: worktree:config"))
        .stdout(predicate::str::contains("variable_resolve_default_changed"))
        .stdout(predicate::str::contains(
            "change: variable resolve default changed",
        ))
        .stdout(predicate::str::contains("before: 1800"))
        .stdout(predicate::str::contains("after: 2400"))
        .stdout(predicate::str::contains("resolution impact:"));
}

#[test]
fn diff_compares_explicit_git_refs() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let package = repo.join("config");
    write_basic_package(&package, 1800);
    init_git_repo(&repo);

    fs::write(
        package.join("variables/summary_token_budget.toml"),
        variable_toml(2400),
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "update"]);

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir(&repo)
        .env("NO_COLOR", "1")
        .args(["diff", "config", "--from", "HEAD~1", "--to", "HEAD"])
        .assert()
        .success()
        .stdout(predicate::str::contains("before: HEAD~1:config"))
        .stdout(predicate::str::contains("after: HEAD:config"))
        .stdout(predicate::str::contains("variable_resolve_default_changed"))
        .stdout(predicate::str::contains("before: 1800"))
        .stdout(predicate::str::contains("after: 2400"))
        .stdout(predicate::str::contains("resolution impact:").not());
}

#[test]
fn diff_uses_cli_design_system_colors_when_forced() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let package = repo.join("config");
    write_basic_package(&package, 1800);
    init_git_repo(&repo);

    fs::write(
        package.join("variables/summary_token_budget.toml"),
        variable_toml(2400),
    )
    .unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir(&repo)
        .env("FORCE_COLOR", "1")
        .env_remove("NO_COLOR")
        .env_remove("COLORTERM")
        .args(["diff", "config", "--context", "{}"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\u{1b}[38;5;220mSEMANTIC CHANGES\u{1b}[0m",
        ))
        .stdout(predicate::str::contains("\u{1b}[38;5;220m~\u{1b}[0m"))
        .stdout(predicate::str::contains(
            "\u{1b}[38;5;220mvariable resolve default changed\u{1b}[0m",
        ))
        .stdout(predicate::str::contains(
            "\u{1b}[38;5;245mkind:\u{1b}[0m \u{1b}[38;5;245mvariable_resolve_default_changed\u{1b}[0m",
        ))
        .stdout(predicate::str::contains("\u{1b}[38;5;203mbefore:\u{1b}[0m"))
        .stdout(predicate::str::contains("\u{1b}[38;5;78mafter:\u{1b}[0m"));
}

#[test]
fn bare_setup_requires_tty_or_explicit_targets() {
    Command::cargo_bin("rototo")
        .unwrap()
        .arg("setup")
        .assert()
        .failure()
        .stderr(predicate::str::contains("rototo setup needs a terminal"));
}

#[test]
fn setup_neovim_lsp_uses_rototo_package_root_marker() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let config = temp.path().join("config");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&config).unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config)
        .args(["setup", "--editor", "neovim"])
        .assert()
        .success()
        .stdout(predicate::str::contains("neovim-lsp"));

    let lsp_config = fs::read_to_string(config.join("nvim/lua/rototo.lua")).unwrap();
    assert!(lsp_config.contains(r#"root_markers = { "rototo-package.toml" }"#));
    assert!(!lsp_config.contains(".git"));
    assert!(lsp_config.contains(r#"cmd = { "rototo", "lsp" }"#));

    let init = fs::read_to_string(config.join("nvim/init.lua")).unwrap();
    assert!(init.contains(r#"require("rototo")"#));
}

#[test]
fn setup_agent_walks_upward_to_existing_instruction_file() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let nested = root.join("packages/checkout");
    fs::create_dir_all(&nested).unwrap();
    fs::write(root.join("AGENTS.md"), "# Existing instructions\n").unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir(&nested)
        .args(["setup", "--agent", "codex"])
        .assert()
        .success()
        .stdout(predicate::str::contains("codex-guidance"));

    let instructions = fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(instructions.contains("# Existing instructions"));
    assert!(instructions.contains("<!-- BEGIN rototo setup -->"));
    assert!(
        instructions
            .contains("runtime configuration that can change system behavior after deployment")
    );
    assert!(!nested.join("AGENTS.md").exists());
}

#[test]
fn setup_agent_all_does_not_create_claude_instructions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir_all(&root).unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir(&root)
        .args(["setup", "--agent", "all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("codex-guidance"))
        .stdout(predicate::str::contains("claude-guidance"));

    let agents = fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("runtime configuration that can change system behavior after deployment")
    );
    assert!(!root.join("CLAUDE.md").exists());
}

#[test]
fn setup_agent_all_updates_existing_claude_instructions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let nested = root.join("packages/checkout");
    fs::create_dir_all(&nested).unwrap();
    fs::write(root.join("AGENTS.md"), "# Agent instructions\n").unwrap();
    fs::write(root.join("CLAUDE.md"), "# Claude instructions\n").unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir(&nested)
        .args(["setup", "--agent", "all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("codex-guidance"))
        .stdout(predicate::str::contains("claude-guidance"));

    let agents = fs::read_to_string(root.join("AGENTS.md")).unwrap();
    let claude = fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(agents.contains("<!-- BEGIN rototo setup -->"));
    assert!(claude.contains("<!-- BEGIN rototo setup -->"));
    assert!(!nested.join("AGENTS.md").exists());
    assert!(!nested.join("CLAUDE.md").exists());
}

#[test]
fn exposes_lsp_command() {
    // The server is useful to editors only if the public CLI exposes the stdio
    // entry point they can launch.
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lsp", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Run the rototo Language Server Protocol server over stdio",
        ));
}

#[test]
fn resolve_help_omits_lint_selectors() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["resolve", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--variable"))
        .stdout(predicate::str::contains("--qualifier").not())
        .stdout(predicate::str::contains("--context"))
        .stdout(predicate::str::contains("--lint-rule").not())
        .stdout(predicate::str::contains("--lint-authority").not())
        .stdout(predicate::str::contains("--linter").not());
}

#[test]
fn resolve_rejects_lint_selectors_at_parse_time() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--lint-rule",
            "consumer-experience/message-not-empty",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--lint-rule"));
}

#[test]
fn lists_bundled_docs() {
    let assert = Command::cargo_bin("rototo")
        .unwrap()
        .arg("docs")
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    for page in DOCS {
        assert!(
            stdout.contains(page.id),
            "docs list did not include page id `{}`",
            page.id
        );
    }
}

#[test]
fn shows_bundled_docs_by_prefix_as_markdown() {
    Command::cargo_bin("rototo")
        .unwrap()
        .env("NO_COLOR", "1")
        .args(["docs", "-p", "motivation"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Rototo's core premise is that behavioral configuration should live as files in a git repository",
        ))
        .stdout(predicate::str::contains("## Rototo's approach"));
}

#[test]
fn exports_bundled_docs_as_static_site() {
    let temp = tempfile::tempdir().unwrap();
    let site = temp.path().join("site");

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "--export", site.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("exported documentation to"));

    let homepage = fs::read_to_string(site.join("index.html")).unwrap();
    assert!(homepage.contains("<!doctype html>"));
    assert!(homepage.contains("Runtime configuration, reviewed like code."));
    assert!(homepage.contains(r#"<a class="primary" href="docs/motivation.html">"#));
    assert!(homepage.contains(r#"href="docs/concepts.html""#));

    let motivation = fs::read_to_string(site.join("docs/motivation.html")).unwrap();
    assert!(motivation.contains("<!doctype html>"));
    assert!(motivation.contains("Rototo's core premise is that behavioral configuration"));
    assert!(motivation.contains(r#"<header class="topbar">"#));
    assert!(
        motivation.contains(
            r#"<img class="brand-wordmark" src="assets/rototo-wordmark.svg" alt="rototo">"#
        )
    );
    assert!(motivation.contains(r#"<a href="../index.html">Home</a>"#));
    assert!(motivation.contains(r#"<a href="motivation.html" aria-current="page">Docs</a>"#));
    assert!(motivation.contains(r#"<aside class="tree sidenav" aria-label="Documentation">"#));
    assert!(motivation.contains(r#"<aside class="toc" aria-label="On this page">"#));
    assert!(motivation.contains(r#"<h2 id="rototos-approach">Rototo's approach</h2>"#));
    assert!(motivation.contains(r#"<nav class="page-nav" aria-label="Page">"#));

    let redirects = fs::read_to_string(site.join("_redirects")).unwrap();
    assert!(redirects.contains("/motivation.html /docs/motivation.html 301"));
    assert!(redirects.contains("/concepts.html /docs/concepts.html 301"));
    assert!(!redirects.contains("/index.html /docs/index.html"));
    assert!(site.join("docs/motivation.html").is_file());
    assert!(site.join("docs/concepts.html").is_file());
    assert!(!site.join("docs/getting-started.html").exists());
    assert!(!site.join("docs/reference-sdk-resolution.html").exists());
    assert!(site.join("assets/rototo-docs.css").is_file());
    assert!(site.join("assets/favicon.svg").is_file());
    assert!(site.join("docs/assets/rototo-docs.css").is_file());
    assert!(site.join("docs/assets/favicon.svg").is_file());
    assert!(site.join("docs/assets/rototo-mark.svg").is_file());
    assert!(site.join("docs/assets/rototo-wordmark.svg").is_file());

    let css = fs::read_to_string(site.join("assets/rototo-docs.css")).unwrap();
    assert!(css.contains("text-size-adjust: 100%"));
    assert!(css.contains("--sea-500: oklch(0.572 0.148 178);"));
    assert!(css.contains("--cyan-500: oklch(0.640 0.152 205);"));
    assert!(css.contains("--ok-500: oklch(0.625 0.165 150);"));
    assert!(css.contains("--term-violet"));
    assert!(css.contains("--text-6xl"));
    assert!(css.contains("--leading-normal"));
    assert!(css.contains("--shadow-4"));
    assert!(css.contains("--grid-line"));
    assert!(css.contains("--ct-keyword"));
    assert!(css.contains(".doc :not(pre) > code"));
    assert!(css.contains(".hljs-keyword"));
    assert!(css.contains(".hljs-title.function_"));
    assert!(css.contains(".hljs-string"));
    assert!(css.contains(".hljs-addition"));
    assert!(css.contains(".token.comment"));
    assert!(css.contains(".token.inserted-sign"));
    assert!(!css.contains(".sx-"));
    assert!(!css.contains("--clay-500"));
    assert!(!css.contains(".doc pre.language-text"));

    let wordmark = fs::read_to_string(site.join("assets/rototo-wordmark.svg")).unwrap();
    assert!(wordmark.contains("#006252"));
    assert!(!wordmark.contains("#008572"));

    let favicon = fs::read_to_string(site.join("assets/favicon.svg")).unwrap();
    assert!(favicon.contains("#006252"));
    assert!(!favicon.contains("#008572"));

    let concepts = fs::read_to_string(site.join("docs/concepts.html")).unwrap();
    assert!(concepts.contains(r#"<h1 id="rototo-concepts">Rototo Concepts</h1>"#));
    assert!(
        concepts.contains(r#"<pre class="code-block language-toml"><code class="language-ini">"#)
    );
    assert!(concepts.contains("https://unpkg.com/@highlightjs/cdn-assets@11.9.0/highlight.min.js"));
    assert!(
        concepts.contains("https://unpkg.com/@highlightjs/cdn-assets@11.9.0/languages/bash.min.js")
    );
    assert!(
        concepts
            .contains("https://unpkg.com/@highlightjs/cdn-assets@11.9.0/languages/gradle.min.js")
    );
    assert!(
        concepts
            .contains(r#"window.hljs.registerAliases(["sh", "shell"], { languageName: "bash" });"#)
    );
    assert!(concepts.contains("window.hljs.highlightAll();"));
    assert!(!concepts.contains(r#"id="sdk-language""#));
    assert!(!concepts.contains(r#"<span class="sx-"#));
}

#[test]
#[ignore = "temporarily disabled while SDK package README docs are being rewritten"]
fn generates_python_package_readme_from_docs() {
    let temp = tempfile::tempdir().unwrap();
    let readme = temp.path().join("README.md");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "docs",
            "--package-readme",
            "python",
            "--out",
            readme.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated python package README"));

    let actual = fs::read_to_string(readme).unwrap();
    let expected = rototo::docs::render_package_readme("python").unwrap();
    assert_eq!(actual, expected);
    assert!(actual.contains("https://docs.rototo.dev/reference-package-sources.html"));
}

#[test]
#[ignore = "temporarily disabled while SDK package README docs are being rewritten"]
fn generates_typescript_package_readme_from_docs() {
    let temp = tempfile::tempdir().unwrap();
    let readme = temp.path().join("README.md");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "docs",
            "--package-readme",
            "typescript",
            "--out",
            readme.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "generated typescript package README",
        ));

    let actual = fs::read_to_string(readme).unwrap();
    let expected = rototo::docs::render_package_readme("typescript").unwrap();
    assert_eq!(actual, expected);
    assert!(actual.contains("https://docs.rototo.dev/reference-package-sources.html"));
}

#[test]
#[ignore = "temporarily disabled while SDK package README docs are being rewritten"]
fn generates_package_readme_with_custom_docs_base_url() {
    let temp = tempfile::tempdir().unwrap();
    let readme = temp.path().join("README.md");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "docs",
            "--package-readme",
            "python",
            "--docs-base-url",
            "https://docs.example.test/base/",
            "--out",
            readme.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated python package README"));

    let actual = fs::read_to_string(readme).unwrap();
    let expected = rototo::docs::render_package_readme_with_base_url(
        "python",
        "https://docs.example.test/base/",
    )
    .unwrap();
    assert_eq!(actual, expected);
    assert!(actual.contains("https://docs.example.test/base/reference-package-sources.html"));
    assert!(!actual.contains("https://docs.rototo.dev/"));
}

#[test]
#[ignore = "temporarily disabled while SDK package README docs are being rewritten"]
fn generates_java_package_readme_from_docs() {
    let temp = tempfile::tempdir().unwrap();
    let readme = temp.path().join("README.md");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "docs",
            "--package-readme",
            "java",
            "--out",
            readme.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated java package README"));

    let actual = fs::read_to_string(readme).unwrap();
    let expected = rototo::docs::render_package_readme("java").unwrap();
    assert_eq!(actual, expected);
}

#[test]
#[ignore = "temporarily disabled while SDK package README docs are being rewritten"]
fn generates_go_package_readme_from_docs() {
    let temp = tempfile::tempdir().unwrap();
    let readme = temp.path().join("README.md");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "docs",
            "--package-readme",
            "go",
            "--out",
            readme.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated go package README"));

    let actual = fs::read_to_string(readme).unwrap();
    let expected = rototo::docs::render_package_readme("go").unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn docs_page_prefix_reports_unknown_page() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "-p", "missing-page"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("documentation page not found"));
}

#[test]
fn docs_search_uses_regex() {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "-s", "configuration"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\x1b[7mconfiguration\x1b[0m"));
    assert!(
        !stdout
            .lines()
            .any(|line| line.trim_start().starts_with('^')),
        "docs search should highlight matches inline instead of printing marker lines:\n{stdout}"
    );
}

#[test]
fn docs_search_rejects_invalid_regex() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["docs", "-s", "["])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "invalid documentation search regex",
        ));
}

fn write_basic_package(package: &Path, default: i64) {
    fs::create_dir_all(package.join("variables")).unwrap();
    fs::write(package.join("rototo-package.toml"), "schema_version = 1\n").unwrap();
    fs::write(
        package.join("variables/summary_token_budget.toml"),
        variable_toml(default),
    )
    .unwrap();
}

fn variable_toml(default: i64) -> String {
    format!(
        r#"schema_version = 1
type = "int"

[resolve]
default = {default}
"#
    )
}

fn init_git_repo(repo: &Path) {
    git(repo, &["init"]);
    git(repo, &["config", "user.email", "rototo@example.com"]);
    git(repo, &["config", "user.name", "Rototo Tests"]);
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "initial"]);
}

#[test]
fn package_writes_a_deterministic_content_addressed_archive() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["package", "examples/basic", "--output"])
        .arg(&first)
        .assert()
        .success()
        .stdout(predicate::str::contains("sha256:"));
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["package", "examples/basic", "--output"])
        .arg(&second)
        .assert()
        .success();

    let first_name = single_file_name(&first);
    let second_name = single_file_name(&second);
    // The archive name is the content address, and packaging the same tree twice
    // produces the same digest, so the same file name.
    assert!(first_name.starts_with("sha256:"));
    assert!(first_name.ends_with(".tar.gz"));
    assert_eq!(first_name, second_name);
    // The bytes are byte-for-byte identical, not just same-named.
    assert_eq!(
        fs::read(first.join(&first_name)).unwrap(),
        fs::read(second.join(&second_name)).unwrap()
    );
}

#[test]
fn package_unpacked_writes_the_flattened_projection() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("projection");

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["package", "examples/acme-overlay", "--unpacked"])
        .arg(&target)
        .assert()
        .success()
        .stdout(predicate::str::contains("unpacked"));

    // The projection stands alone: extends is stripped from the manifest.
    let manifest = fs::read_to_string(target.join("rototo-package.toml")).unwrap();
    assert!(!manifest.contains("extends"), "{manifest}");
    // Markers are consumed by composition, not carried into the projection.
    assert!(!target.join("variables/support_banner.update.toml").exists());
    assert!(
        !target
            .join("data/catalogs/support_banner/german_hours.deleted.toml")
            .exists()
    );
    assert!(
        !target
            .join("data/catalogs/support_banner/mobile_help.update.toml")
            .exists()
    );
    // The composed results are: the updated variable, the merged entry, and
    // the base entry the overlay deleted is gone.
    assert!(target.join("variables/support_banner.toml").exists());
    assert!(
        target
            .join("data/catalogs/support_banner/mobile_help.toml")
            .exists()
    );
    assert!(
        !target
            .join("data/catalogs/support_banner/german_hours.toml")
            .exists()
    );
}

#[test]
fn package_unpacked_refuses_a_non_empty_target() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("projection");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("stale.txt"), "leftover").unwrap();

    Command::cargo_bin("rototo")
        .unwrap()
        .env("NO_COLOR", "1")
        .args(["package", "examples/basic", "--unpacked"])
        .arg(&target)
        .assert()
        .failure()
        .stderr(predicate::str::contains("is not empty"));
}

#[test]
fn package_help_is_listed() {
    Command::cargo_bin("rototo")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("package"))
        .stdout(predicate::str::contains(
            "content-addressed distributable archive",
        ));
}

fn single_file_name(dir: &Path) -> String {
    let mut entries = fs::read_dir(dir).unwrap();
    let entry = entries.next().unwrap().unwrap();
    assert!(
        entries.next().is_none(),
        "expected exactly one archive file"
    );
    entry.file_name().to_string_lossy().into_owned()
}

fn git(repo: &Path, args: &[&str]) {
    let output = StdCommand::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run git {args:?}: {err}"));
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn package_token_entries_share_one_grammar_across_flag_and_env() {
    // Two bare tokens (one from the flag, one from the environment) are
    // ambiguous: a bare token is single-origin sugar and must stand alone.
    Command::cargo_bin("rototo")
        .unwrap()
        .env("NO_COLOR", "1")
        .env("ROTOTO_PACKAGE_TOKEN", "env-token")
        .args(["lint", "examples/basic", "--package-token", "flag-token"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("more than one bare package token"));
}

#[test]
fn package_token_rejects_mixing_bare_and_scoped_entries() {
    Command::cargo_bin("rototo")
        .unwrap()
        .env("NO_COLOR", "1")
        .args([
            "lint",
            "examples/basic",
            "--package-token",
            "bare-token",
            "--package-token",
            "https://config.acme.com=scoped",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be mixed"));
}

#[test]
fn package_token_accepts_base64_padded_bare_tokens() {
    // A padded token ends in '=' but is still one bare token, because entry
    // classification keys on the https:// spelling, never on '='.
    Command::cargo_bin("rototo")
        .unwrap()
        .env("ROTOTO_PACKAGE_TOKEN", "dG9rZW4=")
        .args(["--quiet", "lint", "examples/basic"])
        .assert()
        .success();
}

#[test]
fn package_token_env_takes_whitespace_separated_scoped_entries() {
    // Scoped entries never touch local loads; parsing them must succeed.
    Command::cargo_bin("rototo")
        .unwrap()
        .env(
            "ROTOTO_PACKAGE_TOKEN",
            "https://config.acme.com/team-a=one https://archives.example.net=two",
        )
        .args(["--quiet", "lint", "examples/basic"])
        .assert()
        .success();
}

#[test]
fn package_token_prefix_without_a_token_is_rejected() {
    Command::cargo_bin("rototo")
        .unwrap()
        .env("NO_COLOR", "1")
        .args([
            "lint",
            "examples/basic",
            "--package-token",
            "https://config.acme.com",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("has no `=TOKEN`"));
}

/// Lint selectors filter which diagnostics count: a matching rule filter
/// shows only that rule and keeps the failing exit; a filter matching
/// nothing reports ok. Authority filtering selects the custom-lint side.
#[test]
fn lint_selectors_filter_diagnostics_and_exit_status() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "lint",
            "tests/fixtures/packages/lint-failures",
            "--lint-rule",
            "rototo/layer-shape",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("rototo/layer-shape"))
        .stdout(predicate::str::contains("rototo/variable-rule-shape").not());

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "lint",
            "tests/fixtures/packages/lint-failures",
            "--lint-rule",
            "rototo/package-not-found",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok:"));

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "lint",
            "tests/fixtures/packages/lint-failures",
            "--lint-authority",
            "fixture",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("fixture/custom-variable-rejected"))
        .stdout(predicate::str::contains("rototo/").not());
}
