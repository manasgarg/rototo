use super::api::{ApiError, ApiResult, ConsoleState};
use super::github::GitHubRepoIdentity;
use super::stage::{
    CachedPackageLocator, PackageLocator, PackagePath, SemanticPackage, SourceTreeOrigin,
    SourceTreeRevision, TokenIdentity,
};
use super::store::PackageRecord;
use crate::error::{Result, RototoError};
use crate::sdk::Package;
use std::sync::Arc;

/// Store/API input needed to build a stage package locator.
///
/// This keeps raw `PackageRecord` fields at the console adapter boundary so
/// stage only receives normalized locators.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PackageSourceInput<'a> {
    pub(crate) principal_id: &'a str,
    pub(crate) token: &'a str,
    pub(crate) path: &'a str,
    pub(crate) revision: &'a str,
    pub(crate) source: &'a str,
}

pub(crate) async fn cached_package_locator_for_base(
    input: PackageSourceInput<'_>,
) -> Result<CachedPackageLocator> {
    let parsed = ParsedPackageSource::parse(input.source).await?;
    let revision = match parsed.kind {
        PackageSourceBacking::Git => {
            SourceTreeRevision::git_ref_or_commit(selected_git_ref(input, &parsed))?
        }
        PackageSourceBacking::LocalFolder => SourceTreeRevision::LocalWorkingTree,
        PackageSourceBacking::Archive => SourceTreeRevision::ArchiveSnapshot,
    };
    cached_package_from_parts(input, parsed.tree, revision)
}

pub(crate) async fn cached_package_locator_for_branch(
    input: PackageSourceInput<'_>,
    branch: impl AsRef<str>,
) -> Result<CachedPackageLocator> {
    let parsed = ParsedPackageSource::parse(input.source).await?;
    let revision = match parsed.kind {
        PackageSourceBacking::Git => SourceTreeRevision::git_branch(branch)?,
        PackageSourceBacking::LocalFolder => SourceTreeRevision::LocalWorkingTree,
        PackageSourceBacking::Archive => {
            return Err(RototoError::new(
                "branch package sources require a git branch or local working tree source",
            ));
        }
    };
    cached_package_from_parts(input, parsed.tree, revision)
}

pub(crate) fn github_repo_for_package(package: &PackageRecord) -> Result<GitHubRepoIdentity> {
    super::github::repo_identity_from_source(&package.source)
}

pub(crate) async fn package_source_for_base(
    state: &ConsoleState,
    principal_id: &str,
    token: &str,
    package: &PackageRecord,
) -> ApiResult<CachedPackageLocator> {
    package_source_for_base_source(
        state.fixed_package_source.as_deref(),
        principal_id,
        token,
        package,
    )
    .await
}

pub(crate) async fn package_source_for_branch(
    state: &ConsoleState,
    principal_id: &str,
    token: &str,
    package: &PackageRecord,
    branch: &str,
) -> ApiResult<CachedPackageLocator> {
    let source = source_tree_source(state.fixed_package_source.as_deref(), package);
    package_source_for_branch_source(source, principal_id, token, package, branch).await
}

async fn package_source_for_base_source(
    fixed_package_source: Option<&str>,
    principal_id: &str,
    token: &str,
    package: &PackageRecord,
) -> ApiResult<CachedPackageLocator> {
    cached_package_locator_for_base(package_source_input(
        principal_id,
        token,
        source_tree_source(fixed_package_source, package),
        package,
    ))
    .await
    .map_err(|err| ApiError::internal(err.to_string()))
}

async fn package_source_for_branch_source(
    source: &str,
    principal_id: &str,
    token: &str,
    package: &PackageRecord,
    branch: &str,
) -> ApiResult<CachedPackageLocator> {
    let input = package_source_input(principal_id, token, source, package);
    cached_package_locator_for_branch(input, branch)
        .await
        .map_err(|err| ApiError::internal(err.to_string()))
}

fn package_source_input<'a>(
    principal_id: &'a str,
    token: &'a str,
    source: &'a str,
    package: &'a PackageRecord,
) -> PackageSourceInput<'a> {
    PackageSourceInput {
        principal_id,
        token,
        path: &package.path,
        revision: &package.revision,
        source,
    }
}

pub(crate) async fn semantic_package_for_base(
    state: &ConsoleState,
    principal_id: &str,
    token: &str,
    package: &PackageRecord,
) -> ApiResult<SemanticPackage> {
    let package_source = package_source_for_base(state, principal_id, token, package).await?;
    state
        .stage
        .get_semantic_package(package_source, token)
        .await
        .map_err(|err| ApiError::internal(err.to_string()))
}

pub(crate) async fn runtime_package_for_base(
    state: &ConsoleState,
    principal_id: &str,
    token: &str,
    package: &PackageRecord,
) -> ApiResult<Arc<Package>> {
    let package_source = package_source_for_base(state, principal_id, token, package).await?;
    state
        .stage
        .get_runtime_package(package_source, token)
        .await
        .map_err(|err| ApiError::internal(err.to_string()))
}

fn source_tree_source<'a>(
    fixed_package_source: Option<&'a str>,
    package: &'a PackageRecord,
) -> &'a str {
    fixed_package_source.unwrap_or(&package.source)
}

#[derive(Debug)]
struct ParsedPackageSource {
    tree: SourceTreeOrigin,
    ref_: Option<String>,
    kind: PackageSourceBacking,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PackageSourceBacking {
    Git,
    LocalFolder,
    Archive,
}

impl ParsedPackageSource {
    async fn parse(source: &str) -> Result<Self> {
        let source = source.trim();
        let Some(uri) = ParsedSourceUri::parse(source)? else {
            return Ok(Self {
                tree: SourceTreeOrigin::local_folder(source).await?,
                ref_: None,
                kind: PackageSourceBacking::LocalFolder,
            });
        };

        if uri.scheme == "file" {
            if uri.ref_.is_some() || uri.subdir.is_some() {
                return Err(RototoError::new(
                    "file:// package sources do not support fragments",
                ));
            }
            return Ok(Self {
                tree: SourceTreeOrigin::local_folder(&uri.base).await?,
                ref_: None,
                kind: PackageSourceBacking::LocalFolder,
            });
        }

        if uri.scheme.starts_with("git+") {
            return Ok(Self {
                tree: SourceTreeOrigin::git_remote(uri.source_without_fragment())?,
                ref_: uri.ref_,
                kind: PackageSourceBacking::Git,
            });
        }

        if uri.scheme == "https" {
            if let Some((owner, name, _)) = github_archive_source(&uri.base) {
                return Ok(Self {
                    tree: SourceTreeOrigin::github(owner, name)?,
                    ref_: uri.ref_,
                    kind: PackageSourceBacking::Git,
                });
            }
            return Ok(Self {
                tree: SourceTreeOrigin::archive(source)?,
                ref_: None,
                kind: PackageSourceBacking::Archive,
            });
        }

        Err(RototoError::new(format!(
            "package source scheme is not supported: {}",
            uri.scheme
        )))
    }
}

#[derive(Clone, Debug)]
struct ParsedSourceUri {
    scheme: String,
    base: String,
    ref_: Option<String>,
    subdir: Option<String>,
}

impl ParsedSourceUri {
    fn parse(source: &str) -> Result<Option<Self>> {
        let Some((scheme, rest)) = source.split_once("://") else {
            return Ok(None);
        };
        if scheme.is_empty() || rest.is_empty() {
            return Err(RototoError::new(format!(
                "package source URI is invalid: {source}"
            )));
        }
        let (base, fragment) = match rest.split_once('#') {
            Some((base, fragment)) => (base, Some(fragment)),
            None => (rest, None),
        };
        if base.is_empty() {
            return Err(RototoError::new(format!(
                "package source URI is invalid: {source}"
            )));
        }
        let (ref_, subdir) = match fragment {
            Some(fragment) => match fragment.split_once(':') {
                Some((ref_, subdir)) => (
                    (!ref_.is_empty()).then(|| ref_.to_owned()),
                    (!subdir.is_empty()).then(|| subdir.to_owned()),
                ),
                None => ((!fragment.is_empty()).then(|| fragment.to_owned()), None),
            },
            None => (None, None),
        };
        Ok(Some(Self {
            scheme: scheme.to_ascii_lowercase(),
            base: base.to_owned(),
            ref_,
            subdir,
        }))
    }

    fn source_without_fragment(&self) -> String {
        format!("{}://{}", self.scheme, self.base)
    }
}

fn cached_package_from_parts(
    input: PackageSourceInput<'_>,
    tree: SourceTreeOrigin,
    revision: SourceTreeRevision,
) -> Result<CachedPackageLocator> {
    CachedPackageLocator::new(
        input.principal_id,
        PackageLocator::new(tree, revision, PackagePath::new(input.path)?),
        TokenIdentity::from_console_token(input.token),
    )
}

fn selected_git_ref<'a>(input: PackageSourceInput<'a>, parsed: &'a ParsedPackageSource) -> &'a str {
    let revision = input.revision.trim();
    if revision.is_empty() {
        parsed.ref_.as_deref().unwrap_or("main")
    } else {
        revision
    }
}

fn github_archive_source(base: &str) -> Option<(&str, &str, &str)> {
    let rest = strip_prefix_ignore_ascii_case(base, "api.github.com/repos/")?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    let archive_kind = parts.next()?;
    if !matches!(archive_kind, "tarball" | "zipball") {
        return None;
    }
    let git_ref = parts.next()?;
    Some((owner, name, git_ref))
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        value.get(prefix.len()..)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::stage::{BranchName, GitRefName, SourceTreeOrigin, SourceTreeRevision};
    use crate::console::stage::{PackageLocator, PackagePath, TokenIdentity};
    use tempfile::TempDir;

    fn github_package_input() -> PackageSourceInput<'static> {
        PackageSourceInput {
            principal_id: "user_123",
            token: "",
            path: "packages/payments",
            revision: "main",
            source: "git+https://github.com/Rototo/Config.git#main:packages/payments",
        }
    }

    fn package() -> PackageRecord {
        PackageRecord {
            id: "package_1".to_owned(),
            source_tree_id: "repo_1".to_owned(),
            slug: "octo-configs-root".to_owned(),
            source_tree_label: "octo/configs".to_owned(),
            display_path: ".".to_owned(),
            path: ".".to_owned(),
            revision: "main".to_owned(),
            source: "https://api.github.com/repos/octo/configs/tarball/main".to_owned(),
            discovered_at: "2026-06-13T00:00:00Z".to_owned(),
        }
    }

    #[tokio::test]
    async fn base_package_locator_adapts_current_github_store_shape() {
        let source = cached_package_locator_for_base(github_package_input())
            .await
            .unwrap();

        assert_eq!(
            source,
            CachedPackageLocator::new(
                "user_123",
                PackageLocator::new(
                    SourceTreeOrigin::GitHub {
                        owner: "rototo".to_owned(),
                        name: "config".to_owned(),
                    },
                    SourceTreeRevision::GitRef(GitRefName::new("main").unwrap()),
                    PackagePath::new("packages/payments").unwrap(),
                ),
                TokenIdentity::None,
            )
            .unwrap()
        );
    }

    #[tokio::test]
    async fn base_package_locator_uses_commit_revision_for_pinned_git_ref() {
        let input = PackageSourceInput {
            revision: "8D3C4B5A6F7081920A1B2C3D4E5F60718293A4B5",
            ..github_package_input()
        };

        let source = cached_package_locator_for_base(input).await.unwrap();

        match source.package.source_tree.revision {
            SourceTreeRevision::GitCommit(commit) => {
                assert_eq!(commit.as_ref(), "8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5");
            }
            revision => panic!("expected git commit revision, got {revision:?}"),
        }
    }

    #[tokio::test]
    async fn branch_package_locator_adapts_token_and_branch_identity() {
        let input = PackageSourceInput {
            token: "ghp_secret",
            ..github_package_input()
        };

        let source =
            cached_package_locator_for_branch(input, "rototo-console/alice/change-checkout")
                .await
                .unwrap();

        assert_eq!(
            source.token,
            TokenIdentity::Sha256Hex(
                "4c281411e1ccc93c230902001a09e7b863cb12a3f3b341089eb980a34aa9e434".to_owned()
            )
        );
        assert_eq!(
            source.package.source_tree.revision,
            SourceTreeRevision::GitBranch(
                BranchName::new("rototo-console/alice/change-checkout").unwrap()
            )
        );
    }

    #[tokio::test]
    async fn package_locator_adapter_preserves_generic_git_remote_identity() {
        let input = PackageSourceInput {
            principal_id: "user_123",
            token: "",
            path: "services/api",
            revision: "main",
            source: "git+https://Git.Example.com/Team/Config.git#main:services/api",
        };

        let source = cached_package_locator_for_base(input).await.unwrap();

        assert_eq!(
            source.package.source_tree.origin,
            SourceTreeOrigin::GitRemote {
                remote_url: "git+https://git.example.com/Team/Config.git".to_owned()
            }
        );
        assert_eq!(
            source.package.path,
            PackagePath::new("services/api").unwrap()
        );
        assert_eq!(
            source.package.source_tree.revision,
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap())
        );
    }

    #[tokio::test]
    async fn package_locator_adapter_maps_local_package_to_working_tree() {
        let tempdir = TempDir::new().expect("tempdir");
        let input = PackageSourceInput {
            principal_id: "local-user",
            token: "",
            path: ".",
            revision: "main",
            source: tempdir.path().to_str().expect("utf8 temp path"),
        };

        let source = cached_package_locator_for_base(input).await.unwrap();

        assert_eq!(
            source.package.source_tree.origin,
            SourceTreeOrigin::LocalFolder {
                root: tempdir.path().canonicalize().unwrap()
            }
        );
        assert_eq!(source.package.path, PackagePath::root());
        assert_eq!(
            source.package.source_tree.revision,
            SourceTreeRevision::LocalWorkingTree
        );
    }

    #[tokio::test]
    async fn package_locator_adapter_maps_github_archive_store_shape_to_git_tree() {
        let input = PackageSourceInput {
            principal_id: "user_123",
            token: "",
            path: "packages/payments",
            revision: "main",
            source: "https://API.GITHUB.com/repos/Rototo/Config/tarball/main#:packages/payments",
        };

        let source = cached_package_locator_for_base(input).await.unwrap();

        assert_eq!(
            source.package.source_tree.origin,
            SourceTreeOrigin::GitHub {
                owner: "rototo".to_owned(),
                name: "config".to_owned(),
            }
        );
        assert_eq!(
            source.package.source_tree.revision,
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap())
        );
    }

    #[tokio::test]
    async fn package_locator_adapter_maps_arbitrary_archive_to_snapshot() {
        let input = PackageSourceInput {
            principal_id: "user_123",
            token: "",
            path: "packages/payments",
            revision: "main",
            source: "https://example.com/releases/config.tar.gz#:packages/payments",
        };

        let source = cached_package_locator_for_base(input).await.unwrap();

        assert_eq!(
            source.package.source_tree.origin,
            SourceTreeOrigin::Archive {
                url: "https://example.com/releases/config.tar.gz".to_owned()
            }
        );
        assert_eq!(
            source.package.source_tree.revision,
            SourceTreeRevision::ArchiveSnapshot
        );
        assert!(
            cached_package_locator_for_branch(input, "rototo-console/alice/change")
                .await
                .is_err()
        );
    }

    #[test]
    fn source_tree_source_prefers_fixed_source_tree_root() {
        let mut package = package();
        package.path = "apps/payments".to_owned();
        package.source = "/tmp/configs/apps/payments".to_owned();

        assert_eq!(
            source_tree_source(Some("/tmp/configs"), &package),
            "/tmp/configs"
        );
        assert_eq!(source_tree_source(None, &package), package.source);
    }

    #[tokio::test]
    async fn branch_package_source_selects_branch_for_git_package() {
        let mut package = package();
        package.path = "apps/payments".to_owned();
        package.source = "git+https://github.com/octo/configs.git#main:apps/payments".to_owned();

        let source = expect_ok(
            package_source_for_branch_source(
                &package.source,
                "user_123",
                "",
                &package,
                "feature/payments",
            )
            .await,
        );

        assert_eq!(
            source.package.source_tree.origin,
            SourceTreeOrigin::GitHub {
                owner: "octo".to_owned(),
                name: "configs".to_owned(),
            }
        );
        assert_eq!(
            source.package.source_tree.revision,
            SourceTreeRevision::GitBranch(BranchName::new("feature/payments").unwrap())
        );
    }

    fn expect_ok<T>(result: ApiResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{}", err.message),
        }
    }
}
