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
fn lists_variables_from_discovered_package() {
    Command::cargo_bin("rototo")
        .unwrap()
        .current_dir("examples/basic")
        .args(["show", "--variables"])
        .assert()
        .success()
        .stdout(predicate::str::contains("checkout-redesign"))
        .stdout(predicate::str::contains("llm-agent-config"))
        .stdout(predicate::str::contains("tenant-limits"))
        .stdout(predicate::str::contains("user-is-admin"))
        .stdout(predicate::str::contains("type: catalog:checkout-redesign"))
        .stdout(predicate::str::contains(
            "resolve: default \"control\" / 1 rule",
        ))
        .stdout(predicate::str::contains("variable://checkout-redesign").not());
}

#[test]
fn shows_package_inventory_including_linters() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("qualifiers:"))
        .stdout(predicate::str::contains("catalogs:"))
        .stdout(predicate::str::contains("variables:"))
        .stdout(predicate::str::contains(
            "llm-agent-config  catalogs/llm-agent-config.schema.json",
        ))
        .stdout(predicate::str::contains("catalog://llm-agent-config").not())
        .stdout(predicate::str::contains("lint authorities:"))
        .stdout(predicate::str::contains(
            "consumer-experience/checkout-heading-required",
        ))
        .stdout(predicate::str::contains("linters:"))
        .stdout(predicate::str::contains(
            "checkout-redesign  lint/checkout-redesign.lua",
        ))
        .stdout(predicate::str::contains(
            "premium-message  lint/premium-message.lua",
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
            r#""path": "evaluation-contexts/request.schema.json""#,
        ))
        .stdout(predicate::str::contains(r#""catalogs": ["#))
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
        .stdout(predicate::str::contains("qualifier: premium-users"))
        .stdout(predicate::str::contains(
            "path: qualifiers/premium-users.toml",
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
        .args(["show", "--variable", "user-is-admin"])
        .assert()
        .success()
        .stdout(predicate::str::contains("type = \"bool\""));
}

#[test]
fn gets_catalog_with_entries() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "examples/basic", "--catalog", "llm-agent-config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("catalog: llm-agent-config"))
        .stdout(predicate::str::contains("values: 3 entries"))
        .stdout(predicate::str::contains("source:"))
        .stdout(predicate::str::contains(r#""entries": {"#))
        .stdout(predicate::str::contains(r#""local": {"#))
        .stdout(predicate::str::contains(r#""model": "local-small""#))
        .stdout(predicate::str::contains(r#""model": "gpt-5""#));
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
        .stdout(predicate::str::contains(
            r#""when": "(context.user.tier == \"premium\")""#,
        ));
}

#[test]
fn lints_qualifier_by_id() {
    let package = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", package.display());

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
            r#"{"lane":"prod","user":{"tier":"premium","id":"user-123","role":"admin","email_domain":"example.com","language":"en","session_count":1},"account":{"plan":"enterprise","seats":250},"cart":{"total_usd":300},"device":{"platform":"web"},"request":{"country":"DE"}}"#,
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
    assert!(stdout.contains(r#"when: (context.user.tier == "premium")"#));
    assert!(stdout.contains("result: true"));
    assert!(!stdout.contains("premium-users=true"));
    assert!(stdout.find("when:").unwrap() < stdout.find("result: true").unwrap());
}

#[test]
fn lints_variable_from_discovered_package() {
    let package = std::path::absolute("examples/basic").unwrap();
    let expected = format!("ok: {}\n", package.display());

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
            "--context",
            r#"{"user":{"tier":"premium"}}"#,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "checkout-redesign""#))
        .stdout(predicate::str::contains(
            r#""catalog": "checkout-redesign""#,
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
            "summary-token-budget",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "summary-token-budget""#))
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
            "agent-config",
            "--context",
            "@examples/production/evaluation-contexts/request-samples/eu-enterprise.json",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "agent-config""#))
        .stdout(predicate::str::contains(r#""catalog": "agent-config""#))
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
            "@examples/basic/evaluation-contexts/request-samples/premium-enterprise.json",
            "--context",
            "lane=prod",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "checkout-redesign""#))
        .stdout(predicate::str::contains(r#""id": "admin-navigation""#))
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
            "checkout-redesign",
            "--context",
            "user.tier=free",
            "--context",
            "user.tier=premium",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""catalog": "checkout-redesign""#,
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
            "checkout-redesign",
            "--context",
            "user.tier=premium",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(stdout.contains("variable: checkout-redesign"));
    assert!(stdout.contains("qualifier: premium-users"));
    assert!(stdout.contains(r#"when: (context.user.tier == "premium")"#));
    assert!(stdout.contains(r#"rule[0] if qualifier["premium-users"] ->"#));
    assert!(stdout.contains(r#""variant":"premium""#));
    assert!(stdout.contains("default ->"));
    assert!(stdout.contains("source: checkout-redesign:premium"));
    assert!(
        stdout.find("qualifiers:").unwrap() < stdout.find("  result:").unwrap(),
        "qualifier conditions should be printed before the final variable result"
    );
}

#[test]
fn resolve_rejects_context_that_does_not_match_package_schema() {
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
            "--qualifier",
            "premium-users",
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
            "checkout-redesign",
            "--context",
            "lane=prd",
            "--context",
            "user.tier=premium",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""catalog": "checkout-redesign""#,
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
            "checkout-redesign",
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
fn missing_package_context_fails() {
    Command::cargo_bin("rototo")
        .unwrap()
        .args(["show", "--qualifiers"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "package not found: pass a package source or run inside a rototo package",
        ));
}
