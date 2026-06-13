use assert_cmd::Command;
use predicates::prelude::*;
use std::process::Stdio;
use std::time::{Duration, Instant};

use rototo::{
    LintMode, LoadOptions, RefreshOptions, RefreshOutcome, RefreshingWorkspace, ResolveContext,
    ResolveOptions, SourceOptions, Workspace, diagnostic_for_rule,
    diagnostics_catalog_for_workspace, inspect_workspace, lint_qualifier, lint_workspace,
    list_catalogs, list_variables, read_catalog, read_qualifiers, read_variable, read_variables,
    resolve_qualifier, resolve_variable, stage_workspace_source,
};

async fn run_git(repo: &std::path::Path, args: &[&str]) {
    let status = tokio::process::Command::new("git")
        .current_dir(repo)
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

async fn git_output(repo: &std::path::Path, args: &[&str]) -> String {
    let output = tokio::process::Command::new("git")
        .current_dir(repo)
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

async fn write_minimal_workspace(root: &std::path::Path) {
    write_minimal_workspace_with_message(root, "hello").await;
}

async fn write_minimal_workspace_with_message(root: &std::path::Path, message: &str) {
    tokio::fs::create_dir_all(root.join("variables"))
        .await
        .unwrap();
    tokio::fs::write(
        root.join("rototo-workspace.toml"),
        r#"schema_version = 1
"#,
    )
    .await
    .unwrap();
    tokio::fs::write(
        root.join("variables/message.toml"),
        format!(
            r#"schema_version = 1

description = "Message"
type = "string"

[values]
default = "{message}"

[resolve]
default = "default"
"#,
        ),
    )
    .await
    .unwrap();
}

async fn write_string_variable(root: &std::path::Path, id: &str, value: &str) {
    tokio::fs::create_dir_all(root.join("variables"))
        .await
        .unwrap();
    tokio::fs::write(
        root.join(format!("variables/{id}.toml")),
        format!(
            r#"schema_version = 1
type = "string"

[values]
default = "{value}"

[resolve]
default = "default"
"#,
        ),
    )
    .await
    .unwrap();
}

async fn commit_all(repo: &std::path::Path, message: &str) {
    run_git(repo, &["add", "."]).await;
    run_git(
        repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "commit",
            "-m",
            message,
        ],
    )
    .await;
}

async fn wait_for_condition<F, Fut>(mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if condition().await {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("condition was not met");
}

#[tokio::test]
async fn sdk_inspects_workspace() {
    let inspection = inspect_workspace("examples/basic".as_ref()).await.unwrap();

    assert!(
        inspection
            .qualifiers
            .iter()
            .any(|qualifier| qualifier.uri == "qualifier://premium-users")
    );
    assert!(
        inspection
            .variables
            .iter()
            .any(|variable| variable.uri == "variable://checkout-redesign")
    );
    assert!(
        inspection
            .schemas
            .iter()
            .any(|schema| schema.path == std::path::Path::new("schemas/context.schema.json"))
    );
    assert!(
        inspection
            .linters
            .iter()
            .any(|linter| linter.id == "checkout-redesign")
    );
}

#[tokio::test]
async fn sdk_lints_workspace() {
    let lint = lint_workspace("examples/basic".as_ref()).await.unwrap();

    assert!(lint.diagnostics.is_empty());
}

#[tokio::test]
async fn sdk_lints_qualifier() {
    let lint = lint_qualifier("examples/basic".as_ref(), "premium-users")
        .await
        .unwrap();

    assert!(lint.diagnostics.is_empty());
}

#[tokio::test]
async fn sdk_lists_variables_for_apps() {
    let variables = list_variables("examples/basic".as_ref()).await.unwrap();

    assert!(variables.len() > 2);
    assert!(
        variables
            .iter()
            .any(|variable| variable.uri == "variable://checkout-redesign")
    );
}

#[tokio::test]
async fn sdk_lists_catalogs_for_apps() {
    let catalogs = list_catalogs("examples/basic".as_ref()).await.unwrap();

    assert!(catalogs.len() > 2);
    assert!(
        catalogs
            .iter()
            .any(|catalog| catalog.uri == "catalog://checkout-redesign")
    );
}

#[tokio::test]
async fn sdk_reads_variable_config() {
    let variable = read_variable("examples/basic".as_ref(), "checkout-redesign")
        .await
        .unwrap();

    assert_eq!(variable.id, "checkout-redesign");
    assert_eq!(
        variable.value["description"],
        "Checkout page content and layout variant"
    );
}

#[tokio::test]
async fn sdk_reads_catalog_config() {
    let catalog = read_catalog("examples/basic".as_ref(), "checkout-redesign")
        .await
        .unwrap();

    assert_eq!(catalog.id, "checkout-redesign");
    assert_eq!(catalog.value["entries"]["premium"]["variant"], "premium");
}

#[tokio::test]
async fn sdk_reads_primitive_variable_values() {
    let variable = read_variable("examples/basic".as_ref(), "premium-message")
        .await
        .unwrap();

    assert_eq!(variable.value["values"]["control"], "Welcome back.");
    assert_eq!(
        variable.value["values"]["premium"],
        "Welcome back, premium member."
    );
}

#[tokio::test]
async fn sdk_reads_all_basic_variable_configs_with_declared_sources() {
    let variables = read_variables("examples/basic".as_ref()).await.unwrap();

    assert!(variables.len() > 10);
    for variable in variables {
        let type_name = variable.value["type"].as_str().unwrap_or_default();
        if type_name.starts_with("catalog:") {
            assert!(
                variable.value.get("values").is_none(),
                "variable://{} should not declare inline values",
                variable.id
            );
        } else {
            assert!(
                variable.value["values"]
                    .as_object()
                    .is_some_and(|values| !values.is_empty()),
                "variable://{} should expose at least one value",
                variable.id
            );
        }
    }
}

#[tokio::test]
async fn catalog_entry_files_are_whole_toml_objects() {
    let catalogs_dir = std::path::Path::new("examples/basic/catalogs");
    for entry in std::fs::read_dir(catalogs_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.is_dir()
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("-entries"))
        {
            continue;
        }

        for value_entry in std::fs::read_dir(&path).unwrap() {
            let value_path = value_entry.unwrap().path();
            if value_path
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("toml")
            {
                continue;
            }
            let text = std::fs::read_to_string(&value_path).unwrap();
            let toml = text.parse::<toml::Value>().unwrap();
            let table = toml
                .as_table()
                .unwrap_or_else(|| panic!("{} should be a TOML table", value_path.display()));
            assert!(
                !table.is_empty(),
                "{} should contain an object value",
                value_path.display()
            );
            assert!(
                table.get("value").and_then(toml::Value::as_table).is_none(),
                "{} should not use a wrapper table",
                value_path.display()
            );
        }
    }
}

#[tokio::test]
async fn sdk_sample_app_runs() {
    Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--offline",
            "--locked",
            "--manifest-path",
            "examples/sdk-app/Cargo.toml",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("premium-users: true"))
        .stdout(predicate::str::contains("enterprise-accounts: true"))
        .stdout(predicate::str::contains("checkout variant: premium"))
        .stdout(predicate::str::contains("agent model: gpt-5"))
        .stdout(predicate::str::contains(
            "message: Welcome back, premium member.",
        ));
}

#[tokio::test]
async fn sdk_reads_all_qualifier_configs() {
    let qualifiers = read_qualifiers("examples/basic".as_ref()).await.unwrap();

    assert!(qualifiers.len() > 1);
    assert!(
        qualifiers
            .iter()
            .any(|qualifier| qualifier.uri == "qualifier://premium-users")
    );
}

#[tokio::test]
async fn sdk_reads_diagnostic_catalog() {
    let catalog = diagnostics_catalog_for_workspace("examples/basic".as_ref())
        .await
        .unwrap();
    let diagnostic = diagnostic_for_rule(&catalog, "rototo/qualifier-parse-failed").unwrap();

    assert_eq!(
        diagnostic.entity,
        Some(rototo::diagnostics::DiagnosticEntity::Qualifier)
    );
}

#[tokio::test]
async fn workspace_sdk_loads_file_source() {
    let root = std::path::absolute("examples/basic").unwrap();
    let workspace = Workspace::load(format!("file://{}", root.display()))
        .await
        .unwrap();

    assert!(
        workspace
            .inspection()
            .variables
            .iter()
            .any(|variable| variable.id == "checkout-redesign")
    );
}

#[tokio::test]
async fn workspace_sdk_loads_git_file_source_with_ref_and_subdir() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let workspace_root = repo.join("rototo");
    tokio::fs::create_dir_all(&repo).await.unwrap();
    write_minimal_workspace(&workspace_root).await;
    run_git(&repo, &["init", "--initial-branch", "main"]).await;
    commit_all(&repo, "initial").await;

    let source = format!("git+file://{}#main:rototo", repo.display());
    let workspace = Workspace::load(source).await.unwrap();

    assert_eq!(workspace.inspection().variables[0].id, "message");
    let resolution = workspace
        .resolve_variable(
            "message",
            &ResolveContext::from_json(serde_json::json!({})).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resolution.value, "hello");
}

#[tokio::test]
async fn refreshing_workspace_manual_refresh_updates_git_source() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let workspace_root = repo.join("rototo");
    tokio::fs::create_dir_all(&repo).await.unwrap();
    write_minimal_workspace_with_message(&workspace_root, "hello").await;
    run_git(&repo, &["init", "--initial-branch", "main"]).await;
    commit_all(&repo, "initial").await;

    let source = format!("git+file://{}#main:rototo", repo.display());
    let workspace = RefreshingWorkspace::load(source, RefreshOptions::new())
        .await
        .unwrap();
    let context = ResolveContext::from_json(serde_json::json!({})).unwrap();

    let resolution = workspace
        .resolve_variable("message", &context)
        .await
        .unwrap();
    assert_eq!(resolution.value, "hello");

    write_minimal_workspace_with_message(&workspace_root, "goodbye").await;
    commit_all(&repo, "update").await;

    assert_eq!(
        workspace.refresh_now().await.unwrap(),
        RefreshOutcome::Refreshed
    );
    let resolution = workspace
        .resolve_variable("message", &context)
        .await
        .unwrap();
    assert_eq!(resolution.value, "goodbye");
    assert_eq!(workspace.status().await.consecutive_failures, 0);
}

#[tokio::test]
async fn refreshing_workspace_failed_refresh_keeps_last_loaded_git_workspace() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let workspace_root = repo.join("rototo");
    tokio::fs::create_dir_all(&repo).await.unwrap();
    write_minimal_workspace_with_message(&workspace_root, "hello").await;
    run_git(&repo, &["init", "--initial-branch", "main"]).await;
    commit_all(&repo, "initial").await;

    let source = format!("git+file://{}#main:rototo", repo.display());
    let workspace = RefreshingWorkspace::load(source, RefreshOptions::new())
        .await
        .unwrap();
    let context = ResolveContext::from_json(serde_json::json!({})).unwrap();

    tokio::fs::write(workspace_root.join("rototo-workspace.toml"), "not = [valid")
        .await
        .unwrap();
    commit_all(&repo, "break workspace").await;

    assert!(workspace.refresh_now().await.is_err());
    let resolution = workspace
        .resolve_variable("message", &context)
        .await
        .unwrap();
    assert_eq!(resolution.value, "hello");
    let status = workspace.status().await;
    assert_eq!(status.consecutive_failures, 1);
    assert!(status.last_error.is_some());
}

#[tokio::test]
async fn refreshing_workspace_snapshots_local_source_for_last_known_good_resolution() {
    let temp = tempfile::TempDir::new().unwrap();
    let workspace_root = temp.path().join("rototo");
    write_minimal_workspace_with_message(&workspace_root, "hello").await;

    let workspace =
        RefreshingWorkspace::load(workspace_root.to_string_lossy(), RefreshOptions::new())
            .await
            .unwrap();
    let context = ResolveContext::from_json(serde_json::json!({})).unwrap();

    tokio::fs::write(workspace_root.join("rototo-workspace.toml"), "not = [valid")
        .await
        .unwrap();

    assert!(workspace.refresh_now().await.is_err());
    let resolution = workspace
        .resolve_variable("message", &context)
        .await
        .unwrap();
    assert_eq!(resolution.value, "hello");
}

#[tokio::test]
async fn refreshing_workspace_refreshes_when_parent_layer_changes() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let child = temp.path().join("child");

    tokio::fs::create_dir_all(&base).await.unwrap();
    tokio::fs::write(
        base.join("rototo-workspace.toml"),
        r#"schema_version = 1
"#,
    )
    .await
    .unwrap();
    write_string_variable(&base, "base-only", "before").await;

    tokio::fs::create_dir_all(&child).await.unwrap();
    tokio::fs::write(
        child.join("rototo-workspace.toml"),
        r#"schema_version = 1
extends = ["../base"]
"#,
    )
    .await
    .unwrap();
    write_string_variable(&child, "child-only", "child").await;

    let workspace = RefreshingWorkspace::load(child.to_string_lossy(), RefreshOptions::new())
        .await
        .unwrap();
    let context = ResolveContext::from_json(serde_json::json!({})).unwrap();

    assert_eq!(
        workspace
            .resolve_variable("base-only", &context)
            .await
            .unwrap()
            .value,
        "before"
    );

    write_string_variable(&base, "base-only", "after").await;

    assert_eq!(
        workspace.refresh_now().await.unwrap(),
        RefreshOutcome::Refreshed
    );
    assert_eq!(
        workspace
            .resolve_variable("base-only", &context)
            .await
            .unwrap()
            .value,
        "after"
    );
}

#[tokio::test]
async fn refreshing_workspace_unchanged_git_source_skips_reload() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let workspace_root = repo.join("rototo");
    tokio::fs::create_dir_all(&repo).await.unwrap();
    write_minimal_workspace(&workspace_root).await;
    run_git(&repo, &["init", "--initial-branch", "main"]).await;
    commit_all(&repo, "initial").await;

    let source = format!("git+file://{}#main:rototo", repo.display());
    let workspace = RefreshingWorkspace::load(source, RefreshOptions::new())
        .await
        .unwrap();

    assert_eq!(
        workspace.refresh_now().await.unwrap(),
        RefreshOutcome::Unchanged
    );
}

#[tokio::test]
async fn refreshing_workspace_pinned_git_commit_is_immutable() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let workspace_root = repo.join("rototo");
    tokio::fs::create_dir_all(&repo).await.unwrap();
    write_minimal_workspace(&workspace_root).await;
    run_git(&repo, &["init", "--initial-branch", "main"]).await;
    commit_all(&repo, "initial").await;
    let commit = git_output(&repo, &["rev-parse", "HEAD"]).await;

    let source = format!("git+file://{}#{}:rototo", repo.display(), commit);
    let workspace = RefreshingWorkspace::load(
        source,
        RefreshOptions::new().with_period(std::time::Duration::from_millis(10)),
    )
    .await
    .unwrap();

    assert!(workspace.status().await.immutable);
    assert_eq!(
        workspace.refresh_now().await.unwrap(),
        RefreshOutcome::Immutable
    );
}

#[tokio::test(start_paused = true)]
async fn refreshing_workspace_background_loop_refreshes_local_source() {
    let temp = tempfile::TempDir::new().unwrap();
    let workspace_root = temp.path().join("rototo");
    write_minimal_workspace_with_message(&workspace_root, "hello").await;

    let workspace = RefreshingWorkspace::load(
        workspace_root.to_string_lossy(),
        RefreshOptions::new().with_period(Duration::from_secs(5)),
    )
    .await
    .unwrap();
    let context = ResolveContext::from_json(serde_json::json!({})).unwrap();

    write_minimal_workspace_with_message(&workspace_root, "goodbye").await;
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(5)).await;
    wait_for_condition(|| async {
        workspace
            .resolve_variable("message", &context)
            .await
            .is_ok_and(|resolution| resolution.value == "goodbye")
    })
    .await;

    assert_eq!(workspace.status().await.consecutive_failures, 0);
    workspace.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn refreshing_workspace_background_failures_back_off_and_keep_snapshot() {
    let temp = tempfile::TempDir::new().unwrap();
    let workspace_root = temp.path().join("rototo");
    write_minimal_workspace_with_message(&workspace_root, "hello").await;

    let workspace = RefreshingWorkspace::load(
        workspace_root.to_string_lossy(),
        RefreshOptions::new()
            .with_period(Duration::from_secs(5))
            .with_failure_backoff(Duration::from_secs(60), Duration::from_secs(60)),
    )
    .await
    .unwrap();
    let context = ResolveContext::from_json(serde_json::json!({})).unwrap();

    tokio::fs::write(
        workspace_root.join("variables/message.toml"),
        "not = [valid",
    )
    .await
    .unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(5)).await;
    tokio::task::yield_now().await;
    wait_for_condition(|| async {
        let status = workspace.status().await;
        status.consecutive_failures == 1 && !status.refreshing
    })
    .await;
    let status = workspace.status().await;
    assert_eq!(status.consecutive_failures, 1, "status: {status:?}");
    let first_attempt = status.last_attempt;

    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(59)).await;
    tokio::task::yield_now().await;
    let status = workspace.status().await;
    assert_eq!(status.consecutive_failures, 1);
    assert_eq!(status.last_attempt, first_attempt);

    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(5)).await;
    tokio::task::yield_now().await;
    wait_for_condition(|| async {
        let status = workspace.status().await;
        status.consecutive_failures == 2 && !status.refreshing
    })
    .await;
    let resolution = workspace
        .resolve_variable("message", &context)
        .await
        .unwrap();
    assert_eq!(resolution.value, "hello");
    workspace.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn refreshing_workspace_shutdown_stops_background_refresh() {
    let temp = tempfile::TempDir::new().unwrap();
    let workspace_root = temp.path().join("rototo");
    write_minimal_workspace(&workspace_root).await;

    let workspace = RefreshingWorkspace::load(
        workspace_root.to_string_lossy(),
        RefreshOptions::new().with_period(Duration::from_secs(1)),
    )
    .await
    .unwrap();

    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    wait_for_condition(|| async { workspace.status().await.last_attempt.is_some() }).await;

    workspace.shutdown().await;
}

#[tokio::test]
async fn refreshing_workspace_resolves_while_manual_refresh_runs() {
    let temp = tempfile::TempDir::new().unwrap();
    let workspace_root = temp.path().join("rototo");
    write_minimal_workspace_with_message(&workspace_root, "hello").await;

    let workspace = std::sync::Arc::new(
        RefreshingWorkspace::load(workspace_root.to_string_lossy(), RefreshOptions::new())
            .await
            .unwrap(),
    );
    let context = ResolveContext::from_json(serde_json::json!({})).unwrap();
    write_minimal_workspace_with_message(&workspace_root, "goodbye").await;

    let refresh_workspace = workspace.clone();
    let resolve_workspace = workspace.clone();
    let resolve_context = context.clone();
    let (refresh, resolves) = tokio::join!(
        async move { refresh_workspace.refresh_now().await },
        async move {
            let mut results = Vec::new();
            for _ in 0..10 {
                results.push(
                    resolve_workspace
                        .resolve_variable("message", &resolve_context)
                        .await
                        .map(|resolution| resolution.value),
                );
            }
            results
        }
    );

    assert!(refresh.is_ok());
    for resolve in resolves {
        assert!(matches!(
            resolve.unwrap().as_str(),
            Some("hello") | Some("goodbye")
        ));
    }
}

#[tokio::test]
async fn workspace_source_rejects_http_archive_source() {
    let err = stage_workspace_source(
        "http://127.0.0.1/workspace.tar.gz",
        &SourceOptions::default(),
    )
    .await
    .unwrap_err();

    assert_eq!(
        err.to_string(),
        "http:// workspace sources are not supported; use https://"
    );
}

#[tokio::test]
async fn sdk_resolves_qualifier() {
    let context = serde_json::json!({
        "user": {
            "tier": "premium"
        }
    });

    let resolution = resolve_qualifier("examples/basic".as_ref(), "premium-users", &context)
        .await
        .unwrap();

    assert!(resolution.value);
}

#[tokio::test]
async fn sdk_resolves_variable() {
    let context = serde_json::json!({
        "user": {
            "tier": "premium"
        }
    });

    let resolution = resolve_variable("examples/basic".as_ref(), "checkout-redesign", &context)
        .await
        .unwrap();

    assert_eq!(resolution.value_key, "premium");
    assert_eq!(resolution.value["variant"], "premium");
}

#[tokio::test]
async fn sdk_resolves_primitive_variable() {
    let context = serde_json::json!({
        "user": {
            "tier": "premium"
        }
    });

    let resolution = resolve_variable("examples/basic".as_ref(), "premium-message", &context)
        .await
        .unwrap();

    assert_eq!(resolution.value_key, "premium");
    assert_eq!(resolution.value, "Welcome back, premium member.");
}

#[tokio::test]
async fn workspace_sdk_loads_linted_workspace() {
    let workspace = Workspace::load("examples/basic").await.unwrap();

    assert!(workspace.context_schema().is_some());
}

#[tokio::test]
async fn workspace_sdk_resolves_from_loaded_runtime_snapshot() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().join("workspace");
    write_minimal_workspace_with_message(&root, "loaded").await;

    let workspace = Workspace::load(root.to_str().unwrap()).await.unwrap();
    write_minimal_workspace_with_message(&root, "changed").await;

    let context = ResolveContext::from_json(serde_json::json!({})).unwrap();
    let resolution = workspace
        .resolve_variable("message", &context)
        .await
        .unwrap();

    assert_eq!(resolution.value, "loaded");
}

#[tokio::test]
async fn workspace_sdk_loads_layered_workspace_with_child_overrides() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().join("base");
    let child = temp.path().join("child");

    tokio::fs::create_dir_all(&base).await.unwrap();
    tokio::fs::write(
        base.join("rototo-workspace.toml"),
        r#"schema_version = 1
"#,
    )
    .await
    .unwrap();
    write_string_variable(&base, "message", "base").await;
    write_string_variable(&base, "base-only", "base-only").await;

    tokio::fs::create_dir_all(&child).await.unwrap();
    tokio::fs::write(
        child.join("rototo-workspace.toml"),
        r#"schema_version = 1
extends = ["../base"]
"#,
    )
    .await
    .unwrap();
    write_string_variable(&child, "message", "child").await;
    write_string_variable(&child, "child-only", "child-only").await;

    let workspace = Workspace::load(child.to_str().unwrap()).await.unwrap();
    let context = ResolveContext::from_json(serde_json::json!({})).unwrap();

    assert_eq!(workspace.source_layers().len(), 2);
    assert_eq!(
        workspace
            .resolve_variable("message", &context)
            .await
            .unwrap()
            .value,
        "child"
    );
    assert_eq!(
        workspace
            .resolve_variable("base-only", &context)
            .await
            .unwrap()
            .value,
        "base-only"
    );
    assert_eq!(
        workspace
            .resolve_variable("child-only", &context)
            .await
            .unwrap()
            .value,
        "child-only"
    );
}

#[tokio::test]
async fn workspace_sdk_rejects_workspace_when_lint_fails() {
    let err = Workspace::load("tests/fixtures/workspaces/lint-failures")
        .await
        .unwrap_err();

    assert!(err.to_string().contains("workspace lint failed"));
}

#[tokio::test]
async fn workspace_sdk_loads_workspace_when_lint_only_warns() {
    let workspace = Workspace::load("tests/fixtures/workspaces/rules/graph/qualifier-unreferenced")
        .await
        .unwrap();

    assert_eq!(workspace.inspection().qualifiers[0].id, "unused");
}

#[tokio::test]
async fn workspace_sdk_can_inspect_without_linting() {
    let workspace = Workspace::inspect("tests/fixtures/workspaces/lint-failures")
        .await
        .unwrap();

    assert!(!workspace.inspection().variables.is_empty());
}

#[tokio::test]
async fn workspace_sdk_resolves_with_context_contract() {
    let workspace = Workspace::load("examples/basic").await.unwrap();
    let context = ResolveContext::from_json(serde_json::json!({
        "user": {
            "tier": "premium"
        }
    }))
    .unwrap();

    let resolution = workspace
        .resolve_variable("checkout-redesign", &context)
        .await
        .unwrap();

    assert_eq!(resolution.value_key, "premium");
}

#[tokio::test]
async fn workspace_sdk_validates_resolve_context_against_schema() {
    let workspace = Workspace::load("examples/basic").await.unwrap();
    let context = ResolveContext::from_json(serde_json::json!({
        "unknown": true
    }))
    .unwrap();

    let err = workspace
        .resolve_qualifier("premium-users", &context)
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("resolve context does not match schema")
    );
}

#[tokio::test]
async fn workspace_sdk_rejects_missing_predicate_context_even_when_schema_allows_it() {
    let workspace = Workspace::load("examples/basic").await.unwrap();
    let context = ResolveContext::from_json(serde_json::json!({
        "user": {
            "id": "user-123"
        }
    }))
    .unwrap();

    let err = workspace
        .resolve_qualifier("premium-users", &context)
        .await
        .unwrap_err();

    assert_eq!(
        err.to_string(),
        "missing resolve context attribute: user.tier required by qualifier://premium-users"
    );
}

#[tokio::test]
async fn workspace_sdk_resolves_from_context_only() {
    let workspace = Workspace::load("examples/basic").await.unwrap();
    let context = ResolveContext::from_json(serde_json::json!({
        "lane": "prd",
        "user": {
            "tier": "premium"
        }
    }))
    .unwrap();

    let resolution = workspace
        .resolve_variable_with_options(
            "checkout-redesign",
            &context,
            ResolveOptions {
                validate_context: false,
            },
        )
        .await
        .unwrap();

    assert_eq!(resolution.value_key, "premium");
}

#[tokio::test]
async fn workspace_sdk_loads_malformed_context_config_when_lint_is_skipped_for_inspection() {
    let workspace = Workspace::load_with_options(
        "tests/fixtures/workspaces/bad-context-config",
        LoadOptions::new().with_lint(LintMode::Skip),
    )
    .await
    .unwrap();

    let context = ResolveContext::from_json(serde_json::json!({})).unwrap();
    let err = workspace
        .resolve_qualifier("anything", &context)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("loaded without a runtime model"));
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_sdk_rejects_context_schema_symlink_escape() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().join("workspace");
    tokio::fs::create_dir_all(root.join("schemas"))
        .await
        .unwrap();
    tokio::fs::write(
        root.join("rototo-workspace.toml"),
        r#"schema_version = 1
"#,
    )
    .await
    .unwrap();
    tokio::fs::write(
        temp.path().join("outside.schema.json"),
        r#"{"type":"object"}"#,
    )
    .await
    .unwrap();
    std::os::unix::fs::symlink(
        temp.path().join("outside.schema.json"),
        root.join("schemas/context.schema.json"),
    )
    .unwrap();

    let err = Workspace::load(root.to_str().unwrap()).await.unwrap_err();

    assert!(err.to_string().contains("workspace lint failed"));
}

#[tokio::test]
async fn workspace_sdk_rejects_non_object_resolve_context() {
    let err = ResolveContext::from_json(serde_json::json!(["not", "an", "object"])).unwrap_err();

    assert_eq!(err.to_string(), "resolve context must be a JSON object");
}

#[tokio::test]
async fn workspace_sdk_can_load_with_lint_skipped_for_inspection_tools() {
    let workspace = Workspace::load_with_options(
        "tests/fixtures/workspaces/lint-failures",
        LoadOptions::new().with_lint(LintMode::Skip),
    )
    .await
    .unwrap();

    assert!(!workspace.inspection().variables.is_empty());
}

#[tokio::test]
async fn workspace_sdk_can_bypass_context_validation_explicitly() {
    let workspace = Workspace::load("examples/basic").await.unwrap();
    let context = ResolveContext::from_json(serde_json::json!({
        "unknown": true,
        "user": {
            "tier": "free"
        }
    }))
    .unwrap();

    let resolution = workspace
        .resolve_qualifier_with_options(
            "premium-users",
            &context,
            ResolveOptions {
                validate_context: false,
            },
        )
        .await
        .unwrap();

    assert!(!resolution.value);
}
