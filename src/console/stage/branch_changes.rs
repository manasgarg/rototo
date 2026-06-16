use std::collections::BTreeSet;
use std::path::Path;

use tokio::process::Command;

use super::{
    BranchChanges, BranchName, GitRefName, RepoRelativePath, TreeRevision, TreeSource,
    WorkspacePath,
};
use crate::error::{Result, RototoError};
use crate::source::SourceOptions;

const WORKSPACE_MANIFEST: &str = "rototo-workspace.toml";

#[derive(Clone, Copy, Debug)]
pub(super) enum BranchChangeSource {
    LocalWorkingTree,
    GitBranch,
}

pub(super) async fn get_branch_changes(
    repo: &Path,
    source: BranchChangeSource,
    branch: BranchName,
    base_ref: GitRefName,
    workspaces: &[WorkspacePath],
) -> Result<BranchChanges> {
    let changed_files = changed_files_for_staged_repo(repo, source, &base_ref).await?;
    let affected_workspaces = affected_workspaces(&changed_files, workspaces);
    Ok(BranchChanges {
        branch,
        base_ref,
        changed_files,
        affected_workspaces,
    })
}

pub(super) fn source_for_changes(tree: &TreeSource) -> Result<BranchChangeSource> {
    match tree {
        TreeSource::LocalFolder { .. } => Ok(BranchChangeSource::LocalWorkingTree),
        TreeSource::GitHub { .. } | TreeSource::GitRemote { .. } => {
            Ok(BranchChangeSource::GitBranch)
        }
        TreeSource::Archive { .. } => Err(RototoError::new(
            "branch changes require a git-backed source tree",
        )),
    }
}

pub(super) fn revision_for_changes(tree: &TreeSource, branch: &BranchName) -> Result<TreeRevision> {
    match tree {
        TreeSource::LocalFolder { .. } => Ok(TreeRevision::LocalWorkingTree),
        TreeSource::GitHub { .. } | TreeSource::GitRemote { .. } => {
            Ok(TreeRevision::GitBranch(branch.clone()))
        }
        TreeSource::Archive { .. } => Err(RototoError::new(
            "branch changes require a git-backed source tree",
        )),
    }
}

async fn changed_files_for_staged_repo(
    repo: &Path,
    source: BranchChangeSource,
    base_ref: &GitRefName,
) -> Result<Vec<RepoRelativePath>> {
    match source {
        BranchChangeSource::LocalWorkingTree => {
            changed_files_in_repo(
                repo,
                &[format!("{}...HEAD", base_ref.as_str())],
                &SourceOptions::default(),
            )
            .await
        }
        BranchChangeSource::GitBranch => {
            let options = SourceOptions::default();
            ensure_remote_base_ref(repo, base_ref, &options).await?;
            changed_files_in_repo(
                repo,
                &[
                    format!("{}...HEAD", base_ref.as_str()),
                    format!("origin/{}...HEAD", base_ref.as_str()),
                    "FETCH_HEAD...HEAD".to_owned(),
                ],
                &options,
            )
            .await
        }
    }
}

async fn ensure_remote_base_ref(
    repo: &Path,
    base_ref: &GitRefName,
    options: &SourceOptions,
) -> Result<()> {
    if matches!(
        git_output(
            Some(repo),
            &["rev-parse", "--is-shallow-repository"],
            options
        )
        .await
        .as_deref()
        .map(str::trim),
        Ok("true")
    ) {
        git_output(
            Some(repo),
            &["fetch", "--quiet", "--unshallow", "origin"],
            options,
        )
        .await?;
    }
    git_output(
        Some(repo),
        &["fetch", "--quiet", "origin", base_ref.as_str()],
        options,
    )
    .await?;
    Ok(())
}

async fn changed_files_in_repo(
    repo: &Path,
    diff_bases: &[String],
    options: &SourceOptions,
) -> Result<Vec<RepoRelativePath>> {
    let mut paths = BTreeSet::new();
    let mut diff_error = None;
    for diff_base in diff_bases {
        match git_output(
            Some(repo),
            &["diff", "--name-only", diff_base, "--", "."],
            options,
        )
        .await
        {
            Ok(diff) => {
                paths.extend(diff.lines().filter_map(repo_relative_path_string));
                diff_error = None;
                break;
            }
            Err(err) => diff_error = Some(err),
        }
    }
    if let Some(err) = diff_error {
        return Err(err);
    }
    let status = git_output(
        Some(repo),
        &["status", "--porcelain", "-uall", "--", "."],
        options,
    )
    .await?;
    paths.extend(status.lines().filter_map(status_path));
    Ok(paths
        .into_iter()
        .filter_map(|path| RepoRelativePath::new(path).ok())
        .collect())
}

fn affected_workspaces(
    changed_files: &[RepoRelativePath],
    workspaces: &[WorkspacePath],
) -> Vec<WorkspacePath> {
    let mut affected = BTreeSet::new();
    for workspace in workspaces {
        if changed_files
            .iter()
            .any(|file| file_affects_workspace(file.as_str(), workspace.as_str()))
        {
            affected.insert(workspace.as_str().to_owned());
        }
    }
    affected
        .into_iter()
        .filter_map(|path| WorkspacePath::new(path).ok())
        .collect()
}

fn file_affects_workspace(file_path: &str, workspace_path: &str) -> bool {
    workspace_path == "." || file_path.starts_with(&format!("{workspace_path}/"))
}

fn status_path(line: &str) -> Option<String> {
    let line = line.trim_end();
    if line.len() < 4 {
        return None;
    }
    let path = if line.starts_with('R') || line.starts_with('C') {
        line.rsplit(" -> ").next().unwrap_or(&line[3..])
    } else {
        &line[3..]
    };
    repo_relative_path_string(path)
}

fn repo_relative_path_string(path: &str) -> Option<String> {
    let path = path.trim().trim_matches('"');
    RepoRelativePath::new(path)
        .ok()
        .map(|path| path.as_str().to_owned())
}

async fn git_output(repo: Option<&Path>, args: &[&str], options: &SourceOptions) -> Result<String> {
    let mut command = Command::new("git");
    command.kill_on_drop(true);
    if let Some(repo) = repo {
        command.arg("-C").arg(repo);
    }
    command.args(args);
    scrub_git_process_variables(&mut command);
    let output = tokio::time::timeout(options.git_timeout(), command.output())
        .await
        .map_err(|_| RototoError::new(format!("git {} timed out", args.join(" "))))?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                RototoError::new("required tool `git` was not found on PATH")
            } else {
                RototoError::new(format!("failed to run git: {err}"))
            }
        })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(RototoError::new(format!(
        "git {} failed: {}",
        args.join(" "),
        if stderr.is_empty() {
            output.status.to_string()
        } else {
            stderr
        }
    )))
}

fn scrub_git_process_variables(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    for key in [
        "GIT_INDEX_FILE",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::super::source_tree;
    use super::*;
    use crate::console::stage::{CachedTreeSource, TokenIdentity};
    use tempfile::TempDir;

    #[tokio::test]
    async fn branch_changes_map_files_to_root_and_nested_workspaces() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_workspace(repo.path()).await;
        write_workspace(&repo.path().join("workspaces/payments")).await;
        write_workspace(&repo.path().join("workspaces/search")).await;
        commit_all(repo.path(), "add workspaces");
        run_git(repo.path(), &["checkout", "-b", "feature/payments"]);
        tokio::fs::write(
            repo.path()
                .join("workspaces/payments/variables/checkout.toml"),
            "schema_version = 1\n",
        )
        .await
        .unwrap();
        commit_all(repo.path(), "change payments");

        let cached_tree = source_key(TreeSource::GitRemote {
            remote_url: format!("git+file://{}", repo.path().display()),
        });
        let branch = BranchName::new("feature/payments").unwrap();
        let staged_tree = source_tree::stage_tree_for_revision(
            cached_tree.clone(),
            revision_for_changes(&cached_tree.tree, &branch).unwrap(),
        )
        .await
        .unwrap();

        let changes = get_branch_changes(
            staged_tree.root(),
            source_for_changes(&cached_tree.tree).unwrap(),
            branch,
            GitRefName::new("main").unwrap(),
            &[
                WorkspacePath::root(),
                WorkspacePath::new("workspaces/payments").unwrap(),
                WorkspacePath::new("workspaces/search").unwrap(),
            ],
        )
        .await
        .unwrap();

        assert_eq!(
            path_strings(&changes.changed_files),
            vec!["workspaces/payments/variables/checkout.toml"]
        );
        assert_eq!(
            workspace_strings(&changes.affected_workspaces),
            vec![".", "workspaces/payments"]
        );
    }

    #[tokio::test]
    async fn local_branch_changes_include_working_tree_status() {
        let repo = TempDir::new().expect("repo tempdir");
        init_repo(repo.path());
        write_workspace(repo.path()).await;
        commit_all(repo.path(), "add workspace");
        run_git(repo.path(), &["checkout", "-b", "feature/local"]);
        tokio::fs::write(
            repo.path().join("variables/checkout.toml"),
            "schema_version = 1\n",
        )
        .await
        .unwrap();
        tokio::fs::write(
            repo.path().join("variables/new.toml"),
            "schema_version = 1\n",
        )
        .await
        .unwrap();

        let cached_tree = source_key(TreeSource::local_folder(repo.path()).await.unwrap());
        let branch = BranchName::new("feature/local").unwrap();
        let staged_tree = source_tree::stage_tree_for_revision(
            cached_tree.clone(),
            revision_for_changes(&cached_tree.tree, &branch).unwrap(),
        )
        .await
        .unwrap();
        let changes = get_branch_changes(
            staged_tree.root(),
            source_for_changes(&cached_tree.tree).unwrap(),
            branch,
            GitRefName::new("main").unwrap(),
            &[WorkspacePath::root()],
        )
        .await
        .unwrap();

        assert_eq!(
            path_strings(&changes.changed_files),
            vec!["variables/checkout.toml", "variables/new.toml"]
        );
        assert_eq!(workspace_strings(&changes.affected_workspaces), vec!["."]);
    }

    #[tokio::test]
    async fn archive_sources_do_not_have_branch_changes() {
        let err =
            source_for_changes(&TreeSource::archive("https://example.com/configs.tar.gz").unwrap())
                .unwrap_err();

        assert!(err.to_string().contains("git-backed source tree"));
    }

    async fn write_workspace(path: &Path) {
        tokio::fs::create_dir_all(path.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(path.join(WORKSPACE_MANIFEST), "schema_version = 1\n")
            .await
            .unwrap();
    }

    fn source_key(source: TreeSource) -> CachedTreeSource {
        CachedTreeSource::new("user_123", source, TokenIdentity::none()).unwrap()
    }

    fn path_strings(paths: &[RepoRelativePath]) -> Vec<&str> {
        paths.iter().map(RepoRelativePath::as_str).collect()
    }

    fn workspace_strings(workspaces: &[WorkspacePath]) -> Vec<&str> {
        workspaces.iter().map(WorkspacePath::as_str).collect()
    }

    fn init_repo(root: &Path) {
        run_git(root, &["init", "-b", "main"]);
        run_git(root, &["config", "user.email", "rototo@example.com"]);
        run_git(root, &["config", "user.name", "Rototo Test"]);
    }

    fn commit_all(root: &Path, message: &str) {
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-m", message]);
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap_or_else(|err| panic!("failed to run git {}: {err}", args.join(" ")));
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
