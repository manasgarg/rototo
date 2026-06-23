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
        .stdout(predicate::str::contains("catalogs:"))
        .stdout(predicate::str::contains("catalog: checkout-redesign"))
        .stdout(predicate::str::contains(
            "schema: catalogs/checkout-redesign.schema.json",
        ))
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
fn inspect_human_conditions_show_values() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", "examples/basic", "--qualifier", "premium-users"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#"when: (context.user.tier == "premium")"#,
        ));

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
            r#"when: (bucket(context.user.id, "checkout-redesign-2026-05", 0, 1000))"#,
        ));
}

#[test]
fn inspect_human_values_show_config_values() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", "examples/basic", "--catalog", "tenant-limits"])
        .assert()
        .success()
        .stdout(predicate::str::contains("catalog: tenant-limits"))
        .stdout(predicate::str::contains("enterprise = {"))
        .stdout(predicate::str::contains(r#""support_tier":"dedicated""#))
        .stdout(predicate::str::contains(
            "variable tenant-limits  variables/tenant-limits.toml",
        ))
        .stdout(predicate::str::contains("variable tenant-limits type").not())
        .stdout(predicate::str::contains("enterprise  inline  variables/tenant-limits.toml").not());
}

#[test]
fn inspect_human_variable_rules_show_when_expressions() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "inspect",
            "examples/console-runtime",
            "--variable",
            "console-request-observability",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#"rule[0] if qualifier["request-dev-console"] -> "dev-all""#,
        ))
        .stdout(predicate::str::contains(
            r#"rule[1] if qualifier["request-server-error"] -> "retain-errors""#,
        ))
        .stdout(predicate::str::contains(
            r#"rule[2] if qualifier["request-slow"] -> "retain-slow""#,
        ))
        .stdout(predicate::str::contains("if <missing>").not());
}

#[test]
fn inspects_basic_workspace_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["--json", "inspect", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""catalogs": ["#))
        .stdout(predicate::str::contains(
            r#""path": "catalogs/checkout-redesign.schema.json""#,
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
fn inspect_json_variable_rules_include_when_expressions() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "--json",
            "inspect",
            "examples/basic",
            "--variable",
            "checkout-redesign",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""when": "qualifier[\"premium-users\"]""#,
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
            r#""when": "(context.user.session_count >= 2)""#,
        ))
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
            "--context",
            r#"{"user":{"tier":"premium"}}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "checkout-redesign""#))
        .stdout(predicate::str::contains(
            r#""catalog": "checkout-redesign""#,
        ))
        .stdout(predicate::str::contains(r#""value": "premium""#))
        .stdout(predicate::str::contains(r#""matched": true"#))
        .stdout(predicate::str::contains(r#""qualifier_traces": ["#))
        .stdout(predicate::str::contains(
            r#""when": "(context.user.tier == \"premium\")""#,
        ));
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
        .stdout(predicate::str::contains("resolve:"))
        .stdout(predicate::str::contains(r#"default -> "missing""#));
}

#[test]
fn inspect_lint_rule_shows_emitted_diagnostics() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "inspect",
            "tests/fixtures/workspaces/rules/value/variable-value-type-mismatch",
            "--lint-rule",
            "rototo/variable-value-type-mismatch",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("lint rules:"))
        .stdout(predicate::str::contains(
            "rototo/variable-value-type-mismatch",
        ))
        .stdout(predicate::str::contains(
            "resolve default does not match type",
        ));
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
        .stdout(predicate::str::contains(
            "target: /catalogs/checkout-redesign/entries",
        ))
        .stdout(predicate::str::contains("runs during: policy lint stage"))
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
fn inspect_variable_context_resolves_without_extra_input() {
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
        .success()
        .stdout(predicate::str::contains("trace: checkout-redesign:premium"));
}
