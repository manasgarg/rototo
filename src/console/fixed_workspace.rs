use std::path::Path;

use crate::error::{Result, RototoError};
use crate::source::{SourceOptions, stage_source_tree};

use super::capabilities;
use super::local_git;
use super::stage::discover_workspaces_in_tree;
use super::store::{DiscoveredWorkspaceInput, SourceTreeKind};

pub(crate) struct FixedWorkspaceRegistration {
    pub(crate) kind: SourceTreeKind,
    pub(crate) source: String,
    pub(crate) display_name: String,
    pub(crate) default_revision: String,
    pub(crate) workspaces: Vec<DiscoveredWorkspaceInput>,
}

pub(crate) async fn registration(source: &str) -> Result<FixedWorkspaceRegistration> {
    let (display_name, mut default_revision, _path) = synthetic_registration(source);
    let source_kind = capabilities::classify_workspace_source(source);
    if matches!(
        source_kind,
        capabilities::WorkspaceSourceKind::LocalPath | capabilities::WorkspaceSourceKind::FileUrl
    ) && let Ok(branch) = local_git::current_branch(source).await
    {
        default_revision = branch;
    }

    let staged = stage_source_tree(source, &SourceOptions::default()).await?;
    let workspace_paths = discover_workspaces_in_tree(staged.root())
        .await?
        .paths
        .into_iter()
        .map(|workspace| workspace.to_string())
        .collect::<Vec<_>>();

    if workspace_paths.is_empty() {
        return Err(RototoError::new(format!(
            "no rototo workspace manifests found under `{source}`"
        )));
    }

    let workspaces = workspace_paths
        .into_iter()
        .map(|path| DiscoveredWorkspaceInput {
            source: source_for_path(source, &default_revision, &path),
            path,
            revision: default_revision.clone(),
        })
        .collect();

    Ok(FixedWorkspaceRegistration {
        kind: source_tree_kind(source_kind),
        source: source.to_owned(),
        display_name,
        default_revision,
        workspaces,
    })
}

fn source_tree_kind(kind: capabilities::WorkspaceSourceKind) -> SourceTreeKind {
    match kind {
        capabilities::WorkspaceSourceKind::GitHubArchive
        | capabilities::WorkspaceSourceKind::GitHubGit => SourceTreeKind::GitHub,
        capabilities::WorkspaceSourceKind::GitFile
        | capabilities::WorkspaceSourceKind::GenericGitRemote => SourceTreeKind::GitRemote,
        capabilities::WorkspaceSourceKind::HttpsArchive => SourceTreeKind::Archive,
        capabilities::WorkspaceSourceKind::LocalPath
        | capabilities::WorkspaceSourceKind::FileUrl => SourceTreeKind::LocalFolder,
    }
}

/// Best-effort label/ref/path fields for an arbitrary source tree.
/// Staging always uses the source tree identity; these feed display labels and
/// the revision recorded for discovered workspace rows.
fn synthetic_registration(source: &str) -> (String, String, String) {
    let (base, fragment) = match source.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (source, None),
    };
    let path = fragment
        .and_then(|fragment| fragment.split_once(':').map(|(_, path)| path))
        .filter(|path| !path.is_empty())
        .unwrap_or(".")
        .to_owned();
    let ref_from_fragment = fragment
        .map(|fragment| {
            fragment
                .split_once(':')
                .map(|(git_ref, _)| git_ref)
                .unwrap_or(fragment)
        })
        .filter(|git_ref| !git_ref.is_empty());

    // GitHub archive: https://api.github.com/repos/{owner}/{name}/tarball/{ref}
    if let Some(rest) = base.strip_prefix("https://api.github.com/repos/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 4 && (parts[2] == "tarball" || parts[2] == "zipball") {
            return (
                format!("{}/{}", parts[0], parts[1]),
                parts[3].to_owned(),
                path,
            );
        }
    }
    // Git URL: git+https://github.com/{owner}/{name}.git
    if let Some(at) = base.find("://")
        && base.starts_with("git+")
    {
        let rest = &base[at + 3..];
        if !rest.starts_with('/') {
            let mut segments = rest.split('/').skip(1);
            if let (Some(owner), Some(name)) = (segments.next(), segments.next()) {
                let name = name.strip_suffix(".git").unwrap_or(name);
                return (
                    format!("{owner}/{name}"),
                    ref_from_fragment.unwrap_or("main").to_owned(),
                    path,
                );
            }
        }
    }
    // Local paths and anything else.
    let name = base
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace");
    (
        name.to_owned(),
        ref_from_fragment.unwrap_or("main").to_owned(),
        path,
    )
}

fn source_for_path(source: &str, git_ref: &str, workspace_path: &str) -> String {
    let path = workspace_path.trim();
    if let Some((scheme, rest)) = source.split_once("://") {
        if scheme.starts_with("git+") {
            let base = source
                .split_once('#')
                .map(|(base, _)| base)
                .unwrap_or(source);
            return if path == "." {
                format!("{base}#{git_ref}")
            } else {
                format!("{base}#{git_ref}:{path}")
            };
        }

        if scheme.eq_ignore_ascii_case("file") {
            let base = rest.split_once('#').map(|(base, _)| base).unwrap_or(rest);
            return if path == "." {
                format!("file://{base}")
            } else {
                format!("file://{}", Path::new(base).join(path).display())
            };
        }

        if scheme.eq_ignore_ascii_case("https")
            && source.starts_with("https://api.github.com/repos/")
        {
            return archive_source_for_path(source, path);
        }

        if scheme.eq_ignore_ascii_case("https") {
            return archive_source_for_path(source, path);
        }
    }

    if path == "." {
        source.to_owned()
    } else {
        Path::new(source).join(path).display().to_string()
    }
}

fn archive_source_for_path(source: &str, workspace_path: &str) -> String {
    if workspace_path == "." {
        return source.to_owned();
    }
    let (base, existing_path) = match source.split_once('#') {
        Some((base, fragment)) => (
            base,
            fragment.strip_prefix(':').filter(|path| !path.is_empty()),
        ),
        None => (source, None),
    };
    let path = match existing_path {
        Some(prefix) => format!("{}/{}", prefix.trim_matches('/'), workspace_path),
        None => workspace_path.to_owned(),
    };
    format!("{base}#:{path}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    use tempfile::TempDir;

    #[test]
    fn synthetic_registration_parses_source_forms() {
        assert_eq!(
            synthetic_registration("https://api.github.com/repos/octo/configs/tarball/main"),
            ("octo/configs".to_owned(), "main".to_owned(), ".".to_owned())
        );
        assert_eq!(
            synthetic_registration(
                "https://api.github.com/repos/octo/configs/tarball/v2#:payments/flags"
            ),
            (
                "octo/configs".to_owned(),
                "v2".to_owned(),
                "payments/flags".to_owned()
            )
        );
        assert_eq!(
            synthetic_registration("git+https://github.com/octo/configs.git#release:apps"),
            (
                "octo/configs".to_owned(),
                "release".to_owned(),
                "apps".to_owned()
            )
        );
        assert_eq!(
            synthetic_registration("examples/basic"),
            ("basic".to_owned(), "main".to_owned(), ".".to_owned())
        );
    }

    #[test]
    fn source_for_path_preserves_source_forms() {
        assert_eq!(
            source_for_path("git+https://github.com/octo/configs.git#main", "dev", "."),
            "git+https://github.com/octo/configs.git#dev"
        );
        assert_eq!(
            source_for_path(
                "git+https://github.com/octo/configs.git#main:old",
                "dev",
                "apps/payments"
            ),
            "git+https://github.com/octo/configs.git#dev:apps/payments"
        );
        assert_eq!(
            source_for_path("examples/root", "main", "apps/payments"),
            format!(
                "examples{}root{}apps{}payments",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR
            )
        );
        assert_eq!(
            source_for_path(
                "https://example.com/config.tar.gz#:base",
                "main",
                "apps/payments"
            ),
            "https://example.com/config.tar.gz#:base/apps/payments"
        );
        assert_eq!(
            source_for_path("https://example.com/config.tar.gz#:base", "main", "."),
            "https://example.com/config.tar.gz#:base"
        );
    }

    #[tokio::test]
    async fn registration_discovers_local_workspace_tree() {
        let temp = TempDir::new().expect("temp dir");
        write_manifest(temp.path());
        write_manifest(&temp.path().join("apps/payments"));

        let registration = registration(path_str(temp.path()))
            .await
            .expect("fixed workspace registration");

        assert_eq!(registration.kind, SourceTreeKind::LocalFolder);
        assert_eq!(
            registration.display_name,
            temp.path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        );
        assert_workspace_paths(&registration.workspaces, &[".", "apps/payments"]);
        assert_eq!(
            registration.workspaces[1].source,
            temp.path().join("apps/payments").display().to_string()
        );
    }

    #[tokio::test]
    async fn registration_discovers_git_source_tree() {
        let temp = TempDir::new().expect("temp dir");
        init_git_repo(temp.path());
        write_manifest(temp.path());
        write_manifest(&temp.path().join("apps/payments"));
        git(temp.path(), &["add", "."]);
        git(temp.path(), &["commit", "-m", "add workspaces"]);

        let source = format!("git+file://{}#main", path_str(temp.path()));
        let registration = registration(&source)
            .await
            .expect("fixed workspace registration");

        assert_eq!(registration.kind, SourceTreeKind::GitRemote);
        assert_eq!(registration.default_revision, "main");
        assert_workspace_paths(&registration.workspaces, &[".", "apps/payments"]);
        assert_eq!(
            registration.workspaces[1].source,
            format!("git+file://{}#main:apps/payments", path_str(temp.path()))
        );
    }

    #[tokio::test]
    async fn registration_rejects_source_tree_without_workspaces() {
        let temp = TempDir::new().expect("temp dir");

        let err = match registration(path_str(temp.path())).await {
            Ok(_) => panic!("source tree should need a workspace manifest"),
            Err(err) => err,
        };

        assert!(
            err.to_string()
                .contains("no rototo workspace manifests found")
        );
    }

    fn write_manifest(path: &Path) {
        std::fs::create_dir_all(path).expect("create workspace dir");
        std::fs::write(path.join("rototo-workspace.toml"), "schema_version = 1\n")
            .expect("write workspace manifest");
    }

    fn path_str(path: &Path) -> &str {
        path.to_str().expect("utf-8 path")
    }

    fn assert_workspace_paths(workspaces: &[DiscoveredWorkspaceInput], expected: &[&str]) {
        let paths = workspaces
            .iter()
            .map(|workspace| workspace.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, expected);
    }

    fn init_git_repo(path: &Path) {
        git(path, &["init", "-b", "main"]);
        git(path, &["config", "user.email", "console@example.com"]);
        git(path, &["config", "user.name", "Console Test"]);
    }

    fn git(path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(path)
            .status()
            .expect("run git");
        assert!(status.success(), "git {:?} failed", args);
    }
}
