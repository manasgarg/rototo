use crate::error::{Result, RototoError};
use crate::source::{SourceOptions, StagedSourceTree, stage_source_tree};

use super::{CachedSourceTreeOrigin, SourceTreeOrigin, SourceTreeRevision};

pub async fn stage_tree_for_revision(
    cached_tree: CachedSourceTreeOrigin,
    revision: SourceTreeRevision,
) -> Result<StagedSourceTree> {
    let source = source_for_revision(&cached_tree.origin, &revision)?;
    stage_source_tree(source, &SourceOptions::default()).await
}

fn source_for_revision(tree: &SourceTreeOrigin, revision: &SourceTreeRevision) -> Result<String> {
    match tree {
        SourceTreeOrigin::LocalFolder { root }
            if matches!(revision, SourceTreeRevision::LocalWorkingTree) =>
        {
            Ok(root.to_string_lossy().into_owned())
        }
        SourceTreeOrigin::GitHub { owner, name } => {
            let Some(git_ref) = git_ref_for_revision(revision) else {
                return Err(invalid_selection_error());
            };
            Ok(format!(
                "git+https://github.com/{owner}/{name}.git#{git_ref}"
            ))
        }
        SourceTreeOrigin::GitRemote { remote_url } => {
            let Some(git_ref) = git_ref_for_revision(revision) else {
                return Err(invalid_selection_error());
            };
            Ok(format!("{remote_url}#{git_ref}"))
        }
        _ => Err(invalid_selection_error()),
    }
}

fn git_ref_for_revision(revision: &SourceTreeRevision) -> Option<&str> {
    match revision {
        SourceTreeRevision::GitRef(ref_) => Some(ref_.as_ref()),
        SourceTreeRevision::GitBranch(branch) => Some(branch.as_ref()),
        SourceTreeRevision::GitCommit(commit) => Some(commit.as_ref()),
        SourceTreeRevision::LocalWorkingTree => None,
    }
}

fn invalid_selection_error() -> RototoError {
    RototoError::new("tree revision is not valid for source tree staging")
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::console::stage::{GitRefName, TokenIdentity};

    #[tokio::test]
    async fn stages_local_working_tree_root() {
        let tree = TempDir::new().expect("tree tempdir");
        tokio::fs::write(
            tree.path().join("rototo-workspace.toml"),
            "schema_version = 1\n",
        )
        .await
        .unwrap();

        let staged = stage_tree_for_revision(
            CachedSourceTreeOrigin::new(
                "user_123",
                SourceTreeOrigin::local_folder(tree.path()).await.unwrap(),
                TokenIdentity::None,
            )
            .unwrap(),
            SourceTreeRevision::LocalWorkingTree,
        )
        .await
        .unwrap();

        assert!(staged.root().join("rototo-workspace.toml").is_file());
    }

    #[tokio::test]
    async fn rejects_revision_that_does_not_match_source_tree_origin() {
        let tree = TempDir::new().expect("tree tempdir");
        let err = stage_tree_for_revision(
            CachedSourceTreeOrigin::new(
                "user_123",
                SourceTreeOrigin::local_folder(tree.path()).await.unwrap(),
                TokenIdentity::None,
            )
            .unwrap(),
            SourceTreeRevision::GitRef(GitRefName::new("main").unwrap()),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("revision is not valid"));
    }
}
