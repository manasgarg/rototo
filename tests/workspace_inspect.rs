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
        .stdout(predicate::str::contains("schemas:"))
        .stdout(predicate::str::contains("schema: checkout-page.schema"))
        .stdout(predicate::str::contains("qualifiers:"))
        .stdout(predicate::str::contains("qualifier: premium-users"))
        .stdout(predicate::str::contains("qualifier: premium-beta-users"))
        .stdout(predicate::str::contains(
            "----------------------------------------",
        ))
        .stdout(predicate::str::contains("variables:"))
        .stdout(predicate::str::contains("variable: checkout-redesign"))
        .stdout(predicate::str::contains("variable: tenant-limits"))
        .stdout(predicate::str::contains("linters:"))
        .stdout(predicate::str::contains("linter: checkout-redesign"))
        .stdout(predicate::str::contains(
            "consumer-experience/checkout-heading-required",
        ))
        .stdout(predicate::str::contains("lint authorities:"))
        .stdout(predicate::str::contains(
            "lint authority: consumer-experience",
        ))
        .stdout(predicate::str::contains("variable://checkout-redesign").not());
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
        .stdout(predicate::str::contains("qualifier: premium-users"));
}

#[test]
fn inspect_human_predicates_show_values() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", "examples/basic", "--qualifier", "premium-users"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"[0] user.tier eq "premium""#));

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "inspect",
            "examples/basic",
            "--qualifier",
            "beta-rollout-bucket",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "[0] user.id bucket salt=checkout-redesign-2026-05 range=[0,1000]",
        ));
}

#[test]
fn inspect_human_values_show_config_values() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", "examples/basic", "--variable", "tenant-limits"])
        .assert()
        .success()
        .stdout(predicate::str::contains("variable: tenant-limits"))
        .stdout(predicate::str::contains("enterprise (inline) = {"))
        .stdout(predicate::str::contains(r#""support_tier":"dedicated""#))
        .stdout(predicate::str::contains("enterprise  inline  variables/tenant-limits.toml").not());
}

#[test]
fn inspects_basic_workspace_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["--json", "inspect", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""environments": ["#))
        .stdout(predicate::str::contains(r#""schemas": ["#))
        .stdout(predicate::str::contains(
            r#""path": "schemas/checkout-page.schema.json""#,
        ))
        .stdout(predicate::str::contains(r#""qualifiers": ["#))
        .stdout(predicate::str::contains(r#""variables": ["#))
        .stdout(predicate::str::contains(r#""lint_authorities": ["#))
        .stdout(predicate::str::contains(
            r#""authority": "consumer-experience""#,
        ))
        .stdout(predicate::str::contains(
            r#""uri": "qualifier://premium-users""#,
        ))
        .stdout(predicate::str::contains(
            r#""uri": "variable://checkout-redesign""#,
        ));
}

#[test]
fn inspect_json_hides_structural_text_spans() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "--json",
            "inspect",
            "examples/basic",
            "--qualifier",
            "returning-users",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "returning-users""#))
        .stdout(predicate::str::contains(
            r#""attribute": "user.session_count""#,
        ))
        .stdout(predicate::str::contains(r#""value": 2"#))
        .stdout(predicate::str::contains(r#""location": {"#).not())
        .stdout(predicate::str::contains(r#""character":"#).not());
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

#[test]
fn inspects_variable_resolution_trace_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "--json",
            "inspect",
            "examples/basic",
            "--variable",
            "checkout-redesign",
            "--env",
            "prod",
            "--context",
            r#"{"user":{"tier":"premium"}}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "checkout-redesign""#))
        .stdout(predicate::str::contains(r#""value_key": "premium""#))
        .stdout(predicate::str::contains(r#""matched": true"#))
        .stdout(predicate::str::contains(r#""qualifier_traces": ["#))
        .stdout(predicate::str::contains(r#""actual": "premium""#));
}

#[test]
fn inspect_broken_workspace_still_shows_partial_model_and_lint() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "inspect",
            "tests/fixtures/workspaces/rules/reference/variable-unknown-value",
            "--variable",
            "message",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("runtime: unavailable"))
        .stdout(predicate::str::contains("rototo/variable-unknown-value"))
        .stdout(predicate::str::contains("pathways:"))
        .stdout(predicate::str::contains("fallback -> missing"));
}

#[test]
fn inspect_lint_rule_shows_emitted_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "inspect",
            "tests/fixtures/workspaces/rules/graph/variable-value-unused",
            "--lint-rule",
            "rototo/variable-value-unused",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("lint rules:"))
        .stdout(predicate::str::contains("rototo/variable-value-unused"))
        .stdout(predicate::str::contains("variable value is not referenced"));
}

#[test]
fn inspect_linter_output_is_readable() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", "examples/basic", "--linter", "checkout-redesign"])
        .assert()
        .success()
        .stdout(predicate::str::contains("linter: checkout-redesign"))
        .stdout(predicate::str::contains("path: lint/checkout-redesign.lua"))
        .stdout(predicate::str::contains("registrations:"))
        .stdout(predicate::str::contains(
            "[0] consumer-experience/checkout-heading-required",
        ))
        .stdout(predicate::str::contains("target: value.heading"))
        .stdout(predicate::str::contains("runs during: value lint stage"))
        .stdout(predicate::str::contains("handler: check_heading"))
        .stdout(predicate::str::contains("value value field=").not());
}

#[test]
fn inspect_context_requires_resolution_target() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "inspect",
            "examples/basic",
            "--context",
            r#"{"user":{"tier":"premium"}}"#,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "inspect --context requires at least one",
        ));
}

#[test]
fn inspect_variable_context_requires_env() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "inspect",
            "examples/basic",
            "--variable",
            "checkout-redesign",
            "--context",
            r#"{"user":{"tier":"premium"}}"#,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--env is required when inspecting variables with --context",
        ));
}
