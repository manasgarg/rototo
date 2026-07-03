use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn inspects_basic_package() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("package: "))
        .stdout(predicate::str::contains("catalogs:"))
        .stdout(predicate::str::contains("catalog: checkout_redesign"))
        .stdout(predicate::str::contains(
            "schema: model/catalogs/checkout_redesign.schema.json",
        ))
        .stdout(predicate::str::contains("variable: premium_users"))
        .stdout(predicate::str::contains("variable: premium_beta_users"))
        .stdout(predicate::str::contains(
            "----------------------------------------",
        ))
        .stdout(predicate::str::contains("variables:"))
        .stdout(predicate::str::contains("variable: checkout_redesign"))
        .stdout(predicate::str::contains("variable: tenant_limits"))
        .stdout(predicate::str::contains("linters:"))
        .stdout(predicate::str::contains("linter: checkout_redesign"))
        .stdout(predicate::str::contains(
            "consumer-experience/checkout-heading-required",
        ))
        .stdout(predicate::str::contains("lint authorities:"))
        .stdout(predicate::str::contains(
            "lint authority: consumer-experience",
        ))
        .stdout(predicate::str::contains("variable://checkout_redesign").not());
}

#[test]
fn inspects_discovered_package() {
    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .arg("inspect")
        .assert()
        .success()
        .stdout(predicate::str::contains("package: "))
        .stdout(predicate::str::contains("variable: premium_users"));
}

#[test]
fn inspect_human_conditions_show_values() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", "examples/basic", "--variable", "premium_users"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#"if (context.user.tier == "premium") -> true"#,
        ));

    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "inspect",
            "examples/basic",
            "--variable",
            "beta_rollout_bucket",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#"if (bucket(context.user.id, "checkout_redesign_2026_05", 0, 1000)) -> true"#,
        ));
}

#[test]
fn inspect_human_values_show_config_values() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["inspect", "examples/basic", "--catalog", "tenant_limits"])
        .assert()
        .success()
        .stdout(predicate::str::contains("catalog: tenant_limits"))
        .stdout(predicate::str::contains("enterprise = {"))
        .stdout(predicate::str::contains(r#""support_tier":"dedicated""#))
        .stdout(predicate::str::contains(
            "variable tenant_limits  variables/tenant_limits.toml",
        ))
        .stdout(predicate::str::contains("variable tenant_limits type").not())
        .stdout(predicate::str::contains("enterprise  inline  variables/tenant_limits.toml").not());
}

#[test]
fn inspect_human_variable_rules_show_when_expressions() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "inspect",
            "examples/console-runtime",
            "--variable",
            "console_request_observability",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#"rule[0] if variables["request_dev_console"] -> "dev_all""#,
        ))
        .stdout(predicate::str::contains(
            r#"rule[1] if variables["request_server_error"] -> "retain_errors""#,
        ))
        .stdout(predicate::str::contains(
            r#"rule[2] if variables["request_slow"] -> "retain_slow""#,
        ))
        .stdout(predicate::str::contains("if <missing>").not());
}

#[test]
fn inspects_basic_package_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["--json", "inspect", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""catalogs": ["#))
        .stdout(predicate::str::contains(
            r#""path": "model/catalogs/checkout_redesign.schema.json""#,
        ))
        .stdout(predicate::str::contains(r#""variables": ["#))
        .stdout(predicate::str::contains(r#""lint_authorities": ["#))
        .stdout(predicate::str::contains(
            r#""authority": "consumer-experience""#,
        ))
        .stdout(predicate::str::contains(
            r#""uri": "variable://premium_users""#,
        ))
        .stdout(predicate::str::contains(
            r#""uri": "variable://checkout_redesign""#,
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
            "checkout_redesign",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""when": "variables[\"premium_users\"]""#,
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
            "--variable",
            "returning_users",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "returning_users""#))
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
            r#""uri": "variable://premium_users""#,
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
            "checkout_redesign",
            "--context",
            r#"{"user":{"tier":"premium"}}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "checkout_redesign""#))
        .stdout(predicate::str::contains(
            r#""catalog": "checkout_redesign""#,
        ))
        .stdout(predicate::str::contains(r#""value": "premium""#))
        .stdout(predicate::str::contains(r#""matched": true"#))
        .stdout(predicate::str::contains(
            r#""condition": "variables[\"premium_users\"]""#,
        ));
}

#[test]
fn inspect_broken_package_still_shows_partial_model_and_lint() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "inspect",
            "tests/fixtures/packages/rules/reference/variable-unknown-value",
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
            "tests/fixtures/packages/rules/value/variable-value-type-mismatch",
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
        .args(["inspect", "examples/basic", "--linter", "checkout_redesign"])
        .assert()
        .success()
        .stdout(predicate::str::contains("linter: checkout_redesign"))
        .stdout(predicate::str::contains("path: lint/checkout_redesign.lua"))
        .stdout(predicate::str::contains("registrations:"))
        .stdout(predicate::str::contains(
            "[0] consumer-experience/checkout-heading-required",
        ))
        .stdout(predicate::str::contains(
            "target: /catalogs/checkout_redesign/entries",
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
            "checkout_redesign",
            "--context",
            r#"{"user":{"tier":"premium"}}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("trace: checkout_redesign:premium"));
}
