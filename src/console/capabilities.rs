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
/// This is process configuration, not package state. Route handlers combine
/// it with the package source kind and current credential to decide whether a
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

/// Concrete backend that will perform an allowed package write.
///
/// The browser only receives this as explanation. Server routes recompute the
/// backend for each mutation so a stale client response cannot grant writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WriteBackend {
    GitHubApi,
    LocalWorkingTree,
}

/// Read capability for a package under the current credential.
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

/// Write capability for a package under the process write policy.
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

/// Full capability summary for one package response.
///
/// It is assembled alongside package data and has no durable lifecycle. The
/// durable facts are the package source, startup write policy, and current
/// user's credential.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageCapabilities {
    pub read: Capability,
    pub write: WriteCapability,
}

/// Normalized class of package source URI.
///
/// Classification keeps routing policy explicit: local paths can use local git,
/// GitHub sources can use the GitHub API, and generic remotes remain read-only
/// until a write backend is intentionally added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PackageSourceKind {
    LocalPath,
    FileUrl,
    GitFile,
    GitHubArchive,
    GitHubGit,
    HttpsArchive,
    GenericGitRemote,
}

pub fn classify_package_source(source: &str) -> PackageSourceKind {
    let trimmed = source.trim();
    if trimmed.starts_with("file://") {
        return PackageSourceKind::FileUrl;
    }
    if trimmed.starts_with("git+file://") {
        return PackageSourceKind::GitFile;
    }
    if trimmed.starts_with("https://api.github.com/repos/")
        && (trimmed.contains("/tarball/") || trimmed.contains("/zipball/"))
    {
        return PackageSourceKind::GitHubArchive;
    }
    if trimmed.starts_with("git+https://github.com/")
        || trimmed.starts_with("git+ssh://git@github.com/")
        || trimmed.starts_with("git+ssh://github.com/")
    {
        return PackageSourceKind::GitHubGit;
    }
    if trimmed.starts_with("git+") {
        return PackageSourceKind::GenericGitRemote;
    }
    if trimmed.starts_with("https://") {
        return PackageSourceKind::HttpsArchive;
    }
    PackageSourceKind::LocalPath
}

pub fn package_capabilities(
    kind: PackageSourceKind,
    policy: WritePolicy,
    deployment: &DeploymentType,
    has_github_token: bool,
) -> PackageCapabilities {
    let read = match kind {
        PackageSourceKind::GitHubArchive | PackageSourceKind::GitHubGit => {
            if has_github_token {
                Capability::Allowed
            } else {
                Capability::MissingCredential {
                    reason: "a GitHub token is required for private GitHub package sources"
                        .to_owned(),
                }
            }
        }
        _ => Capability::Allowed,
    };
    let write = match policy {
        WritePolicy::Disabled => WriteCapability::Disabled {
            reason: "package writes are disabled for this console".to_owned(),
        },
        WritePolicy::PullRequest => match kind {
            PackageSourceKind::GitHubArchive | PackageSourceKind::GitHubGit => {
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
                reason: "only GitHub configuration sources support pull-request edits".to_owned(),
            },
        },
        WritePolicy::DirectPush => match kind {
            PackageSourceKind::GitHubArchive | PackageSourceKind::GitHubGit => {
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
            PackageSourceKind::LocalPath | PackageSourceKind::FileUrl
                if deployment == &DeploymentType::Local =>
            {
                WriteCapability::DirectPush {
                    backend: WriteBackend::LocalWorkingTree,
                }
            }
            PackageSourceKind::LocalPath | PackageSourceKind::FileUrl => {
                WriteCapability::Disabled {
                    reason: "local folder edits require a local console deployment".to_owned(),
                }
            }
            _ => WriteCapability::Disabled {
                reason: "only GitHub or local folder configuration sources support direct edits"
                    .to_owned(),
            },
        },
    };
    PackageCapabilities { read, write }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_sources_can_use_pull_request_writes_with_token() {
        let capabilities = package_capabilities(
            PackageSourceKind::GitHubGit,
            WritePolicy::PullRequest,
            &DeploymentType::Local,
            true,
        );

        assert!(matches!(
            capabilities.write,
            WriteCapability::PullRequest {
                backend: WriteBackend::GitHubApi
            }
        ));
    }

    #[test]
    fn non_github_sources_do_not_support_pull_request_writes() {
        for kind in [
            PackageSourceKind::LocalPath,
            PackageSourceKind::FileUrl,
            PackageSourceKind::GitFile,
            PackageSourceKind::HttpsArchive,
            PackageSourceKind::GenericGitRemote,
        ] {
            let capabilities =
                package_capabilities(kind, WritePolicy::PullRequest, &DeploymentType::Local, true);
            assert!(matches!(
                capabilities.write,
                WriteCapability::Disabled { .. }
            ));
        }
    }

    #[test]
    fn local_sources_use_local_working_tree_for_direct_writes() {
        for kind in [PackageSourceKind::LocalPath, PackageSourceKind::FileUrl] {
            let capabilities =
                package_capabilities(kind, WritePolicy::DirectPush, &DeploymentType::Local, false);
            assert!(matches!(
                capabilities.write,
                WriteCapability::DirectPush {
                    backend: WriteBackend::LocalWorkingTree
                }
            ));
        }
    }

    #[test]
    fn unsupported_sources_stay_read_only_under_direct_writes() {
        for kind in [
            PackageSourceKind::GitFile,
            PackageSourceKind::HttpsArchive,
            PackageSourceKind::GenericGitRemote,
        ] {
            let capabilities =
                package_capabilities(kind, WritePolicy::DirectPush, &DeploymentType::Local, true);
            assert!(matches!(
                capabilities.write,
                WriteCapability::Disabled { .. }
            ));
        }
    }

    #[test]
    fn local_sources_are_read_only_in_hosted_deployments() {
        let capabilities = package_capabilities(
            PackageSourceKind::LocalPath,
            WritePolicy::DirectPush,
            &DeploymentType::Hosted,
            false,
        );

        assert!(matches!(
            capabilities.write,
            WriteCapability::Disabled { .. }
        ));
    }
}
