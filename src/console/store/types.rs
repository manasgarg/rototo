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

/// Lifecycle state for a draft session.
///
/// The status lives in the `draft_sessions` table. Open drafts accept edits,
/// published drafts represent a direct push or PR handoff, and abandoned drafts
/// are retained for history but normally omitted from active lists.
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

/// Persisted tracking state for a repository branch.
///
/// Active branches are currently selected for work, recent branches remain
/// visible as useful history, and archived branches are hidden from normal
/// lists without deleting the branch from the remote source of truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackedBranchStatus {
    Active,
    Recent,
    Archived,
}

/// Branch selected by a user within a repository.
///
/// This is the branch-level replacement for draft session identity. It stores
/// only local lifecycle metadata needed by the console: which branch a user is
/// working with, which workspaces inside the repo were selected for that
/// branch, and any observed pull request metadata. The branch contents remain
/// the source of truth.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedBranchRecord {
    pub id: String,
    pub repo_id: String,
    pub principal_id: String,
    pub branch: String,
    pub base_ref: String,
    pub base_commit: Option<String>,
    pub pr_url: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_state: Option<String>,
    pub pr_merged_at: Option<String>,
    pub pr_synced_at: Option<String>,
    pub last_selected_workspace_path: Option<String>,
    pub last_seen_commit: Option<String>,
    pub status: TrackedBranchStatus,
    pub created_at: String,
    pub last_opened_at: String,
    pub last_edited_at: Option<String>,
    pub archived_at: Option<String>,
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

/// Inputs for creating a hosted OAuth session.
///
/// The store hashes a new session token, encrypts the GitHub token, and writes
/// the session row. The plaintext token exists only long enough to create that
/// row and return the browser cookie value.
pub struct NewSession {
    pub identity: ActorIdentity,
    pub github_token: String,
}

/// Inputs for inserting a new draft session row.
///
/// Routes build this after selecting a write backend and validating branch
/// access. The store assigns ids and timestamps; later draft operations mutate
/// the resulting row by id.
pub struct NewDraftSession {
    pub workspace_id: String,
    pub principal_id: String,
    pub branch: String,
    pub base_ref: String,
}

/// Inputs for selecting or creating a tracked branch.
///
/// The store derives the repository and last selected workspace path from the
/// workspace id. Re-selecting an existing branch updates its lifecycle metadata
/// and ensures the workspace is attached to that branch.
#[allow(dead_code)]
pub struct TrackBranchInput {
    pub workspace_id: String,
    pub principal_id: String,
    pub branch: String,
    pub base_ref: String,
    pub base_commit: Option<String>,
    pub last_seen_commit: Option<String>,
}

/// Upsert input for the net change tracked inside a draft.
///
/// Save routes pass semantic before/after JSON for a file or value target.
/// The store creates, updates, or removes the durable change row depending on
/// whether the after value still differs from the before value.
pub struct DraftChangeInput {
    pub draft_id: String,
    pub file_path: String,
    pub target_path: Option<String>,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
}

/// Append-only draft timeline input.
///
/// Mutation routes create these after user-visible actions. The store assigns
/// ids and timestamps and never updates the event afterward.
pub struct DraftEventInput {
    pub draft_id: String,
    pub kind: String,
    pub summary: String,
    pub detail: Option<serde_json::Value>,
}

/// Pull request metadata observed for a published draft.
///
/// Sync and publish routes pass this after reading GitHub. The store updates
/// the draft row so later screens can show PR state without polling GitHub on
/// every render.
pub struct PullRequestStateInput {
    pub draft_id: String,
    pub pr_number: i64,
    pub pr_state: String,
    pub pr_url: String,
    pub pr_merged_at: Option<String>,
}

/// Pull request metadata observed for a tracked branch.
#[allow(dead_code)]
pub struct TrackedBranchPullRequestInput {
    pub branch_id: String,
    pub pr_number: i64,
    pub pr_state: String,
    pub pr_url: String,
    pub pr_merged_at: Option<String>,
}

/// Workspace discovered inside a registered repository.
///
/// Discovery creates these from GitHub tree results or fixed local sources.
/// The store folds them into repo-scoped workspace rows and marks stale rows
/// inactive or deletes them when safe.
pub struct DiscoveredWorkspaceInput {
    pub path: String,
    pub git_ref: String,
    pub source: String,
}
