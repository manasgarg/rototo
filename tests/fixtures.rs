use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;

#[test]
fn fixtures_command_prints_resolve_commands() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "fixtures",
            "examples/basic",
            "--variable",
            "user-is-admin",
            "--variable",
            "premium-users",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "rototo resolve examples/basic --variable user-is-admin",
        ))
        .stdout(predicate::str::contains(
            "rototo resolve examples/basic --variable premium-users --context",
        ))
        .stdout(predicate::str::contains("# =>"));
}

#[test]
fn fixtures_command_defaults_to_whole_package() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["fixtures", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--variable user-is-admin"))
        .stdout(predicate::str::contains("--variable premium-users"));
}

#[test]
fn fixtures_command_renders_json_context_form() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "fixtures",
            "examples/basic",
            "--variable",
            "premium-users",
            "--context-form",
            "json",
        ])
        .assert()
        .success()
        // A single JSON object argument, shell-quoted.
        .stdout(predicate::str::contains("--context '{"));
}

#[test]
fn fixtures_command_json_output_describes_invocations() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "fixtures",
            "examples/basic",
            "--variable",
            "user-is-admin",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"target\": \"variable:user-is-admin\"",
        ))
        .stdout(predicate::str::contains("\"command\":"))
        .stdout(predicate::str::contains("\"expect\":"))
        .stdout(predicate::str::contains("\"kind\":"));
}

/// The printed commands must be runnable: feeding one back through a real shell
/// (which undoes the shell-quoting) and the real resolve `--context` parser must
/// succeed, proving rendering is the inverse of parsing.
#[test]
fn printed_resolve_command_runs_end_to_end() {
    let output = Command::cargo_bin("rototo")
        .unwrap()
        .env("NO_COLOR", "1")
        .args(["fixtures", "examples/basic", "--variable", "user-is-admin"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Pick a command line that carries a context, stripping the trailing comment.
    let line = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with("rototo resolve"))
        .find(|line| line.contains("--context"))
        .expect("expected a resolve command with a context");
    let command = line.split("  #").next().unwrap().trim();

    let bin = cargo_bin("rototo");
    let bin_dir = bin.parent().unwrap();
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "command failed: {command}\nstderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}
