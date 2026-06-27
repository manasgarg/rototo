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
    async fn lsp_document_symbols_use_snapshot_index_and_unsaved_overlay() {
        // Document symbols power editor outlines. This checks that the outline
        // is built from rototo's semantic snapshot for every package file
        // kind, not from a shallow TOML/JSON parse of the current document.
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
        let manifest_path = root.join("rototo-package.toml");
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
when = "context.account.tier == \"premium\""
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
        let catalog_path = root.join("catalogs/message.schema.json");
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
        let catalog_entry_path = root.join("catalogs/message-entries/external.toml");
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
when = 'qualifier["premium"]'
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

        // A qualifier definition is exposed as the named qualifier in the
        // editor outline.
        let qualifier_symbols = server
            .document_symbols(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                }
            }))
            .await
            .unwrap();
        let qualifier = child_symbol(&qualifier_symbols, "premium");
        assert!(qualifier.children.is_empty());

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
                .any(|child| child.name == "rule 1: qualifier[\"premium\"] -> \"welcome\"")
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
    async fn lsp_completion_items_use_snapshot_index_and_unsaved_overlays() {
        // Completion is context-sensitive. The server should suggest rototo
        // concepts that make sense at the cursor, and it should include facts
        // from unsaved editor overlays while excluding unrelated suggestion
        // categories.
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
        tokio::fs::create_dir_all(root.join("evaluation-contexts"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("lint")).await.unwrap();
        let manifest_path = root.join("rototo-package.toml");
        let disk_manifest = r#"schema_version = 1
"#;
        tokio::fs::write(&manifest_path, disk_manifest)
            .await
            .unwrap();
        let qualifier_path = root.join("qualifiers/premium.toml");
        let disk_qualifier = r#"schema_version = 1
when = "context.account.tier == \"premium\""
"#;
        tokio::fs::write(&qualifier_path, disk_qualifier)
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
        let catalog_path = root.join("catalogs/message.schema.json");
        let disk_catalog = r#"{
  "type": "object",
  "required": ["heading", "body"],
  "properties": {
    "heading": { "type": "string" },
    "body": { "type": "string" }
  },
  "additionalProperties": false
}
"#;
        tokio::fs::write(&catalog_path, disk_catalog).await.unwrap();
        let catalog_entry_path = root.join("catalogs/message-entries/default.toml");
        let disk_catalog_entry = r#"heading = "Hello"
body = "World"
"#;
        tokio::fs::write(&catalog_entry_path, disk_catalog_entry)
            .await
            .unwrap();
        let evaluation_context_path = root.join("evaluation-contexts/request.schema.json");
        let disk_evaluation_context = r#"{
  "type": "object",
  "properties": {
    "account": {
      "type": "object",
      "properties": {
        "tier": { "type": "string" },
        "region": { "type": "string" }
      }
    },
    "device": {
      "type": "object",
      "properties": {
        "platform": { "type": "string" }
      }
    }
  }
}
"#;
        tokio::fs::write(&evaluation_context_path, disk_evaluation_context)
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
        server.package_root = Some(tokio::fs::canonicalize(root).await.unwrap());
        // These overlays add an unsaved package extend and an unsaved
        // variable rule. The assertions below verify that completion reads the
        // same snapshot the rest of the language server uses.
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

[resolve]
default = "hello"

[[resolve.rule]]
when = 'qualifier["premium"]'
value = "welcome"
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
                    "line": 7,
                    "character": 22
                }
            }))
            .await
            .unwrap();

        // In a variable rule's condition expression, qualifier ids are useful; base
        // package paths, variable values, predicate operators, and custom
        // lint field names are not.
        assert_no_completion(&completions, "../base", "package extend");
        assert_completion(&completions, "premium", "qualifier");
        assert_no_completion(&completions, "treatment", "variable value");
        assert_no_completion(&completions, "bucket", "predicate operator");
        assert_no_completion(&completions, "extends", "custom lint field selector");

        // CEL completions use package schemas. `context.` suggests top-level
        // evaluation context properties, and nested context paths continue through
        // the same JSON Schema.
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display()),
                    "version": 4
                },
                "contentChanges": [
                    {
                        "text": "schema_version = 1\nwhen = \"context.\""
                    }
                ]
            }))
            .unwrap();
        let context_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                },
                "position": {
                    "line": 1,
                    "character": 16
                }
            }))
            .await
            .unwrap();
        assert_completion(&context_completions, "context.account", "context field");
        assert_completion(&context_completions, "context.device", "context field");

        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display()),
                    "version": 5
                },
                "contentChanges": [
                    {
                        "text": "schema_version = 1\nwhen = \"context.account.\""
                    }
                ]
            }))
            .unwrap();
        let nested_context_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                },
                "position": {
                    "line": 1,
                    "character": 24
                }
            }))
            .await
            .unwrap();
        assert_completion(
            &nested_context_completions,
            "context.account.region",
            "context field",
        );
        assert_completion(
            &nested_context_completions,
            "context.account.tier",
            "context field",
        );

        // Other CEL expression positions expose expression roots and supported
        // function names.
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display()),
                    "version": 6
                },
                "contentChanges": [
                    {
                        "text": "schema_version = 1\nwhen = \"buck\""
                    }
                ]
            }))
            .unwrap();
        let expression_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                },
                "position": {
                    "line": 1,
                    "character": 12
                }
            }))
            .await
            .unwrap();
        assert_completion(&expression_completions, "bucket(", "expression function");
        assert_completion(&expression_completions, "context.", "expression root");

        // Once the cursor follows a complete boolean expression, composition
        // operators are the useful next tokens. Expression roots and functions
        // only come back after the operator starts the next operand.
        let boolean_expression_text =
            "schema_version = 1\nwhen = \"context.account.tier == 'premium' \"";
        let boolean_expression_character = boolean_expression_text
            .lines()
            .nth(1)
            .unwrap()
            .chars()
            .count()
            - 1;
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display()),
                    "version": 16
                },
                "contentChanges": [
                    {
                        "text": boolean_expression_text
                    }
                ]
            }))
            .unwrap();
        let operator_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                },
                "position": {
                    "line": 1,
                    "character": boolean_expression_character
                }
            }))
            .await
            .unwrap();
        assert_completion_insert_text(&operator_completions, "&&", "expression operator", "&& ");
        assert_completion_insert_text(&operator_completions, "||", "expression operator", "|| ");
        assert_completion_labels(&operator_completions, &["&&", "||"]);
        assert_no_completion(&operator_completions, "context.", "expression root");
        assert_no_completion(&operator_completions, "bucket(", "expression function");

        let next_operand_text =
            "schema_version = 1\nwhen = \"context.account.tier == 'premium' && \"";
        let next_operand_character = next_operand_text.lines().nth(1).unwrap().chars().count() - 1;
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display()),
                    "version": 17
                },
                "contentChanges": [
                    {
                        "text": next_operand_text
                    }
                ]
            }))
            .unwrap();
        let next_operand_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                },
                "position": {
                    "line": 1,
                    "character": next_operand_character
                }
            }))
            .await
            .unwrap();
        assert_completion(&next_operand_completions, "context.", "expression root");
        assert_completion(&next_operand_completions, "bucket(", "expression function");
        assert_no_completion(&next_operand_completions, "&&", "expression operator");

        let partial_and_text = "schema_version = 1\nwhen = \"context.account.tier == 'premium' &\"";
        let partial_and_character = partial_and_text.lines().nth(1).unwrap().chars().count() - 1;
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display()),
                    "version": 18
                },
                "contentChanges": [
                    {
                        "text": partial_and_text
                    }
                ]
            }))
            .unwrap();
        let partial_and_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                },
                "position": {
                    "line": 1,
                    "character": partial_and_character
                }
            }))
            .await
            .unwrap();
        assert_completion_labels(&partial_and_completions, &["&&"]);

        let partial_or_text = "schema_version = 1\nwhen = \"context.account.tier == 'premium' |\"";
        let partial_or_character = partial_or_text.lines().nth(1).unwrap().chars().count() - 1;
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display()),
                    "version": 19
                },
                "contentChanges": [
                    {
                        "text": partial_or_text
                    }
                ]
            }))
            .unwrap();
        let partial_or_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                },
                "position": {
                    "line": 1,
                    "character": partial_or_character
                }
            }))
            .await
            .unwrap();
        assert_completion_labels(&partial_or_completions, &["||"]);

        // Qualifier TOML positions get field completion, but not CEL concepts or
        // variable-value suggestions at this cursor position.
        let qualifier_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                },
                "position": {
                    "line": 4,
                    "character": 6
                }
            }))
            .await
            .unwrap();
        assert_no_completion(&qualifier_completions, "bucket", "predicate operator");
        assert_completion(&qualifier_completions, "description", "qualifier field");
        assert_no_completion(&qualifier_completions, "when", "qualifier field");
        assert_no_completion(&qualifier_completions, "premium", "qualifier");
        assert_no_completion(&qualifier_completions, "treatment", "variable value");
        assert_completion_labels(&qualifier_completions, &["description"]);

        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display()),
                    "version": 7
                },
                "contentChanges": [
                    {
                        "text": "schema_version = 1\n\n"
                    }
                ]
            }))
            .unwrap();
        let empty_qualifier_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                },
                "position": {
                    "line": 2,
                    "character": 0
                }
            }))
            .await
            .unwrap();
        assert_completion_insert_text(
            &empty_qualifier_completions,
            "description",
            "qualifier field",
            "description = \"\"",
        );
        assert_completion_labels(&empty_qualifier_completions, &["description", "when"]);

        // While the user is halfway through typing a field name, the TOML can
        // be temporarily malformed. Field completion should still use the source
        // document kind instead of depending on a projected qualifier node.
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display()),
                    "version": 8
                },
                "contentChanges": [
                    {
                        "text": "schema_version = 1\ndesc"
                    }
                ]
            }))
            .unwrap();
        let partial_qualifier_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", qualifier_path.display())
                },
                "position": {
                    "line": 1,
                    "character": 4
                }
            }))
            .await
            .unwrap();
        assert_completion(
            &partial_qualifier_completions,
            "description",
            "qualifier field",
        );

        // Variable files get the same field-name support, including while the
        // current buffer is temporarily malformed during typing.
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 4
                },
                "contentChanges": [
                    {
                        "text": "schema_version = 1\ndesc"
                    }
                ]
            }))
            .unwrap();
        let partial_variable_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                },
                "position": {
                    "line": 1,
                    "character": 4
                }
            }))
            .await
            .unwrap();
        assert_completion(
            &partial_variable_completions,
            "description",
            "variable field",
        );
        assert_no_completion(&partial_variable_completions, "schema", "variable field");
        assert_completion_insert_text(
            &partial_variable_completions,
            "[resolve]",
            "variable block",
            "[resolve]\ndefault = ",
        );
        assert_completion_labels(
            &partial_variable_completions,
            &["description", "type", "[resolve]", "[[resolve.rule]]"],
        );

        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 5
                },
                "contentChanges": [
                    {
                        "text": "schema_version = 1\ntype = \"string\"\n\n"
                    }
                ]
            }))
            .unwrap();
        let empty_variable_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                },
                "position": {
                    "line": 2,
                    "character": 0
                }
            }))
            .await
            .unwrap();
        assert_completion_insert_text(
            &empty_variable_completions,
            "[[resolve.rule]]",
            "variable block",
            "[[resolve.rule]]\nwhen = \"\"\nvalue = ",
        );
        assert_completion_labels(
            &empty_variable_completions,
            &["description", "[resolve]", "[[resolve.rule]]"],
        );

        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 6
                },
                "contentChanges": [
                    {
                        "text": "schema_version = 1\ntype = \"string\"\n\n[resolve]\n"
                    }
                ]
            }))
            .unwrap();
        let resolve_block_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                },
                "position": {
                    "line": 4,
                    "character": 0
                }
            }))
            .await
            .unwrap();
        assert_completion_insert_text(
            &resolve_block_completions,
            "default",
            "variable field",
            "default = ",
        );
        assert_completion_labels(&resolve_block_completions, &["default", "[[resolve.rule]]"]);
        assert_no_completion(&resolve_block_completions, "type", "variable field");

        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 7
                },
                "contentChanges": [
                    {
                        "text": "schema_version = 1\ntype = \"string\"\n\n[resolve]\ndefault = \"hello\"\n"
                    }
                ]
            }))
            .unwrap();
        let complete_resolve_block_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                },
                "position": {
                    "line": 5,
                    "character": 0
                }
            }))
            .await
            .unwrap();
        assert_completion_labels(&complete_resolve_block_completions, &["[[resolve.rule]]"]);

        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 8
                },
                "contentChanges": [
                    {
                        "text": "schema_version = 1\ntype = \"string\"\n\n[resolve]\ndefault = \"hello\"\n\n[[resolve.rule]]\nwhen = \"context.user.tier == 'premium'\"\n"
                    }
                ]
            }))
            .unwrap();
        let rule_block_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                },
                "position": {
                    "line": 8,
                    "character": 0
                }
            }))
            .await
            .unwrap();
        assert_completion_insert_text(
            &rule_block_completions,
            "value",
            "variable field",
            "value = ",
        );
        assert_completion_labels(
            &rule_block_completions,
            &["query", "value", "[[resolve.rule]]"],
        );
        assert_no_completion(&rule_block_completions, "when", "variable field");
        assert_no_completion(&rule_block_completions, "default", "variable field");

        // Query expressions on list<catalog:...> variables complete `entry.`
        // paths from the catalog schema for the entries being filtered.
        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 10
                },
                "contentChanges": [
                    {
                        "text": r#"schema_version = 1
type = "list<catalog:message>"

[resolve]
default = ["default"]

[[resolve.rule]]
query = 'entry.'
value = ["default"]
"#
                    }
                ]
            }))
            .unwrap();
        let entry_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                },
                "position": {
                    "line": 7,
                    "character": 15
                }
            }))
            .await
            .unwrap();
        assert_completion(&entry_completions, "entry.heading", "entry field");
        assert_completion(&entry_completions, "entry.body", "entry field");

        // Catalog entry files derive field-name completion from their catalog's
        // JSON Schema properties.
        server
            .open_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", catalog_entry_path.display()),
                    "version": 2,
                    "text": "hea"
                }
            }))
            .unwrap();
        let catalog_entry_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", catalog_entry_path.display())
                },
                "position": {
                    "line": 0,
                    "character": 3
                }
            }))
            .await
            .unwrap();
        assert_completion(&catalog_entry_completions, "heading", "catalog entry field");
        assert_completion(&catalog_entry_completions, "body", "catalog entry field");
        assert_completion_insert_text(
            &catalog_entry_completions,
            "heading",
            "catalog entry field",
            "heading = ",
        );

        server
            .change_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", catalog_entry_path.display()),
                    "version": 3
                },
                "contentChanges": [
                    {
                        "text": "heading = \"Hello\"\n\n"
                    }
                ]
            }))
            .unwrap();
        let partial_catalog_entry_completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", catalog_entry_path.display())
                },
                "position": {
                    "line": 1,
                    "character": 0
                }
            }))
            .await
            .unwrap();
        assert_no_completion(
            &partial_catalog_entry_completions,
            "heading",
            "catalog entry field",
        );
        assert_completion(
            &partial_catalog_entry_completions,
            "body",
            "catalog entry field",
        );

        // Custom Lua lint files get field selector completions because those
        // handlers target rototo fields rather than qualifier predicates.
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
        // Opening and changing documents through LSP must not persist overlays
        // to disk.
        assert_eq!(
            tokio::fs::read_to_string(&manifest_path).await.unwrap(),
            disk_manifest
        );
        assert_eq!(
            tokio::fs::read_to_string(&qualifier_path).await.unwrap(),
            disk_qualifier
        );
        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );
        assert_eq!(
            tokio::fs::read_to_string(&catalog_path).await.unwrap(),
            disk_catalog
        );
        assert_eq!(
            tokio::fs::read_to_string(&catalog_entry_path)
                .await
                .unwrap(),
            disk_catalog_entry
        );
        assert_eq!(
            tokio::fs::read_to_string(&evaluation_context_path)
                .await
                .unwrap(),
            disk_evaluation_context
        );
    }

    #[tokio::test]
    async fn lsp_hover_uses_snapshot_index_and_unsaved_overlays() {
        // Hover should explain the rototo concept under the cursor: descriptions,
        // variable types, rule selections, qualifier definitions, and lint
        // failures. The source of truth is again the overlay-aware snapshot.
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
        tokio::fs::create_dir_all(root.join("evaluation-contexts"))
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
        let qualifier_path = root.join("qualifiers/premium.toml");
        tokio::fs::write(
            &qualifier_path,
            r#"schema_version = 1
description = "Premium accounts"
when = "context.account.tier == \"premium\""
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
            root.join("evaluation-contexts/request.schema.json"),
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
when = 'qualifier["premium"]'
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
            &hover_contents(&server, &qualifier_path, 1, 17).await,
            "Premium accounts",
        );
        assert_hover_contains(
            &hover_contents(&server, &qualifier_path, 1, 17).await,
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
    async fn lsp_rejects_incremental_did_change_ranges() {
        // initialize_result advertises full-document sync. If a client sends an
        // incremental range edit anyway, fail loudly so the server never mixes
        // partial edits into its overlay model.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        let variable_path = root.join("variables/message.toml");

        let mut server = LspServer::new();
        server.package_root = Some(tokio::fs::canonicalize(root).await.unwrap());
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
    async fn lsp_query_expressions_use_snapshot_index_and_unsaved_overlays() {
        // Query rules are CEL expressions too. Editor features should work when
        // the cursor is inside a query expression, not only inside rule `when`.
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        tokio::fs::create_dir_all(root.join("qualifiers"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("catalogs"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("rototo-package.toml"),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let qualifier_path = root.join("qualifiers/premium.toml");
        tokio::fs::write(
            &qualifier_path,
            r#"schema_version = 1
when = "context.account.tier == \"premium\""
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("catalogs/message.schema.json"),
            r#"{"type":"string"}"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "list<catalog:message>"

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
type = "list<catalog:message>"

[resolve]
default = []

[[resolve.rule]]
query = 'qualifier["premium"]'
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
                .any(|child| child.name == "rule 1: qualifier[\"premium\"]")
        );

        let completions = server
            .completion_items(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display())
                },
                "position": {
                    "line": 7,
                    "character": 21
                }
            }))
            .await
            .unwrap();
        assert_completion(&completions, "premium", "qualifier");

        assert_hover_contains(
            &hover_contents(&server, &variable_path, 7, 21).await,
            "Condition `qualifier[\"premium\"]`.",
        );

        let definition = definition_location(&server, &variable_path, 7, 21).await;
        assert!(definition.uri.ends_with("/qualifiers/premium.toml"));

        let references = reference_locations(&server, &variable_path, 7, 21, true).await;
        assert_eq!(references.len(), 2);
        assert!(
            references
                .iter()
                .any(|location| location.uri.ends_with("/qualifiers/premium.toml"))
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
        // concepts: catalog-backed variable types, qualifier rules, and
        // qualifier composition.
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
        tokio::fs::create_dir_all(root.join("evaluation-contexts"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("rototo-package.toml"),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let beta_qualifier_path = root.join("qualifiers/beta.toml");
        tokio::fs::write(
            &beta_qualifier_path,
            r#"schema_version = 1
when = "context.account.beta == true"
"#,
        )
        .await
        .unwrap();
        let premium_qualifier_path = root.join("qualifiers/premium.toml");
        tokio::fs::write(
            &premium_qualifier_path,
            r#"schema_version = 1
when = "qualifier[\"beta\"]"
"#,
        )
        .await
        .unwrap();
        let schema_path = root.join("catalogs/message.schema.json");
        tokio::fs::write(&schema_path, r#"{"type":"string"}"#)
            .await
            .unwrap();
        tokio::fs::write(
            root.join("catalogs/message-entries/welcome.toml"),
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
        // The variable only becomes catalog-backed and qualifier-referencing in
        // the unsaved editor buffer, so every definition below also checks that
        // go-to-definition is overlay-aware.
        server
            .open_document(json!({
                "textDocument": {
                    "uri": format!("file://{}", variable_path.display()),
                    "version": 6,
                    "text": r#"schema_version = 1
type = "catalog:message"

[resolve]
default = "welcome"

[[resolve.rule]]
when = 'qualifier["premium"]'
value = "welcome"
"#,
                }
            }))
            .unwrap();

        // The catalog type segment in `catalog:message` jumps to the schema.
        let schema_definition = definition_location(&server, &variable_path, 1, 16).await;
        assert!(
            schema_definition
                .uri
                .ends_with("/catalogs/message.schema.json")
        );

        // A variable resolve rule's qualifier id jumps to the qualifier file.
        let qualifier_definition = definition_location(&server, &variable_path, 7, 18).await;
        assert!(
            qualifier_definition
                .uri
                .ends_with("/qualifiers/premium.toml")
        );

        // A composed qualifier reference jumps to the qualifier it depends on.
        let when_definition = definition_location(&server, &premium_qualifier_path, 1, 20).await;
        assert!(when_definition.uri.ends_with("/qualifiers/beta.toml"));

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
        tokio::fs::create_dir_all(root.join("qualifiers"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("variables"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("catalogs/message-entries"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("evaluation-contexts"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("rototo-package.toml"),
            r#"schema_version = 1
"#,
        )
        .await
        .unwrap();
        let beta_qualifier_path = root.join("qualifiers/beta.toml");
        tokio::fs::write(
            &beta_qualifier_path,
            r#"schema_version = 1
when = "context.account.beta == true"
"#,
        )
        .await
        .unwrap();
        let gamma_qualifier_path = root.join("qualifiers/gamma.toml");
        tokio::fs::write(
            &gamma_qualifier_path,
            r#"schema_version = 1
when = "context.account.beta == true"
"#,
        )
        .await
        .unwrap();
        let premium_qualifier_path = root.join("qualifiers/premium.toml");
        tokio::fs::write(
            &premium_qualifier_path,
            r#"schema_version = 1
when = "qualifier[\"beta\"]"
"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("evaluation-contexts/request.schema.json"),
            r#"{"type":"object","properties":{"account":{"type":"object","properties":{"beta":{"type":"boolean"}}}}}"#,
        )
        .await
        .unwrap();
        let message_schema_path = root.join("catalogs/message.schema.json");
        tokio::fs::write(&message_schema_path, r#"{"type":"string"}"#)
            .await
            .unwrap();
        tokio::fs::write(
            root.join("catalogs/message-entries/welcome.toml"),
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
type = "catalog:message"

[resolve]
default = "welcome"

[[resolve.rule]]
when = 'qualifier["premium"]'
value = "welcome"
"#,
                }
            }))
            .unwrap();

        // `beta` is declared in its own qualifier file and used by `premium`.
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

        // `premium` is declared as a qualifier and used by the unsaved variable
        // rule.
        let premium_references = reference_locations(&server, &variable_path, 7, 18, true).await;
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
                .ends_with("/catalogs/message-entries/welcome.toml")
        }));

        // Context attributes are also indexed. Both qualifiers read
        // `account.beta`, so they should both be returned.
        let context_attribute_references =
            reference_locations(&server, &beta_qualifier_path, 1, 20, true).await;
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
        let (mut client, server_io) = tokio::io::duplex(8192);
        let (server_read, server_write) = tokio::io::split(server_io);
        let server =
            tokio::spawn(async move { serve(BufReader::new(server_read), server_write).await });

        // No package has been initialized, so a document request fails.
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

        // The server should still accept later requests after the failed one.
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

        // LSP exits only after shutdown; reaching this await proves the server
        // loop terminated cleanly.
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

    fn assert_completion_insert_text(
        completions: &[LspCompletionItem],
        label: &str,
        detail: &str,
        insert_text: &str,
    ) {
        assert!(
            completions.iter().any(|completion| {
                completion.label == label
                    && completion.detail == detail
                    && completion.insert_text.as_deref() == Some(insert_text)
            }),
            "missing completion {label} ({detail}) with insert text {insert_text:?}"
        );
    }

    fn assert_completion_labels(completions: &[LspCompletionItem], expected: &[&str]) {
        let labels = completions
            .iter()
            .map(|completion| completion.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, expected);
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
