use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

use assert_cmd::Command;
use serde_json::Value as JsonValue;

#[test]
fn diff_json_reports_semantic_value_change_and_resolution_impact() {
    let temp = tempfile::TempDir::new().unwrap();
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    copy_dir(Path::new("examples/basic"), &before);
    copy_dir(Path::new("examples/basic"), &after);

    let variable_path = after.join("variables/premium_message.toml");
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
            "@examples/basic/model/context/request-samples/premium_enterprise.json",
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
                && change["target"]["entity"]["variable"] == "premium_message"
                && change["target"]["entity"]["index"] == 0
        })
        .expect("premium_message rule value change");
    assert_eq!(value_change["before"], "Welcome back, premium member.");
    assert_eq!(
        value_change["after"],
        "Welcome back, valued premium member."
    );

    let impacts = diff["resolution_impacts"].as_array().unwrap();
    let impact = impacts
        .iter()
        .find(|impact| impact["variable"] == "premium_message")
        .expect("premium_message resolution impact");
    assert_eq!(impact["before"]["source"]["kind"], "literal");
    assert_eq!(impact["after"]["source"]["kind"], "literal");
    assert_eq!(impact["before"]["value"], "Welcome back, premium member.");
    assert_eq!(
        impact["after"]["value"],
        "Welcome back, valued premium member."
    );
}

/// Lists, samples, and evaluation-context schemas are semantic surface too:
/// a change set touching them must say so, not just show the commits.
#[test]
fn diff_json_reports_list_sample_and_context_schema_changes() {
    let temp = tempfile::TempDir::new().unwrap();
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    copy_dir(Path::new("examples/basic"), &before);
    copy_dir(Path::new("examples/basic"), &after);

    // A list changes membership, and a brand-new list appears.
    let list = "schema_version = 1\ntype = \"string\"\nmembers = [\"starter\", \"growth\"]\n";
    fs::create_dir_all(before.join("lists")).unwrap();
    fs::create_dir_all(after.join("lists")).unwrap();
    fs::write(before.join("lists/plan_tiers.toml"), list).unwrap();
    fs::write(
        after.join("lists/plan_tiers.toml"),
        list.replace("\"growth\"", "\"growth\", \"enterprise\""),
    )
    .unwrap();
    fs::write(after.join("lists/regions.toml"), list).unwrap();

    // A saved sample changes one fact.
    let sample_path = "model/context/request-samples/premium_enterprise.json";
    let sample = fs::read_to_string(before.join(sample_path)).unwrap();
    fs::write(
        after.join(sample_path),
        sample.replace("\"premium\"", "\"basic\""),
    )
    .unwrap();

    // The evaluation context schema grows a property.
    let schema_path = "model/context/request.schema.json";
    let schema = fs::read_to_string(before.join(schema_path)).unwrap();
    fs::write(
        after.join(schema_path),
        schema.replacen(
            "\"properties\"",
            "\"x-note\": \"reviewed\", \"properties\"",
            1,
        ),
    )
    .unwrap();

    let diff = diff_json(&before, &after, &[]);
    let changes = diff["changes"].as_array().unwrap();

    let members_change = changes
        .iter()
        .find(|change| change["kind"] == "list_changed")
        .expect("list membership change");
    assert_eq!(members_change["target"]["entity"]["id"], "plan_tiers");
    assert_eq!(
        members_change["target"]["field"]["path"],
        serde_json::json!(["members"])
    );

    let added = changes
        .iter()
        .find(|change| change["kind"] == "list_added")
        .expect("list added");
    assert_eq!(added["target"]["entity"]["id"], "regions");

    let sample_change = changes
        .iter()
        .find(|change| change["kind"] == "sample_changed")
        .expect("sample change");
    assert_eq!(
        sample_change["target"]["entity"]["evaluation_context"],
        "request"
    );
    assert_eq!(
        sample_change["target"]["entity"]["key"],
        "premium_enterprise"
    );
    assert_eq!(sample_change["before"], "premium");
    assert_eq!(sample_change["after"], "basic");

    let schema_change = changes
        .iter()
        .find(|change| change["kind"] == "evaluation_context_schema_changed")
        .expect("evaluation context schema change");
    assert_eq!(schema_change["target"]["entity"]["id"], "request");
}

#[test]
fn diff_json_reports_layer_allocation_and_resolution_changes() {
    let temp = tempfile::TempDir::new().unwrap();
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    copy_dir(Path::new("examples/basic"), &before);
    copy_dir(Path::new("examples/basic"), &after);

    // Conclude the experiment and shrink an arm's claim.
    let layer_path = after.join("layers/checkout.toml");
    let layer = fs::read_to_string(&layer_path).unwrap();
    fs::write(
        &layer_path,
        layer
            .replace(r#"status = "running""#, r#"status = "concluded""#)
            .replace(r#"buckets = "500-999""#, r#"buckets = "500-899""#),
    )
    .unwrap();

    // Move a query variable's limit: a resolution-shape change, not a rule.
    let variable_path = after.join("variables/active_support_banners.toml");
    let variable = fs::read_to_string(&variable_path).unwrap();
    fs::write(&variable_path, format!("{variable}limit = 1\n")).unwrap();

    let diff = diff_json(&before, &after, &[]);
    let changes = diff["changes"].as_array().unwrap();
    let kinds: Vec<&str> = changes
        .iter()
        .filter_map(|change| change["kind"].as_str())
        .collect();
    assert!(kinds.contains(&"allocation_status_changed"), "{kinds:?}");
    assert!(kinds.contains(&"allocation_arms_reassigned"), "{kinds:?}");
    assert!(kinds.contains(&"variable_resolution_changed"), "{kinds:?}");

    let status = changes
        .iter()
        .find(|change| change["kind"] == "allocation_status_changed")
        .unwrap();
    assert_eq!(status["before"], "running");
    assert_eq!(status["after"], "concluded");

    // Shrinking "500-999" to "500-899" releases 100 claimed buckets back to
    // the default: enrolled units change value, so the change is flagged as
    // a reassignment with the blast radius in the detail.
    let arms = changes
        .iter()
        .find(|change| change["kind"] == "allocation_arms_reassigned")
        .unwrap();
    assert_eq!(arms["detail"]["released_buckets"], 100);
    assert_eq!(arms["detail"]["reassigned_buckets"], 0);
    assert_eq!(arms["detail"]["claimed_buckets"], 0);
}

#[test]
fn diff_json_classifies_arm_expansion_as_safe() {
    let temp = tempfile::TempDir::new().unwrap();
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    copy_dir(Path::new("examples/basic"), &before);
    copy_dir(Path::new("examples/basic"), &after);

    // The before side leaves buckets 900-999 unclaimed; the after side grows
    // the arm into them. No claimed bucket changes hands.
    let layer_path = before.join("layers/checkout.toml");
    let layer = fs::read_to_string(&layer_path).unwrap();
    fs::write(
        &layer_path,
        layer.replace(r#"buckets = "500-999""#, r#"buckets = "500-899""#),
    )
    .unwrap();

    let diff = diff_json(&before, &after, &[]);
    let changes = diff["changes"].as_array().unwrap();
    let arms = changes
        .iter()
        .find(|change| change["kind"] == "allocation_arms_expanded")
        .unwrap();
    assert_eq!(arms["detail"]["claimed_buckets"], 100);
    assert_eq!(arms["detail"]["released_buckets"], 0);
    assert_eq!(arms["detail"]["reassigned_buckets"], 0);
}

#[test]
fn diff_json_reports_empty_changes_for_identical_packages() {
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
fn diff_json_reports_added_and_removed_package_entities() {
    let temp = tempfile::TempDir::new().unwrap();
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    copy_dir(Path::new("examples/basic"), &before);
    copy_dir(Path::new("examples/basic"), &after);

    write_file(
        &before.join("variables/retired_message.toml"),
        r#"schema_version = 1

description = "Before-only variable"
type = "string"

[resolve]
default = "retired"
"#,
    );
    write_file(
        &after.join("variables/new_message.toml"),
        r#"schema_version = 1

description = "After-only variable"
type = "string"

[resolve]
default = "new"
"#,
    );

    write_file(
        &before.join("model/catalogs/retired.schema.json"),
        r#"{
  "type": "object",
  "additionalProperties": true
}
"#,
    );
    write_file(
        &after.join("model/catalogs/new.schema.json"),
        r#"{
  "type": "object",
  "additionalProperties": true
}
"#,
    );

    write_file(
        &before.join("data/catalogs/support_banner/retired.toml"),
        r#"enabled = false
tone = "quiet"
heading = "Retired"
body = "This entry exists only before the diff."
cta = "Dismiss"
"#,
    );
    write_file(
        &after.join("data/catalogs/support_banner/new.toml"),
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
        &[("id", "new_message")],
    );
    assert_entity_change(
        &diff,
        "variable_removed",
        "variable",
        &[("id", "retired_message")],
    );
    assert_entity_change(&diff, "catalog_added", "catalog", &[("id", "new")]);
    assert_entity_change(&diff, "catalog_removed", "catalog", &[("id", "retired")]);

    let added_entry = assert_entity_change(
        &diff,
        "catalog_entry_added",
        "catalog_entry",
        &[("catalog", "support_banner"), ("key", "new")],
    );
    assert_eq!(added_entry["after"]["heading"], "New");

    let removed_entry = assert_entity_change(
        &diff,
        "catalog_entry_removed",
        "catalog_entry",
        &[("catalog", "support_banner"), ("key", "retired")],
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

    let variable_path = after.join("variables/support_banner.toml");
    let variable = fs::read_to_string(&variable_path).unwrap();
    fs::write(
        &variable_path,
        variable
            .replace(r#"default = "hidden""#, r#"default = "mobile_help""#)
            .replace(
                r#"when = 'variables["mobile_users"]'"#,
                r#"when = 'variables["enterprise_accounts"]'"#,
            ),
    )
    .unwrap();

    let diff = diff_json(&before, &after, &[]);

    let default_change = assert_entity_change(
        &diff,
        "variable_resolve_default_changed",
        "variable",
        &[("id", "support_banner")],
    );
    assert_eq!(default_change["before"], "hidden");
    assert_eq!(default_change["after"], "mobile_help");

    let when_change = changes(&diff)
        .iter()
        .find(|change| {
            change["kind"] == "variable_rule_when_changed"
                && change["target"]["entity"]["kind"] == "rule"
                && change["target"]["entity"]["variable"] == "support_banner"
                && change["target"]["entity"]["index"] == 0
        })
        .expect("support_banner rule when change");
    assert_eq!(when_change["before"], r#"variables["mobile_users"]"#);
    assert_eq!(when_change["after"], r#"variables["enterprise_accounts"]"#);
}

#[tokio::test]
async fn diff_with_contexts_reports_lenient_per_context_impacts() {
    let temp = tempfile::TempDir::new().unwrap();
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    copy_dir(Path::new("examples/basic"), &before);
    copy_dir(Path::new("examples/basic"), &after);

    let variable_path = after.join("variables/premium_message.toml");
    let variable = fs::read_to_string(&variable_path).unwrap();
    fs::write(
        &variable_path,
        variable.replace(
            r#"value = "Welcome back, premium member.""#,
            r#"value = "Welcome back, valued premium member.""#,
        ),
    )
    .unwrap();

    let sample: JsonValue = serde_json::from_str(
        &fs::read_to_string("examples/basic/model/context/request-samples/premium_enterprise.json")
            .unwrap(),
    )
    .unwrap();
    let mut premium_with_lane = sample.clone();
    premium_with_lane["lane"] = JsonValue::from("live");
    let free_user = serde_json::json!({
        "user": { "tier": "free", "region": "us", "language": "en-US" },
        "lane": "live",
    });

    let contexts = vec![
        rototo::model::LabeledContext {
            label: "sample:premium_enterprise".to_owned(),
            context: sample,
        },
        rototo::model::LabeledContext {
            label: "premium_with_lane".to_owned(),
            context: premium_with_lane,
        },
        rototo::model::LabeledContext {
            label: "free_user".to_owned(),
            context: free_user,
        },
    ];
    let diff = rototo::diff_packages_with_contexts(&before, &after, &contexts)
        .await
        .unwrap();
    assert!(diff.impact_error.is_none());
    let json = serde_json::to_value(&diff).unwrap();
    let impacts = json["context_impacts"].as_array().unwrap();
    assert_eq!(impacts.len(), 3);

    // The bare sample lacks `lane`, so lane-reading variables fail on both
    // sides identically: not compared, not an impact. The premium change
    // still surfaces.
    let bare = &impacts[0];
    assert_eq!(bare["context"], "sample:premium_enterprise");
    let bare_impacts = bare["impacts"].as_array().unwrap();
    assert_eq!(bare_impacts.len(), 1, "{bare_impacts:?}");
    assert_eq!(bare_impacts[0]["variable"], "premium_message");
    assert_eq!(
        bare_impacts[0]["before"]["value"],
        "Welcome back, premium member."
    );
    assert_eq!(
        bare_impacts[0]["after"]["value"],
        "Welcome back, valued premium member."
    );
    let full = &impacts[1];
    let bare_compared = bare["compared"].as_u64().unwrap();
    let full_compared = full["compared"].as_u64().unwrap();
    assert!(
        bare_compared < full_compared,
        "the incomplete context must admit its smaller denominator \
         ({bare_compared} vs {full_compared})"
    );

    // A context the changed rule never fires for reports no impact at all.
    let free = &impacts[2];
    assert_eq!(free["context"], "free_user");
    assert_eq!(free["impacts"].as_array().unwrap().len(), 0);
}

fn diff_json(before: &Path, after: &Path, extra_args: &[&str]) -> JsonValue {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let package = repo.join("config");
    copy_dir(before, &package);
    init_git_repo(&repo);
    fs::remove_dir_all(&package).unwrap();
    copy_dir(after, &package);

    let mut command = Command::cargo_bin("rototo").unwrap();
    command
        .current_dir(&repo)
        .arg("--json")
        .arg("diff")
        .arg("config");
    for arg in extra_args {
        if let Some(path) = arg.strip_prefix('@')
            && Path::new(path).is_relative()
        {
            command.arg(format!(
                "@{}",
                Path::new(env!("CARGO_MANIFEST_DIR")).join(path).display()
            ));
        } else {
            command.arg(arg);
        }
    }
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

fn init_git_repo(repo: &Path) {
    git(repo, &["init"]);
    git(repo, &["config", "user.email", "rototo@example.com"]);
    git(repo, &["config", "user.name", "Rototo Tests"]);
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "initial"]);
}

fn git(repo: &Path, args: &[&str]) {
    let output = StdCommand::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run git {args:?}: {err}"));
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
