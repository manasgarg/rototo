mod convert;
mod protocol;
mod server;
mod transport;
mod uri;

#[cfg(feature = "console")]
pub(crate) use server::serve;
pub use server::serve_stdio;
#[cfg(feature = "console")]
pub(crate) use transport::{read_message, write_notification, write_request};

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{Value as JsonValue, json};
    use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

    use super::protocol::{LspCompletionItem, LspDocumentSymbol, LspLocation, initialize_result};
    use super::server::{LspServer, serve};

    #[tokio::test]
    async fn lsp_diagnostics_use_unsaved_overlay_and_clear_by_document() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("rototo-workspace.toml"),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[values]
control = "hello"

[resolve]
default = "control"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();

        let mut server = LspServer::new();
        server.workspace_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        let uri = format!("file://{}", variable_path.display());
        server
            .open_document(json!({
                "textDocument": {
                    "uri": uri,
                    "version": 7,
                    "text": r#"schema_version = 1
type = "missing"

[values]
control = "hello"

[resolve]
default = "control"
"#,
                }
            }))
            .unwrap();

        let publications = server.workspace_diagnostics().await.unwrap();
        let variable_publication = publications
            .iter()
            .find(|publication| publication.uri.ends_with("/variables/message.toml"))
            .unwrap();
        assert_eq!(variable_publication.version, Some(7));
        assert_eq!(variable_publication.diagnostics.len(), 1);
        assert_eq!(
            variable_publication.diagnostics[0].code,
            "rototo/variable-unknown-type"
        );
        assert!(publications.iter().any(|publication| {
            publication.uri.ends_with("/rototo-workspace.toml")
                && publication.diagnostics.is_empty()
        }));
        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );

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
        let cleared = server.workspace_diagnostics().await.unwrap();
        let variable_publication = cleared
            .iter()
            .find(|publication| publication.uri.ends_with("/variables/message.toml"))
            .unwrap();
        assert_eq!(variable_publication.version, Some(8));
        assert!(variable_publication.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn lsp_document_symbols_use_snapshot_index_and_unsaved_overlay() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("qualifiers"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("catalogs/message-entries"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("lint")).await.unwrap();
        let manifest_path = root.join("rototo-workspace.toml");
        tokio::fs::write(
            &manifest_path,
            r#"schema_version = 1
extends = ["../base"]
"#,
        )
        .await
        .unwrap();
        let qualifier_path = root.join("qualifiers/premium.toml");
        tokio::fs::write(
            &qualifier_path,
            r#"schema_version = 1

[[predicate]]
attribute = "account.tier"
op = "eq"
value = "premium"
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[values]
control = "hello"

[resolve]
default = "control"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();
        let catalog_path = root.join("catalogs/message.toml");
        tokio::fs::write(
            &catalog_path,
            r#"schema_version = 1
schema = "../schemas/message.schema.json"
"#,
        )
        .await
        .unwrap();
        let catalog_entry_path = root.join("catalogs/message-entries/external.toml");
        tokio::fs::write(&catalog_entry_path, r#"value = "external""#)
            .await
            .unwrap();

        let mut server = LspServer::new();
        server.workspace_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        let variable_uri = format!("file://{}", variable_path.display());
        server
            .open_document(json!({
                "textDocument": {
                    "uri": variable_uri,
                    "version": 3,
                    "text": r#"schema_version = 1
type = "string"

[values]
control = "hello"
treatment = "welcome"

[resolve]
default = "control"

[[resolve.rule]]
qualifier = "premium"
value = "treatment"
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

        let qualifier_symbols = server
            .document_symbols(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                }
            }))
            .await
            .unwrap();
        let qualifier = child_symbol(&qualifier_symbols, "premium");
        assert!(
            qualifier
                .children
                .iter()
                .any(|child| child.name == "predicate 1: account.tier eq")
        );

        let variable_symbols = server
            .document_symbols(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                }
            }))
            .await
            .unwrap();
        let variable = child_symbol(&variable_symbols, "message");
        let values = child_symbol(&variable.children, "values");
        let treatment = child_symbol(&values.children, "treatment");
        assert_eq!(treatment.range.start.line, 5);

        let resolve = child_symbol(&variable.children, "resolve");
        assert!(
            resolve
                .children
                .iter()
                .any(|child| child.name == "rule 1: premium -> treatment")
        );
        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );

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
    async fn lsp_completion_items_use_snapshot_index_and_unsaved_overlays() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("qualifiers"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("lint")).await.unwrap();
        let manifest_path = root.join("rototo-workspace.toml");
        let disk_manifest = r#"schema_version = 1
"#;
        tokio::fs::write(&manifest_path, disk_manifest)
            .await
            .unwrap();
        tokio::fs::write(
            root.join("qualifiers/premium.toml"),
            r#"schema_version = 1

[[predicate]]
attribute = "account.tier"
op = "eq"
value = "premium"
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[values]
control = "hello"

[resolve]
default = "control"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();
        let lint_path = root.join("lint/fields.lua");
        tokio::fs::write(
            &lint_path,
            r#"function register(lint)
end
"#,
        )
        .await
        .unwrap();

        let mut server = LspServer::new();
        server.workspace_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        server
            .open_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", manifest_path.display()),
                    "version": 2,
                    "text": r#"schema_version = 1
extends = ["../base"]
"#,
                }
            }))
            .unwrap();
        server
            .open_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 3,
                    "text": r#"schema_version = 1
type = "string"

[values]
control = "hello"
treatment = "welcome"

[resolve]
default = "control"

[[resolve.rule]]
qualifier = "premium"
value = "treatment"
"#,
                }
            }))
            .unwrap();

        let completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                },
                "position": {
                    "line": 10,
                    "character": 8
                }
            }))
            .await
            .unwrap();

        assert_no_completion(&completions, "../base", "workspace extend");
        assert_completion(&completions, "premium", "qualifier");
        assert_completion(&completions, "treatment", "variable value");
        assert_no_completion(&completions, "bucket", "predicate operator");
        assert_no_completion(&completions, "extends", "custom lint field selector");

        let qualifier_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", root.join("qualifiers/premium.toml").display())
                },
                "position": {
                    "line": 4,
                    "character": 6
                }
            }))
            .await
            .unwrap();
        assert_completion(&qualifier_completions, "bucket", "predicate operator");
        assert_completion(&qualifier_completions, "premium", "qualifier");
        assert_no_completion(&qualifier_completions, "treatment", "variable value");

        let custom_lint_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", lint_path.display())
                },
                "position": {
                    "line": 1,
                    "character": 0
                }
            }))
            .await
            .unwrap();
        assert_completion(
            &custom_lint_completions,
            "extends",
            "custom lint field selector",
        );
        assert_completion(
            &custom_lint_completions,
            "resolve",
            "custom lint field selector",
        );
        assert_completion(
            &custom_lint_completions,
            "value.",
            "custom lint field selector",
        );
        assert_no_completion(&custom_lint_completions, "bucket", "predicate operator");
        assert_eq!(
            tokio::fs::read_to_string(&manifest_path).await.unwrap(),
            disk_manifest
        );
        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );
    }

    #[tokio::test]
    async fn lsp_hover_uses_snapshot_index_and_unsaved_overlays() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("qualifiers"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("schemas"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("lint")).await.unwrap();
        let manifest_path = root.join("rototo-workspace.toml");
        tokio::fs::write(
            &manifest_path,
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let qualifier_path = root.join("qualifiers/premium.toml");
        tokio::fs::write(
            &qualifier_path,
            r#"schema_version = 1
description = "Premium accounts"

[[predicate]]
attribute = "account.tier"
op = "eq"
value = "premium"
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
description = "Disk message"
type = "string"

[values]
control = "hello"

[resolve]
default = "control"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();
        tokio::fs::write(
            root.join("schemas/context.schema.json"),
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
  lint:on({
    stage = "policy",
    entity = "variable",
    rule = {
      id = "operations/message-not-empty",
      title = "Operational message is empty",
      help = "Set a non-empty message before releasing the workspace.",
    },
    handler = "check_variable",
  })
end

function check_variable(variable)
  return {}
end
"#,
        )
        .await
        .unwrap();

        let mut server = LspServer::new();
        server.workspace_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        let variable_uri = format!("file://{}", variable_path.display());
        server
            .open_document(json!({
                "textDocument": {
                    "uri": variable_uri,
                    "version": 4,
                    "text": r#"schema_version = 1
description = "Overlay message hover"
type = "string"

[values]
control = "hello"
treatment = "welcome"

[resolve]
default = "control"

[[resolve.rule]]
qualifier = "premium"
value = "treatment"
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
            &hover_contents(&server, &variable_path, 6, 14).await,
            "Value `treatment`",
        );
        assert_hover_contains(
            &hover_contents(&server, &qualifier_path, 1, 17).await,
            "Premium accounts",
        );
        assert_hover_contains(
            &hover_contents(&server, &qualifier_path, 4, 14).await,
            "Predicate 1",
        );

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

[values]
control = "hello"

[resolve]
default = "control"
"#
                    }
                ]
            }))
            .unwrap();
        assert_hover_contains(
            &hover_contents(&server, &variable_path, 2, 8).await,
            "Variable type is unknown",
        );
        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );
    }

    #[tokio::test]
    async fn lsp_rejects_incremental_did_change_ranges() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        let variable_path = root.join("variables/message.toml");

        let mut server = LspServer::new();
        server.workspace_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        let err = server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 2
                },
                "contentChanges": [
                    {
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 0 }
                        },
                        "text": "schema_version = 1"
                    }
                ]
            }))
            .unwrap_err();

        assert!(err.to_string().contains("incremental didChange"));
    }

    #[tokio::test]
    async fn lsp_definition_uses_snapshot_index_and_unsaved_overlays() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("qualifiers"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("schemas"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("rototo-workspace.toml"),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let beta_qualifier_path = root.join("qualifiers/beta.toml");
        tokio::fs::write(
            &beta_qualifier_path,
            r#"schema_version = 1

[[predicate]]
attribute = "account.beta"
op = "eq"
value = true
"#,
        )
        .await
        .unwrap();
        let premium_qualifier_path = root.join("qualifiers/premium.toml");
        tokio::fs::write(
            &premium_qualifier_path,
            r#"schema_version = 1

[[predicate]]
attribute = "qualifier.beta"
op = "eq"
value = true
"#,
        )
        .await
        .unwrap();
        let schema_path = root.join("schemas/message.schema.json");
        tokio::fs::write(&schema_path, r#"{"type":"string"}"#)
            .await
            .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[values]
control = "hello"

[resolve]
default = "control"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();

        let mut server = LspServer::new();
        server.workspace_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        server
            .open_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 6,
                    "text": r#"schema_version = 1
schema = "../schemas/message.schema.json"

[values]
control = "hello"
treatment = "welcome"

[resolve]
default = "control"

[[resolve.rule]]
qualifier = "premium"
value = "treatment"
"#,
                }
            }))
            .unwrap();

        let schema_definition = definition_location(&server, &variable_path, 1, 12).await;
        assert!(
            schema_definition
                .uri
                .ends_with("/schemas/message.schema.json")
        );

        let qualifier_definition = definition_location(&server, &variable_path, 11, 18).await;
        assert!(
            qualifier_definition
                .uri
                .ends_with("/qualifiers/premium.toml")
        );

        let value_definition = definition_location(&server, &variable_path, 12, 9).await;
        assert!(value_definition.uri.ends_with("/variables/message.toml"));
        assert_eq!(value_definition.range.start.line, 5);

        let predicate_definition =
            definition_location(&server, &premium_qualifier_path, 3, 14).await;
        assert!(predicate_definition.uri.ends_with("/qualifiers/beta.toml"));

        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );
    }

    #[tokio::test]
    async fn lsp_references_use_snapshot_index_and_unsaved_overlays() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("qualifiers"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("schemas"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("rototo-workspace.toml"),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let beta_qualifier_path = root.join("qualifiers/beta.toml");
        tokio::fs::write(
            &beta_qualifier_path,
            r#"schema_version = 1

[[predicate]]
attribute = "account.beta"
op = "eq"
value = true
"#,
        )
        .await
        .unwrap();
        let gamma_qualifier_path = root.join("qualifiers/gamma.toml");
        tokio::fs::write(
            &gamma_qualifier_path,
            r#"schema_version = 1

[[predicate]]
attribute = "account.beta"
op = "eq"
value = true
"#,
        )
        .await
        .unwrap();
        let premium_qualifier_path = root.join("qualifiers/premium.toml");
        tokio::fs::write(
            &premium_qualifier_path,
            r#"schema_version = 1

[[predicate]]
attribute = "qualifier.beta"
op = "eq"
value = true
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("schemas/context.schema.json"),
            r#"{"type":"object","properties":{"account":{"type":"object","properties":{"beta":{"type":"boolean"}}}}}"#,
        )
        .await
        .unwrap();
        let message_schema_path = root.join("schemas/message.schema.json");
        tokio::fs::write(&message_schema_path, r#"{"type":"string"}"#)
            .await
            .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[values]
control = "hello"

[resolve]
default = "control"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();

        let mut server = LspServer::new();
        server.workspace_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        server
            .open_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 7,
                    "text": r#"schema_version = 1
schema = "../schemas/message.schema.json"

[values]
control = "hello"
treatment = "welcome"

[resolve]
default = "control"

[[resolve.rule]]
qualifier = "premium"
value = "treatment"
"#,
                }
            }))
            .unwrap();

        let beta_references = reference_locations(&server, &beta_qualifier_path, 0, 0, true).await;
        assert_eq!(beta_references.len(), 2);
        assert!(
            beta_references
                .iter()
                .any(|location| location.uri.ends_with("/qualifiers/beta.toml"))
        );
        assert!(
            beta_references
                .iter()
                .any(|location| location.uri.ends_with("/qualifiers/premium.toml"))
        );

        let premium_references = reference_locations(&server, &variable_path, 11, 18, true).await;
        assert_eq!(premium_references.len(), 2);
        assert!(
            premium_references
                .iter()
                .any(|location| location.uri.ends_with("/qualifiers/premium.toml"))
        );
        assert!(
            premium_references
                .iter()
                .any(|location| location.uri.ends_with("/variables/message.toml"))
        );

        let treatment_references = reference_locations(&server, &variable_path, 5, 14, true).await;
        assert_eq!(treatment_references.len(), 2);
        assert_eq!(
            treatment_references
                .iter()
                .filter(|location| location.uri.ends_with("/variables/message.toml"))
                .count(),
            2
        );

        let context_attribute_references =
            reference_locations(&server, &beta_qualifier_path, 3, 14, true).await;
        assert_eq!(context_attribute_references.len(), 2);
        assert!(
            context_attribute_references
                .iter()
                .any(|location| location.uri.ends_with("/qualifiers/beta.toml"))
        );
        assert!(
            context_attribute_references
                .iter()
                .any(|location| location.uri.ends_with("/qualifiers/gamma.toml"))
        );

        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );
    }

    #[test]
    fn initialize_advertises_completion_provider() {
        let result = initialize_result();
        assert_eq!(
            result
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("positionEncoding"))
                .and_then(JsonValue::as_str),
            Some("utf-16")
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
        let (mut client, server_io) = tokio::io::duplex(8192);
        let (server_read, server_write) = tokio::io::split(server_io);
        let server =
            tokio::spawn(async move { serve(BufReader::new(server_read), server_write).await });

        write_lsp_message(
            &mut client,
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
        let failed = read_lsp_message(&mut client).await;
        assert_eq!(failed["id"], 1);
        assert_eq!(failed["error"]["code"], -32603);

        write_lsp_message(
            &mut client,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "shutdown"
            }),
        )
        .await;
        let shutdown = read_lsp_message(&mut client).await;
        assert_eq!(shutdown["id"], 2);
        assert!(shutdown["result"].is_null());

        write_lsp_message(
            &mut client,
            json!({
                "jsonrpc": "2.0",
                "method": "exit"
            }),
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

    fn assert_no_completion(completions: &[LspCompletionItem], label: &str, detail: &str) {
        assert!(
            !completions
                .iter()
                .any(|completion| completion.label == label && completion.detail == detail),
            "unexpected completion {label} ({detail})"
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

    async fn read_lsp_message<R>(reader: &mut R) -> JsonValue
    where
        R: AsyncRead + Unpin,
    {
        let mut reader = BufReader::new(reader);
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
        tokio::io::AsyncReadExt::read_exact(&mut reader, &mut body)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }
}
