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

    assert_eq!(user_version(&conn), 3);
}

#[test]
fn schema_initialization_baselines_legacy_version_zero_stores() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE repos (
          id TEXT PRIMARY KEY,
          principal_id TEXT NOT NULL,
          owner TEXT NOT NULL,
          name TEXT NOT NULL,
          default_ref TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          last_discovered_at TEXT,
          UNIQUE(principal_id, owner, name)
        );

        CREATE TABLE workspaces (
          id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          owner TEXT NOT NULL,
          name TEXT NOT NULL,
          path TEXT NOT NULL,
          ref_ TEXT NOT NULL,
          source TEXT NOT NULL,
          discovered_at TEXT NOT NULL,
          UNIQUE(repo_id, path, ref_),
          FOREIGN KEY(repo_id) REFERENCES repos(id) ON DELETE CASCADE
        );

        INSERT INTO repos (
          id, principal_id, owner, name, default_ref,
          created_at, updated_at, last_discovered_at
        ) VALUES (
          'repo-1', '42', 'octo', 'configs', 'main',
          '2026-06-14T00:00:00Z', '2026-06-14T00:00:00Z', NULL
        );

        INSERT INTO workspaces (
          id, repo_id, owner, name, path, ref_, source, discovered_at
        ) VALUES (
          'workspace-1', 'repo-1', 'octo', 'configs', '.', 'main',
          'git+https://github.com/octo/configs#main:.', '2026-06-14T00:00:00Z'
        );
        "#,
    )
    .unwrap();

    schema::initialize_schema(&conn).unwrap();

    assert_eq!(user_version(&conn), 3);
    assert!(
        table_columns(&conn, "workspaces")
            .iter()
            .any(|column| column == "active")
    );
    let active: i32 = conn
        .query_row(
            "SELECT active FROM workspaces WHERE id = 'workspace-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active, 1);
}

#[test]
fn schema_migration_v2_generalizes_draft_change_targets() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE draft_sessions (
          id TEXT PRIMARY KEY,
          workspace_id TEXT NOT NULL,
          principal_id TEXT NOT NULL,
          branch TEXT NOT NULL,
          base_ref TEXT NOT NULL,
          status TEXT NOT NULL,
          pr_url TEXT,
          pr_number INTEGER,
          pr_state TEXT,
          pr_merged_at TEXT,
          pr_synced_at TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          published_at TEXT
        );

        CREATE TABLE repos (
          id TEXT PRIMARY KEY,
          principal_id TEXT NOT NULL,
          owner TEXT NOT NULL,
          name TEXT NOT NULL,
          default_ref TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          last_discovered_at TEXT
        );

        CREATE TABLE workspaces (
          id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          owner TEXT NOT NULL,
          name TEXT NOT NULL,
          path TEXT NOT NULL,
          ref_ TEXT NOT NULL,
          source TEXT NOT NULL,
          discovered_at TEXT NOT NULL,
          active INTEGER NOT NULL DEFAULT 1,
          FOREIGN KEY(repo_id) REFERENCES repos(id) ON DELETE CASCADE
        );

        CREATE TABLE draft_changes (
          id TEXT PRIMARY KEY,
          draft_id TEXT NOT NULL,
          file_path TEXT NOT NULL,
          variable_id TEXT NOT NULL,
          value_key TEXT NOT NULL,
          before_json TEXT NOT NULL,
          after_json TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          UNIQUE(draft_id, variable_id, value_key)
        );

        INSERT INTO draft_sessions (
          id, workspace_id, principal_id, branch, base_ref, status,
          created_at, updated_at
        ) VALUES (
          'draft-1', 'workspace-1', '42', 'draft-branch', 'main', 'open',
          '2026-06-14T00:00:00Z', '2026-06-14T00:00:00Z'
        );

        INSERT INTO repos (
          id, principal_id, owner, name, default_ref,
          created_at, updated_at, last_discovered_at
        ) VALUES (
          'repo-1', '42', 'octo', 'configs', 'main',
          '2026-06-14T00:00:00Z', '2026-06-14T00:00:00Z', NULL
        );

        INSERT INTO workspaces (
          id, repo_id, owner, name, path, ref_, source, discovered_at, active
        ) VALUES (
          'workspace-1', 'repo-1', 'octo', 'configs', '.', 'main',
          'git+https://github.com/octo/configs.git#main', '2026-06-14T00:00:00Z', 1
        );

        INSERT INTO draft_changes (
          id, draft_id, file_path, variable_id, value_key,
          before_json, after_json, updated_at
        ) VALUES (
          'change-1', 'draft-1', 'variables/banner.toml', 'banner', 'control/test~key',
          'false', 'true', '2026-06-14T00:00:00Z'
        );

        PRAGMA user_version = 1;
        "#,
    )
    .unwrap();

    schema::initialize_schema(&conn).unwrap();

    assert_eq!(user_version(&conn), 3);
    let columns = table_columns(&conn, "draft_changes");
    assert!(columns.iter().any(|column| column == "target_path"));
    assert!(!columns.iter().any(|column| column == "variable_id"));
    assert!(
        table_columns(&conn, "draft_workspaces")
            .iter()
            .any(|column| column == "workspace_id")
    );
    let target_path: String = conn
        .query_row(
            "SELECT target_path FROM draft_changes WHERE id = 'change-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(target_path, "/values/control~1test~0key");
}

#[test]
fn schema_initialization_rejects_newer_store_schema() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA user_version = 99").unwrap();

    let err = schema::initialize_schema(&conn).unwrap_err();

    assert!(err.to_string().contains("newer than this rototo binary"));
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
async fn repo_upsert_lists_workspaces_with_slugs() {
    let store = test_store().await;
    let repo = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![discovered("."), discovered("payments/flags")],
        )
        .await
        .unwrap();
    assert_eq!(repo.workspaces.len(), 2);
    assert_eq!(repo.workspaces[0].slug, "configs");
    assert_eq!(repo.workspaces[1].slug, "configs-payments-flags");

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
            .delete_repo_for_user(&repo.repo.id, "42")
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
async fn repo_upsert_preserves_existing_workspace_rows() {
    let store = test_store().await;
    let repo = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![discovered("."), discovered("payments/flags")],
        )
        .await
        .unwrap();
    let root_id = repo.workspaces[0].id.clone();
    let flags_id = repo.workspaces[1].id.clone();
    let draft = store
        .create_draft_session(NewDraftSession {
            workspace_id: flags_id.clone(),
            principal_id: "42".to_owned(),
            branch: "draft-flags".to_owned(),
            base_ref: "main".to_owned(),
        })
        .await
        .unwrap();

    let rediscovered = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![discovered("payments/flags"), discovered("support")],
        )
        .await
        .unwrap();

    let paths: Vec<&str> = rediscovered
        .workspaces
        .iter()
        .map(|workspace| workspace.path.as_str())
        .collect();
    assert_eq!(paths, ["payments/flags", "support"]);
    let flags = rediscovered
        .workspaces
        .iter()
        .find(|workspace| workspace.path == "payments/flags")
        .unwrap();
    assert_eq!(flags.id, flags_id);
    assert_ne!(flags.id, root_id);

    let drafts = store
        .list_draft_sessions_for_workspace(&flags_id, "42")
        .await
        .unwrap();
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].id, draft.id);
    assert!(
        store
            .get_workspace_for_user(&root_id, "42")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn repo_upsert_hides_missing_workspace_but_keeps_drafts() {
    let store = test_store().await;
    let repo = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![discovered(".")],
        )
        .await
        .unwrap();
    let workspace = repo.workspaces[0].clone();
    let draft = store
        .create_draft_session(NewDraftSession {
            workspace_id: workspace.id.clone(),
            principal_id: "42".to_owned(),
            branch: "draft-root".to_owned(),
            base_ref: "main".to_owned(),
        })
        .await
        .unwrap();

    let rediscovered = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![],
        )
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
    assert!(
        store
            .get_workspace_for_user(&workspace.slug, "42")
            .await
            .unwrap()
            .is_some()
    );
    let drafts = store.list_draft_sessions_for_user("42").await.unwrap();
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].draft.id, draft.id);
    assert_eq!(drafts[0].workspace.id, workspace.id);
}

#[tokio::test]
async fn repo_draft_can_include_multiple_workspaces() {
    let store = test_store().await;
    let repo = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![discovered("."), discovered("payments/flags")],
        )
        .await
        .unwrap();
    let root = repo.workspaces[0].clone();
    let flags = repo.workspaces[1].clone();
    let draft = store
        .create_draft_session(NewDraftSession {
            workspace_id: root.id.clone(),
            principal_id: "42".to_owned(),
            branch: "draft-config".to_owned(),
            base_ref: "main".to_owned(),
        })
        .await
        .unwrap();

    let existing = store
        .find_open_draft_for_repo_branch(&flags.id, "42", "draft-config")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(existing.id, draft.id);

    store
        .ensure_draft_workspace(&draft.id, &flags.id)
        .await
        .unwrap();

    let root_drafts = store
        .list_draft_sessions_for_workspace(&root.id, "42")
        .await
        .unwrap();
    let flags_drafts = store
        .list_draft_sessions_for_workspace(&flags.id, "42")
        .await
        .unwrap();
    assert_eq!(root_drafts[0].id, draft.id);
    assert_eq!(flags_drafts[0].id, draft.id);
    let workspaces = store.list_workspaces_for_draft(&draft.id).await.unwrap();
    let paths: Vec<&str> = workspaces
        .iter()
        .map(|workspace| workspace.path.as_str())
        .collect();
    assert_eq!(paths, [".", "payments/flags"]);
}

#[tokio::test]
async fn lists_user_drafts_with_workspaces() {
    let store = test_store().await;
    let repo = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![discovered("."), discovered("payments/flags")],
        )
        .await
        .unwrap();
    let root = repo.workspaces[0].clone();
    let flags = repo.workspaces[1].clone();
    store
        .create_draft_session(NewDraftSession {
            workspace_id: root.id.clone(),
            principal_id: "42".to_owned(),
            branch: "draft-root".to_owned(),
            base_ref: "main".to_owned(),
        })
        .await
        .unwrap();
    store
        .create_draft_session(NewDraftSession {
            workspace_id: flags.id.clone(),
            principal_id: "42".to_owned(),
            branch: "draft-flags".to_owned(),
            base_ref: "main".to_owned(),
        })
        .await
        .unwrap();
    store
        .create_draft_session(NewDraftSession {
            workspace_id: root.id.clone(),
            principal_id: "99".to_owned(),
            branch: "other-user".to_owned(),
            base_ref: "main".to_owned(),
        })
        .await
        .unwrap();

    let drafts = store.list_draft_sessions_for_user("42").await.unwrap();
    assert_eq!(drafts.len(), 2);

    let mut branches: Vec<&str> = drafts
        .iter()
        .map(|entry| entry.draft.branch.as_str())
        .collect();
    branches.sort_unstable();
    assert_eq!(branches, ["draft-flags", "draft-root"]);

    let mut paths: Vec<&str> = drafts
        .iter()
        .map(|entry| entry.workspace.path.as_str())
        .collect();
    paths.sort_unstable();
    assert_eq!(paths, [".", "payments/flags"]);

    assert!(
        store
            .list_draft_sessions_for_user("99")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn draft_change_revert_deletes_row() {
    let store = test_store().await;
    let repo = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![discovered(".")],
        )
        .await
        .unwrap();
    let workspace = repo.workspaces[0].clone();
    let draft = store
        .create_draft_session(NewDraftSession {
            workspace_id: workspace.id.clone(),
            principal_id: "42".to_owned(),
            branch: "rototo-console/octocat/abc/20260613000000".to_owned(),
            base_ref: "main".to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(draft.status, DraftStatus::Open);

    let change = store
        .record_draft_change(DraftChangeInput {
            draft_id: draft.id.clone(),
            file_path: "variables/banner.toml".to_owned(),
            target_path: Some("/values/control".to_owned()),
            before: serde_json::json!(false),
            after: serde_json::json!(true),
        })
        .await
        .unwrap();
    assert!(change.is_some());
    let changes = store.list_draft_changes(&draft.id).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].file_path, "variables/banner.toml");
    assert_eq!(changes[0].target_path.as_deref(), Some("/values/control"));

    // Reverting back to the original value clears the tracked change.
    let reverted = store
        .record_draft_change(DraftChangeInput {
            draft_id: draft.id.clone(),
            file_path: "variables/banner.toml".to_owned(),
            target_path: Some("/values/control".to_owned()),
            before: serde_json::json!(true),
            after: serde_json::json!(false),
        })
        .await
        .unwrap();
    assert!(reverted.is_none());
    assert!(
        store
            .list_draft_changes(&draft.id)
            .await
            .unwrap()
            .is_empty()
    );

    let kinds: Vec<String> = store
        .list_draft_events(&draft.id)
        .await
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect();
    assert_eq!(
        kinds,
        ["draft.created", "change.created", "change.reverted"]
    );
}

#[tokio::test]
async fn whole_file_draft_change_revert_deletes_row() {
    let store = test_store().await;
    let repo = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![discovered(".")],
        )
        .await
        .unwrap();
    let workspace = repo.workspaces[0].clone();
    let draft = store
        .create_draft_session(NewDraftSession {
            workspace_id: workspace.id.clone(),
            principal_id: "42".to_owned(),
            branch: "draft-branch".to_owned(),
            base_ref: "main".to_owned(),
        })
        .await
        .unwrap();

    store
        .record_draft_change(DraftChangeInput {
            draft_id: draft.id.clone(),
            file_path: "lint/max-token-budget.lua".to_owned(),
            target_path: None,
            before: serde_json::json!("old"),
            after: serde_json::json!("new"),
        })
        .await
        .unwrap();
    let updated = store
        .record_draft_change(DraftChangeInput {
            draft_id: draft.id.clone(),
            file_path: "lint/max-token-budget.lua".to_owned(),
            target_path: None,
            before: serde_json::json!("new"),
            after: serde_json::json!("newer"),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.target_path, None);

    let changes = store.list_draft_changes(&draft.id).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].before_json, "\"old\"");
    assert_eq!(changes[0].after_json, "\"newer\"");

    let reverted = store
        .record_draft_change(DraftChangeInput {
            draft_id: draft.id.clone(),
            file_path: "lint/max-token-budget.lua".to_owned(),
            target_path: None,
            before: serde_json::json!("newer"),
            after: serde_json::json!("old"),
        })
        .await
        .unwrap();
    assert!(reverted.is_none());
    assert!(
        store
            .list_draft_changes(&draft.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn abandoned_drafts_leave_active_lists() {
    let store = test_store().await;
    let repo = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![discovered(".")],
        )
        .await
        .unwrap();
    let workspace = repo.workspaces[0].clone();
    let draft = store
        .create_draft_session(NewDraftSession {
            workspace_id: workspace.id.clone(),
            principal_id: "42".to_owned(),
            branch: "draft-branch".to_owned(),
            base_ref: "main".to_owned(),
        })
        .await
        .unwrap();

    let abandoned = store.mark_draft_abandoned(&draft.id).await.unwrap();
    assert_eq!(abandoned.status, DraftStatus::Abandoned);
    assert_eq!(abandoned.branch, "draft-branch");

    let fetched = store
        .get_draft_session_for_user(&draft.id, &workspace.id, "42")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, DraftStatus::Abandoned);
    assert!(
        store
            .list_draft_sessions_for_workspace(&workspace.id, "42")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .list_draft_sessions_for_user("42")
            .await
            .unwrap()
            .is_empty()
    );

    let kinds: Vec<String> = store
        .list_draft_events(&draft.id)
        .await
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect();
    assert_eq!(kinds, ["draft.created", "draft.abandoned"]);
}

#[tokio::test]
async fn closed_unmerged_pull_request_reopens_draft() {
    let store = test_store().await;
    let repo = store
        .upsert_repo_with_workspaces(
            "42".to_owned(),
            "octo".to_owned(),
            "configs".to_owned(),
            "main".to_owned(),
            vec![discovered(".")],
        )
        .await
        .unwrap();
    let draft = store
        .create_draft_session(NewDraftSession {
            workspace_id: repo.workspaces[0].id.clone(),
            principal_id: "42".to_owned(),
            branch: "draft-branch".to_owned(),
            base_ref: "main".to_owned(),
        })
        .await
        .unwrap();
    store
        .mark_draft_published(
            &draft.id,
            7,
            "open",
            "https://github.com/octo/configs/pull/7",
        )
        .await
        .unwrap();
    let published = store
        .get_draft_session_for_user(&draft.id, &draft.workspace_id, "42")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(published.status, DraftStatus::Published);

    let reopened = store
        .update_draft_pull_request_state(PullRequestStateInput {
            draft_id: draft.id.clone(),
            pr_number: 7,
            pr_state: "closed".to_owned(),
            pr_url: "https://github.com/octo/configs/pull/7".to_owned(),
            pr_merged_at: None,
        })
        .await
        .unwrap();
    assert_eq!(reopened.status, DraftStatus::Open);
    assert_eq!(reopened.pr_number, None);
    assert_eq!(reopened.pr_url, None);
}
