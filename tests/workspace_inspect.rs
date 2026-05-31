use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn inspects_basic_workspace() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("workspace: "))
        .stdout(predicate::str::contains("environments:"))
        .stdout(predicate::str::contains("qualifiers:"))
        .stdout(predicate::str::contains(
            "qualifier://premium-users  qualifiers/premium-users.toml",
        ))
        .stdout(predicate::str::contains(
            "qualifier://premium-beta-users  qualifiers/premium-beta-users.toml",
        ))
        .stdout(predicate::str::contains("variables:"))
        .stdout(predicate::str::contains(
            "variable://checkout-redesign  variables/checkout-redesign.toml",
        ))
        .stdout(predicate::str::contains(
            "variable://tenant-limits  variables/tenant-limits.toml",
        ));
}

#[test]
fn inspects_discovered_workspace() {
    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .arg("inspect")
        .assert()
        .success()
        .stdout(predicate::str::contains("workspace: "))
        .stdout(predicate::str::contains("qualifier://premium-users"));
}

#[test]
fn inspects_basic_workspace_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["--json", "inspect", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""environments": ["#))
        .stdout(predicate::str::contains(r#""qualifiers": ["#))
        .stdout(predicate::str::contains(r#""variables": ["#))
        .stdout(predicate::str::contains(
            r#""uri": "qualifier://premium-users""#,
        ))
        .stdout(predicate::str::contains(
            r#""uri": "variable://checkout-redesign""#,
        ));
}

#[test]
fn json_is_a_trailing_global_arg() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", "examples/basic", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""uri": "qualifier://premium-users""#,
        ));
}
