use serde::Serialize;

/// Console deployment mode selected at startup.
///
/// Local mode trusts the workstation and can use ambient credentials. Hosted
/// mode requires GitHub OAuth and encrypted session tokens. The value is stored
/// in `ConsoleState` for the life of the process and serialized so the browser
/// can choose the correct auth flow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentType {
    Local,
    Hosted,
}

impl DeploymentType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Hosted => "hosted",
        }
    }
}

/// Write policy selected by CLI flags at startup.
///
/// This is process configuration, not workspace state. Route handlers combine
/// it with the workspace source kind and current credential to decide whether a
/// mutation can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritePolicy {
    Disabled,
    PullRequest,
    DirectPush,
}

impl WritePolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::PullRequest => "pull-request",
            Self::DirectPush => "direct-push",
        }
    }
}

/// Concrete backend that will perform an allowed workspace write.
///
/// The browser only receives this as explanation. Server routes recompute the
/// backend for each mutation so a stale client response cannot grant writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WriteBackend {
    GitHubApi,
    LocalGit,
}

/// Read capability for a workspace under the current credential.
///
/// It is calculated per response from the source kind and token availability,
/// then discarded. It exists to let the UI explain missing credentials before a
/// user hits an operation that would fail.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Capability {
    Allowed,
    MissingCredential { reason: String },
}

/// Write capability for a workspace under the process write policy.
///
/// This is a browser-facing decision summary; the server does not trust it on
/// follow-up requests and instead reselects a write backend during mutation.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WriteCapability {
    Disabled { reason: String },
    PullRequest { backend: WriteBackend },
    DirectPush { backend: WriteBackend },
}

/// Full capability summary for one workspace response.
///
/// It is assembled alongside workspace data and has no durable lifecycle. The
/// durable facts are the workspace source, startup write policy, and current
/// user's credential.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCapabilities {
    pub read: Capability,
    pub write: WriteCapability,
}

/// Normalized class of workspace source URI.
///
/// Classification keeps routing policy explicit: local paths can use local git,
/// GitHub sources can use the GitHub API, and generic remotes remain read-only
/// until a write backend is intentionally added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceSourceKind {
    LocalPath,
    FileUrl,
    GitFile,
    GitHubArchive,
    GitHubGit,
    HttpsArchive,
    GenericGitRemote,
}

pub fn classify_workspace_source(source: &str) -> WorkspaceSourceKind {
    let trimmed = source.trim();
    if trimmed.starts_with("file://") {
        return WorkspaceSourceKind::FileUrl;
    }
    if trimmed.starts_with("git+file://") {
        return WorkspaceSourceKind::GitFile;
    }
    if trimmed.starts_with("https://api.github.com/repos/")
        && (trimmed.contains("/tarball/") || trimmed.contains("/zipball/"))
    {
        return WorkspaceSourceKind::GitHubArchive;
    }
    if trimmed.starts_with("git+https://github.com/")
        || trimmed.starts_with("git+ssh://git@github.com/")
        || trimmed.starts_with("git+ssh://github.com/")
    {
        return WorkspaceSourceKind::GitHubGit;
    }
    if trimmed.starts_with("git+") {
        return WorkspaceSourceKind::GenericGitRemote;
    }
    if trimmed.starts_with("https://") {
        return WorkspaceSourceKind::HttpsArchive;
    }
    WorkspaceSourceKind::LocalPath
}

pub fn workspace_capabilities(
    kind: WorkspaceSourceKind,
    policy: WritePolicy,
    has_github_token: bool,
) -> WorkspaceCapabilities {
    let read = match kind {
        WorkspaceSourceKind::GitHubArchive | WorkspaceSourceKind::GitHubGit => {
            if has_github_token {
                Capability::Allowed
            } else {
                Capability::MissingCredential {
                    reason: "a GitHub token is required for private GitHub workspace sources"
                        .to_owned(),
                }
            }
        }
        _ => Capability::Allowed,
    };
    let write = match policy {
        WritePolicy::Disabled => WriteCapability::Disabled {
            reason: "workspace writes are disabled for this console".to_owned(),
        },
        WritePolicy::PullRequest => match kind {
            WorkspaceSourceKind::GitHubArchive | WorkspaceSourceKind::GitHubGit => {
                if has_github_token {
                    WriteCapability::PullRequest {
                        backend: WriteBackend::GitHubApi,
                    }
                } else {
                    WriteCapability::Disabled {
                        reason: "GitHub pull requests need a GitHub token".to_owned(),
                    }
                }
            }
            _ => WriteCapability::Disabled {
                reason: "pull-request writes are only implemented for GitHub workspaces".to_owned(),
            },
        },
        WritePolicy::DirectPush => match kind {
            WorkspaceSourceKind::LocalPath | WorkspaceSourceKind::FileUrl => {
                WriteCapability::DirectPush {
                    backend: WriteBackend::LocalGit,
                }
            }
            WorkspaceSourceKind::GitHubArchive | WorkspaceSourceKind::GitHubGit => {
                if has_github_token {
                    WriteCapability::DirectPush {
                        backend: WriteBackend::GitHubApi,
                    }
                } else {
                    WriteCapability::Disabled {
                        reason: "GitHub direct push needs a GitHub token".to_owned(),
                    }
                }
            }
            _ => WriteCapability::Disabled {
                reason: "direct-push writes are not implemented for this workspace source"
                    .to_owned(),
            },
        },
    };
    WorkspaceCapabilities { read, write }
}
