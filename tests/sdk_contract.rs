use serde::Deserialize;
use serde_json::Value as JsonValue;

use rototo::{EvaluationContext, LoadOptions, Package};

#[derive(Debug, Deserialize)]
struct ContractCase {
    name: String,
    schema_version: u32,
    package: String,
    operation: String,
    id: Option<String>,
    fallback: Option<String>,
    #[serde(default)]
    context: JsonValue,
    expect: ContractExpectation,
}

#[derive(Debug, Deserialize)]
struct ContractExpectation {
    ok: bool,
    #[serde(default)]
    result: JsonValue,
    diagnostics: Option<usize>,
    error: Option<ContractErrorExpectation>,
}

#[derive(Debug, Deserialize)]
struct ContractErrorExpectation {
    contains: String,
}

#[tokio::test]
async fn shared_sdk_contract_cases_match_rust_sdk() {
    for case in contract_cases() {
        assert_eq!(case.schema_version, 1, "{}", case.name);
        let actual = run_contract_case(&case).await;
        match (&actual, case.expect.ok) {
            (Ok(_), false) => panic!("{} unexpectedly succeeded", case.name),
            (Err(err), true) => panic!("{} unexpectedly failed: {err}", case.name),
            (Err(err), false) => {
                let expected = case
                    .expect
                    .error
                    .as_ref()
                    .expect("failing contract cases should include an error expectation");
                assert!(
                    err.contains(&expected.contains),
                    "{} error `{err}` did not contain `{}`",
                    case.name,
                    expected.contains
                );
            }
            (Ok(actual), true) => assert_expected_json_subset(&case.name, actual, &case.expect),
        }
    }
}

async fn run_contract_case(case: &ContractCase) -> Result<JsonValue, String> {
    match case.operation.as_str() {
        "load_package" => {
            Package::load(&case.package)
                .await
                .map_err(|err| err.to_string())?;
            Ok(serde_json::json!({ "ok": true }))
        }
        "lint_package" => {
            let package = Package::inspect(&case.package)
                .await
                .map_err(|err| err.to_string())?;
            let lint = package.lint().await.map_err(|err| err.to_string())?;
            Ok(serde_json::json!({
                "diagnostics": lint.diagnostics.len(),
            }))
        }
        "resolve_variable" => {
            let package = Package::load(&case.package)
                .await
                .map_err(|err| err.to_string())?;
            let context = EvaluationContext::from_json(case.context.clone())
                .map_err(|err| err.to_string())?;
            let id = case_id(case)?;
            let resolution = package
                .resolve_variable(id, &context)
                .map_err(|err| err.to_string())?;
            serde_json::to_value(resolution).map_err(|err| err.to_string())
        }
        "load_package_with_fallback" => {
            let fallback = case
                .fallback
                .as_deref()
                .ok_or_else(|| format!("contract case `{}` is missing fallback", case.name))?;
            let package = Package::load_with_options(
                &case.package,
                LoadOptions::new().with_fallback_source(fallback),
            )
            .await
            .map_err(|err| err.to_string())?;
            Ok(serde_json::json!({ "servedFallback": package.served_fallback() }))
        }
        "package_identity" => {
            let package = Package::load(&case.package)
                .await
                .map_err(|err| err.to_string())?;
            let identity = package.identity();
            Ok(serde_json::json!({
                "releaseId": identity.release_id,
                "immutable": identity.immutable,
            }))
        }
        operation => Err(format!("unsupported contract operation: {operation}")),
    }
}

fn case_id(case: &ContractCase) -> Result<&str, String> {
    case.id
        .as_deref()
        .ok_or_else(|| format!("contract case `{}` is missing id", case.name))
}

fn assert_expected_json_subset(name: &str, actual: &JsonValue, expect: &ContractExpectation) {
    if let Some(diagnostics) = expect.diagnostics {
        assert_eq!(
            actual.get("diagnostics").and_then(JsonValue::as_u64),
            Some(diagnostics as u64),
            "{name}"
        );
    }

    if !expect.result.is_null() {
        assert_json_subset(name, actual, &expect.result);
    }
}

fn assert_json_subset(name: &str, actual: &JsonValue, expected: &JsonValue) {
    match expected {
        JsonValue::Object(expected_object) => {
            let actual_object = actual
                .as_object()
                .unwrap_or_else(|| panic!("{name}: expected object, got {actual}"));
            for (key, value) in expected_object {
                let actual_value = actual_object
                    .get(key)
                    .unwrap_or_else(|| panic!("{name}: missing key `{key}` in {actual}"));
                assert_json_subset(name, actual_value, value);
            }
        }
        _ => assert_eq!(actual, expected, "{name}"),
    }
}

fn contract_cases() -> Vec<ContractCase> {
    include_str!("sdk-contract/cases.jsonl")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("contract case should parse"))
        .collect()
}
