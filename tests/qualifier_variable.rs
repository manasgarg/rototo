use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn lists_condition_variables() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--variables"])
        .assert()
        .success()
        .stdout(predicate::str::contains("admin_users"))
        .stdout(predicate::str::contains("premium_users"))
        .stdout(predicate::str::contains("premium_beta_users"));
}

#[test]
fn lists_variables_from_discovered_package() {
    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .args(["show", "--variables"])
        .assert()
        .success()
        .stdout(predicate::str::contains("checkout_redesign"))
        .stdout(predicate::str::contains("llm_agent_config"))
        .stdout(predicate::str::contains("tenant_limits"))
        .stdout(predicate::str::contains("user_is_admin"))
        .stdout(predicate::str::contains("type: catalog:checkout_redesign"))
        .stdout(predicate::str::contains(
            "resolve: default \"control\" / 1 rule",
        ))
        .stdout(predicate::str::contains("variable://checkout_redesign").not());
}

#[test]
fn shows_package_inventory_including_linters() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("catalogs:"))
        .stdout(predicate::str::contains("variables:"))
        .stdout(predicate::str::contains(
            "llm_agent_config  model/catalogs/llm_agent_config.schema.json",
        ))
        .stdout(predicate::str::contains("catalog://llm_agent_config").not())
        .stdout(predicate::str::contains("lint authorities:"))
        .stdout(predicate::str::contains(
            "consumer-experience/checkout-heading-required",
        ))
        .stdout(predicate::str::contains("linters:"))
        .stdout(predicate::str::contains(
            "checkout_redesign  lint/checkout_redesign.lua",
        ))
        .stdout(predicate::str::contains(
            "premium_message  lint/premium_message.lua",
        ));
}

#[test]
fn shows_package_inventory_as_json_including_top_level_entries() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""path": "model/context/request.schema.json""#,
        ))
        .stdout(predicate::str::contains(r#""catalogs": ["#))
        .stdout(predicate::str::contains(r#""variables": ["#))
        .stdout(predicate::str::contains(r#""lint_authorities": ["#))
        .stdout(predicate::str::contains(
            r#""authority": "consumer-experience""#,
        ))
        .stdout(predicate::str::contains(r#""linters": ["#));
}

#[test]
fn lists_condition_variables_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--variables", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""variables": ["#))
        .stdout(predicate::str::contains(
            r#""uri": "variable://premium_users""#,
        ));
}

#[test]
fn gets_condition_variable_by_id() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--variable", "premium_users"])
        .assert()
        .success()
        .stdout(predicate::str::contains("variable: premium_users"))
        .stdout(predicate::str::contains(
            "path: variables/premium_users.toml",
        ))
        .stdout(predicate::str::contains("source:"))
        .stdout(predicate::str::contains(
            "description = \"Users on the premium plan\"",
        ))
        .stdout(predicate::str::contains(
            "when = '(context.user.tier == \"premium\")'",
        ));
}

#[test]
fn gets_variable_from_discovered_package() {
    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .args(["show", "--variable", "user_is_admin"])
        .assert()
        .success()
        .stdout(predicate::str::contains("type = \"bool\""));
}

#[test]
fn gets_catalog_with_entries() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--catalog", "llm_agent_config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("catalog: llm_agent_config"))
        .stdout(predicate::str::contains("values: 3 entries"))
        .stdout(predicate::str::contains("source:"))
        .stdout(predicate::str::contains(r#""entries": {"#))
        .stdout(predicate::str::contains(r#""local": {"#))
        .stdout(predicate::str::contains(r#""model": "local-small""#))
        .stdout(predicate::str::contains(r#""model": "gpt-5""#));
}

#[test]
fn gets_condition_variable_by_id_as_json() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "--json",
            "show",
            "examples/basic",
            "--variable",
            "premium_users",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "premium_users""#))
        .stdout(predicate::str::contains(
            r#""uri": "variable://premium_users""#,
        ))
        .stdout(predicate::str::contains(
            r#""when": "(context.user.tier == \"premium\")""#,
        ));
}

#[test]
fn lints_condition_variable_by_id() {
    let package = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", package.display());

    Command::cargo_bin("rototo")
        .unwrap()
        .args(["lint", "examples/basic", "--variable", "premium_users"])
        .assert()
        .success()
        .stdout(predicate::eq(expected));
}

#[test]
fn resolves_condition_variable_by_id() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "premium_users",
            "--context",
            r#"{"user":{"tier":"premium","id":"a=b"}}"#,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "premium_users""#))
        .stdout(predicate::str::contains(r#""value": true"#));
}

#[test]
fn resolves_all_variables_including_conditions() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variables",
            "--context",
            r#"{"lane":"prod","user":{"tier":"premium","id":"user-123","role":"admin","email_domain":"example.com","language":"en","session_count":1},"account":{"plan":"enterprise","seats":250},"cart":{"total_usd":300},"device":{"platform":"web"},"request":{"country":"DE"}}"#,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "premium_users""#))
        .stdout(predicate::str::contains(r#""id": "enterprise_accounts""#))
        .stdout(predicate::str::contains(r#""id": "eu_premium_users""#));
}

#[test]
fn resolves_condition_variable_with_trace_output() {
    let assert = Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "premium_users",
            "--context",
            "user.tier=premium",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(stdout.contains("variable: premium_users"));
    assert!(stdout.contains(r#"rule[0] if (context.user.tier == "premium") -> true (matched)"#));
    assert!(stdout.contains("value: true"));
    assert!(stdout.find("rule[0]").unwrap() < stdout.find("value: true").unwrap());
}

#[test]
fn lints_variable_from_discovered_package() {
    let package = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", package.display());

    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .args(["lint", "--variable", "checkout_redesign"])
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
            "checkout_redesign",
            "--context",
            r#"{"user":{"tier":"premium"}}"#,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "checkout_redesign""#))
        .stdout(predicate::str::contains(
            r#""catalog": "checkout_redesign""#,
        ))
        .stdout(predicate::str::contains(r#""value": "premium""#))
        .stdout(predicate::str::contains(r#""variant": "premium""#));
}

#[test]
fn resolves_variable_without_context_as_empty_object() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/quickstart",
            "--variable",
            "summary_token_budget",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "summary_token_budget""#))
        .stdout(predicate::str::contains(r#""kind": "literal""#))
        .stdout(predicate::str::contains(r#""value": 1800"#));
}

#[test]
fn resolves_production_example_enterprise_profile() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/production",
            "--variable",
            "agent_config",
            "--context",
            "@examples/production/model/context/request-samples/eu_enterprise.json",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "agent_config""#))
        .stdout(predicate::str::contains(r#""catalog": "agent_config""#))
        .stdout(predicate::str::contains(r#""value": "enterprise""#))
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
            "--context",
            "@examples/basic/model/context/request-samples/premium_enterprise.json",
            "--context",
            "lane=prod",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "checkout_redesign""#))
        .stdout(predicate::str::contains(r#""id": "admin_navigation""#))
        .stdout(predicate::str::contains(r#""value": "enterprise""#));
}

#[test]
fn resolves_variable_with_context_assignments() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "checkout_redesign",
            "--context",
            "user.tier=free",
            "--context",
            "user.tier=premium",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""catalog": "checkout_redesign""#,
        ))
        .stdout(predicate::str::contains(r#""value": "premium""#));
}

#[test]
fn resolves_variable_with_trace_output() {
    let assert = Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "checkout_redesign",
            "--context",
            "user.tier=premium",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(stdout.contains("variable: checkout_redesign"));
    assert!(stdout.contains(r#"rule[0] if variables["premium_users"] ->"#));
    assert!(stdout.contains(r#""variant":"premium""#));
    assert!(stdout.contains("default ->"));
    assert!(stdout.contains("source: checkout_redesign:premium"));
}

#[test]
fn resolve_rejects_context_that_does_not_match_package_schema() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "premium_users",
            "--context",
            r#"{"unknown":true}"#,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "evaluation context does not match any compatible evaluation context",
        ));
}

#[test]
fn resolve_rejects_missing_condition_context_even_when_schema_allows_it() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "premium_users",
            "--context",
            r#"{"user":{"id":"user-123"}}"#,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No such key"));
}

#[test]
fn resolve_accepts_lane_as_context() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "checkout_redesign",
            "--context",
            "lane=prd",
            "--context",
            "user.tier=premium",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""catalog": "checkout_redesign""#,
        ))
        .stdout(predicate::str::contains(r#""value": "premium""#));
}

#[test]
fn resolve_rejects_missing_context_for_variable_rules() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "resolve",
            "examples/basic",
            "--variable",
            "checkout_redesign",
            "--context",
            "{}",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No such key"));
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
fn missing_variable_id_fails() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--variable", "missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "variable not found: variable://missing",
        ));
}

#[test]
fn missing_package_context_fails() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "--variables"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "package not found: pass a package source or run inside a rototo package",
        ));
}
