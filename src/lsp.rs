use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};

use crate::diagnostics::{DiagnosticLocation, LintDiagnostic, Severity, SourceRange};
use crate::error::{Result, RototoError};
use crate::lint::{LintInput, OverlayDocument, lint_workspace_snapshot};
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
                write_response(writer, id, JsonValue::Array(Vec::new())).await?;
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
        let Some(root) = &self.workspace_root else {
            return Ok(Vec::new());
        };
        let mut input = LintInput::new(root.clone());
        input.overlays = self.overlays.clone();
        let lint = lint_workspace_snapshot(input).await?;
        Ok(publish_diagnostics_params(&lint))
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
            }
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
}
