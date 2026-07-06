mod convert;
mod protocol;
#[cfg(test)]
mod scenario;
mod server;
mod transport;
mod uri;

pub use server::{serve, serve_stdio};

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{Value as JsonValue, json};
    use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

    use super::protocol::{LspCompletionItem, LspDocumentSymbol, LspLocation, initialize_result};
    use super::server::{LspServer, serve};

    #[tokio::test]
    async fn lsp_diagnostics_use_unsaved_overlay_and_clear_by_document() {
        // Editor diagnostics must be based on the text currently open in the
        // editor, even when that text has not been saved back to disk.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("rototo-package.toml"),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[resolve]
default = "hello"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();

        let mut server = LspServer::new();
        server.package_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        let uri = format!("file://{}", variable_path.display());
        // The disk file is valid, but the open editor buffer changes the type
        // to an unknown value. The LSP overlay should win over the file system.
        server
            .open_document(json!({
                "textDocument": {
                    "uri": uri,
                    "version": 7,
                    "text": r#"schema_version = 1
type = "missing"

[resolve]
default = "hello"
"#,
                }
            }))
            .unwrap();

        let publications = server.package_diagnostics().await.unwrap();
        let variable_publication = publications
            .iter()
            .find(|publication| publication.uri.ends_with("/variables/message.toml"))
            .unwrap();
        // The diagnostic belongs to the unsaved document version, and the
        // valid manifest still receives an empty publication so stale editor
        // diagnostics can be cleared.
        assert_eq!(variable_publication.version, Some(7));
        assert_eq!(variable_publication.diagnostics.len(), 1);
        assert_eq!(
            variable_publication.diagnostics[0].code,
            "rototo/variable-unknown-type"
        );
        assert!(publications.iter().any(|publication| {
            publication.uri.ends_with("/rototo-package.toml") && publication.diagnostics.is_empty()
        }));
        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );

        // Full-document didChange replaces the overlay. Restoring the valid
        // text should clear the diagnostic without requiring a save.
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 8
                },
                "contentChanges": [
                    {
                        "text": disk_variable
                    }
                ]
            }))
            .unwrap();
        let cleared = server.package_diagnostics().await.unwrap();
        let variable_publication = cleared
            .iter()
            .find(|publication| publication.uri.ends_with("/variables/message.toml"))
            .unwrap();
        assert_eq!(variable_publication.version, Some(8));
        assert!(variable_publication.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn lsp_snapshot_cache_reuses_until_overlays_change() {
        // The lint pipeline reads the whole package on every build, so requests
        // that do not change any buffer must share one snapshot. Editing a buffer
        // bumps the revision and forces exactly one rebuild on the next request.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(root.join("rototo-package.toml"), "schema_version = 1\n")
            .await
            .unwrap();
        let variable_path = root.join("variables/message.toml");
        let variable_text =
            "schema_version = 1\ntype = \"string\"\n\n[resolve]\ndefault = \"hello\"\n";
        tokio::fs::write(&variable_path, variable_text)
            .await
            .unwrap();

        let mut server = LspServer::new();
        server.package_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        let uri = format!("file://{}", variable_path.display());
        server
            .open_document(json!({
                "textDocument": { "uri": uri, "version": 1, "text": variable_text }
            }))
            .unwrap();

        assert_eq!(server.snapshot_build_count(), 0);
        // Three reads with no intervening edit share a single build.
        for _ in 0..3 {
            server
                .completion_items(json!({
                    "textDocument": { "uri": format!("file://{}", variable_path.display()) },
                    "position": { "line": 1, "character": 0 }
                }))
                .await
                .unwrap();
            server.package_diagnostics().await.unwrap();
        }
        assert_eq!(server.snapshot_build_count(), 1);

        // A full-document change invalidates the cache; the next request rebuilds.
        server
            .change_document(json!({
                "textDocument": { "uri": format!("file://{}", variable_path.display()), "version": 2 },
                "contentChanges": [{ "text": "schema_version = 1\ntype = \"int\"\n\n[resolve]\ndefault = 1\n" }]
            }))
            .unwrap();
        server.package_diagnostics().await.unwrap();
        assert_eq!(server.snapshot_build_count(), 2);
    }

    #[tokio::test]
    async fn lsp_document_symbols_use_snapshot_index_and_unsaved_overlay() {
        // Document symbols power editor outlines. This checks that the outline
        // is built from rototo's semantic snapshot for every package file
        // kind, not from a shallow TOML/JSON parse of the current document.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("model/catalogs"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("data/catalogs/message"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("lint")).await.unwrap();
        let manifest_path = root.join("rototo-package.toml");
        tokio::fs::write(
            &manifest_path,
            r#"schema_version = 1
extends = ["../base"]
"#,
        )
        .await
        .unwrap();
        let condition_path = root.join("variables/premium.toml");
        tokio::fs::write(
            &condition_path,
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "context.account.tier == \"premium\""
value = true
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[resolve]
default = "hello"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();
        let catalog_path = root.join("model/catalogs/message.schema.json");
        tokio::fs::write(
            &catalog_path,
            r#"{
  "type": "object",
  "properties": { "value": { "type": "string" } },
  "required": ["value"],
  "additionalProperties": false
}
"#,
        )
        .await
        .unwrap();
        let catalog_entry_path = root.join("data/catalogs/message/external.toml");
        tokio::fs::write(&catalog_entry_path, r#"value = "external""#)
            .await
            .unwrap();

        let mut server = LspServer::new();
        server.package_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        let variable_uri = format!("file://{}", variable_path.display());
        // The resolve rule exists only in the unsaved overlay. If it appears in
        // the symbols below, the server is indexing editor state correctly.
        server
            .open_document(json!({
                "textDocument": {
                    "uri": variable_uri,
                    "version": 3,
                    "text": r#"schema_version = 1
type = "string"

[resolve]
default = "hello"

[[resolve.rule]]
when = 'variables["premium"]'
value = "welcome"
"#,
                }
            }))
            .unwrap();

        let manifest_symbols = server
            .document_symbols(json!({
                "textDocument": {
                    "uri": format!("file://{}", manifest_path.display())
                }
            }))
            .await
            .unwrap();
        let extends = child_symbol(&manifest_symbols, "extends");
        assert!(extends.children.iter().any(|child| child.name == "../base"));

        // A condition variable is exposed as the named variable in the
        // editor outline.
        let condition_symbols = server
            .document_symbols(json!({
                "textDocument": {
                    "uri": format!("file://{}", condition_path.display())
                }
            }))
            .await
            .unwrap();
        let condition = child_symbol(&condition_symbols, "premium");
        assert!(!condition.children.is_empty());

        // The variable outline includes the unsaved resolve rule, proving that
        // document symbols are snapshot-backed and overlay-aware.
        let variable_symbols = server
            .document_symbols(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                }
            }))
            .await
            .unwrap();
        let variable = child_symbol(&variable_symbols, "message");

        let resolve = child_symbol(&variable.children, "resolve");
        assert!(
            resolve
                .children
                .iter()
                .any(|child| child.name == "rule 1: variables[\"premium\"] -> \"welcome\"")
        );
        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );

        // Catalog entry files are indexed under their catalog-qualified value
        // names so the outline matches rototo's domain model.
        let catalog_entry_symbols = server
            .document_symbols(json!({
                "textDocument": {
                    "uri": format!("file://{}", catalog_entry_path.display())
                }
            }))
            .await
            .unwrap();
        child_symbol(&catalog_entry_symbols, "message.external");
    }

    #[tokio::test]
    async fn completion_scenarios() {
        // Completion behavior is specified by data-driven scenarios under
        // tests/fixtures/lsp/scenarios/completion. Each scenario is a single
        // editor buffer with a `$0` cursor marker and a declarative expectation,
        // so the question under test is legible without reconstructing a buffer
        // from line/character numbers.
        super::scenario::run_completion_scenarios("completion").await;
    }

    #[tokio::test]
    async fn lsp_hover_uses_snapshot_index_and_unsaved_overlays() {
        // Hover should explain the rototo concept under the cursor: descriptions,
        // variable types, rule selections, and lint failures. The source of
        // truth is again the overlay-aware snapshot.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("model/catalogs"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("data/catalogs/message"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("model/context"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("lint")).await.unwrap();
        let manifest_path = root.join("rototo-package.toml");
        tokio::fs::write(
            &manifest_path,
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let condition_path = root.join("variables/premium.toml");
        tokio::fs::write(
            &condition_path,
            r#"schema_version = 1
description = "Premium accounts"
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "context.account.tier == \"premium\""
value = true
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
description = "Disk message"
type = "string"

[resolve]
default = "hello"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();
        tokio::fs::write(
            root.join("model/context/request.schema.json"),
            r#"{
  "type": "object",
  "properties": {
    "account": {
      "type": "object",
      "properties": {
        "tier": { "type": "string" }
      }
    }
  }
}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("lint/message.lua"),
            r#"function register(lint)
  lint:rule({
    id = "operations/message-not-empty",
    title = "Operational message is empty",
    help = "Set a non-empty message before releasing the package.",
    target = "/variables/message",
    handler = "check_variable",
  })
end

function check_variable(package, variable)
  return {}
end
"#,
        )
        .await
        .unwrap();

        let mut server = LspServer::new();
        server.package_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        let variable_uri = format!("file://{}", variable_path.display());
        // The variable description and resolve rule are unsaved. Hover should
        // still show them, which is what an editor user expects while typing.
        server
            .open_document(json!({
                "textDocument": {
                    "uri": variable_uri,
                    "version": 4,
                    "text": r#"schema_version = 1
description = "Overlay message hover"
type = "string"

[resolve]
default = "hello"

[[resolve.rule]]
when = 'variables["premium"]'
value = "welcome"
"#,
                }
            }))
            .unwrap();

        assert_hover_contains(
            &hover_contents(&server, &variable_path, 1, 18).await,
            "Overlay message hover",
        );
        assert_hover_contains(
            &hover_contents(&server, &variable_path, 2, 8).await,
            "Type: `string`",
        );
        assert_hover_contains(
            &hover_contents(&server, &variable_path, 8, 9).await,
            "selects value `\"welcome\"`",
        );
        assert_hover_contains(
            &hover_contents(&server, &condition_path, 1, 17).await,
            "Premium accounts",
        );

        // A later overlay introduces a lint failure. Hovering the invalid type
        // should surface the diagnostic help rather than the normal type text.
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 5
                },
                "contentChanges": [
                    {
                        "text": r#"schema_version = 1
description = "Overlay message hover"
type = "missing"

[resolve]
default = "hello"
"#
                    }
                ]
            }))
            .unwrap();
        assert_hover_contains(
            &hover_contents(&server, &variable_path, 2, 8).await,
            "Variable type is unknown",
        );
        // The invalid overlay is editor state only; the saved package file
        // remains unchanged.
        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );
    }

    #[tokio::test]
    async fn lsp_query_expressions_use_snapshot_index_and_unsaved_overlays() {
        // Query rules are CEL expressions too. Editor features should work when
        // the cursor is inside a query expression, not only inside rule `when`.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("model/catalogs"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("rototo-package.toml"),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let condition_path = root.join("variables/premium.toml");
        tokio::fs::write(
            &condition_path,
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "context.account.tier == \"premium\""
value = true
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("model/catalogs/message.schema.json"),
            r#"{"type":"string"}"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "list<catalog=message>"

[resolve]
default = []
"#;
        let variable_path = root.join("variables/messages.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();

        let mut server = LspServer::new();
        server.package_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        server
            .open_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 8,
                    "text": r#"schema_version = 1
type = "list<catalog=message>"

[resolve]
method = "query"
from = "message"
filter = 'variables["premium"]'
"#,
                }
            }))
            .unwrap();

        let variable_symbols = server
            .document_symbols(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                }
            }))
            .await
            .unwrap();
        let variable = child_symbol(&variable_symbols, "messages");
        let resolve = child_symbol(&variable.children, "resolve");
        assert!(
            resolve
                .children
                .iter()
                .any(|child| child.name == "query: message")
        );

        let completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                },
                "position": {
                    "line": 6,
                    "character": 21
                }
            }))
            .await
            .unwrap();
        assert_completion(&completions, "premium", "variable");

        assert_hover_contains(
            &hover_contents(&server, &variable_path, 6, 21).await,
            "Selects entries from catalog `message` where `variables[\"premium\"]`.",
        );

        let definition = definition_location(&server, &variable_path, 6, 21).await;
        assert!(definition.uri.ends_with("/variables/premium.toml"));

        let references = reference_locations(&server, &variable_path, 6, 21, true).await;
        assert_eq!(references.len(), 2);
        assert!(
            references
                .iter()
                .any(|location| location.uri.ends_with("/variables/premium.toml"))
        );
        assert!(
            references
                .iter()
                .any(|location| location.uri.ends_with("/variables/messages.toml"))
        );

        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );
    }

    #[tokio::test]
    async fn lsp_definition_uses_snapshot_index_and_unsaved_overlays() {
        // Go-to-definition should follow rototo references across package
        // concepts: catalog-backed variable types, rule conditions, and
        // condition composition.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("model/catalogs"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("data/catalogs/message"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("model/context"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("rototo-package.toml"),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let beta_condition_path = root.join("variables/beta.toml");
        tokio::fs::write(
            &beta_condition_path,
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "context.account.beta == true"
value = true
"#,
        )
        .await
        .unwrap();
        let premium_condition_path = root.join("variables/premium.toml");
        tokio::fs::write(
            &premium_condition_path,
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "variables[\"beta\"]"
value = true
"#,
        )
        .await
        .unwrap();
        let schema_path = root.join("model/catalogs/message.schema.json");
        tokio::fs::write(&schema_path, r#"{"type":"string"}"#)
            .await
            .unwrap();
        tokio::fs::write(
            root.join("data/catalogs/message/welcome.toml"),
            r#"value = "welcome""#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[resolve]
default = "hello"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();

        let mut server = LspServer::new();
        server.package_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        // The variable only becomes catalog-backed and condition-referencing in
        // the unsaved editor buffer, so every definition below also checks that
        // go-to-definition is overlay-aware.
        server
            .open_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 6,
                    "text": r#"schema_version = 1
type = "catalog=message"

[resolve]
default = "welcome"

[[resolve.rule]]
when = 'variables["premium"]'
value = "welcome"
"#,
                }
            }))
            .unwrap();

        // The catalog type segment in `catalog=message` jumps to the schema.
        let schema_definition = definition_location(&server, &variable_path, 1, 16).await;
        assert!(
            schema_definition
                .uri
                .ends_with("/catalogs/message.schema.json")
        );

        // A variable resolve rule's condition reference jumps to the variable file.
        let condition_definition = definition_location(&server, &variable_path, 7, 15).await;
        assert!(
            condition_definition
                .uri
                .ends_with("/variables/premium.toml")
        );

        // A composed condition reference jumps to the variable it depends on.
        let when_definition = definition_location(&server, &premium_condition_path, 7, 15).await;
        assert!(when_definition.uri.ends_with("/variables/beta.toml"));

        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );
    }

    #[tokio::test]
    async fn lsp_references_use_snapshot_index_and_unsaved_overlays() {
        // Find-references should use the same semantic index as definitions,
        // but return every declaration/use site for the symbol under the cursor.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("model/catalogs"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("data/catalogs/message"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("model/context"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("rototo-package.toml"),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let beta_condition_path = root.join("variables/beta.toml");
        tokio::fs::write(
            &beta_condition_path,
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "context.account.beta == true"
value = true
"#,
        )
        .await
        .unwrap();
        let gamma_condition_path = root.join("variables/gamma.toml");
        tokio::fs::write(
            &gamma_condition_path,
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "context.account.beta == true"
value = true
"#,
        )
        .await
        .unwrap();
        let premium_condition_path = root.join("variables/premium.toml");
        tokio::fs::write(
            &premium_condition_path,
            r#"schema_version = 1
type = "bool"

[resolve]
default = false

[[resolve.rule]]
when = "variables[\"beta\"]"
value = true
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("model/context/request.schema.json"),
            r#"{"type":"object","properties":{"account":{"type":"object","properties":{"beta":{"type":"boolean"}}}}}"#,
        )
        .await
        .unwrap();
        let message_schema_path = root.join("model/catalogs/message.schema.json");
        tokio::fs::write(&message_schema_path, r#"{"type":"string"}"#)
            .await
            .unwrap();
        tokio::fs::write(
            root.join("data/catalogs/message/welcome.toml"),
            r#"value = "welcome""#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[resolve]
default = "hello"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();

        let mut server = LspServer::new();
        server.package_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        // The variable's catalog type, default value, rule condition, and rule
        // value all live in the overlay. Reference search must include those
        // unsaved use sites.
        server
            .open_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 7,
                    "text": r#"schema_version = 1
type = "catalog=message"

[resolve]
default = "welcome"

[[resolve.rule]]
when = 'variables["premium"]'
value = "welcome"
"#,
                }
            }))
            .unwrap();

        // `beta` is declared in its own variable file and used by `premium`.
        let beta_references = reference_locations(&server, &beta_condition_path, 0, 0, true).await;
        assert_eq!(beta_references.len(), 2);
        assert!(
            beta_references
                .iter()
                .any(|location| location.uri.ends_with("/variables/beta.toml"))
        );
        assert!(
            beta_references
                .iter()
                .any(|location| location.uri.ends_with("/variables/premium.toml"))
        );

        // `premium` is declared as a condition variable and used by the unsaved
        // variable rule.
        let premium_references = reference_locations(&server, &variable_path, 7, 15, true).await;
        assert_eq!(premium_references.len(), 2);
        assert!(
            premium_references
                .iter()
                .any(|location| location.uri.ends_with("/variables/premium.toml"))
        );
        assert!(
            premium_references
                .iter()
                .any(|location| location.uri.ends_with("/variables/message.toml"))
        );

        // The catalog value `welcome` is referenced by the overlay default and
        // rule, and declared by the catalog entry file.
        let catalog_value_references =
            reference_locations(&server, &variable_path, 8, 9, true).await;
        assert_eq!(catalog_value_references.len(), 3);
        assert!(
            catalog_value_references
                .iter()
                .any(|location| location.uri.ends_with("/variables/message.toml"))
        );
        assert!(catalog_value_references.iter().any(|location| {
            location
                .uri
                .ends_with("/data/catalogs/message/welcome.toml")
        }));

        // Context attributes are also indexed. Both condition variables read
        // `account.beta`, so they should both be returned.
        let context_attribute_references =
            reference_locations(&server, &beta_condition_path, 7, 20, true).await;
        assert_eq!(context_attribute_references.len(), 2);
        assert!(
            context_attribute_references
                .iter()
                .any(|location| location.uri.ends_with("/variables/beta.toml"))
        );
        assert!(
            context_attribute_references
                .iter()
                .any(|location| location.uri.ends_with("/variables/gamma.toml"))
        );

        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );
    }

    #[test]
    fn initialize_advertises_completion_provider() {
        // The initialize response is the contract an editor sees before sending
        // feature requests. Keep it aligned with the methods implemented by
        // LspServer.
        let result = initialize_result();
        assert_eq!(
            result
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("positionEncoding"))
                .and_then(JsonValue::as_str),
            Some("utf-16")
        );
        // Incremental sync (kind 2): the server applies ranged didChange edits.
        assert_eq!(
            result
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("textDocumentSync"))
                .and_then(|sync| sync.get("change"))
                .and_then(JsonValue::as_u64),
            Some(2)
        );
        assert_eq!(
            result
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("completionProvider"))
                .and_then(|provider| provider.get("resolveProvider"))
                .and_then(JsonValue::as_bool),
            Some(false)
        );
        assert_eq!(
            result
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("hoverProvider"))
                .and_then(JsonValue::as_bool),
            Some(true)
        );
        assert_eq!(
            result
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("definitionProvider"))
                .and_then(JsonValue::as_bool),
            Some(true)
        );
        assert_eq!(
            result
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("referencesProvider"))
                .and_then(JsonValue::as_bool),
            Some(true)
        );
    }

    #[tokio::test]
    async fn lsp_request_errors_do_not_stop_the_server() {
        // This uses the real JSON-RPC transport loop instead of calling
        // LspServer methods directly. A bad request should return an error
        // response, but the session must stay alive for shutdown.
        let (client_io, server_io) = tokio::io::duplex(8192);
        let (client_read, mut client_write) = tokio::io::split(client_io);
        let mut client_read = BufReader::new(client_read);
        let (server_read, server_write) = tokio::io::split(server_io);
        let server =
            tokio::spawn(async move { serve(BufReader::new(server_read), server_write).await });

        // No package has been initialized, so a document request fails.
        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "textDocument/documentSymbol",
                "params": {
                    "textDocument": {
                        "uri": "file:///tmp/outside.toml"
                    }
                }
            }),
        )
        .await;
        let failed = read_lsp_message(&mut client_read).await;
        assert_eq!(failed["id"], 1);
        assert_eq!(failed["error"]["code"], -32603);

        // The server should still accept later requests after the failed one.
        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "shutdown"
            }),
        )
        .await;
        let shutdown = read_lsp_message(&mut client_read).await;
        assert_eq!(shutdown["id"], 2);
        assert!(shutdown["result"].is_null());

        // LSP exits only after shutdown; reaching this await proves the server
        // loop terminated cleanly.
        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "method": "exit"
            }),
        )
        .await;
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn exit_before_shutdown_is_a_protocol_error() {
        // The LSP lifecycle is initialize .. shutdown -> exit. An exit
        // notification with no shutdown first ends the session with an error
        // instead of a clean break, so a crashing editor is distinguishable
        // from an orderly close.
        let (client_io, server_io) = tokio::io::duplex(8192);
        let (_client_read, mut client_write) = tokio::io::split(client_io);
        let (server_read, server_write) = tokio::io::split(server_io);
        let server =
            tokio::spawn(async move { serve(BufReader::new(server_read), server_write).await });

        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "method": "exit"
            }),
        )
        .await;

        let outcome = server.await.unwrap();
        let err = outcome.expect_err("exit before shutdown should error");
        assert!(err.to_string().contains("exit received before shutdown"));
    }

    #[tokio::test]
    async fn applies_incremental_document_changes() {
        // A ranged didChange splices the open buffer instead of resending the
        // whole document. The package root is synthetic: the buffers never touch
        // disk, so this isolates the edit application.
        let mut server = LspServer::new();
        server.package_root = Some(std::path::PathBuf::from("/pkg"));
        server
            .open_document(json!({
                "textDocument": {
                    "uri": "file:///pkg/variables/q.toml",
                    "text": "schema_version = 1\nwhen = \"a\"\n",
                    "version": 1
                }
            }))
            .unwrap();

        // Replace the single character `a` on line 1 (columns 8..9) with `premium`.
        server
            .change_document(json!({
                "textDocument": { "uri": "file:///pkg/variables/q.toml", "version": 2 },
                "contentChanges": [{
                    "range": {
                        "start": { "line": 1, "character": 8 },
                        "end": { "line": 1, "character": 9 }
                    },
                    "text": "premium"
                }]
            }))
            .unwrap();

        assert_eq!(
            server.overlay_text("variables/q.toml"),
            Some("schema_version = 1\nwhen = \"premium\"\n")
        );

        // A change without a range still replaces the whole document.
        server
            .change_document(json!({
                "textDocument": { "uri": "file:///pkg/variables/q.toml", "version": 3 },
                "contentChanges": [{ "text": "schema_version = 1\n" }]
            }))
            .unwrap();
        assert_eq!(
            server.overlay_text("variables/q.toml"),
            Some("schema_version = 1\n")
        );
    }

    #[tokio::test]
    async fn incremental_change_positions_count_utf16_code_units() {
        // The astral `😀` is one scalar but two UTF-16 code units, so the edit
        // columns after it must account for both units, matching the encoding the
        // server advertises.
        let mut server = LspServer::new();
        server.package_root = Some(std::path::PathBuf::from("/pkg"));
        server
            .open_document(json!({
                "textDocument": {
                    "uri": "file:///pkg/q.toml",
                    "text": "x = \"😀ab\"",
                    "version": 1
                }
            }))
            .unwrap();

        // Replace `ab` (columns 7..9 in UTF-16) with `Z`.
        server
            .change_document(json!({
                "textDocument": { "uri": "file:///pkg/q.toml", "version": 2 },
                "contentChanges": [{
                    "range": {
                        "start": { "line": 0, "character": 7 },
                        "end": { "line": 0, "character": 9 }
                    },
                    "text": "Z"
                }]
            }))
            .unwrap();

        assert_eq!(server.overlay_text("q.toml"), Some("x = \"😀Z\""));
    }

    #[tokio::test]
    async fn lsp_publishes_diagnostics_asynchronously_and_supersedes_stale_edits() {
        // didChange returns immediately; diagnostics arrive later from a
        // debounced background build, and a quick follow-up edit supersedes
        // the previous one so only the final buffers are published.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(root.join("rototo-package.toml"), "schema_version = 1\n")
            .await
            .unwrap();
        let variable_path = root.join("variables/flag.toml");
        let clean = "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = false\n";
        tokio::fs::write(&variable_path, clean).await.unwrap();

        let (client_io, server_io) = tokio::io::duplex(65536);
        let (client_read, mut client_write) = tokio::io::split(client_io);
        let mut client_read = BufReader::new(client_read);
        let (server_read, server_write) = tokio::io::split(server_io);
        let server =
            tokio::spawn(async move { serve(BufReader::new(server_read), server_write).await });

        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "rootUri": format!("file://{}", root.display()) }
            }),
        )
        .await;
        let initialized = read_lsp_message(&mut client_read).await;
        assert_eq!(initialized["id"], 1);

        let uri = format!("file://{}", variable_path.display());
        // Open with a broken buffer, then immediately fix it: the broken
        // generation is superseded, so the published diagnostics are clean.
        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "version": 1,
                        "text": "schema_version = 1\ntype = \"bool\"\n"
                    }
                }
            }),
        )
        .await;
        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": uri, "version": 2 },
                    "contentChanges": [{ "text": clean }]
                }
            }),
        )
        .await;

        // A concurrent read answered while diagnostics are still pending.
        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/documentSymbol",
                "params": { "textDocument": { "uri": uri } }
            }),
        )
        .await;

        let mut symbol_response = None;
        let mut flag_diagnostics = None;
        while symbol_response.is_none() || flag_diagnostics.is_none() {
            let message = read_lsp_message(&mut client_read).await;
            if message["id"] == 2 {
                symbol_response = Some(message);
            } else if message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"]
                    .as_str()
                    .is_some_and(|published| published.ends_with("variables/flag.toml"))
            {
                flag_diagnostics = Some(message);
            }
        }
        let symbols = symbol_response.unwrap();
        assert!(symbols["result"].is_array());
        // The superseding edit fixed the file, so the eventual publication for
        // it carries no diagnostics.
        let diagnostics = flag_diagnostics.unwrap();
        assert_eq!(
            diagnostics["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .len(),
            0,
            "{diagnostics:#}"
        );

        write_lsp_message(
            &mut client_write,
            json!({ "jsonrpc": "2.0", "id": 9, "method": "shutdown" }),
        )
        .await;
        loop {
            let message = read_lsp_message(&mut client_read).await;
            if message["id"] == 9 {
                break;
            }
        }
        write_lsp_message(
            &mut client_write,
            json!({ "jsonrpc": "2.0", "method": "exit" }),
        )
        .await;
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn lsp_answers_concurrent_reads_in_any_order() {
        // Two reads sent back to back both get responses; the server no longer
        // requires the first to finish before the second starts.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(root.join("rototo-package.toml"), "schema_version = 1\n")
            .await
            .unwrap();
        let variable_path = root.join("variables/flag.toml");
        tokio::fs::write(
            &variable_path,
            "schema_version = 1\ntype = \"bool\"\n\n[resolve]\ndefault = false\n",
        )
        .await
        .unwrap();

        let (client_io, server_io) = tokio::io::duplex(65536);
        let (client_read, mut client_write) = tokio::io::split(client_io);
        let mut client_read = BufReader::new(client_read);
        let (server_read, server_write) = tokio::io::split(server_io);
        let server =
            tokio::spawn(async move { serve(BufReader::new(server_read), server_write).await });

        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "rootUri": format!("file://{}", root.display()) }
            }),
        )
        .await;
        read_lsp_message(&mut client_read).await;

        let uri = format!("file://{}", variable_path.display());
        for id in [2, 3] {
            write_lsp_message(
                &mut client_write,
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "textDocument/documentSymbol",
                    "params": { "textDocument": { "uri": uri } }
                }),
            )
            .await;
        }

        let mut seen = std::collections::BTreeSet::new();
        while seen.len() < 2 {
            let message = read_lsp_message(&mut client_read).await;
            if let Some(id) = message["id"].as_i64() {
                assert!(message["result"].is_array(), "{message:#}");
                seen.insert(id);
            }
        }
        assert_eq!(seen, [2, 3].into_iter().collect());

        write_lsp_message(
            &mut client_write,
            json!({ "jsonrpc": "2.0", "id": 9, "method": "shutdown" }),
        )
        .await;
        loop {
            let message = read_lsp_message(&mut client_read).await;
            if message["id"] == 9 {
                break;
            }
        }
        write_lsp_message(
            &mut client_write,
            json!({ "jsonrpc": "2.0", "method": "exit" }),
        )
        .await;
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn lsp_cancelled_request_returns_request_cancelled() {
        // A request whose cancellation the server has already seen returns the
        // RequestCancelled error instead of a result. Sending the cancel ahead of
        // the request makes the outcome deterministic: it exercises the same
        // short-circuit the read-ahead loop reaches for the realistic order where
        // the cancel arrives just after the request.
        let (client_io, server_io) = tokio::io::duplex(8192);
        let (client_read, mut client_write) = tokio::io::split(client_io);
        let mut client_read = BufReader::new(client_read);
        let (server_read, server_write) = tokio::io::split(server_io);
        let server =
            tokio::spawn(async move { serve(BufReader::new(server_read), server_write).await });

        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "method": "$/cancelRequest",
                "params": { "id": 1 }
            }),
        )
        .await;
        write_lsp_message(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "textDocument/documentSymbol",
                "params": { "textDocument": { "uri": "file:///tmp/outside.toml" } }
            }),
        )
        .await;

        let cancelled = read_lsp_message(&mut client_read).await;
        assert_eq!(cancelled["id"], 1);
        assert_eq!(cancelled["error"]["code"], -32800);

        // The server is still healthy after a cancellation.
        write_lsp_message(
            &mut client_write,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" }),
        )
        .await;
        let shutdown = read_lsp_message(&mut client_read).await;
        assert_eq!(shutdown["id"], 2);
        assert!(shutdown["result"].is_null());

        write_lsp_message(
            &mut client_write,
            json!({ "jsonrpc": "2.0", "method": "exit" }),
        )
        .await;
        server.await.unwrap().unwrap();
    }

    fn child_symbol<'a>(symbols: &'a [LspDocumentSymbol], name: &str) -> &'a LspDocumentSymbol {
        symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("missing symbol {name}"))
    }

    fn assert_completion(completions: &[LspCompletionItem], label: &str, detail: &str) {
        assert!(
            completions
                .iter()
                .any(|completion| completion.label == label && completion.detail == detail),
            "missing completion {label} ({detail})"
        );
    }

    async fn hover_contents(
        server: &LspServer,
        path: &Path,
        line: usize,
        character: usize,
    ) -> String {
        server
            .hover(json!({
                "textDocument": {
                    "uri": format!("file://{}", path.display())
                },
                "position": {
                    "line": line,
                    "character": character
                }
            }))
            .await
            .unwrap()
            .expect("hover result")
            .contents
            .value
    }

    async fn definition_location(
        server: &LspServer,
        path: &Path,
        line: usize,
        character: usize,
    ) -> LspLocation {
        server
            .definition(json!({
                "textDocument": {
                    "uri": format!("file://{}", path.display())
                },
                "position": {
                    "line": line,
                    "character": character
                }
            }))
            .await
            .unwrap()
            .expect("definition result")
    }

    async fn reference_locations(
        server: &LspServer,
        path: &Path,
        line: usize,
        character: usize,
        include_declaration: bool,
    ) -> Vec<LspLocation> {
        server
            .references(json!({
                "textDocument": {
                    "uri": format!("file://{}", path.display())
                },
                "position": {
                    "line": line,
                    "character": character
                },
                "context": {
                    "includeDeclaration": include_declaration
                }
            }))
            .await
            .unwrap()
    }

    fn assert_hover_contains(contents: &str, expected: &str) {
        assert!(
            contents.contains(expected),
            "hover did not contain {expected:?}: {contents}"
        );
    }

    async fn write_lsp_message<W>(writer: &mut W, message: JsonValue)
    where
        W: AsyncWrite + Unpin,
    {
        let body = serde_json::to_vec(&message).unwrap();
        writer
            .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await
            .unwrap();
        writer.write_all(&body).await.unwrap();
        writer.flush().await.unwrap();
    }

    /// Read one framed message from a persistent buffered reader. The reader
    /// must live across calls: with the concurrent server, frames arrive back
    /// to back, and a per-call BufReader would drop what it read ahead.
    async fn read_lsp_message<R>(reader: &mut R) -> JsonValue
    where
        R: AsyncBufRead + Unpin,
    {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                content_length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
        let mut body = vec![0; content_length.unwrap()];
        tokio::io::AsyncReadExt::read_exact(reader, &mut body)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }
}
