use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn lists_qualifiers() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--qualifiers"])
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
        .args(["show", "--variables"])
        .assert()
        .success()
        .stdout(predicate::str::contains("checkout-redesign"))
        .stdout(predicate::str::contains("llm-agent-config"))
        .stdout(predicate::str::contains("tenant-limits"))
        .stdout(predicate::str::contains("user-is-admin"));
}

#[test]
fn shows_workspace_inventory_including_linters() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("qualifiers:"))
        .stdout(predicate::str::contains("variables:"))
        .stdout(predicate::str::contains("schemas:"))
        .stdout(predicate::str::contains(
            "checkout-page.schema  schemas/checkout-page.schema.json",
        ))
        .stdout(predicate::str::contains("lint authorities:"))
        .stdout(predicate::str::contains(
            "consumer-experience/checkout-heading-required",
        ))
        .stdout(predicate::str::contains("linters:"))
        .stdout(predicate::str::contains(
            "checkout-redesign  lint/checkout-redesign.lua",
        ))
        .stdout(predicate::str::contains(
            "directory-backed-message  lint/directory-backed-message.lua",
        ));
}

#[test]
fn shows_workspace_inventory_as_json_including_top_level_objects() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""environments": ["#))
        .stdout(predicate::str::contains(r#""schemas": ["#))
        .stdout(predicate::str::contains(
            r#""path": "schemas/context.schema.json""#,
        ))
        .stdout(predicate::str::contains(r#""qualifiers": ["#))
        .stdout(predicate::str::contains(r#""variables": ["#))
        .stdout(predicate::str::contains(r#""lint_authorities": ["#))
        .stdout(predicate::str::contains(
            r#""authority": "consumer-experience""#,
        ))
        .stdout(predicate::str::contains(r#""linters": ["#));
}

#[test]
fn lists_qualifiers_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--qualifiers", "--json"])
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
        .args(["show", "examples/basic", "--qualifier", "premium-users"])
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
        .args(["show", "--variable", "user-is-admin"])
        .assert()
        .success()
        .stdout(predicate::str::contains("type = \"bool\""));
}

#[test]
fn gets_directory_backed_variable_with_expanded_values() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--variable", "llm-agent-config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[values.local]"))
        .stdout(predicate::str::contains("model = \"local-small\""))
        .stdout(predicate::str::contains("value = \"enterprise\""));
}

#[test]
fn gets_qualifier_by_id_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "--json",
            "show",
            "examples/basic",
            "--qualifier",
            "premium-users",
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
    let workspace = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", workspace.display());

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", "examples/basic", "--qualifier", "premium-users"])
        .assert()
        .success()
        .stdout(predicate::eq(expected));
}

#[test]
fn resolves_qualifier_by_id() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--qualifier",
            "premium-users",
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
            "resolve",
            "examples/basic",
            "--qualifiers",
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
fn resolves_qualifier_with_trace_output() {
    let assert = Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--qualifier",
            "premium-users",
            "--context",
            "user.tier=premium",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(stdout.contains("qualifier: premium-users"));
    assert!(stdout.contains(r#"[0] context user.tier = "premium""#));
    assert!(stdout.contains(r#"test: eq "premium""#));
    assert!(stdout.contains("matched: true"));
    assert!(stdout.contains("result: true"));
    assert!(!stdout.contains("premium-users=true"));
    assert!(stdout.find("predicates:").unwrap() < stdout.find("result: true").unwrap());
}

#[test]
fn lints_variable_from_discovered_workspace() {
    let workspace = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", workspace.display());

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .args(["lint", "--variable", "checkout-redesign"])
        .assert()
        .success()
        .stdout(predicate::eq(expected));
}

#[test]
fn resolves_variable_by_id() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "checkout-redesign",
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
fn resolves_production_example_enterprise_profile() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/production",
            "--variable",
            "agent-config",
            "--env",
            "prod",
            "--context",
            "@examples/production/contexts/eu-enterprise.json",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "agent-config""#))
        .stdout(predicate::str::contains(r#""value_key": "enterprise""#))
        .stdout(predicate::str::contains(r#""model": "gpt-5""#));
}

#[test]
fn resolves_all_variables() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variables",
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
            "resolve",
            "examples/basic",
            "--variable",
            "checkout-redesign",
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
fn resolves_variable_with_trace_output() {
    let assert = Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "checkout-redesign",
            "--env",
            "prod",
            "--context",
            "user.tier=premium",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(stdout.contains("variable: checkout-redesign"));
    assert!(stdout.contains("environment: prod"));
    assert!(stdout.contains("qualifier: premium-users"));
    assert!(stdout.contains(r#"[0] context user.tier = "premium""#));
    assert!(stdout.contains(r#"test: eq "premium""#));
    assert!(stdout.contains("matched: true"));
    assert!(stdout.contains("rule[0] if premium-users -> premium (matched)"));
    assert!(stdout.contains("fallback -> control"));
    assert!(stdout.contains("value key: premium"));
    assert!(
        stdout.find("qualifiers:").unwrap() < stdout.find("  result:").unwrap(),
        "qualifier predicates should be printed before the final variable result"
    );
}

#[test]
fn resolve_rejects_context_that_does_not_match_workspace_schema() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--qualifier",
            "premium-users",
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
            "resolve",
            "examples/basic",
            "--variable",
            "checkout-redesign",
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
fn resolve_rejects_missing_env_for_variables() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "checkout-redesign",
            "--context",
            "{}",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--env is required when resolving variables",
        ));
}

#[test]
fn resolve_rejects_missing_target() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["resolve", "examples/basic", "--context", "{}"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("resolve requires at least one"));
}

#[test]
fn missing_qualifier_id_fails() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--qualifier", "missing"])
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
        .args(["show", "--qualifiers"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "workspace not found: pass a workspace source or run inside a rototo workspace",
        ));
}
