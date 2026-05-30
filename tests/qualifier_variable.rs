use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn lists_qualifiers() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["qualifier", "list", "--workspace", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("admin-users"))
        .stdout(predicate::str::contains("premium-users"))
        .stdout(predicate::str::contains("premium-beta-users"));
}

#[test]
fn lists_variables_from_discovered_workspace() {
    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .args(["variable", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("checkout-redesign"))
        .stdout(predicate::str::contains("llm-agent-config"))
        .stdout(predicate::str::contains("tenant-limits"))
        .stdout(predicate::str::contains("user-is-admin"));
}

#[test]
fn lists_qualifiers_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "qualifier",
            "list",
            "--workspace",
            "examples/basic",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""qualifiers": ["#))
        .stdout(predicate::str::contains(
            r#""uri": "qualifier://premium-users""#,
        ));
}

#[test]
fn gets_qualifier_by_id() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "qualifier",
            "get",
            "premium-users",
            "--workspace",
            "examples/basic",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "description = \"Users on the premium plan\"",
        ))
        .stdout(predicate::str::contains("attribute = \"user.tier\""));
}

#[test]
fn gets_variable_from_discovered_workspace() {
    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .args(["variable", "get", "user-is-admin"])
        .assert()
        .success()
        .stdout(predicate::str::contains("type = \"bool\""));
}

#[test]
fn gets_qualifier_by_id_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "--json",
            "qualifier",
            "get",
            "premium-users",
            "--workspace",
            "examples/basic",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "premium-users""#))
        .stdout(predicate::str::contains(
            r#""uri": "qualifier://premium-users""#,
        ))
        .stdout(predicate::str::contains(r#""attribute": "user.tier""#));
}

#[test]
fn lints_qualifier_by_id() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "qualifier",
            "lint",
            "premium-users",
            "--workspace",
            "examples/basic",
        ])
        .assert()
        .success()
        .stdout(predicate::eq("ok: qualifier://premium-users\n"));
}

#[test]
fn resolves_qualifier_by_id() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "qualifier",
            "resolve",
            "premium-users",
            "--workspace",
            "examples/basic",
            "--context",
            r#"{"user":{"tier":"premium","id":"a=b"}}"#,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "premium-users""#))
        .stdout(predicate::str::contains(r#""value": true"#));
}

#[test]
fn resolves_all_qualifiers() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "qualifier",
            "resolve-all",
            "--workspace",
            "examples/basic",
            "--context",
            r#"{"user":{"tier":"premium","id":"user-123"},"account":{"plan":"enterprise","seats":250},"request":{"country":"DE"}}"#,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "premium-users""#))
        .stdout(predicate::str::contains(r#""id": "enterprise-accounts""#))
        .stdout(predicate::str::contains(r#""id": "eu-premium-users""#));
}

#[test]
fn lints_variable_from_discovered_workspace() {
    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .args(["variable", "lint", "checkout-redesign"])
        .assert()
        .success()
        .stdout(predicate::eq("ok: variable://checkout-redesign\n"));
}

#[test]
fn resolves_variable_by_id() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "variable",
            "resolve",
            "checkout-redesign",
            "--workspace",
            "examples/basic",
            "--env",
            "prod",
            "--context",
            r#"{"user":{"tier":"premium"}}"#,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "checkout-redesign""#))
        .stdout(predicate::str::contains(r#""value_key": "premium""#))
        .stdout(predicate::str::contains(r#""variant": "premium""#));
}

#[test]
fn resolves_all_variables() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "variable",
            "resolve-all",
            "--workspace",
            "examples/basic",
            "--env",
            "prod",
            "--context",
            "@examples/basic/contexts/premium-enterprise.json",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "checkout-redesign""#))
        .stdout(predicate::str::contains(r#""id": "admin-navigation""#))
        .stdout(predicate::str::contains(r#""value_key": "enterprise""#));
}

#[test]
fn resolves_variable_with_context_assignments() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "variable",
            "resolve",
            "checkout-redesign",
            "--workspace",
            "examples/basic",
            "--env",
            "prod",
            "--context",
            "user.tier=free",
            "--context",
            "user.tier=premium",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""value_key": "premium""#));
}

#[test]
fn resolve_rejects_context_that_does_not_match_workspace_schema() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "qualifier",
            "resolve",
            "premium-users",
            "--workspace",
            "examples/basic",
            "--context",
            r#"{"unknown":true}"#,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "resolve context does not match schema",
        ));
}

#[test]
fn resolve_rejects_unknown_environment_before_fallback() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "variable",
            "resolve",
            "checkout-redesign",
            "--workspace",
            "examples/basic",
            "--env",
            "prd",
            "--context",
            r#"{"user":{"tier":"premium"}}"#,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown environment: prd"));
}

#[test]
fn missing_qualifier_id_fails() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "qualifier",
            "get",
            "missing",
            "--workspace",
            "examples/basic",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "qualifier not found: qualifier://missing",
        ));
}

#[test]
fn missing_workspace_context_fails() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["qualifier", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "workspace not found: pass --workspace or run inside a rototo workspace",
        ));
}
