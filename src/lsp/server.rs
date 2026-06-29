use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value as JsonValue;
use tokio::io::{AsyncBufRead, AsyncWrite, BufReader};
use tokio::sync::mpsc;

use crate::error::{Result, RototoError};
use crate::lint::{LintInput, OverlayDocument, PackageLintSnapshot, lint_package_snapshot};

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
    initialize_package_root, json_i32, package_relative_path, path_from_file_uri,
    source_position_from_json,
};

/// JSON-RPC error code for a request the client asked to cancel (LSP
/// `RequestCancelled`).
const REQUEST_CANCELLED: i64 = -32800;

pub async fn serve_stdio() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    serve(BufReader::new(stdin), stdout).await
}

pub(crate) async fn serve<R, W>(reader: R, mut writer: W) -> Result<()>
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin,
{
    // A dedicated reader task pulls messages off the wire eagerly into a channel.
    // Handling stays sequential on this task, but reading ahead lets a
    // `$/cancelRequest` that the client sent right after a request be observed
    // before that request is dequeued and run — which is the only way
    // cancellation can do anything in an otherwise serial server.
    let (sender, mut receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut reader = reader;
        while let Ok(Some(message)) = read_message(&mut reader).await {
            if sender.send(message).is_err() {
                break;
            }
        }
    });

    let mut server = LspServer::new();
    let mut queue: VecDeque<JsonValue> = VecDeque::new();
    let mut cancellations = PendingCancellations::default();

    loop {
        if queue.is_empty() {
            match receiver.recv().await {
                Some(message) => intake(message, &mut queue, &mut cancellations),
                None => break,
            }
        }
        // Drain everything already buffered so a pending cancel is recorded
        // before the request it targets reaches the front of the queue.
        while let Ok(message) = receiver.try_recv() {
            intake(message, &mut queue, &mut cancellations);
        }
        let Some(message) = queue.pop_front() else {
            continue;
        };

        let id = message.get("id").cloned();
        let method = message
            .get("method")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_owned();

        // A request the client cancelled while it waited in the queue returns the
        // standard RequestCancelled error rather than computing a result.
        if let Some(id) = &id
            && cancellations.take(id)
        {
            write_error_response(
                &mut writer,
                id.clone(),
                REQUEST_CANCELLED,
                "request cancelled",
            )
            .await?;
            continue;
        }

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

/// Route an incoming message: a `$/cancelRequest` is consumed by recording the
/// cancelled id; everything else is queued for sequential handling.
fn intake(
    message: JsonValue,
    queue: &mut VecDeque<JsonValue>,
    cancellations: &mut PendingCancellations,
) {
    if message.get("method").and_then(JsonValue::as_str) == Some("$/cancelRequest") {
        if let Some(id) = message.get("params").and_then(|params| params.get("id")) {
            cancellations.record(id);
        }
        return;
    }
    queue.push_back(message);
}

/// Request ids cancelled by the client but not yet matched to a pending request.
/// Ids can be numbers or strings, so they are keyed by their JSON spelling.
#[derive(Default)]
struct PendingCancellations {
    ids: HashSet<String>,
}

impl PendingCancellations {
    fn record(&mut self, id: &JsonValue) {
        self.ids.insert(id.to_string());
    }

    /// Whether `id` was cancelled, consuming the record so it matches once.
    fn take(&mut self, id: &JsonValue) -> bool {
        self.ids.remove(&id.to_string())
    }
}

pub(super) struct LspServer {
    pub(super) package_root: Option<PathBuf>,
    overlays: BTreeMap<String, OverlayDocument>,
    shutdown_requested: bool,
    /// Bumped on every overlay mutation so a cached snapshot built from an older
    /// set of buffers is recognized as stale.
    revision: u64,
    /// Last package snapshot, reused while `revision` is unchanged. The lint
    /// pipeline reads the whole package from disk plus overlays, so without this
    /// every request and every keystroke would recompute the entire package.
    snapshot_cache: Mutex<Option<(u64, Arc<PackageLintSnapshot>)>>,
    /// Count of actual snapshot builds, used by tests to prove cache reuse.
    build_count: AtomicUsize,
}

impl LspServer {
    pub(super) fn new() -> Self {
        Self {
            package_root: None,
            overlays: BTreeMap::new(),
            shutdown_requested: false,
            revision: 0,
            snapshot_cache: Mutex::new(None),
            build_count: AtomicUsize::new(0),
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
                self.package_root = initialize_package_root(&params).await?;
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
                self.publish_package_diagnostics(writer).await?;
            }
            (None, "textDocument/didOpen") => {
                self.open_document(params)?;
                self.publish_package_diagnostics(writer).await?;
            }
            (None, "textDocument/didChange") => {
                self.change_document(params)?;
                self.publish_package_diagnostics(writer).await?;
            }
            (None, "textDocument/didSave") | (None, "textDocument/didClose") => {
                self.remove_document_overlay(params)?;
                self.publish_package_diagnostics(writer).await?;
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
        let path = self.package_path_for_uri(uri)?;
        self.overlays.insert(
            path,
            OverlayDocument {
                text: text.to_owned(),
                version,
            },
        );
        self.revision += 1;
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
        let changes = params
            .get("contentChanges")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| RototoError::new("didChange missing contentChanges"))?;
        let path = self.package_path_for_uri(uri)?;

        // Apply each change to the running buffer in order: a change with a
        // `range` splices that span (its positions are UTF-16, like every other
        // LSP position), and a change without one replaces the whole document.
        let mut text = self
            .overlays
            .get(&path)
            .map(|document| document.text.clone())
            .unwrap_or_default();
        for change in changes {
            let new_text = change
                .get("text")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| RototoError::new("didChange content change missing text"))?;
            match change.get("range") {
                None => text = new_text.to_owned(),
                Some(range) => {
                    let start = source_position_from_json(
                        range
                            .get("start")
                            .ok_or_else(|| RototoError::new("didChange range missing start"))?,
                    )?;
                    let end = source_position_from_json(
                        range
                            .get("end")
                            .ok_or_else(|| RototoError::new("didChange range missing end"))?,
                    )?;
                    let start_byte = byte_offset_for_position(&text, start.line, start.character);
                    let end_byte = byte_offset_for_position(&text, end.line, end.character);
                    if start_byte > end_byte {
                        return Err(RototoError::new("didChange range start is after its end"));
                    }
                    text.replace_range(start_byte..end_byte, new_text);
                }
            }
        }

        self.overlays
            .insert(path, OverlayDocument { text, version });
        self.revision += 1;
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
        let path = self.package_path_for_uri(uri)?;
        self.overlays.remove(&path);
        self.revision += 1;
        Ok(())
    }

    async fn publish_package_diagnostics<W>(&self, writer: &mut W) -> Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        for publication in self.package_diagnostics().await? {
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

    pub(super) async fn package_diagnostics(&self) -> Result<Vec<PublishDiagnosticsParams>> {
        let Some(snapshot) = self.package_snapshot().await? else {
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
        let path = self.package_path_for_uri(uri)?;
        let Some(snapshot) = self.package_snapshot().await? else {
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
        let path = self.package_path_for_uri(uri)?;
        let Some(snapshot) = self.package_snapshot().await? else {
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
        let path = self.package_path_for_uri(uri)?;
        let Some(snapshot) = self.package_snapshot().await? else {
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
        let path = self.package_path_for_uri(uri)?;
        let Some(snapshot) = self.package_snapshot().await? else {
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
        let path = self.package_path_for_uri(uri)?;
        let Some(snapshot) = self.package_snapshot().await? else {
            return Ok(Vec::new());
        };
        Ok(snapshot
            .references(&path, position, include_declaration)
            .iter()
            .map(lsp_reference)
            .collect())
    }

    async fn package_snapshot(&self) -> Result<Option<Arc<PackageLintSnapshot>>> {
        let Some(root) = &self.package_root else {
            return Ok(None);
        };
        let revision = self.revision;
        if let Some((cached_revision, snapshot)) = self.snapshot_cache.lock().unwrap().as_ref()
            && *cached_revision == revision
        {
            return Ok(Some(Arc::clone(snapshot)));
        }
        let mut input = LintInput::new(root.clone());
        input.overlays = self.overlays.clone();
        self.build_count.fetch_add(1, Ordering::Relaxed);
        let snapshot = Arc::new(lint_package_snapshot(input).await?);
        *self.snapshot_cache.lock().unwrap() = Some((revision, Arc::clone(&snapshot)));
        Ok(Some(snapshot))
    }

    #[cfg(test)]
    pub(super) fn snapshot_build_count(&self) -> usize {
        self.build_count.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(super) fn overlay_text(&self, path: &str) -> Option<&str> {
        self.overlays
            .get(path)
            .map(|document| document.text.as_str())
    }

    fn package_path_for_uri(&self, uri: &str) -> Result<String> {
        let Some(root) = &self.package_root else {
            return Err(RototoError::new("LSP package root is not initialized"));
        };
        let path = path_from_file_uri(uri)?;
        package_relative_path(root, &path)
    }
}

/// Byte offset in `text` of an LSP position. `line` counts `\n`-delimited lines
/// and `character` counts UTF-16 code units into that line (the encoding the
/// server advertises). A position past the end of its line clamps to the line
/// break, and a position past the end of the text clamps to its length, so an
/// out-of-range edit splices at the boundary rather than panicking.
fn byte_offset_for_position(text: &str, line: usize, character: usize) -> usize {
    let line_start = if line == 0 {
        0
    } else {
        let mut seen = 0;
        let mut start = text.len();
        for (byte, ch) in text.char_indices() {
            if ch == '\n' {
                seen += 1;
                if seen == line {
                    start = byte + 1;
                    break;
                }
            }
        }
        start
    };

    let mut utf16 = 0;
    for (byte, ch) in text[line_start..].char_indices() {
        if utf16 >= character || ch == '\n' {
            return line_start + byte;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn intake_records_cancellations_and_queues_other_messages() {
        let mut queue = VecDeque::new();
        let mut cancellations = PendingCancellations::default();

        intake(
            json!({"id": 1, "method": "textDocument/completion"}),
            &mut queue,
            &mut cancellations,
        );
        intake(
            json!({"method": "$/cancelRequest", "params": {"id": 1}}),
            &mut queue,
            &mut cancellations,
        );

        // The cancel is consumed into the registry, not queued for handling.
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0]["id"], 1);

        // The matching request id is cancelled exactly once.
        assert!(cancellations.take(&json!(1)));
        assert!(!cancellations.take(&json!(1)));
        // An unrelated id was never cancelled.
        assert!(!cancellations.take(&json!(2)));
    }

    #[test]
    fn cancellations_distinguish_numeric_and_string_ids() {
        let mut cancellations = PendingCancellations::default();
        cancellations.record(&json!("abc"));
        assert!(!cancellations.take(&json!(1)));
        assert!(cancellations.take(&json!("abc")));
    }
}
