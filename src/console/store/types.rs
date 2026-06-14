use serde::Serialize;

use crate::console::identity::ActorIdentity;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    pub session_hash: String,
    pub principal_id: String,
    pub identity: ActorIdentity,
    #[serde(skip)]
    pub github_token: Option<String>,
}

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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftWithWorkspaceRecord {
    pub draft: DraftSessionRecord,
    pub workspace: WorkspaceRecord,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftChangeRecord {
    pub id: String,
    pub draft_id: String,
    pub file_path: String,
    pub variable_id: String,
    pub value_key: String,
    pub before_json: String,
    pub after_json: String,
    pub updated_at: String,
}

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
    pub variable_id: String,
    pub value_key: String,
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
