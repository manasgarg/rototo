use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use rototo::Workspace;

#[test]
fn fixtures_command_generates_readable_toml_suite() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("fixtures");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "fixtures",
            "examples/basic",
            "--variable",
            "max-output-tokens",
            "--qualifier",
            "premium-users",
            "--qualifier",
            "beta-rollout-bucket",
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("rototo-fixtures.toml"))
        .stdout(predicate::str::contains("variables/max-output-tokens.toml"))
        .stdout(predicate::str::contains("qualifiers/premium-users.toml"));

    let manifest = fs::read_to_string(out.join("rototo-fixtures.toml")).unwrap();
    assert!(manifest.contains("target = \"variable:max-output-tokens\""));
    assert!(manifest.contains("target = \"qualifier:premium-users\""));

    let variable = fs::read_to_string(out.join("variables/max-output-tokens.toml")).unwrap();
    assert!(variable.contains("title = \"Uses the default value when no rule matches\""));
    assert!(variable.contains("matched = \"default\""));
    assert!(variable.contains("matched_rule = 2"));
    assert!(variable.contains("matched_qualifier = \"enterprise-accounts\""));

    let bucket = fs::read_to_string(out.join("qualifiers/beta-rollout-bucket.toml")).unwrap();
    assert!(bucket.contains("[[case.expect.bucket]]"));
    assert!(bucket.contains("id = \"false-outside-user-id-bucket\""));
}

#[test]
fn fixtures_command_defaults_to_whole_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("fixtures");

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["fixtures", "examples/basic", "--out", out.to_str().unwrap()])
        .assert()
        .success();

    let manifest = fs::read_to_string(out.join("rototo-fixtures.toml")).unwrap();
    assert!(manifest.contains("target = \"qualifier:premium-users\""));
    assert!(manifest.contains("target = \"variable:max-output-tokens\""));
}

#[tokio::test]
async fn testing_helper_asserts_generated_fixtures() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("fixtures");

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "fixtures",
            "examples/basic",
            "--variable",
            "max-output-tokens",
            "--qualifier",
            "premium-users",
            "--qualifier",
            "beta-rollout-bucket",
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let workspace = Workspace::load("examples/basic").await.unwrap();
    let report = rototo::testing::assert_fixtures(&workspace, &out)
        .await
        .unwrap();
    assert_eq!(report.cases, 8);
}
