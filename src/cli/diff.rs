use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tempfile::TempDir;

use rototo::{Result, RototoError, SourceOptions, StagedPackage, diff_packages, find_package_root};

use crate::output::print_package_diff;
use crate::{DiffArgs, package_source_for_lint, parse_context};

struct DiffPackageSide {
    package: StagedPackage,
    label: String,
    _snapshot: Option<TempDir>,
}

pub(crate) async fn run_diff(
    args: DiffArgs,
    source_options: &SourceOptions,
    json: bool,
) -> Result<ExitCode> {
    let package_root = local_diff_package_path(args.package).await?;
    let git = local_git_context(&package_root, source_options).await?;
    let from_ref = args.from.unwrap_or_else(|| "HEAD".to_owned());
    validate_diff_git_ref(&from_ref)?;
    let before = stage_git_ref_diff_side(&git, &from_ref, source_options).await?;
    let after = match args.to {
        Some(to_ref) => {
            validate_diff_git_ref(&to_ref)?;
            stage_git_ref_diff_side(&git, &to_ref, source_options).await?
        }
        None => stage_worktree_diff_side(&git, source_options).await?,
    };
    let context = if args.context.is_empty() {
        None
    } else {
        Some(parse_context(&args.context).await?)
    };
    let mut diff = diff_packages(
        before.package.path(),
        after.package.path(),
        context.as_ref(),
    )
    .await?;
    diff.before = before.label;
    diff.after = after.label;
    print_package_diff(&diff, json)?;
    Ok(ExitCode::SUCCESS)
}

struct LocalGitContext {
    repo_root: PathBuf,
    package_root: PathBuf,
    package_relative: PathBuf,
    package_label: String,
}

async fn local_diff_package_path(package: Option<String>) -> Result<PathBuf> {
    match package {
        Some(package) => {
            if package.contains("://") || package.starts_with("git+") {
                return Err(RototoError::new(
                    "diff currently requires a local package path; use --from and --to with local Git refs",
                ));
            }
            tokio::fs::canonicalize(&package)
                .await
                .map_err(|err| RototoError::new(format!("failed to resolve package path: {err}")))
        }
        None => {
            let current_dir = tokio::task::spawn_blocking(std::env::current_dir)
                .await
                .map_err(|err| RototoError::new(format!("current directory task failed: {err}")))?
                .map_err(|err| {
                    RototoError::new(format!("failed to read current directory: {err}"))
                })?;
            find_package_root(&current_dir).await
        }
    }
}

async fn local_git_context(
    package_root: &Path,
    source_options: &SourceOptions,
) -> Result<LocalGitContext> {
    let repo_root = git_stdout(
        package_root,
        &["rev-parse", "--show-toplevel"],
        source_options,
        "find repository root",
    )
    .await?;
    let repo_root = PathBuf::from(String::from_utf8_lossy(&repo_root).trim());
    let repo_root = tokio::fs::canonicalize(&repo_root).await.map_err(|err| {
        RototoError::new(format!(
            "failed to resolve Git repository root {}: {err}",
            repo_root.display()
        ))
    })?;
    let package_root = tokio::fs::canonicalize(package_root).await.map_err(|err| {
        RototoError::new(format!(
            "failed to resolve package path {}: {err}",
            package_root.display()
        ))
    })?;
    let package_relative = package_root.strip_prefix(&repo_root).map_err(|_| {
        RototoError::new(format!(
            "package {} is not inside Git repository {}",
            package_root.display(),
            repo_root.display()
        ))
    })?;
    let package_relative = package_relative.to_path_buf();
    let package_label = git_path_label(&package_relative);
    Ok(LocalGitContext {
        repo_root,
        package_root,
        package_relative,
        package_label,
    })
}

async fn stage_git_ref_diff_side(
    git: &LocalGitContext,
    ref_: &str,
    source_options: &SourceOptions,
) -> Result<DiffPackageSide> {
    let tempdir = TempDir::new()
        .map_err(|err| RototoError::new(format!("failed to create tempdir: {err}")))?;
    let extract_root = tempdir.path().join("repo");
    tokio::fs::create_dir_all(&extract_root)
        .await
        .map_err(|err| {
            RototoError::new(format!(
                "failed to create Git snapshot directory {}: {err}",
                extract_root.display()
            ))
        })?;

    let mut args = vec![
        "archive".to_owned(),
        "--format=tar".to_owned(),
        ref_.to_owned(),
        "--".to_owned(),
    ];
    if !git.package_relative.as_os_str().is_empty() {
        args.push(git_path_label(&git.package_relative));
    }
    let archive = git_stdout_owned(&git.repo_root, &args, source_options, "archive ref").await?;
    let extract_root_for_task = extract_root.clone();
    tokio::task::spawn_blocking(move || {
        let mut archive = tar::Archive::new(Cursor::new(archive));
        archive.unpack(&extract_root_for_task).map_err(|err| {
            RototoError::new(format!(
                "failed to extract Git snapshot {}: {err}",
                extract_root_for_task.display()
            ))
        })
    })
    .await
    .map_err(|err| RototoError::new(format!("Git snapshot extraction task failed: {err}")))??;

    let package_root = if git.package_relative.as_os_str().is_empty() {
        extract_root
    } else {
        extract_root.join(&git.package_relative)
    };
    if !tokio::fs::metadata(package_root.join("rototo-package.toml"))
        .await
        .is_ok_and(|metadata| metadata.is_file())
    {
        return Err(RototoError::new(format!(
            "package {} was not found at Git ref {ref_}",
            git.package_label
        )));
    }
    let label = format!("{ref_}:{}", git.package_label);
    stage_diff_side_from_path(package_root, label, Some(tempdir), source_options).await
}

async fn stage_worktree_diff_side(
    git: &LocalGitContext,
    source_options: &SourceOptions,
) -> Result<DiffPackageSide> {
    stage_diff_side_from_path(
        git.package_root.clone(),
        format!("worktree:{}", git.package_label),
        None,
        source_options,
    )
    .await
}

async fn stage_diff_side_from_path(
    package_root: PathBuf,
    label: String,
    snapshot: Option<TempDir>,
    source_options: &SourceOptions,
) -> Result<DiffPackageSide> {
    let package = package_source_for_lint(
        Some(package_root.to_string_lossy().into_owned()),
        source_options,
    )
    .await?;
    Ok(DiffPackageSide {
        package,
        label,
        _snapshot: snapshot,
    })
}

async fn git_stdout(
    current_dir: &Path,
    args: &[&str],
    source_options: &SourceOptions,
    operation: &str,
) -> Result<Vec<u8>> {
    let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    git_stdout_owned(current_dir, &args, source_options, operation).await
}

async fn git_stdout_owned(
    current_dir: &Path,
    args: &[String],
    source_options: &SourceOptions,
    operation: &str,
) -> Result<Vec<u8>> {
    let mut command = tokio::process::Command::new("git");
    command.kill_on_drop(true);
    command.current_dir(current_dir);
    for arg in args {
        command.arg(arg);
    }
    scrub_git_process_variables(&mut command);
    let output = tokio::time::timeout(source_options.git_timeout(), command.output())
        .await
        .map_err(|_| RototoError::new(format!("git {operation} timed out")))?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                RototoError::new("required tool `git` was not found on PATH")
            } else {
                RototoError::new(format!("failed to run git: {err}"))
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RototoError::new(format!(
            "git {operation} failed: {}",
            stderr.trim()
        )));
    }
    Ok(output.stdout)
}

fn validate_diff_git_ref(ref_: &str) -> Result<()> {
    if ref_.starts_with('-') {
        return Err(RototoError::new(format!(
            "diff git ref must not begin with '-': {ref_}"
        )));
    }
    Ok(())
}

fn git_path_label(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

fn scrub_git_process_variables(command: &mut tokio::process::Command) {
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
