//! Data-driven harness for LSP feature tests.
//!
//! A scenario is a single TOML file under `tests/fixtures/lsp/scenarios/<category>`.
//! It names a fixture package, an edited file, and the open editor buffer for that
//! file with a `$0` marker where the cursor sits. Expectations are declared as data,
//! so each file answers exactly one "what is suggested when the cursor is here?"
//! question without the reader reconstructing a buffer from line/character numbers.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;

use super::protocol::LspCompletionItem;
use super::server::LspServer;

/// Marker that locates the cursor inside an overlay buffer. Stripped before linting.
const CURSOR_MARKER: &str = "$0";

#[derive(Deserialize)]
struct Scenario {
    package: String,
    file: String,
    /// The open editor buffer for `file`, containing exactly one `$0` cursor marker.
    overlay: String,
    #[serde(default)]
    also_open: Vec<OpenDocument>,
    expect: Expect,
}

#[derive(Deserialize)]
struct OpenDocument {
    file: String,
    text: String,
}

#[derive(Default, Deserialize)]
struct Expect {
    #[serde(default)]
    includes: Vec<ExpectItem>,
    #[serde(default)]
    excludes: Vec<ExpectItem>,
    /// When set, the returned labels must equal exactly this set (order-insensitive).
    #[serde(default)]
    exact: Option<Vec<String>>,
    /// Label of the item whose `textEdit` range is asserted against `replaces`.
    #[serde(default)]
    selecting: Option<String>,
    /// The text the selected item's `textEdit` range must cover, ending at `$0`.
    #[serde(default)]
    replaces: Option<String>,
}

#[derive(Deserialize)]
struct ExpectItem {
    label: String,
    detail: String,
    /// Optional: assert the item's `insertText`.
    insert_text: Option<String>,
}

/// Run every completion scenario in the given category, asserting each in turn.
pub(super) async fn run_completion_scenarios(category: &str) {
    let scenarios = load(category);
    assert!(
        !scenarios.is_empty(),
        "no scenarios found for category {category:?}"
    );
    for (path, scenario) in scenarios {
        run_completion(&path, &scenario).await;
    }
}

fn load(category: &str) -> Vec<(PathBuf, Scenario)> {
    let pattern = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lsp/scenarios")
        .join(category)
        .join("*.toml");
    let mut scenarios = glob::glob(pattern.to_str().expect("scenario glob is valid UTF-8"))
        .expect("scenario glob pattern compiles")
        .map(|entry| {
            let path = entry.expect("scenario path is readable");
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("read scenario {}: {err}", path.display()));
            let scenario: Scenario = toml::from_str(&text)
                .unwrap_or_else(|err| panic!("parse scenario {}: {err}", path.display()));
            (path, scenario)
        })
        .collect::<Vec<_>>();
    scenarios.sort_by(|(left, _), (right, _)| left.cmp(right));
    scenarios
}

async fn run_completion(path: &Path, scenario: &Scenario) {
    let name = path.file_name().unwrap().to_string_lossy();
    let tempdir = tempfile::tempdir().unwrap();
    let root = tempdir.path();
    copy_package(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/lsp/packages")
            .join(&scenario.package),
        root,
    );

    let mut server = LspServer::new();
    server.package_root = Some(tokio::fs::canonicalize(root).await.unwrap());

    let (buffer, position) = split_cursor(&scenario.overlay).unwrap_or_else(|| {
        panic!("scenario {name}: overlay is missing the {CURSOR_MARKER} marker")
    });
    server
        .open_document(json!({
            "textDocument": {
                "uri": file_uri(root, &scenario.file),
                "version": 1,
                "text": buffer,
            }
        }))
        .unwrap();
    for (index, document) in scenario.also_open.iter().enumerate() {
        server
            .open_document(json!({
                "textDocument": {
                    "uri": file_uri(root, &document.file),
                    "version": (index as i64) + 2,
                    "text": document.text,
                }
            }))
            .unwrap();
    }

    let completions = server
        .completion_items(json!({
            "textDocument": { "uri": file_uri(root, &scenario.file) },
            "position": { "line": position.0, "character": position.1 },
        }))
        .await
        .unwrap();

    assert_expectations(&name, &scenario.expect, position, &completions);
}

fn assert_expectations(
    name: &str,
    expect: &Expect,
    position: (usize, usize),
    completions: &[LspCompletionItem],
) {
    for item in &expect.includes {
        let found = completions
            .iter()
            .find(|c| c.label == item.label && c.detail == item.detail);
        let found = found.unwrap_or_else(|| {
            panic!(
                "scenario {name}: missing completion {} ({})\ngot: {}",
                item.label,
                item.detail,
                describe(completions)
            )
        });
        if let Some(insert_text) = &item.insert_text {
            assert_eq!(
                found.insert_text.as_deref(),
                Some(insert_text.as_str()),
                "scenario {name}: completion {} has wrong insert text",
                item.label
            );
        }
    }
    for item in &expect.excludes {
        assert!(
            !completions
                .iter()
                .any(|c| c.label == item.label && c.detail == item.detail),
            "scenario {name}: unexpected completion {} ({})",
            item.label,
            item.detail
        );
    }
    if let Some(exact) = &expect.exact {
        let mut got = completions
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>();
        got.sort();
        let mut want = exact.clone();
        want.sort();
        assert_eq!(got, want, "scenario {name}: completion label set mismatch");
    }
    if let Some(selecting) = &expect.selecting {
        let item = completions
            .iter()
            .find(|c| &c.label == selecting)
            .unwrap_or_else(|| panic!("scenario {name}: no completion labeled {selecting}"));
        let edit = item
            .text_edit
            .as_ref()
            .unwrap_or_else(|| panic!("scenario {name}: {selecting} has no textEdit"));
        let (line, cursor) = position;
        let replaces = expect.replaces.clone().unwrap_or_default();
        let start = cursor - replaces.encode_utf16().count();
        assert_eq!(
            (edit.range.start.line, edit.range.start.character),
            (line, start),
            "scenario {name}: {selecting} textEdit start covers the wrong span"
        );
        assert_eq!(
            (edit.range.end.line, edit.range.end.character),
            (line, cursor),
            "scenario {name}: {selecting} textEdit end is not at the cursor"
        );
    }
}

fn describe(completions: &[LspCompletionItem]) -> String {
    completions
        .iter()
        .map(|c| format!("{} ({})", c.label, c.detail))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Strip the `$0` marker and return the cleaned buffer plus the `(line, utf16_column)`
/// cursor position. LSP positions use UTF-16 code units, so the column is measured in
/// UTF-16 units of the text preceding the marker on its line.
fn split_cursor(overlay: &str) -> Option<(String, (usize, usize))> {
    let marker = overlay.find(CURSOR_MARKER)?;
    let before = &overlay[..marker];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let column = before[line_start..].encode_utf16().count();
    let mut buffer = String::with_capacity(overlay.len() - CURSOR_MARKER.len());
    buffer.push_str(before);
    buffer.push_str(&overlay[marker + CURSOR_MARKER.len()..]);
    Some((buffer, (line, column)))
}

fn file_uri(root: &Path, file: &str) -> String {
    format!("file://{}", root.join(file).display())
}

fn copy_package(source: &Path, destination: &Path) {
    for entry in std::fs::read_dir(source)
        .unwrap_or_else(|err| panic!("read fixture {}: {err}", source.display()))
    {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_package(&entry.path(), &target);
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}
