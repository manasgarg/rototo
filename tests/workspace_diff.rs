use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value as JsonValue;

#[test]
fn diff_json_reports_semantic_value_change_and_resolution_impact() {
    let temp = tempfile::TempDir::new().unwrap();
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    copy_dir(Path::new("examples/basic"), &before);
    copy_dir(Path::new("examples/basic"), &after);

    let variable_path = after.join("variables/premium-message.toml");
    let variable = fs::read_to_string(&variable_path).unwrap();
    fs::write(
        &variable_path,
        variable.replace(
            r#"value = "Welcome back, premium member.""#,
            r#"value = "Welcome back, valued premium member.""#,
        ),
    )
    .unwrap();

    let diff = diff_json(
        &before,
        &after,
        &[
            "--context",
            "@examples/basic/request-contexts/request-entries/premium-enterprise.json",
            "--context",
            "lane=stage",
        ],
    );

    let changes = diff["changes"].as_array().unwrap();
    let value_change = changes
        .iter()
        .find(|change| {
            change["kind"] == "variable_rule_value_changed"
                && change["target"]["entity"]["kind"] == "rule"
                && change["target"]["entity"]["variable"] == "premium-message"
                && change["target"]["entity"]["index"] == 0
        })
        .expect("premium-message rule value change");
    assert_eq!(value_change["before"], "Welcome back, premium member.");
    assert_eq!(
        value_change["after"],
        "Welcome back, valued premium member."
    );

    let impacts = diff["resolution_impacts"].as_array().unwrap();
    let impact = impacts
        .iter()
        .find(|impact| impact["variable"] == "premium-message")
        .expect("premium-message resolution impact");
    assert_eq!(impact["before"]["source"]["kind"], "literal");
    assert_eq!(impact["after"]["source"]["kind"], "literal");
    assert_eq!(impact["before"]["value"], "Welcome back, premium member.");
    assert_eq!(
        impact["after"]["value"],
        "Welcome back, valued premium member."
    );
}

#[test]
fn diff_json_reports_empty_changes_for_identical_workspaces() {
    let temp = tempfile::TempDir::new().unwrap();
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    copy_dir(Path::new("examples/basic"), &before);
    copy_dir(Path::new("examples/basic"), &after);

    let diff = diff_json(&before, &after, &[]);

    assert!(changes(&diff).is_empty());
    assert!(
        diff.get("resolution_impacts").is_none(),
        "resolution impacts are omitted when no context is supplied"
    );
}

#[test]
fn diff_json_reports_added_and_removed_workspace_entities() {
    let temp = tempfile::TempDir::new().unwrap();
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    copy_dir(Path::new("examples/basic"), &before);
    copy_dir(Path::new("examples/basic"), &after);

    write_file(
        &before.join("variables/retired-message.toml"),
        r#"schema_version = 1

description = "Before-only variable"
type = "string"

[resolve]
default = "retired"
"#,
    );
    write_file(
        &after.join("variables/new-message.toml"),
        r#"schema_version = 1

description = "After-only variable"
type = "string"

[resolve]
default = "new"
"#,
    );

    write_file(
        &before.join("qualifiers/retired-users.toml"),
        r#"schema_version = 1

description = "Before-only qualifier"
when = 'context.user.tier == "legacy"'
"#,
    );
    write_file(
        &after.join("qualifiers/new-users.toml"),
        r#"schema_version = 1

description = "After-only qualifier"
when = 'context.user.tier == "new"'
"#,
    );

    write_file(
        &before.join("catalogs/retired.schema.json"),
        r#"{
  "type": "object",
  "additionalProperties": true
}
"#,
    );
    write_file(
        &after.join("catalogs/new.schema.json"),
        r#"{
  "type": "object",
  "additionalProperties": true
}
"#,
    );

    write_file(
        &before.join("catalogs/support-banner-entries/retired.toml"),
        r#"enabled = false
tone = "quiet"
heading = "Retired"
body = "This entry exists only before the diff."
cta = "Dismiss"
"#,
    );
    write_file(
        &after.join("catalogs/support-banner-entries/new.toml"),
        r#"enabled = true
tone = "help"
heading = "New"
body = "This entry exists only after the diff."
cta = "Open"
"#,
    );

    let diff = diff_json(&before, &after, &[]);

    assert_entity_change(
        &diff,
        "variable_added",
        "variable",
        &[("id", "new-message")],
    );
    assert_entity_change(
        &diff,
        "variable_removed",
        "variable",
        &[("id", "retired-message")],
    );
    assert_entity_change(
        &diff,
        "qualifier_added",
        "qualifier",
        &[("id", "new-users")],
    );
    assert_entity_change(
        &diff,
        "qualifier_removed",
        "qualifier",
        &[("id", "retired-users")],
    );
    assert_entity_change(&diff, "catalog_added", "catalog", &[("id", "new")]);
    assert_entity_change(&diff, "catalog_removed", "catalog", &[("id", "retired")]);

    let added_entry = assert_entity_change(
        &diff,
        "catalog_entry_added",
        "catalog_entry",
        &[("catalog", "support-banner"), ("key", "new")],
    );
    assert_eq!(added_entry["after"]["heading"], "New");

    let removed_entry = assert_entity_change(
        &diff,
        "catalog_entry_removed",
        "catalog_entry",
        &[("catalog", "support-banner"), ("key", "retired")],
    );
    assert_eq!(removed_entry["before"]["heading"], "Retired");
}

#[test]
fn diff_json_reports_resolve_default_and_rule_condition_changes() {
    let temp = tempfile::TempDir::new().unwrap();
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    copy_dir(Path::new("examples/basic"), &before);
    copy_dir(Path::new("examples/basic"), &after);

    let variable_path = after.join("variables/support-banner.toml");
    let variable = fs::read_to_string(&variable_path).unwrap();
    fs::write(
        &variable_path,
        variable
            .replace(r#"default = "hidden""#, r#"default = "mobile_help""#)
            .replace(
                r#"when = 'qualifier["mobile-users"]'"#,
                r#"when = 'qualifier["enterprise-accounts"]'"#,
            ),
    )
    .unwrap();

    let diff = diff_json(&before, &after, &[]);

    let default_change = assert_entity_change(
        &diff,
        "variable_resolve_default_changed",
        "variable",
        &[("id", "support-banner")],
    );
    assert_eq!(default_change["before"], "hidden");
    assert_eq!(default_change["after"], "mobile_help");

    let when_change = changes(&diff)
        .iter()
        .find(|change| {
            change["kind"] == "variable_rule_when_changed"
                && change["target"]["entity"]["kind"] == "rule"
                && change["target"]["entity"]["variable"] == "support-banner"
                && change["target"]["entity"]["index"] == 0
        })
        .expect("support-banner rule when change");
    assert_eq!(when_change["before"], r#"qualifier["mobile-users"]"#);
    assert_eq!(when_change["after"], r#"qualifier["enterprise-accounts"]"#);
}

fn diff_json(before: &Path, after: &Path, extra_args: &[&str]) -> JsonValue {
    let mut command = Command::cargo_bin("rototo").unwrap();
    command
        .arg("--json")
        .arg("diff")
        .arg(before)
        .arg(after)
        .args(extra_args);
    let output = command.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).unwrap()
}

fn changes(diff: &JsonValue) -> &[JsonValue] {
    diff["changes"].as_array().unwrap()
}

fn assert_entity_change<'a>(
    diff: &'a JsonValue,
    kind: &str,
    entity_kind: &str,
    fields: &[(&str, &str)],
) -> &'a JsonValue {
    changes(diff)
        .iter()
        .find(|change| {
            change["kind"] == kind
                && change["target"]["entity"]["kind"] == entity_kind
                && fields
                    .iter()
                    .all(|(field, expected)| change["target"]["entity"][*field] == *expected)
        })
        .unwrap_or_else(|| panic!("missing {kind} change for {entity_kind} with fields {fields:?}"))
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
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
