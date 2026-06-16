use crate::console::identity::ActorIdentity;
use crate::console::token_crypto::TokenCrypto;

use rusqlite::Connection;

use super::*;

async fn test_store() -> Store {
    Store::open_in_memory(TokenCrypto::generate().unwrap()).unwrap()
}

fn discovered(path: &str) -> DiscoveredWorkspaceInput {
    DiscoveredWorkspaceInput {
        path: path.to_owned(),
        git_ref: "main".to_owned(),
        source: format!("https://api.github.com/repos/o/r/tarball/main#:{path}"),
    }
}

fn github_source_tree(workspaces: Vec<DiscoveredWorkspaceInput>) -> RegisterSourceTreeInput {
    RegisterSourceTreeInput {
        principal_id: "42".to_owned(),
        kind: SourceTreeKind::GitHub,
        source: "git+https://github.com/octo/configs.git#main".to_owned(),
        display_name: "octo/configs".to_owned(),
        default_revision: "main".to_owned(),
        workspace_owner: "octo".to_owned(),
        workspace_name: "configs".to_owned(),
        workspaces,
    }
}

fn local_source_tree(workspaces: Vec<DiscoveredWorkspaceInput>) -> RegisterSourceTreeInput {
    RegisterSourceTreeInput {
        principal_id: "42".to_owned(),
        kind: SourceTreeKind::LocalFolder,
        source: "/tmp/configs".to_owned(),
        display_name: "demo/configs".to_owned(),
        default_revision: "main".to_owned(),
        workspace_owner: "demo".to_owned(),
        workspace_name: "configs".to_owned(),
        workspaces,
    }
}

fn user_version(conn: &Connection) -> i32 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    statement
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[test]
fn schema_initialization_sets_store_schema_version() {
    let conn = Connection::open_in_memory().unwrap();
    schema::initialize_schema(&conn).unwrap();

    assert_eq!(user_version(&conn), 7);
    assert!(
        table_columns(&conn, "source_trees")
            .iter()
            .any(|column| column == "source")
    );
    assert!(
        table_columns(&conn, "source_tree_workspaces")
            .iter()
            .any(|column| column == "source_tree_id")
    );
    assert!(
        table_columns(&conn, "active_branches")
            .iter()
            .any(|column| column == "branch")
    );
    assert!(
        table_columns(&conn, "active_branch_workspaces")
            .iter()
            .any(|column| column == "workspace_path")
    );
    assert!(table_columns(&conn, "repos").is_empty());
    assert!(table_columns(&conn, "workspaces").is_empty());
}

#[test]
fn schema_initialization_rejects_newer_store_schema() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA user_version = 99").unwrap();

    let err = schema::initialize_schema(&conn).unwrap_err();

    assert!(err.to_string().contains("newer than this rototo binary"));
}

#[test]
fn schema_initialization_rejects_older_nonzero_store_schema() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA user_version = 5").unwrap();

    let err = schema::initialize_schema(&conn).unwrap_err();

    assert!(err.to_string().contains("is not supported"));
}

#[tokio::test]
async fn sessions_round_trip_and_expire_tokens_encrypted() {
    let store = test_store().await;
    let token = store
        .create_session(NewSession {
            identity: ActorIdentity::GitHub {
                id: "42".to_owned(),
                login: "octocat".to_owned(),
                name: Some("Octo Cat".to_owned()),
                avatar_url: None,
            },
            github_token: "gho_secret".to_owned(),
        })
        .await
        .unwrap();
    let user = store.get_session(&token).await.unwrap().unwrap();
    assert_eq!(user.principal_id, "github:42");
    assert_eq!(user.github_token.as_deref(), Some("gho_secret"));
    match user.identity {
        ActorIdentity::GitHub {
            id, login, name, ..
        } => {
            assert_eq!(id, "42");
            assert_eq!(login, "octocat");
            assert_eq!(name.as_deref(), Some("Octo Cat"));
        }
        ActorIdentity::GitConfig { .. } => panic!("expected GitHub identity"),
    }
    store.delete_session(&token).await.unwrap();
    assert!(store.get_session(&token).await.unwrap().is_none());
}

#[tokio::test]
async fn oauth_states_consume_once() {
    let store = test_store().await;
    store.create_oauth_state("abc").await.unwrap();
    assert!(store.consume_oauth_state("abc").await.unwrap());
    assert!(!store.consume_oauth_state("abc").await.unwrap());
    assert!(!store.consume_oauth_state("missing").await.unwrap());
}

#[tokio::test]
async fn source_tree_upsert_lists_workspaces_with_slugs() {
    let store = test_store().await;
    let registered = store
        .upsert_source_tree_with_workspaces(github_source_tree(vec![
            discovered("."),
            discovered("payments/flags"),
        ]))
        .await
        .unwrap();
    assert_eq!(registered.source_tree.kind, SourceTreeKind::GitHub);
    assert_eq!(registered.source_tree.display_name, "octo/configs");
    assert!(registered.source_tree.capabilities.can_branch);
    assert_eq!(registered.workspaces.len(), 2);
    assert_eq!(registered.workspaces[0].slug, "configs");
    assert_eq!(registered.workspaces[1].slug, "configs-payments-flags");

    let by_slug = store
        .get_workspace_for_user("configs-payments-flags", "42")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_slug.path, "payments/flags");
    let by_id = store
        .get_workspace_for_user(&by_slug.id, "42")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_id.id, by_slug.id);
    assert!(
        store
            .get_workspace_for_user(&by_slug.id, "999")
            .await
            .unwrap()
            .is_none()
    );

    assert!(
        store
            .delete_source_tree_for_user(&registered.source_tree.id, "42")
            .await
            .unwrap()
    );
    assert!(
        store
            .list_workspaces_for_user("42")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn read_only_source_tree_kind_disables_branch_capabilities() {
    let store = test_store().await;
    let registered = store
        .upsert_source_tree_with_workspaces(local_source_tree(vec![discovered(".")]))
        .await
        .unwrap();

    assert_eq!(registered.source_tree.kind, SourceTreeKind::LocalFolder);
    assert!(registered.source_tree.capabilities.can_load_workspaces);
    assert!(!registered.source_tree.capabilities.can_branch);
    assert!(!registered.source_tree.capabilities.can_edit);
    assert!(!registered.source_tree.capabilities.can_open_pull_request);
}

#[tokio::test]
async fn active_branch_can_include_multiple_workspaces() {
    let store = test_store().await;
    let registered = store
        .upsert_source_tree_with_workspaces(github_source_tree(vec![
            discovered("."),
            discovered("payments/flags"),
        ]))
        .await
        .unwrap();
    let root = registered.workspaces[0].clone();
    let flags = registered.workspaces[1].clone();

    let branch = store
        .select_branch(SelectBranchInput {
            workspace_id: root.id.clone(),
            principal_id: "42".to_owned(),
            branch: "feature/payments".to_owned(),
            base_ref: "main".to_owned(),
            base_commit: Some("0123456789abcdef0123456789abcdef01234567".to_owned()),
            last_seen_commit: None,
        })
        .await
        .unwrap();
    assert_eq!(branch.status, ActiveBranchStatus::Active);
    assert_eq!(branch.last_selected_workspace_path.as_deref(), Some("."));

    let existing = store
        .find_active_branch_for_source_tree_branch(&flags.id, "42", "feature/payments")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(existing.id, branch.id);

    let branch = store
        .ensure_active_branch_workspace(&branch.id, &flags.id, "42")
        .await
        .unwrap();
    assert_eq!(
        branch.last_selected_workspace_path.as_deref(),
        Some("payments/flags")
    );

    let root_branches = store
        .list_active_branches_for_workspace(&root.id, "42")
        .await
        .unwrap();
    let flags_branches = store
        .list_active_branches_for_workspace(&flags.id, "42")
        .await
        .unwrap();
    assert_eq!(root_branches[0].id, branch.id);
    assert_eq!(flags_branches[0].id, branch.id);

    let workspaces = store
        .list_workspaces_for_active_branch(&branch.id)
        .await
        .unwrap();
    let paths: Vec<&str> = workspaces
        .iter()
        .map(|workspace| workspace.path.as_str())
        .collect();
    assert_eq!(paths, [".", "payments/flags"]);
}

#[tokio::test]
async fn active_branch_lists_recent_but_not_archived_branches() {
    let store = test_store().await;
    let registered = store
        .upsert_source_tree_with_workspaces(github_source_tree(vec![discovered(".")]))
        .await
        .unwrap();
    let workspace = registered.workspaces[0].clone();

    let active = store
        .select_branch(SelectBranchInput {
            workspace_id: workspace.id.clone(),
            principal_id: "42".to_owned(),
            branch: "feature/active".to_owned(),
            base_ref: "main".to_owned(),
            base_commit: None,
            last_seen_commit: None,
        })
        .await
        .unwrap();
    let recent = store
        .select_branch(SelectBranchInput {
            workspace_id: workspace.id.clone(),
            principal_id: "42".to_owned(),
            branch: "feature/recent".to_owned(),
            base_ref: "main".to_owned(),
            base_commit: None,
            last_seen_commit: None,
        })
        .await
        .unwrap();
    let archived = store
        .select_branch(SelectBranchInput {
            workspace_id: workspace.id.clone(),
            principal_id: "42".to_owned(),
            branch: "feature/archived".to_owned(),
            base_ref: "main".to_owned(),
            base_commit: None,
            last_seen_commit: None,
        })
        .await
        .unwrap();

    let recent = store.mark_active_branch_recent(&recent.id).await.unwrap();
    assert_eq!(recent.status, ActiveBranchStatus::Recent);
    let archived = store.archive_active_branch(&archived.id).await.unwrap();
    assert_eq!(archived.status, ActiveBranchStatus::Archived);
    assert!(archived.archived_at.is_some());

    let mut branches: Vec<String> = store
        .list_active_branches_for_workspace(&workspace.id, "42")
        .await
        .unwrap()
        .into_iter()
        .map(|branch| branch.branch)
        .collect();
    branches.sort();
    assert_eq!(branches, ["feature/active", "feature/recent"]);

    let fetched = store
        .get_active_branch_for_user(&archived.id, &workspace.id, "42")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, ActiveBranchStatus::Archived);
    assert_eq!(active.branch, "feature/active");
}

#[tokio::test]
async fn active_branch_updates_edit_and_pull_request_metadata() {
    let store = test_store().await;
    let registered = store
        .upsert_source_tree_with_workspaces(github_source_tree(vec![discovered(".")]))
        .await
        .unwrap();
    let branch = store
        .select_branch(SelectBranchInput {
            workspace_id: registered.workspaces[0].id.clone(),
            principal_id: "42".to_owned(),
            branch: "feature/pr".to_owned(),
            base_ref: "main".to_owned(),
            base_commit: None,
            last_seen_commit: None,
        })
        .await
        .unwrap();

    let edited = store
        .record_active_branch_edit(
            &branch.id,
            Some("fedcba9876543210fedcba9876543210fedcba98".to_owned()),
        )
        .await
        .unwrap();
    assert!(edited.last_edited_at.is_some());
    assert_eq!(
        edited.last_seen_commit.as_deref(),
        Some("fedcba9876543210fedcba9876543210fedcba98")
    );

    let updated = store
        .update_active_branch_pull_request_state(BranchPullRequestInput {
            branch_id: branch.id.clone(),
            pr_number: 12,
            pr_state: "open".to_owned(),
            pr_url: "https://github.com/octo/configs/pull/12".to_owned(),
            pr_merged_at: None,
        })
        .await
        .unwrap();
    assert_eq!(updated.pr_number, Some(12));
    assert_eq!(updated.pr_state.as_deref(), Some("open"));
    assert!(updated.pr_synced_at.is_some());
}

#[tokio::test]
async fn source_tree_upsert_hides_missing_workspace_but_keeps_active_branches() {
    let store = test_store().await;
    let registered = store
        .upsert_source_tree_with_workspaces(github_source_tree(vec![discovered(".")]))
        .await
        .unwrap();
    let workspace = registered.workspaces[0].clone();
    let branch = store
        .select_branch(SelectBranchInput {
            workspace_id: workspace.id.clone(),
            principal_id: "42".to_owned(),
            branch: "feature/root".to_owned(),
            base_ref: "main".to_owned(),
            base_commit: None,
            last_seen_commit: None,
        })
        .await
        .unwrap();

    let rediscovered = store
        .upsert_source_tree_with_workspaces(github_source_tree(vec![]))
        .await
        .unwrap();

    assert!(rediscovered.workspaces.is_empty());
    assert!(
        store
            .list_workspaces_for_user("42")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .get_workspace_for_user(&workspace.id, "42")
            .await
            .unwrap()
            .is_some()
    );
    let branches = store.list_active_branches_for_user("42").await.unwrap();
    assert_eq!(branches.len(), 1);
    assert_eq!(branches[0].id, branch.id);
    let workspaces = store
        .list_workspaces_for_active_branch(&branch.id)
        .await
        .unwrap();
    assert_eq!(workspaces[0].id, workspace.id);
}
