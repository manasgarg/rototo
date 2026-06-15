use rusqlite::{Connection, OptionalExtension, params};

use crate::console::time::now_iso;
use crate::error::{Result, RototoError};

use super::Store;
use super::rows::{change_from_row, draft_from_row, event_from_row, workspace_from_row_at};
use super::types::{
    DraftChangeInput, DraftChangeRecord, DraftEventInput, DraftEventRecord, DraftSessionRecord,
    DraftStatus, DraftWithWorkspaceRecord, NewDraftSession, PullRequestStateInput, WorkspaceRecord,
};
use super::util::{db_err, new_id};

impl Store {
    pub async fn create_draft_session(&self, input: NewDraftSession) -> Result<DraftSessionRecord> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let id = new_id();
            conn.execute(
                "INSERT INTO draft_sessions (
               id, workspace_id, principal_id, branch, base_ref, status,
               created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7)",
                params![
                    id,
                    input.workspace_id,
                    input.principal_id,
                    input.branch,
                    input.base_ref,
                    now,
                    now,
                ],
            )
            .map_err(db_err)?;
            insert_draft_workspace_sync(conn, &id, &input.workspace_id, &now)?;
            let draft = draft_session_by_id(conn, &id)?
                .ok_or_else(|| RototoError::new("draft session creation failed"))?;
            record_draft_event_sync(
                conn,
                &DraftEventInput {
                    draft_id: draft.id.clone(),
                    kind: "draft.created".to_owned(),
                    summary: format!("Created draft branch {}", draft.branch),
                    detail: Some(serde_json::json!({
                        "branch": draft.branch,
                        "baseRef": draft.base_ref,
                    })),
                },
            )?;
            Ok(draft)
        })
        .await
    }

    pub async fn list_draft_sessions_for_workspace(
        &self,
        workspace_id: &str,
        principal_id: &str,
    ) -> Result<Vec<DraftSessionRecord>> {
        let workspace_id = workspace_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT d.id, d.workspace_id, d.principal_id, d.branch, d.base_ref, d.status,
                        d.pr_url, d.pr_number, d.pr_state, d.pr_merged_at, d.pr_synced_at,
                        d.created_at, d.updated_at, d.published_at
                 FROM draft_sessions d
                 INNER JOIN draft_workspaces dw ON dw.draft_id = d.id
                 WHERE dw.workspace_id = ?1 AND d.principal_id = ?2
                   AND d.status != 'abandoned'
                 ORDER BY d.updated_at DESC",
                )
                .map_err(db_err)?;
            let drafts = statement
                .query_map(params![workspace_id, principal_id], draft_from_row)
                .map_err(db_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_err)?;
            Ok(drafts)
        })
        .await
    }

    pub async fn list_draft_sessions_for_user(
        &self,
        principal_id: &str,
    ) -> Result<Vec<DraftWithWorkspaceRecord>> {
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT d.id, d.workspace_id, d.principal_id, d.branch, d.base_ref, d.status,
                        d.pr_url, d.pr_number, d.pr_state, d.pr_merged_at, d.pr_synced_at,
                        d.created_at, d.updated_at, d.published_at,
                        w.id, w.repo_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
                 FROM draft_sessions d
                 INNER JOIN workspaces w ON w.id = d.workspace_id
                 INNER JOIN repos r ON r.id = w.repo_id
                 WHERE d.principal_id = ?1 AND r.principal_id = ?1
                   AND d.status != 'abandoned'
                 ORDER BY d.updated_at DESC",
                )
                .map_err(db_err)?;
            let drafts = statement
                .query_map(params![principal_id], |row| {
                    Ok(DraftWithWorkspaceRecord {
                        draft: draft_from_row(row)?,
                        workspace: workspace_from_row_at(row, 14)?,
                    })
                })
                .map_err(db_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_err)?;
            Ok(drafts)
        })
        .await
    }

    pub async fn get_draft_session_for_user(
        &self,
        draft_id: &str,
        workspace_id: &str,
        principal_id: &str,
    ) -> Result<Option<DraftSessionRecord>> {
        let draft_id = draft_id.to_owned();
        let workspace_id = workspace_id.to_owned();
        let principal_id = principal_id.to_owned();
        self.with_conn(move |conn, _| {
            conn.query_row(
                "SELECT d.id, d.workspace_id, d.principal_id, d.branch, d.base_ref, d.status,
                    d.pr_url, d.pr_number, d.pr_state, d.pr_merged_at, d.pr_synced_at,
                    d.created_at, d.updated_at, d.published_at
             FROM draft_sessions d
             INNER JOIN draft_workspaces dw ON dw.draft_id = d.id
             WHERE d.id = ?1 AND dw.workspace_id = ?2 AND d.principal_id = ?3",
                params![draft_id, workspace_id, principal_id],
                draft_from_row,
            )
            .optional()
            .map_err(db_err)
        })
        .await
    }

    pub async fn find_open_draft_for_repo_branch(
        &self,
        workspace_id: &str,
        principal_id: &str,
        branch: &str,
    ) -> Result<Option<DraftSessionRecord>> {
        let workspace_id = workspace_id.to_owned();
        let principal_id = principal_id.to_owned();
        let branch = branch.to_owned();
        self.with_conn(move |conn, _| {
            conn.query_row(
                "SELECT d.id, d.workspace_id, d.principal_id, d.branch, d.base_ref, d.status,
                    d.pr_url, d.pr_number, d.pr_state, d.pr_merged_at, d.pr_synced_at,
                    d.created_at, d.updated_at, d.published_at
                 FROM draft_sessions d
                 INNER JOIN workspaces draft_workspace ON draft_workspace.id = d.workspace_id
                 INNER JOIN workspaces requested_workspace ON requested_workspace.id = ?1
                 WHERE draft_workspace.repo_id = requested_workspace.repo_id
                   AND d.principal_id = ?2
                   AND d.branch = ?3
                   AND d.status = 'open'
                 ORDER BY d.updated_at DESC
                 LIMIT 1",
                params![workspace_id, principal_id, branch],
                draft_from_row,
            )
            .optional()
            .map_err(db_err)
        })
        .await
    }

    pub async fn ensure_draft_workspace(
        &self,
        draft_id: &str,
        workspace_id: &str,
    ) -> Result<DraftSessionRecord> {
        let draft_id = draft_id.to_owned();
        let workspace_id = workspace_id.to_owned();
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let inserted = insert_draft_workspace_sync(conn, &draft_id, &workspace_id, &now)?;
            if inserted {
                conn.execute(
                    "UPDATE draft_sessions SET updated_at = ?1 WHERE id = ?2",
                    params![now, draft_id],
                )
                .map_err(db_err)?;
                let workspace_path: String = conn
                    .query_row(
                        "SELECT path FROM workspaces WHERE id = ?1",
                        params![workspace_id],
                        |row| row.get(0),
                    )
                    .map_err(db_err)?;
                record_draft_event_sync(
                    conn,
                    &DraftEventInput {
                        draft_id: draft_id.clone(),
                        kind: "draft.workspace_added".to_owned(),
                        summary: format!("Added workspace {workspace_path} to draft"),
                        detail: Some(serde_json::json!({
                            "workspaceId": workspace_id,
                            "workspacePath": workspace_path,
                        })),
                    },
                )?;
            }
            draft_session_by_id(conn, &draft_id)?
                .ok_or_else(|| RototoError::new("draft workspace membership update failed"))
        })
        .await
    }

    pub async fn list_workspaces_for_draft(&self, draft_id: &str) -> Result<Vec<WorkspaceRecord>> {
        let draft_id = draft_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT w.id, w.repo_id, w.owner, w.name, w.path, w.ref_, w.source, w.discovered_at
                     FROM draft_workspaces dw
                     INNER JOIN workspaces w ON w.id = dw.workspace_id
                     WHERE dw.draft_id = ?1
                     ORDER BY w.path ASC",
                )
                .map_err(db_err)?;
            statement
                .query_map(params![draft_id], |row| workspace_from_row_at(row, 0))
                .map_err(db_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_err)
        })
        .await
    }

    /// Records one semantic change. Reverting a change back to its original
    /// value deletes the row and returns `None`.
    pub async fn record_draft_change(
        &self,
        input: DraftChangeInput,
    ) -> Result<Option<DraftChangeRecord>> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let target_path = input
                .target_path
                .clone()
                .filter(|target| !target.is_empty());
            let target_label = draft_change_target_label(&input.file_path, target_path.as_deref());
            let existing = conn
                .query_row(
                    "SELECT id, draft_id, file_path, target_path, before_json, after_json,
                        updated_at
                 FROM draft_changes
                 WHERE draft_id = ?1
                   AND file_path = ?2
                   AND COALESCE(target_path, '') = COALESCE(?3, '')",
                    params![input.draft_id, input.file_path, target_path.as_deref()],
                    change_from_row,
                )
                .optional()
                .map_err(db_err)?;
            let before = existing
                .as_ref()
                .and_then(|change| serde_json::from_str(&change.before_json).ok())
                .unwrap_or_else(|| input.before.clone());

            if before == input.after {
                if existing.is_some() {
                    conn.execute(
                        "DELETE FROM draft_changes
                     WHERE draft_id = ?1
                       AND file_path = ?2
                       AND COALESCE(target_path, '') = COALESCE(?3, '')",
                        params![input.draft_id, input.file_path, target_path.as_deref()],
                    )
                    .map_err(db_err)?;
                    conn.execute(
                        "UPDATE draft_sessions SET updated_at = ?1 WHERE id = ?2",
                        params![now, input.draft_id],
                    )
                    .map_err(db_err)?;
                    record_draft_event_sync(
                        conn,
                        &DraftEventInput {
                            draft_id: input.draft_id.clone(),
                            kind: "change.reverted".to_owned(),
                            summary: format!("Reverted {target_label}"),
                            detail: Some(serde_json::json!({
                                "filePath": input.file_path,
                                "targetPath": target_path,
                            })),
                        },
                    )?;
                }
                return Ok(None);
            }

            if existing.is_some() {
                conn.execute(
                    "UPDATE draft_changes
                 SET after_json = ?1, updated_at = ?2
                 WHERE draft_id = ?3
                   AND file_path = ?4
                   AND COALESCE(target_path, '') = COALESCE(?5, '')",
                    params![
                        input.after.to_string(),
                        now,
                        input.draft_id,
                        input.file_path,
                        target_path.as_deref(),
                    ],
                )
                .map_err(db_err)?;
            } else {
                conn.execute(
                    "INSERT INTO draft_changes (
                   id, draft_id, file_path, target_path, before_json, after_json,
                   updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        new_id(),
                        input.draft_id,
                        input.file_path,
                        target_path.as_deref(),
                        input.before.to_string(),
                        input.after.to_string(),
                        now,
                    ],
                )
                .map_err(db_err)?;
            }
            let change = conn
                .query_row(
                    "SELECT id, draft_id, file_path, target_path, before_json, after_json,
                        updated_at
                 FROM draft_changes
                 WHERE draft_id = ?1
                   AND file_path = ?2
                   AND COALESCE(target_path, '') = COALESCE(?3, '')",
                    params![input.draft_id, input.file_path, target_path.as_deref()],
                    change_from_row,
                )
                .map_err(db_err)?;
            conn.execute(
                "UPDATE draft_sessions SET updated_at = ?1 WHERE id = ?2",
                params![now, input.draft_id],
            )
            .map_err(db_err)?;
            record_draft_event_sync(
                conn,
                &DraftEventInput {
                    draft_id: input.draft_id.clone(),
                    kind: if existing.is_some() {
                        "change.updated"
                    } else {
                        "change.created"
                    }
                    .to_owned(),
                    summary: format!(
                        "{} {target_label}",
                        if existing.is_some() {
                            "Updated"
                        } else {
                            "Changed"
                        },
                    ),
                    detail: Some(serde_json::json!({
                        "filePath": input.file_path,
                        "targetPath": target_path,
                    })),
                },
            )?;
            Ok(Some(change))
        })
        .await
    }

    pub async fn list_draft_changes(&self, draft_id: &str) -> Result<Vec<DraftChangeRecord>> {
        let draft_id = draft_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT id, draft_id, file_path, target_path, before_json, after_json,
                        updated_at
                 FROM draft_changes
                 WHERE draft_id = ?1
                 ORDER BY updated_at ASC, file_path ASC, target_path ASC",
                )
                .map_err(db_err)?;
            let changes: Vec<DraftChangeRecord> = statement
                .query_map(params![draft_id], change_from_row)
                .map_err(db_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_err)?;
            Ok(changes.into_iter().filter(is_net_draft_change).collect())
        })
        .await
    }

    pub async fn mark_draft_published(
        &self,
        draft_id: &str,
        pr_number: i64,
        pr_state: &str,
        pr_url: &str,
    ) -> Result<()> {
        let draft_id = draft_id.to_owned();
        let pr_state = pr_state.to_owned();
        let pr_url = pr_url.to_owned();
        self.with_conn(move |conn, _| {
            let now = now_iso();
            conn.execute(
                "UPDATE draft_sessions
             SET status = 'published', pr_url = ?1, pr_number = ?2, pr_state = ?3,
                 pr_synced_at = ?4, updated_at = ?5, published_at = ?6
             WHERE id = ?7",
                params![pr_url, pr_number, pr_state, now, now, now, draft_id],
            )
            .map_err(db_err)?;
            record_draft_event_sync(
                conn,
                &DraftEventInput {
                    draft_id: draft_id.clone(),
                    kind: "pr.created".to_owned(),
                    summary: format!("Created pull request #{pr_number}"),
                    detail: Some(serde_json::json!({
                        "prUrl": pr_url,
                        "prNumber": pr_number,
                        "prState": pr_state,
                    })),
                },
            )?;
            Ok(())
        })
        .await
    }

    pub async fn mark_draft_direct_published(
        &self,
        draft_id: &str,
        summary: String,
        detail: serde_json::Value,
    ) -> Result<DraftSessionRecord> {
        let draft_id = draft_id.to_owned();
        self.with_conn(move |conn, _| {
            let now = now_iso();
            conn.execute(
                "UPDATE draft_sessions
             SET status = 'published', pr_url = NULL, pr_number = NULL, pr_state = NULL,
                 pr_merged_at = NULL, pr_synced_at = NULL, updated_at = ?1, published_at = ?2
             WHERE id = ?3",
                params![now, now, draft_id],
            )
            .map_err(db_err)?;
            record_draft_event_sync(
                conn,
                &DraftEventInput {
                    draft_id: draft_id.clone(),
                    kind: "direct_push.published".to_owned(),
                    summary,
                    detail: Some(detail),
                },
            )?;
            draft_session_by_id(conn, &draft_id)?
                .ok_or_else(|| RototoError::new("draft direct publish update failed"))
        })
        .await
    }

    pub async fn mark_draft_abandoned(&self, draft_id: &str) -> Result<DraftSessionRecord> {
        let draft_id = draft_id.to_owned();
        self.with_conn(move |conn, _| {
            let now = now_iso();
            conn.execute(
                "UPDATE draft_sessions
             SET status = 'abandoned', updated_at = ?1
             WHERE id = ?2 AND status = 'open'",
                params![now, draft_id],
            )
            .map_err(db_err)?;
            let draft = draft_session_by_id(conn, &draft_id)?
                .ok_or_else(|| RototoError::new("draft session update failed"))?;
            if draft.status != DraftStatus::Abandoned {
                return Err(RototoError::new("draft is not open"));
            }
            record_draft_event_sync(
                conn,
                &DraftEventInput {
                    draft_id: draft.id.clone(),
                    kind: "draft.abandoned".to_owned(),
                    summary: format!("Let go of draft branch {}", draft.branch),
                    detail: Some(serde_json::json!({
                        "branch": draft.branch,
                    })),
                },
            )?;
            draft_session_by_id(conn, &draft_id)?
                .ok_or_else(|| RototoError::new("draft session update failed"))
        })
        .await
    }

    pub async fn update_draft_branch(
        &self,
        draft_id: &str,
        branch: &str,
        previous_branch: &str,
    ) -> Result<DraftSessionRecord> {
        let draft_id = draft_id.to_owned();
        let branch = branch.to_owned();
        let previous_branch = previous_branch.to_owned();
        self.with_conn(move |conn, _| {
            conn.execute(
                "UPDATE draft_sessions SET branch = ?1, updated_at = ?2 WHERE id = ?3",
                params![branch, now_iso(), draft_id],
            )
            .map_err(db_err)?;
            record_draft_event_sync(
                conn,
                &DraftEventInput {
                    draft_id: draft_id.clone(),
                    kind: "draft.branch_renamed".to_owned(),
                    summary: format!("Renamed draft branch to {branch}"),
                    detail: Some(serde_json::json!({
                        "previousBranch": previous_branch,
                        "branch": branch,
                    })),
                },
            )?;
            draft_session_by_id(conn, &draft_id)?
                .ok_or_else(|| RototoError::new("draft session update failed"))
        })
        .await
    }

    pub async fn update_draft_pull_request_state(
        &self,
        input: PullRequestStateInput,
    ) -> Result<DraftSessionRecord> {
        self.with_conn(move |conn, _| {
            let now = now_iso();
            let existing = draft_session_by_id(conn, &input.draft_id)?;
            let changed = existing.as_ref().is_none_or(|draft| {
                draft.pr_number != Some(input.pr_number)
                    || draft.pr_state.as_deref() != Some(input.pr_state.as_str())
                    || draft.pr_merged_at != input.pr_merged_at
            });
            let updated_at = if changed {
                now.clone()
            } else {
                existing
                    .as_ref()
                    .map(|draft| draft.updated_at.clone())
                    .unwrap_or_else(|| now.clone())
            };

            // A pull request closed without merging ends the publish attempt,
            // not the draft: reopen it so the branch can be edited and
            // published again. The closed pull request stays on GitHub and in
            // the draft's activity.
            let reopened = input.pr_state == "closed"
                && input.pr_merged_at.is_none()
                && existing
                    .as_ref()
                    .is_some_and(|draft| draft.status == DraftStatus::Published);
            if reopened {
                conn.execute(
                    "UPDATE draft_sessions
                 SET status = 'open', pr_number = NULL, pr_state = NULL, pr_url = NULL,
                     pr_merged_at = NULL, pr_synced_at = ?1, published_at = NULL,
                     updated_at = ?2
                 WHERE id = ?3",
                    params![now, now, input.draft_id],
                )
                .map_err(db_err)?;
                record_draft_event_sync(
                    conn,
                    &DraftEventInput {
                        draft_id: input.draft_id.clone(),
                        kind: "pr.closed".to_owned(),
                        summary: format!(
                            "Pull request #{} was closed without merging — draft reopened",
                            input.pr_number
                        ),
                        detail: Some(serde_json::json!({
                            "prNumber": input.pr_number,
                            "prUrl": input.pr_url,
                        })),
                    },
                )?;
            } else {
                conn.execute(
                    "UPDATE draft_sessions
                 SET pr_number = ?1, pr_state = ?2, pr_url = ?3, pr_merged_at = ?4,
                     pr_synced_at = ?5, updated_at = ?6
                 WHERE id = ?7",
                    params![
                        input.pr_number,
                        input.pr_state,
                        input.pr_url,
                        input.pr_merged_at,
                        now,
                        updated_at,
                        input.draft_id,
                    ],
                )
                .map_err(db_err)?;
                if changed {
                    record_draft_event_sync(
                        conn,
                        &DraftEventInput {
                            draft_id: input.draft_id.clone(),
                            kind: "pr.synced".to_owned(),
                            summary: format!(
                                "Synced pull request #{}: {}",
                                input.pr_number, input.pr_state
                            ),
                            detail: Some(serde_json::json!({
                                "prNumber": input.pr_number,
                                "prState": input.pr_state,
                                "prUrl": input.pr_url,
                                "prMergedAt": input.pr_merged_at,
                            })),
                        },
                    )?;
                }
            }
            draft_session_by_id(conn, &input.draft_id)?
                .ok_or_else(|| RototoError::new("draft pull request state update failed"))
        })
        .await
    }

    pub async fn record_draft_event(&self, input: DraftEventInput) -> Result<DraftEventRecord> {
        self.with_conn(move |conn, _| record_draft_event_sync(conn, &input))
            .await
    }

    pub async fn list_draft_events(&self, draft_id: &str) -> Result<Vec<DraftEventRecord>> {
        let draft_id = draft_id.to_owned();
        self.with_conn(move |conn, _| {
            let mut statement = conn
                .prepare(
                    "SELECT id, draft_id, kind, summary, detail_json, created_at
                 FROM draft_events
                 WHERE draft_id = ?1
                 ORDER BY created_at ASC, rowid ASC",
                )
                .map_err(db_err)?;
            let events = statement
                .query_map(params![draft_id], event_from_row)
                .map_err(db_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_err)?;
            Ok(events)
        })
        .await
    }
}

fn draft_session_by_id(conn: &Connection, draft_id: &str) -> Result<Option<DraftSessionRecord>> {
    conn.query_row(
        "SELECT id, workspace_id, principal_id, branch, base_ref, status,
                pr_url, pr_number, pr_state, pr_merged_at, pr_synced_at,
                created_at, updated_at, published_at
         FROM draft_sessions WHERE id = ?1",
        params![draft_id],
        draft_from_row,
    )
    .optional()
    .map_err(db_err)
}

fn insert_draft_workspace_sync(
    conn: &Connection,
    draft_id: &str,
    workspace_id: &str,
    added_at: &str,
) -> Result<bool> {
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO draft_workspaces (draft_id, workspace_id, added_at)
             VALUES (?1, ?2, ?3)",
            params![draft_id, workspace_id, added_at],
        )
        .map_err(db_err)?;
    Ok(inserted != 0)
}

fn record_draft_event_sync(conn: &Connection, input: &DraftEventInput) -> Result<DraftEventRecord> {
    let id = new_id();
    let now = now_iso();
    let detail_json = input.detail.as_ref().map(|detail| detail.to_string());
    conn.execute(
        "INSERT INTO draft_events (id, draft_id, kind, summary, detail_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            input.draft_id,
            input.kind,
            input.summary,
            detail_json,
            now
        ],
    )
    .map_err(db_err)?;
    Ok(DraftEventRecord {
        id,
        draft_id: input.draft_id.clone(),
        kind: input.kind.clone(),
        summary: input.summary.clone(),
        detail_json,
        created_at: now,
    })
}

fn is_net_draft_change(change: &DraftChangeRecord) -> bool {
    let before: Option<serde_json::Value> = serde_json::from_str(&change.before_json).ok();
    let after: Option<serde_json::Value> = serde_json::from_str(&change.after_json).ok();
    match (before, after) {
        (Some(before), Some(after)) => before != after,
        _ => true,
    }
}

fn draft_change_target_label(file_path: &str, target_path: Option<&str>) -> String {
    match target_path {
        Some(target_path) if !target_path.is_empty() => format!("{file_path}:{target_path}"),
        _ => file_path.to_owned(),
    }
}
