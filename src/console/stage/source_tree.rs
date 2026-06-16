use crate::error::{Result, RototoError};
use crate::source::{SourceOptions, StagedSourceTree, stage_source_tree};

use super::{CachedTreeSource, TreeRevision, TreeSource};

pub async fn stage_tree_for_revision(
    cached_tree: CachedTreeSource,
    revision: TreeRevision,
) -> Result<StagedSourceTree> {
    let source = source_for_revision(&cached_tree.tree, &revision)?;
    stage_source_tree(source, &SourceOptions::default()).await
}

fn source_for_revision(tree: &TreeSource, revision: &TreeRevision) -> Result<String> {
    match tree {
        TreeSource::LocalFolder { root } if matches!(revision, TreeRevision::LocalWorkingTree) => {
            Ok(root.to_string_lossy().into_owned())
        }
        TreeSource::GitHub { owner, name } => {
            let Some(git_ref) = git_ref_for_revision(revision) else {
                return Err(invalid_selection_error());
            };
            Ok(format!(
                "git+https://github.com/{owner}/{name}.git#{git_ref}"
            ))
        }
        TreeSource::GitRemote { remote_url } => {
            let Some(git_ref) = git_ref_for_revision(revision) else {
                return Err(invalid_selection_error());
            };
            Ok(format!("{remote_url}#{git_ref}"))
        }
        TreeSource::Archive { .. } if matches!(revision, TreeRevision::ArchiveSnapshot(_)) => Err(
            RototoError::new("archive source tree staging is not yet supported"),
        ),
        _ => Err(invalid_selection_error()),
    }
}

fn git_ref_for_revision(revision: &TreeRevision) -> Option<&str> {
    match revision {
        TreeRevision::GitRef(ref_) => Some(ref_.as_ref()),
        TreeRevision::GitBranch(branch) => Some(branch.as_ref()),
        TreeRevision::GitCommit(commit) => Some(commit.as_ref()),
        TreeRevision::LocalWorkingTree | TreeRevision::ArchiveSnapshot(_) => None,
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
            CachedTreeSource::new(
                "user_123",
                TreeSource::local_folder(tree.path()).await.unwrap(),
                TokenIdentity::none(),
            )
            .unwrap(),
            TreeRevision::LocalWorkingTree,
        )
        .await
        .unwrap();

        assert!(staged.root().join("rototo-workspace.toml").is_file());
    }

    #[tokio::test]
    async fn rejects_revision_that_does_not_match_tree_source() {
        let tree = TempDir::new().expect("tree tempdir");
        let err = stage_tree_for_revision(
            CachedTreeSource::new(
                "user_123",
                TreeSource::local_folder(tree.path()).await.unwrap(),
                TokenIdentity::none(),
            )
            .unwrap(),
            TreeRevision::GitRef(GitRefName::new("main").unwrap()),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("revision is not valid"));
    }
}
