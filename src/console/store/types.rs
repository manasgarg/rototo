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

/// Source tree registered for one console principal.
///
/// This is the durable source tree row: discovery refreshes its derived
/// workspace rows, and deleting it cascades to branch selections.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceTreeRecord {
    pub id: String,
    pub principal_id: String,
    pub kind: SourceTreeKind,
    pub source: String,
    pub display_name: String,
    pub default_revision: String,
    pub capabilities: SourceTreeCapabilities,
    pub created_at: String,
    pub updated_at: String,
    pub last_discovered_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceTreeKind {
    GitHub,
    GitRemote,
    LocalFolder,
    Archive,
}

impl SourceTreeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::GitRemote => "git_remote",
            Self::LocalFolder => "local_folder",
            Self::Archive => "archive",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "github" => Self::GitHub,
            "git_remote" => Self::GitRemote,
            "archive" => Self::Archive,
            _ => Self::LocalFolder,
        }
    }

    pub fn capabilities(self) -> SourceTreeCapabilities {
        SourceTreeCapabilities {
            can_refresh: true,
            can_discover_workspaces: true,
            can_load_workspaces: true,
            can_branch: matches!(self, Self::GitHub | Self::LocalFolder),
            can_edit: matches!(self, Self::GitHub | Self::LocalFolder),
            can_open_pull_request: matches!(self, Self::GitHub),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceTreeCapabilities {
    pub can_refresh: bool,
    pub can_discover_workspaces: bool,
    pub can_load_workspaces: bool,
    pub can_branch: bool,
    pub can_edit: bool,
    pub can_open_pull_request: bool,
}

/// Discovered rototo workspace inside a source tree.
///
/// This is a derived row rebuilt by discovery. The durable branch state stores
/// workspace paths, not workspace row ids, so these rows can be refreshed from
/// the source tree without changing branch identity.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRecord {
    pub id: String,
    /// Derived, human-readable URL handle (source tree label + workspace path).
    /// Stable across re-discovery, unlike the row id.
    pub slug: String,
    pub source_tree_id: String,
    pub source_tree_label: String,
    /// Browser-facing label for the workspace. Root workspaces keep `path = "."`
    /// as their source-tree identity but can still render a useful local name.
    pub display_path: String,
    pub path: String,
    pub revision: String,
    pub source: String,
    pub discovered_at: String,
}

/// Source tree response with its currently active discovered workspaces.
///
/// This exists as an API projection for source tree navigation and discovery
/// responses. It is not stored independently; each value is rebuilt from one
/// source tree row and that tree's active derived workspace rows.
#[derive(Clone, Debug, Serialize)]
pub struct SourceTreeWithWorkspaces {
    #[serde(flatten)]
    pub source_tree: SourceTreeRecord,
    pub workspaces: Vec<WorkspaceRecord>,
}

/// Persisted tracking state for a source tree branch.
///
/// Active branches are currently selected for work, recent branches remain
/// visible as useful history, and archived branches are hidden from normal
/// lists without deleting the branch from the remote source of truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActiveBranchStatus {
    Active,
    Recent,
    Archived,
}

/// Branch selected by a user within a source tree.
///
/// This stores only local lifecycle metadata needed by the console: which
/// branch a user is working with, which workspaces inside the source tree were
/// selected for that branch, and any observed pull request metadata. The branch
/// contents remain the source of truth.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveBranchRecord {
    pub id: String,
    pub source_tree_id: String,
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
    pub status: ActiveBranchStatus,
    pub created_at: String,
    pub last_opened_at: String,
    pub last_edited_at: Option<String>,
    pub archived_at: Option<String>,
}

/// Branch list item paired with one selected workspace.
///
/// The branch identity is source-tree-scoped, but the console still needs a
/// workspace beside it for navigation. Each value is rebuilt from an active
/// branch joined through path-based branch workspace membership.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveBranchWithWorkspaceRecord {
    pub branch: ActiveBranchRecord,
    pub workspace: WorkspaceRecord,
}

/// Best-effort request context labels for observability policy resolution.
///
/// These are display values, not authorization decisions. Missing values mean
/// the route did not carry that id or the row was not available when middleware
/// built the request context.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestContextNames {
    pub repo: Option<String>,
    pub workspace: Option<String>,
    pub branch: Option<String>,
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

/// Inputs for selecting or creating an active branch.
///
/// The store derives the source tree and last selected workspace path from the
/// workspace id. Re-selecting an existing branch updates its lifecycle metadata
/// and ensures the workspace is attached to that branch.
pub struct SelectBranchInput {
    pub workspace_id: String,
    pub principal_id: String,
    pub branch: String,
    pub base_ref: String,
    pub base_commit: Option<String>,
    pub last_seen_commit: Option<String>,
}

/// Pull request metadata observed for an active branch.
pub struct BranchPullRequestInput {
    pub branch_id: String,
    pub pr_number: i64,
    pub pr_state: String,
    pub pr_url: String,
    pub pr_merged_at: Option<String>,
}

/// Durable source tree row plus discovered workspaces from one registration run.
pub struct RegisterSourceTreeInput {
    pub principal_id: String,
    pub kind: SourceTreeKind,
    pub source: String,
    pub display_name: String,
    pub default_revision: String,
    pub workspaces: Vec<DiscoveredWorkspaceInput>,
}

/// Workspace discovered inside a registered source tree.
///
/// Discovery creates these from GitHub tree results or fixed local sources.
/// The store folds them into source-tree-scoped workspace rows and marks stale rows
/// inactive or deletes them when safe.
pub struct DiscoveredWorkspaceInput {
    pub path: String,
    pub revision: String,
    pub source: String,
}
