use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value as JsonValue;
use tokio::io::{AsyncBufRead, AsyncWrite, BufReader};

use crate::error::{Result, RototoError};
use crate::lint::{LintInput, OverlayDocument, WorkspaceLintSnapshot, lint_workspace_snapshot};

use super::convert::{
    lsp_completion_item, lsp_document_symbol, lsp_hover, lsp_location, lsp_reference,
    publish_diagnostics_params,
};
use super::protocol::{
    LspCompletionItem, LspDocumentSymbol, LspHover, LspLocation, PublishDiagnosticsParams,
    initialize_result,
};
use super::transport::{read_message, write_error_response, write_notification, write_response};
use super::uri::{
    initialize_workspace_root, json_i32, path_from_file_uri, source_position_from_json,
    workspace_relative_path,
};

pub async fn serve_stdio() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    serve(BufReader::new(stdin), stdout).await
}

pub(super) async fn serve<R, W>(mut reader: R, mut writer: W) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut server = LspServer::new();
    while let Some(message) = read_message(&mut reader).await? {
        let id = message.get("id").cloned();
        let method = message
            .get("method")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_owned();
        match server.handle_message(message, &mut writer).await {
            Ok(true) => break,
            Ok(false) => {}
            Err(err) if id.is_some() => {
                write_error_response(
                    &mut writer,
                    id.unwrap(),
                    -32603,
                    &format!("rototo LSP request failed: {err}"),
                )
                .await?;
            }
            Err(err) if method == "exit" => return Err(err),
            Err(err) => {
                tracing::warn!(method = %method, error = %err, "rototo LSP notification failed");
            }
        }
    }
    Ok(())
}

pub(super) struct LspServer {
    pub(super) workspace_root: Option<PathBuf>,
    overlays: BTreeMap<String, OverlayDocument>,
    shutdown_requested: bool,
}

impl LspServer {
    pub(super) fn new() -> Self {
        Self {
            workspace_root: None,
            overlays: BTreeMap::new(),
            shutdown_requested: false,
        }
    }

    async fn handle_message<W>(&mut self, message: JsonValue, writer: &mut W) -> Result<bool>
    where
        W: AsyncWrite + Unpin,
    {
        let method = message
            .get("method")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(JsonValue::Null);

        match (id, method) {
            (Some(id), "initialize") => {
                self.workspace_root = initialize_workspace_root(&params).await?;
                write_response(writer, id, initialize_result()).await?;
            }
            (Some(id), "shutdown") => {
                self.shutdown_requested = true;
                write_response(writer, id, JsonValue::Null).await?;
            }
            (Some(id), "textDocument/documentSymbol") => {
                let symbols = self.document_symbols(params).await?;
                write_response(
                    writer,
                    id,
                    serde_json::to_value(symbols)
                        .map_err(|err| RototoError::new(err.to_string()))?,
                )
                .await?;
            }
            (Some(id), "textDocument/completion") => {
                let completions = self.completion_items(params).await?;
                write_response(
                    writer,
                    id,
                    serde_json::to_value(completions)
                        .map_err(|err| RototoError::new(err.to_string()))?,
                )
                .await?;
            }
            (Some(id), "textDocument/hover") => {
                let hover = self.hover(params).await?;
                let result = hover
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|err| RototoError::new(err.to_string()))?
                    .unwrap_or(JsonValue::Null);
                write_response(writer, id, result).await?;
            }
            (Some(id), "textDocument/definition") => {
                let definition = self.definition(params).await?;
                let result = definition
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|err| RototoError::new(err.to_string()))?
                    .unwrap_or(JsonValue::Null);
                write_response(writer, id, result).await?;
            }
            (Some(id), "textDocument/references") => {
                let references = self.references(params).await?;
                write_response(
                    writer,
                    id,
                    serde_json::to_value(references)
                        .map_err(|err| RototoError::new(err.to_string()))?,
                )
                .await?;
            }
            (Some(id), _) => {
                write_error_response(writer, id, -32601, "method not found").await?;
            }
            (None, "initialized") => {
                self.publish_workspace_diagnostics(writer).await?;
            }
            (None, "textDocument/didOpen") => {
                self.open_document(params)?;
                self.publish_workspace_diagnostics(writer).await?;
            }
            (None, "textDocument/didChange") => {
                self.change_document(params)?;
                self.publish_workspace_diagnostics(writer).await?;
            }
            (None, "textDocument/didSave") | (None, "textDocument/didClose") => {
                self.remove_document_overlay(params)?;
                self.publish_workspace_diagnostics(writer).await?;
            }
            (None, "exit") => {
                if self.shutdown_requested {
                    return Ok(true);
                }
                return Err(RototoError::new("LSP exit received before shutdown"));
            }
            (None, _) => {}
        }

        Ok(false)
    }

    pub(super) fn open_document(&mut self, params: JsonValue) -> Result<()> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| RototoError::new("didOpen missing textDocument"))?;
        let uri = text_document
            .get("uri")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("didOpen missing textDocument.uri"))?;
        let text = text_document
            .get("text")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("didOpen missing textDocument.text"))?;
        let version = json_i32(text_document.get("version"));
        let path = self.workspace_path_for_uri(uri)?;
        self.overlays.insert(
            path,
            OverlayDocument {
                text: text.to_owned(),
                version,
            },
        );
        Ok(())
    }

    pub(super) fn change_document(&mut self, params: JsonValue) -> Result<()> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| RototoError::new("didChange missing textDocument"))?;
        let uri = text_document
            .get("uri")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("didChange missing textDocument.uri"))?;
        let version = json_i32(text_document.get("version"));
        let change = params
            .get("contentChanges")
            .and_then(JsonValue::as_array)
            .and_then(|changes| changes.last())
            .ok_or_else(|| RototoError::new("didChange missing content change"))?;
        if change.get("range").is_some() || change.get("rangeLength").is_some() {
            return Err(RototoError::new(
                "incremental didChange ranges are unsupported; send full document text",
            ));
        }
        let text = change
            .get("text")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("didChange missing full text content change"))?;
        let path = self.workspace_path_for_uri(uri)?;
        self.overlays.insert(
            path,
            OverlayDocument {
                text: text.to_owned(),
                version,
            },
        );
        Ok(())
    }

    pub(super) fn remove_document_overlay(&mut self, params: JsonValue) -> Result<()> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| RototoError::new("document notification missing textDocument"))?;
        let uri = text_document
            .get("uri")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("document notification missing textDocument.uri"))?;
        let path = self.workspace_path_for_uri(uri)?;
        self.overlays.remove(&path);
        Ok(())
    }

    async fn publish_workspace_diagnostics<W>(&self, writer: &mut W) -> Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        for publication in self.workspace_diagnostics().await? {
            write_notification(
                writer,
                "textDocument/publishDiagnostics",
                serde_json::to_value(publication)
                    .map_err(|err| RototoError::new(err.to_string()))?,
            )
            .await?;
        }
        Ok(())
    }

    pub(super) async fn workspace_diagnostics(&self) -> Result<Vec<PublishDiagnosticsParams>> {
        let Some(snapshot) = self.workspace_snapshot().await? else {
            return Ok(Vec::new());
        };
        Ok(publish_diagnostics_params(&snapshot.lint))
    }

    pub(super) async fn document_symbols(
        &self,
        params: JsonValue,
    ) -> Result<Vec<LspDocumentSymbol>> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| RototoError::new("documentSymbol missing textDocument"))?;
        let uri = text_document
            .get("uri")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("documentSymbol missing textDocument.uri"))?;
        let path = self.workspace_path_for_uri(uri)?;
        let Some(snapshot) = self.workspace_snapshot().await? else {
            return Ok(Vec::new());
        };
        Ok(snapshot
            .document_symbols(&path)
            .iter()
            .map(lsp_document_symbol)
            .collect())
    }

    pub(super) async fn completion_items(
        &self,
        params: JsonValue,
    ) -> Result<Vec<LspCompletionItem>> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| RototoError::new("completion missing textDocument"))?;
        let uri = text_document
            .get("uri")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("completion missing textDocument.uri"))?;
        let position = source_position_from_json(
            params
                .get("position")
                .ok_or_else(|| RototoError::new("completion missing position"))?,
        )?;
        let path = self.workspace_path_for_uri(uri)?;
        let Some(snapshot) = self.workspace_snapshot().await? else {
            return Ok(Vec::new());
        };
        Ok(snapshot
            .completion_items(&path, position)
            .iter()
            .map(lsp_completion_item)
            .collect())
    }

    pub(super) async fn hover(&self, params: JsonValue) -> Result<Option<LspHover>> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| RototoError::new("hover missing textDocument"))?;
        let uri = text_document
            .get("uri")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("hover missing textDocument.uri"))?;
        let position = source_position_from_json(
            params
                .get("position")
                .ok_or_else(|| RototoError::new("hover missing position"))?,
        )?;
        let path = self.workspace_path_for_uri(uri)?;
        let Some(snapshot) = self.workspace_snapshot().await? else {
            return Ok(None);
        };
        Ok(snapshot.hover(&path, position).map(lsp_hover))
    }

    pub(super) async fn definition(&self, params: JsonValue) -> Result<Option<LspLocation>> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| RototoError::new("definition missing textDocument"))?;
        let uri = text_document
            .get("uri")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("definition missing textDocument.uri"))?;
        let position = source_position_from_json(
            params
                .get("position")
                .ok_or_else(|| RototoError::new("definition missing position"))?,
        )?;
        let path = self.workspace_path_for_uri(uri)?;
        let Some(snapshot) = self.workspace_snapshot().await? else {
            return Ok(None);
        };
        Ok(snapshot.definition(&path, position).map(lsp_location))
    }

    pub(super) async fn references(&self, params: JsonValue) -> Result<Vec<LspLocation>> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| RototoError::new("references missing textDocument"))?;
        let uri = text_document
            .get("uri")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("references missing textDocument.uri"))?;
        let position = source_position_from_json(
            params
                .get("position")
                .ok_or_else(|| RototoError::new("references missing position"))?,
        )?;
        let include_declaration = params
            .get("context")
            .and_then(|context| context.get("includeDeclaration"))
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let path = self.workspace_path_for_uri(uri)?;
        let Some(snapshot) = self.workspace_snapshot().await? else {
            return Ok(Vec::new());
        };
        Ok(snapshot
            .references(&path, position, include_declaration)
            .iter()
            .map(lsp_reference)
            .collect())
    }

    async fn workspace_snapshot(&self) -> Result<Option<WorkspaceLintSnapshot>> {
        let Some(root) = &self.workspace_root else {
            return Ok(None);
        };
        let mut input = LintInput::new(root.clone());
        input.overlays = self.overlays.clone();
        lint_workspace_snapshot(input).await.map(Some)
    }

    fn workspace_path_for_uri(&self, uri: &str) -> Result<String> {
        let Some(root) = &self.workspace_root else {
            return Err(RototoError::new("LSP workspace root is not initialized"));
        };
        let path = path_from_file_uri(uri)?;
        workspace_relative_path(root, &path)
    }
}
