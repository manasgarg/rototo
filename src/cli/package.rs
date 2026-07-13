use std::path::Path;
use std::process::ExitCode;

use serde::Serialize;

use rototo::{PackagedArchive, Result, RototoError, SourceOptions, pack_package, project_package};

use crate::PackageArgs;
use crate::package_source_string_or_current;
use crate::style;

pub(crate) async fn run_package(
    args: PackageArgs,
    source_options: &SourceOptions,
    json: bool,
    quiet: bool,
) -> Result<ExitCode> {
    let source = package_source_string_or_current(args.package).await?;

    if let Some(target) = &args.unpacked {
        let written = project_package(&source, source_options, target).await?;

        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&UnpackedOutput {
                    package: &source,
                    directory: &target.display().to_string(),
                    files: written.len(),
                })
                .map_err(|err| RototoError::new(err.to_string()))?
            );
            return Ok(ExitCode::SUCCESS);
        }

        if quiet {
            println!("{}", target.display());
            return Ok(ExitCode::SUCCESS);
        }

        println!("{} {}", style::label("package"), style::bold(&source));
        println!(
            "{} {}  {}",
            style::label("unpacked"),
            style::bold(&target.display().to_string()),
            style::dim(&format!("{} files", written.len()))
        );
        return Ok(ExitCode::SUCCESS);
    }

    let archive = pack_package(&source, source_options).await?;
    let path = write_archive(&args.output, &archive).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&PackageOutput {
                package: &source,
                release_id: &archive.release_id,
                archive: &path.display().to_string(),
                bytes: archive.bytes.len(),
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(ExitCode::SUCCESS);
    }

    if quiet {
        println!("{}", path.display());
        return Ok(ExitCode::SUCCESS);
    }

    println!("{} {}", style::label("package"), style::bold(&source));
    println!(
        "{} {}",
        style::label("release"),
        style::sea_bold(&archive.release_id)
    );
    println!(
        "{} {}  {}",
        style::label("archive"),
        style::bold(&path.display().to_string()),
        style::dim(&format!("{} bytes", archive.bytes.len()))
    );
    Ok(ExitCode::SUCCESS)
}

async fn write_archive(output: &Path, archive: &PackagedArchive) -> Result<std::path::PathBuf> {
    tokio::fs::create_dir_all(output).await.map_err(|err| {
        RototoError::new(format!(
            "failed to create output directory {}: {err}",
            output.display()
        ))
    })?;
    let path = output.join(&archive.file_name);
    tokio::fs::write(&path, &archive.bytes)
        .await
        .map_err(|err| {
            RototoError::new(format!(
                "failed to write package archive {}: {err}",
                path.display()
            ))
        })?;
    Ok(path)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnpackedOutput<'a> {
    package: &'a str,
    directory: &'a str,
    files: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageOutput<'a> {
    package: &'a str,
    release_id: &'a str,
    archive: &'a str,
    bytes: usize,
}
