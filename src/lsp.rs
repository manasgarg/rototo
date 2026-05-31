use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};

use crate::diagnostics::{
    DiagnosticLocation, LintDiagnostic, Severity, SourcePosition, SourceRange,
};
use crate::error::{Result, RototoError};
use crate::lint::{
    LintInput, OverlayDocument, WorkspaceCompletionItem, WorkspaceCompletionItemKind,
    WorkspaceDefinition, WorkspaceDocumentSymbol, WorkspaceDocumentSymbolKind, WorkspaceHover,
    WorkspaceLintSnapshot, lint_workspace_snapshot,
};
use crate::model::WorkspaceLint;

const JSONRPC_VERSION: &str = "2.0";
const TEXT_DOCUMENT_SYNC_KIND_FULL: u8 = 1;

pub async fn serve_stdio() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    serve(BufReader::new(stdin), stdout).await
}

async fn serve<R, W>(mut reader: R, mut writer: W) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut server = LspServer::new();
    while let Some(message) = read_message(&mut reader).await? {
        if server.handle_message(message, &mut writer).await? {
            break;
        }
    }
    Ok(())
}

struct LspServer {
    workspace_root: Option<PathBuf>,
    overlays: BTreeMap<String, OverlayDocument>,
    shutdown_requested: bool,
}

impl LspServer {
    fn new() -> Self {
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
            (None, "exit") => return Ok(self.shutdown_requested),
            (None, _) => {}
        }

        Ok(false)
    }

    fn open_document(&mut self, params: JsonValue) -> Result<()> {
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

    fn change_document(&mut self, params: JsonValue) -> Result<()> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| RototoError::new("didChange missing textDocument"))?;
        let uri = text_document
            .get("uri")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("didChange missing textDocument.uri"))?;
        let version = json_i32(text_document.get("version"));
        let text = params
            .get("contentChanges")
            .and_then(JsonValue::as_array)
            .and_then(|changes| changes.last())
            .and_then(|change| change.get("text"))
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

    fn remove_document_overlay(&mut self, params: JsonValue) -> Result<()> {
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

    async fn workspace_diagnostics(&self) -> Result<Vec<PublishDiagnosticsParams>> {
        let Some(snapshot) = self.workspace_snapshot().await? else {
            return Ok(Vec::new());
        };
        Ok(publish_diagnostics_params(&snapshot.lint))
    }

    async fn document_symbols(&self, params: JsonValue) -> Result<Vec<LspDocumentSymbol>> {
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

    async fn completion_items(&self, params: JsonValue) -> Result<Vec<LspCompletionItem>> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| RototoError::new("completion missing textDocument"))?;
        let uri = text_document
            .get("uri")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| RototoError::new("completion missing textDocument.uri"))?;
        let path = self.workspace_path_for_uri(uri)?;
        let Some(snapshot) = self.workspace_snapshot().await? else {
            return Ok(Vec::new());
        };
        Ok(snapshot
            .completion_items(&path)
            .iter()
            .map(lsp_completion_item)
            .collect())
    }

    async fn hover(&self, params: JsonValue) -> Result<Option<LspHover>> {
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

    async fn definition(&self, params: JsonValue) -> Result<Option<LspLocation>> {
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishDiagnosticsParams {
    uri: String,
    version: Option<i32>,
    diagnostics: Vec<LspDiagnostic>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LspDiagnostic {
    range: LspRange,
    severity: u8,
    source: &'static str,
    code: String,
    message: String,
    data: LspDiagnosticData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LspDiagnosticData {
    rule: String,
    stage: String,
    help: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LspDocumentSymbol {
    name: String,
    kind: u8,
    range: LspRange,
    selection_range: LspRange,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<LspDocumentSymbol>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LspCompletionItem {
    label: String,
    kind: u8,
    detail: &'static str,
}

#[derive(Debug, Serialize)]
struct LspHover {
    contents: LspMarkupContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<LspRange>,
}

#[derive(Debug, Serialize)]
struct LspLocation {
    uri: String,
    range: LspRange,
}

#[derive(Debug, Serialize)]
struct LspMarkupContent {
    kind: &'static str,
    value: String,
}

#[derive(Debug, Serialize, Clone, Copy)]
struct LspRange {
    start: LspPosition,
    end: LspPosition,
}

#[derive(Debug, Serialize, Clone, Copy)]
struct LspPosition {
    line: usize,
    character: usize,
}

fn publish_diagnostics_params(lint: &WorkspaceLint) -> Vec<PublishDiagnosticsParams> {
    lint.diagnostics_by_document()
        .into_iter()
        .map(|group| PublishDiagnosticsParams {
            uri: group.document.uri.clone(),
            version: group.document.version,
            diagnostics: group.diagnostics.into_iter().map(lsp_diagnostic).collect(),
        })
        .collect()
}

fn lsp_diagnostic(diagnostic: &LintDiagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: lsp_range(&diagnostic.primary),
        severity: lsp_severity(diagnostic.severity),
        source: "rototo",
        code: diagnostic.rule.as_string(),
        message: diagnostic.message.clone(),
        data: LspDiagnosticData {
            rule: diagnostic.rule.as_string(),
            stage: lint_stage_label(diagnostic.stage).to_owned(),
            help: diagnostic.help.clone(),
        },
    }
}

fn lsp_document_symbol(symbol: &WorkspaceDocumentSymbol) -> LspDocumentSymbol {
    LspDocumentSymbol {
        name: symbol.name.clone(),
        kind: lsp_symbol_kind(symbol.kind),
        range: lsp_range(&symbol.location),
        selection_range: lsp_range(&symbol.selection_location),
        children: symbol.children.iter().map(lsp_document_symbol).collect(),
    }
}

fn lsp_completion_item(item: &WorkspaceCompletionItem) -> LspCompletionItem {
    LspCompletionItem {
        label: item.label.clone(),
        kind: lsp_completion_item_kind(item.kind),
        detail: item.detail,
    }
}

fn lsp_hover(hover: WorkspaceHover) -> LspHover {
    LspHover {
        range: hover.location.range.map(lsp_range_from_source),
        contents: LspMarkupContent {
            kind: "markdown",
            value: hover.contents,
        },
    }
}

fn lsp_location(definition: WorkspaceDefinition) -> LspLocation {
    LspLocation {
        uri: definition.uri,
        range: lsp_range(&definition.location),
    }
}

fn lsp_symbol_kind(kind: WorkspaceDocumentSymbolKind) -> u8 {
    match kind {
        WorkspaceDocumentSymbolKind::WorkspaceEnvironments => 18,
        WorkspaceDocumentSymbolKind::Environment => 15,
        WorkspaceDocumentSymbolKind::Qualifier => 19,
        WorkspaceDocumentSymbolKind::Predicate => 17,
        WorkspaceDocumentSymbolKind::Variable => 13,
        WorkspaceDocumentSymbolKind::Values => 18,
        WorkspaceDocumentSymbolKind::Value => 14,
        WorkspaceDocumentSymbolKind::EnvironmentBlock => 3,
        WorkspaceDocumentSymbolKind::Rule => 8,
    }
}

fn lsp_completion_item_kind(kind: WorkspaceCompletionItemKind) -> u8 {
    match kind {
        WorkspaceCompletionItemKind::Environment => 12,
        WorkspaceCompletionItemKind::Qualifier => 18,
        WorkspaceCompletionItemKind::Value => 12,
        WorkspaceCompletionItemKind::PredicateOperator => 24,
        WorkspaceCompletionItemKind::FieldSelector => 5,
    }
}

fn lsp_range(location: &DiagnosticLocation) -> LspRange {
    location
        .range
        .map_or_else(zero_lsp_range, lsp_range_from_source)
}

fn lsp_range_from_source(range: SourceRange) -> LspRange {
    LspRange {
        start: LspPosition {
            line: range.start.line,
            character: range.start.character,
        },
        end: LspPosition {
            line: range.end.line,
            character: range.end.character,
        },
    }
}

fn zero_lsp_range() -> LspRange {
    LspRange {
        start: LspPosition {
            line: 0,
            character: 0,
        },
        end: LspPosition {
            line: 0,
            character: 0,
        },
    }
}

fn lsp_severity(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 1,
        Severity::Warning => 2,
    }
}

fn lint_stage_label(stage: crate::diagnostics::LintStage) -> &'static str {
    match stage {
        crate::diagnostics::LintStage::Discover => "discover",
        crate::diagnostics::LintStage::Parse => "parse",
        crate::diagnostics::LintStage::Project => "project",
        crate::diagnostics::LintStage::Register => "register",
        crate::diagnostics::LintStage::Reference => "reference",
        crate::diagnostics::LintStage::Value => "value",
        crate::diagnostics::LintStage::Graph => "graph",
        crate::diagnostics::LintStage::Policy => "policy",
    }
}

async fn read_message<R>(reader: &mut R) -> Result<Option<JsonValue>>
where
    R: AsyncBufRead + Unpin,
{
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .await
            .map_err(|err| RototoError::new(format!("failed to read LSP header: {err}")))?;
        if bytes == 0 {
            return Ok(None);
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(value.trim().parse::<usize>().map_err(|err| {
                RototoError::new(format!("invalid LSP Content-Length header: {err}"))
            })?);
        }
    }

    let content_length =
        content_length.ok_or_else(|| RototoError::new("missing LSP Content-Length header"))?;
    let mut body = vec![0; content_length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|err| RototoError::new(format!("failed to read LSP body: {err}")))?;
    let message = serde_json::from_slice(&body)
        .map_err(|err| RototoError::new(format!("failed to parse LSP JSON body: {err}")))?;
    Ok(Some(message))
}

async fn write_response<W>(writer: &mut W, id: JsonValue, result: JsonValue) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    write_message(
        writer,
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id,
            "result": result,
        }),
    )
    .await
}

async fn write_error_response<W>(
    writer: &mut W,
    id: JsonValue,
    code: i64,
    message: &str,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    write_message(
        writer,
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id,
            "error": {
                "code": code,
                "message": message,
            },
        }),
    )
    .await
}

async fn write_notification<W>(writer: &mut W, method: &str, params: JsonValue) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    write_message(
        writer,
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "method": method,
            "params": params,
        }),
    )
    .await
}

async fn write_message<W>(writer: &mut W, message: JsonValue) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(&message)
        .map_err(|err| RototoError::new(format!("failed to serialize LSP message: {err}")))?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await
        .map_err(|err| RototoError::new(format!("failed to write LSP header: {err}")))?;
    writer
        .write_all(&body)
        .await
        .map_err(|err| RototoError::new(format!("failed to write LSP body: {err}")))?;
    writer
        .flush()
        .await
        .map_err(|err| RototoError::new(format!("failed to flush LSP output: {err}")))?;
    Ok(())
}

fn initialize_result() -> JsonValue {
    json!({
        "capabilities": {
            "textDocumentSync": {
                "openClose": true,
                "change": TEXT_DOCUMENT_SYNC_KIND_FULL,
                "save": {
                    "includeText": false
                }
            },
            "documentSymbolProvider": true,
            "completionProvider": {
                "resolveProvider": false,
                "triggerCharacters": [".", "\""]
            },
            "hoverProvider": true,
            "definitionProvider": true
        },
        "serverInfo": {
            "name": "rototo",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

async fn initialize_workspace_root(params: &JsonValue) -> Result<Option<PathBuf>> {
    if let Some(root_uri) = params.get("rootUri").and_then(JsonValue::as_str) {
        return canonicalize_workspace_root(path_from_file_uri(root_uri)?).await;
    }
    if let Some(root_path) = params.get("rootPath").and_then(JsonValue::as_str) {
        return canonicalize_workspace_root(PathBuf::from(root_path)).await;
    }
    if let Some(workspace_folder_uri) = params
        .get("workspaceFolders")
        .and_then(JsonValue::as_array)
        .and_then(|folders| folders.first())
        .and_then(|folder| folder.get("uri"))
        .and_then(JsonValue::as_str)
    {
        return canonicalize_workspace_root(path_from_file_uri(workspace_folder_uri)?).await;
    }
    canonicalize_workspace_root(
        std::env::current_dir()
            .map_err(|err| RototoError::new(format!("failed to read current directory: {err}")))?,
    )
    .await
}

async fn canonicalize_workspace_root(path: PathBuf) -> Result<Option<PathBuf>> {
    let root = tokio::fs::canonicalize(&path).await.unwrap_or(path);
    Ok(Some(root))
}

fn json_i32(value: Option<&JsonValue>) -> Option<i32> {
    value
        .and_then(JsonValue::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn source_position_from_json(value: &JsonValue) -> Result<SourcePosition> {
    let line = value
        .get("line")
        .and_then(JsonValue::as_u64)
        .and_then(|line| usize::try_from(line).ok())
        .ok_or_else(|| RototoError::new("position missing line"))?;
    let character = value
        .get("character")
        .and_then(JsonValue::as_u64)
        .and_then(|character| usize::try_from(character).ok())
        .ok_or_else(|| RototoError::new("position missing character"))?;
    Ok(SourcePosition { line, character })
}

fn path_from_file_uri(uri: &str) -> Result<PathBuf> {
    let path = uri
        .strip_prefix("file://")
        .ok_or_else(|| RototoError::new(format!("unsupported LSP URI: {uri}")))?;
    percent_decode_path(path).map(PathBuf::from)
}

fn percent_decode_path(path: &str) -> Result<String> {
    let mut decoded = Vec::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).copied().and_then(hex_value);
            let low = bytes.get(index + 2).copied().and_then(hex_value);
            match (high, low) {
                (Some(high), Some(low)) => {
                    decoded.push((high << 4) | low);
                    index += 3;
                }
                _ => {
                    return Err(RototoError::new(format!(
                        "invalid percent-encoded LSP URI path: {path}"
                    )));
                }
            }
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded)
        .map_err(|err| RototoError::new(format!("LSP URI path is not UTF-8: {err}")))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn workspace_relative_path(root: &Path, path: &Path) -> Result<String> {
    let canonical_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let relative = canonical_path.strip_prefix(root).map_err(|_| {
        RototoError::new(format!(
            "LSP document is outside workspace: {}",
            path.display()
        ))
    })?;
    let workspace_path = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if workspace_path.is_empty() {
        return Err(RototoError::new("LSP document path is workspace root"));
    }
    Ok(workspace_path)
}

#[cfg(test)]
mod tests {
    use super::*;

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

[environments]
values = ["prod"]
"#,
        )
        .await
        .unwrap();
        let disk_variable = r#"schema_version = 1
type = "string"

[values]
control = "hello"

[env._]
value = "control"
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

[env._]
value = "control"
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
        let manifest_path = root.join("rototo-workspace.toml");
        tokio::fs::write(
            &manifest_path,
            r#"schema_version = 1

[environments]
values = ["prod"]
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

[env._]
value = "control"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("variables/message-values"))
            .await
            .unwrap();
        let external_value_path = root.join("variables/message-values/external.toml");
        tokio::fs::write(&external_value_path, r#"value = "external""#)
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

[env._]
value = "control"

[env.prod]
value = "control"
rule = [
  { qualifier = "premium", value = "treatment" },
]
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
        let environments = child_symbol(&manifest_symbols, "environments");
        assert!(
            environments
                .children
                .iter()
                .any(|child| child.name == "prod")
        );

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

        let prod = child_symbol(&variable.children, "env.prod");
        assert!(
            prod.children
                .iter()
                .any(|child| child.name == "rule 1: premium -> treatment")
        );
        assert_eq!(
            tokio::fs::read_to_string(&variable_path).await.unwrap(),
            disk_variable
        );

        let external_value_symbols = server
            .document_symbols(json!({
                "textDocument": {
                    "uri": format!("file://{}", external_value_path.display())
                }
            }))
            .await
            .unwrap();
        child_symbol(&external_value_symbols, "message.external");
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
        let manifest_path = root.join("rototo-workspace.toml");
        let disk_manifest = r#"schema_version = 1

[environments]
values = ["prod"]
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

[env._]
value = "control"
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
                    "uri": format!("file://{}", manifest_path.display()),
                    "version": 2,
                    "text": r#"schema_version = 1

[environments]
values = ["prod", "stage"]
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

[env._]
value = "control"

[env.prod]
value = "control"
rule = [
  { qualifier = "premium", value = "treatment" },
]
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
                    "line": 8,
                    "character": 8
                }
            }))
            .await
            .unwrap();

        assert_completion(&completions, "stage", "workspace environment");
        assert_completion(&completions, "premium", "qualifier");
        assert_completion(&completions, "treatment", "variable value");
        assert_completion(&completions, "bucket", "predicate operator");
        assert_completion(&completions, "context_schema", "custom lint field selector");
        assert_completion(&completions, "value.", "custom lint field selector");
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
        let manifest_path = root.join("rototo-workspace.toml");
        tokio::fs::write(
            &manifest_path,
            r#"schema_version = 1

[environments]
values = ["prod"]

[[lint.rule]]
id = "operations/message-not-empty"
title = "Operational message is empty"
help = "Set a non-empty message before releasing the workspace."
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

[env._]
value = "control"
"#;
        let variable_path = root.join("variables/message.toml");
        tokio::fs::write(&variable_path, disk_variable)
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

[env._]
value = "control"

[env.prod]
value = "control"
rule = [
  { qualifier = "premium", value = "treatment" },
]
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
        assert_hover_contains(
            &hover_contents(&server, &manifest_path, 6, 7).await,
            "Custom rule `operations/message-not-empty`",
        );
        assert_hover_contains(
            &hover_contents(&server, &manifest_path, 6, 7).await,
            "Operational message is empty",
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

[env._]
value = "control"
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

[environments]
values = ["prod"]
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

[env._]
value = "control"
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

[env._]
value = "control"

[env.prod]
value = "control"
rule = [
  { qualifier = "premium", value = "treatment" },
]
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

        let qualifier_definition = definition_location(&server, &variable_path, 13, 18).await;
        assert!(
            qualifier_definition
                .uri
                .ends_with("/qualifiers/premium.toml")
        );

        let value_definition = definition_location(&server, &variable_path, 13, 39).await;
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

    #[test]
    fn initialize_advertises_completion_provider() {
        let result = initialize_result();
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

    fn assert_hover_contains(contents: &str, expected: &str) {
        assert!(
            contents.contains(expected),
            "hover did not contain {expected:?}: {contents}"
        );
    }
}
