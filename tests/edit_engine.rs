//! The edit engine against a real package: operations applied to a copy of
//! `examples/basic` must produce a tree the lint pipeline accepts, because
//! every console save runs lint on the post-edit stage.

use std::fs;
use std::path::Path;

use serde_json::json;

use rototo::edit::{self, EditOperation, EditOptions};

#[tokio::test]
async fn engine_edits_leave_the_example_package_lint_clean() {
    let temp = tempfile::TempDir::new().unwrap();
    let package = temp.path().join("basic");
    copy_dir(Path::new("examples/basic"), &package);

    let operations = vec![
        // The everyday edits: a default, a new rule, a rollout dial turn.
        EditOperation::SetDefault {
            variable: "checkout_redesign".to_owned(),
            value: json!("premium"),
        },
        EditOperation::AddRule {
            variable: "checkout_redesign".to_owned(),
            position: Some(0),
            when: "variables[\"eu_users\"]".to_owned(),
            value: json!("control"),
        },
        EditOperation::SetArmBuckets {
            layer: "checkout".to_owned(),
            allocation: "cta_copy_test".to_owned(),
            arm: "benefit_led".to_owned(),
            buckets: "500-899".to_owned(),
        },
        // Creation: a condition variable and an entry, both fresh.
        EditOperation::CreateVariable {
            id: "weekend_shoppers".to_owned(),
            variable_type: "bool".to_owned(),
            description: Some("Named condition for weekend traffic".to_owned()),
            default: json!(false),
        },
        EditOperation::CreateEntry {
            catalog: "tenant_limits".to_owned(),
            key: "trial".to_owned(),
            fields: json!({
                "enabled_features": ["basic-reporting"],
                "limits": { "projects": 1, "members": 5, "monthly_requests": 1000 },
                "metadata": { "support_tier": "community", "audit_retention_days": 7 }
            }),
        },
        EditOperation::SetField {
            target: "catalog=tenant_limits:entry=starter#/limits/projects".to_owned(),
            value: json!(15),
        },
    ];

    let outcome = edit::apply_to_package(&package, &operations, &EditOptions::default())
        .await
        .expect("operations apply cleanly");
    edit::write_plan(&package, &outcome.plan)
        .await
        .expect("plan writes cleanly");

    let lint = rototo::lint_package(&package).await.expect("lint runs");
    let errors: Vec<String> = lint
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == rototo::diagnostics::Severity::Error)
        .map(|diagnostic| format!("{}: {}", diagnostic.primary.path, diagnostic.message))
        .collect();
    assert!(errors.is_empty(), "post-edit lint errors: {errors:#?}");

    // The splices really landed.
    let checkout = fs::read_to_string(package.join("variables/checkout_redesign.toml")).unwrap();
    assert!(checkout.contains("default = \"premium\""), "{checkout}");
    let eu = checkout.find("eu_users").unwrap();
    let premium = checkout.find("premium_users").unwrap();
    assert!(
        eu < premium,
        "the new rule went in at position 0:\n{checkout}"
    );

    let layer = fs::read_to_string(package.join("layers/checkout.toml")).unwrap();
    assert!(layer.contains("buckets = \"500-899\""), "{layer}");

    let starter =
        fs::read_to_string(package.join("data/catalogs/tenant_limits/starter.toml")).unwrap();
    assert!(starter.contains("projects = 15"), "{starter}");
    assert!(
        package
            .join("data/catalogs/tenant_limits/trial.toml")
            .exists()
    );

    // Change records name every edit in addressing-grammar terms.
    let addresses: Vec<&str> = outcome
        .records
        .iter()
        .map(|record| record.address.as_str())
        .collect();
    assert_eq!(
        addresses,
        vec![
            "variable=checkout_redesign#/resolve/default",
            "variable=checkout_redesign#/resolve/rule/0",
            "layer=checkout#/allocation/0/arm/1/buckets",
            "variable=weekend_shoppers",
            "catalog=tenant_limits:entry=trial",
            "catalog=tenant_limits:entry=starter#/limits/projects",
        ]
    );
}

#[tokio::test]
async fn deleting_an_unreferenced_variable_stays_lint_clean() {
    let temp = tempfile::TempDir::new().unwrap();
    let package = temp.path().join("basic");
    copy_dir(Path::new("examples/basic"), &package);

    // admin_navigation is a leaf: nothing else references it.
    let outcome = edit::apply_to_package(
        &package,
        &[EditOperation::Delete {
            target: "variable=admin_navigation".to_owned(),
        }],
        &EditOptions::default(),
    )
    .await
    .expect("delete applies");
    edit::write_plan(&package, &outcome.plan)
        .await
        .expect("plan writes");

    assert!(!package.join("variables/admin_navigation.toml").exists());
    let lint = rototo::lint_package(&package).await.expect("lint runs");
    assert!(
        !lint.has_errors(),
        "post-delete lint errors: {:#?}",
        lint.diagnostics
    );
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let from_path = entry.path();
        let to_path = to.join(entry.file_name());
        if from_path.is_dir() {
            copy_dir(&from_path, &to_path);
        } else {
            fs::copy(&from_path, &to_path).unwrap();
        }
    }
}
