//! Pin-keyed staging: the read-cache substrate from the console git-ops
//! design. Entries are keyed by commit, so they can never go stale; the
//! only reason an entry leaves the store is the size budget.

use std::fs;
use std::path::Path;
use std::process::Command;

use rototo::{PinStore, SourceOptions};

#[tokio::test]
async fn stages_a_pin_once_and_reuses_it_from_disk() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let first = commit_file(&repo, "variables/flag.toml", "schema_version = 1\n");
    let second = commit_file(&repo, "variables/flag.toml", "schema_version = 1\n# v2\n");
    assert_ne!(first, second);

    let store = PinStore::new(temp.path().join("pins"), u64::MAX);
    let options = SourceOptions::new();
    let remote = repo.to_str().unwrap();

    // An old pin stays fetchable even though the branch moved past it.
    let old_tree = store.stage(remote, &first, &options).await.unwrap();
    let content = fs::read_to_string(old_tree.join("variables/flag.toml")).unwrap();
    assert!(!content.contains("# v2"));
    assert!(
        !old_tree.join(".git").exists(),
        "staged trees are plain files"
    );

    let new_tree = store.stage(remote, &second, &options).await.unwrap();
    let content = fs::read_to_string(new_tree.join("variables/flag.toml")).unwrap();
    assert!(content.contains("# v2"));

    // Staging the same pin again is a disk read, not a rebuild: even with
    // the remote gone, the entry answers.
    fs::remove_dir_all(&repo).unwrap();
    let cached = store.stage(remote, &second, &options).await.unwrap();
    assert_eq!(cached, new_tree);
    assert!(cached.join("variables/flag.toml").exists());
}

#[tokio::test]
async fn refuses_anything_that_is_not_a_full_commit_sha() {
    let temp = tempfile::TempDir::new().unwrap();
    let store = PinStore::new(temp.path().join("pins"), u64::MAX);
    let err = store
        .stage(
            "https://example.com/repo.git",
            "main",
            &SourceOptions::new(),
        )
        .await
        .expect_err("branch names are not pins")
        .to_string();
    assert!(err.contains("not a full commit SHA"), "{err}");
}

#[tokio::test]
async fn evicts_the_least_recently_used_entry_past_the_budget() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let first = commit_file(&repo, "a.toml", "a = 1\n");
    let second = commit_file(&repo, "b.toml", "b = 2\n");

    let pins = temp.path().join("pins");
    let store = PinStore::new(&pins, 1);
    let options = SourceOptions::new();
    let remote = repo.to_str().unwrap();

    let old_tree = store.stage(remote, &first, &options).await.unwrap();
    // Age the first entry past the eviction grace window.
    let used = old_tree.parent().unwrap().join("used");
    let touch = Command::new("touch")
        .args(["-m", "-t", "202001010000"])
        .arg(&used)
        .status()
        .unwrap();
    assert!(touch.success());

    let new_tree = store.stage(remote, &second, &options).await.unwrap();
    assert!(new_tree.exists(), "the just-staged entry survives");
    assert!(
        !old_tree.exists(),
        "the aged entry is evicted once the store exceeds its budget"
    );
}

/// Writes a file and commits it, returning the commit SHA.
fn commit_file(repo: &Path, path: &str, content: &str) -> String {
    if !repo.join(".git").exists() {
        fs::create_dir_all(repo).unwrap();
        git(repo, &["init", "--quiet"]);
        git(repo, &["config", "user.email", "rototo@example.com"]);
        git(repo, &["config", "user.name", "Rototo Test"]);
        // Let the shallow-by-SHA fast path work against this local remote,
        // the way GitHub allows fetching reachable commits by SHA.
        git(
            repo,
            &["config", "uploadpack.allowReachableSHA1InWant", "true"],
        );
    }
    let file = repo.join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&file, content).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "--quiet", "-m", "change"]);
    let output = Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repo)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}
