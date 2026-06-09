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
            r#"premium = "Welcome back, premium member.""#,
            r#"premium = "Welcome back, valued premium member.""#,
        ),
    )
    .unwrap();

    let output = Command::cargo_bin("rototo")
        .unwrap()
        .args([
            "--json",
            "diff",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
            "--context",
            "@examples/basic/contexts/premium-enterprise.json",
            "--context",
            "lane=stage",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let diff: JsonValue = serde_json::from_slice(&output).unwrap();

    let changes = diff["changes"].as_array().unwrap();
    let value_change = changes
        .iter()
        .find(|change| {
            change["kind"] == "variable_value_changed"
                && change["target"]["entity"]["kind"] == "value"
                && change["target"]["entity"]["variable"] == "premium-message"
                && change["target"]["entity"]["key"] == "premium"
        })
        .expect("premium-message value change");
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
    assert_eq!(impact["before"]["value_key"], "premium");
    assert_eq!(impact["after"]["value_key"], "premium");
    assert_eq!(impact["before"]["value"], "Welcome back, premium member.");
    assert_eq!(
        impact["after"]["value"],
        "Welcome back, valued premium member."
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
