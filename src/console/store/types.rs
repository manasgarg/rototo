use serde::Serialize;

use crate::console::identity::ActorIdentity;

/// Authenticated console user loaded from the `sessions` table.
///
/// This exists so request handlers can authorize a principal and, when writes
/// are allowed, use the user's GitHub token without sending that token back to
/// the browser. Sessions are created after OAuth sign-in, deleted on logout,
/// and lazily removed when a lookup finds that the stored expiry has passed.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    pub session_hash: String,
    pub principal_id: String,
    pub identity: ActorIdentity,
    #[serde(skip)]
    pub github_token: Option<String>,
}

/// Repository registered for one console principal.
///
/// This is the parent record for workspace discovery and draft ownership. It is
/// inserted the first time discovery sees a repo for a principal, refreshed by
/// later discovery runs to update the default ref and discovery timestamps, and
/// deleted explicitly by the user; deleting it cascades to workspaces, drafts,
/// draft changes, and draft events.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRecord {
    pub id: String,
    pub principal_id: String,
    pub owner: String,
    pub name: String,
    pub default_ref: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_discovered_at: Option<String>,
}

/// Discovered rototo workspace inside a repository.
///
/// This exists so the console can address a workspace by id or slug, reconstruct
/// the workspace source, and scope lint, preview, and draft operations. Discovery
/// inserts new rows, refreshes rows it still finds, deletes rows that no longer
/// exist when no draft references them, and otherwise marks missing rows
/// inactive so existing draft links can still resolve.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRecord {
    pub id: String,
    /// Derived, human-readable URL handle (repo name + workspace path).
    /// Stable across re-discovery, unlike the row id.
    pub slug: String,
    pub repo_id: String,
    pub owner: String,
    pub name: String,
    pub path: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub source: String,
    pub discovered_at: String,
}

/// Repository response with its currently active workspaces.
///
/// This exists as an API projection for repo navigation and discovery responses.
/// It is not stored independently; each value is rebuilt from one `repos` row
/// and that repo's active `workspaces` rows, so its lifecycle follows those
/// underlying records.
#[derive(Clone, Debug, Serialize)]
pub struct RepoWithWorkspaces {
    #[serde(flatten)]
    pub repo: RepoRecord,
    pub workspaces: Vec<WorkspaceRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftStatus {
    Open,
    Published,
    Abandoned,
}

/// Editable draft branch for a workspace and principal.
///
/// This exists to group proposed workspace edits, their activity history, and
/// any GitHub pull request state under one branch. Drafts start open, become
/// published after a direct push or pull request creation, can reopen when a
/// pull request is closed without merging, and become abandoned when the user
/// discards them. Deleting the parent workspace or repository cascades to the
/// draft and its child records.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftSessionRecord {
    pub id: String,
    pub workspace_id: String,
    pub principal_id: String,
    pub branch: String,
    pub base_ref: String,
    pub status: DraftStatus,
    pub pr_url: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_state: Option<String>,
    pub pr_merged_at: Option<String>,
    pub pr_synced_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub published_at: Option<String>,
}

/// Draft list item paired with the workspace it edits.
///
/// This exists for user-level draft views that need workspace metadata beside
/// the draft state. It is not stored independently; each value is rebuilt from a
/// `draft_sessions` row joined to its `workspaces` row, and normal lists omit
/// abandoned drafts.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftWithWorkspaceRecord {
    pub draft: DraftSessionRecord,
    pub workspace: WorkspaceRecord,
}

/// Net change inside a draft.
///
/// This exists so the console can render pending edits, rebuild workspace files,
/// and describe changes in a pull request without committing each keystroke.
/// There is at most one row per draft, file path, and optional target path; it
/// is inserted on first divergence from the original value, updated as the user
/// edits, and deleted when the value is reverted. Draft deletion cascades to its
/// changes.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftChangeRecord {
    pub id: String,
    pub draft_id: String,
    pub file_path: String,
    pub target_path: Option<String>,
    pub before_json: String,
    pub after_json: String,
    pub updated_at: String,
}

/// Append-only activity entry for a draft.
///
/// This exists to preserve the console timeline for draft creation, edits,
/// publication, abandonment, and pull request sync events. Events are inserted
/// as side effects of those operations and are not updated; deleting the parent
/// draft cascades to its event history.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftEventRecord {
    pub id: String,
    pub draft_id: String,
    pub kind: String,
    pub summary: String,
    pub detail_json: Option<String>,
    pub created_at: String,
}

pub struct NewSession {
    pub identity: ActorIdentity,
    pub github_token: String,
}

pub struct NewDraftSession {
    pub workspace_id: String,
    pub principal_id: String,
    pub branch: String,
    pub base_ref: String,
}

pub struct DraftChangeInput {
    pub draft_id: String,
    pub file_path: String,
    pub target_path: Option<String>,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
}

pub struct DraftEventInput {
    pub draft_id: String,
    pub kind: String,
    pub summary: String,
    pub detail: Option<serde_json::Value>,
}

pub struct PullRequestStateInput {
    pub draft_id: String,
    pub pr_number: i64,
    pub pr_state: String,
    pub pr_url: String,
    pub pr_merged_at: Option<String>,
}

pub struct DiscoveredWorkspaceInput {
    pub path: String,
    pub git_ref: String,
    pub source: String,
}
