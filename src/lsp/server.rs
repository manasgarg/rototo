use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value as JsonValue;
use tokio::io::{AsyncBufRead, AsyncWrite, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::error::{Result, RototoError};
use crate::lint::{LintInput, OverlayDocument, PackageLintSnapshot, lint_package_snapshot};

use super::convert::{
    lsp_completion_item, lsp_document_symbol, lsp_hover, lsp_location, lsp_reference,
    publish_diagnostics_params,
};
use super::protocol::{
    LspCompletionItem, LspDocumentSymbol, LspHover, LspLocation, initialize_result,
};
use super::transport::{
    error_response_message, notification_message, read_message, response_message, write_message,
};
use super::uri::{
    initialize_package_root, json_i32, package_relative_path, path_from_file_uri,
    source_position_from_json,
};

/// JSON-RPC error code for a request the client asked to cancel (LSP
/// `RequestCancelled`).
const REQUEST_CANCELLED: i64 = -32800;

/// How long a burst of edits settles before diagnostics recompute. Long
/// enough to fold a typing burst into one lint run, short enough that
/// feedback still feels immediate.
const DIAGNOSTICS_DEBOUNCE: Duration = Duration::from_millis(75);

pub async fn serve_stdio() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    serve(BufReader::new(stdin), stdout).await
}

/// Serves one LSP session over any transport pair. `serve_stdio` wraps this
/// for editors; in-process hosts (the console bridge) run it over a duplex.
pub async fn serve<R, W>(reader: R, writer: W) -> Result<()>
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    // A dedicated reader task pulls messages off the wire eagerly into a channel
    // so a `$/cancelRequest` sent right after a request is observed before that
    // request is dequeued, and a dedicated writer task serializes responses that
    // now come from concurrently running request tasks.
    let (inbound_sender, mut inbound) = mpsc::unbounded_channel();
    let reader_task = tokio::spawn(async move {
        let mut reader = reader;
        while let Ok(Some(message)) = read_message(&mut reader).await {
            if inbound_sender.send(message).is_err() {
                break;
            }
        }
    });
    let (outbound, mut outbound_receiver) = mpsc::unbounded_channel::<JsonValue>();
    let writer_task = tokio::spawn(async move {
        let mut writer = writer;
        while let Some(message) = outbound_receiver.recv().await {
            if write_message(&mut writer, message).await.is_err() {
                break;
            }
        }
    });

    let mut server = LspServer::new();
    let mut queue: VecDeque<JsonValue> = VecDeque::new();
    let mut cancellations = PendingCancellations::default();
    // Cancellation senders for requests currently running on spawned tasks.
    let inflight: Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<()>>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    let diagnostics = DiagnosticsScheduler::default();

    let exit = loop {
        if queue.is_empty() {
            match inbound.recv().await {
                Some(message) => intake(message, &mut queue, &mut cancellations, &inflight),
                None => break Ok(()),
            }
        }
        // Drain everything already buffered so a pending cancel is recorded
        // before the request it targets reaches the front of the queue.
        while let Ok(message) = inbound.try_recv() {
            intake(message, &mut queue, &mut cancellations, &inflight);
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
        let params = message.get("params").cloned().unwrap_or(JsonValue::Null);

        // A request the client cancelled while it waited in the queue returns the
        // standard RequestCancelled error rather than computing a result.
        if let Some(id) = &id
            && cancellations.take(id)
        {
            let _ = outbound.send(error_response_message(
                id.clone(),
                REQUEST_CANCELLED,
                "request cancelled",
            ));
            continue;
        }

        match (id, method.as_str()) {
            (Some(id), "initialize") => {
                let result = async {
                    server.package_root = initialize_package_root(&params).await?;
                    Ok::<_, RototoError>(initialize_result())
                }
                .await;
                send_outcome(&outbound, id, result);
            }
            (Some(id), "shutdown") => {
                server.shutdown_requested = true;
                let _ = outbound.send(response_message(id, JsonValue::Null));
            }
            (
                Some(id),
                "textDocument/documentSymbol"
                | "textDocument/completion"
                | "textDocument/hover"
                | "textDocument/definition"
                | "textDocument/references",
            ) => {
                // Reads run concurrently on a spawned task against an immutable
                // view of the session; mutations stay ordered on this loop, so a
                // read spawned after a didChange always sees the newer buffers.
                let context = server.read_context();
                let outbound = outbound.clone();
                let inflight_map = Arc::clone(&inflight);
                let (cancel_sender, mut cancelled) = oneshot::channel();
                let id_key = id.to_string();
                inflight_map
                    .lock()
                    .unwrap()
                    .insert(id_key.clone(), cancel_sender);
                tokio::spawn(async move {
                    let work = handle_read_request(context, &method, params);
                    tokio::pin!(work);
                    let message = tokio::select! {
                        biased;
                        _ = &mut cancelled => {
                            error_response_message(id, REQUEST_CANCELLED, "request cancelled")
                        }
                        result = &mut work => match result {
                            Ok(result) => response_message(id, result),
                            Err(err) => error_response_message(
                                id,
                                -32603,
                                &format!("rototo LSP request failed: {err}"),
                            ),
                        },
                    };
                    inflight_map.lock().unwrap().remove(&id_key);
                    let _ = outbound.send(message);
                });
            }
            (Some(id), _) => {
                let _ = outbound.send(error_response_message(id, -32601, "method not found"));
            }
            (None, "initialized") => {
                diagnostics.schedule(server.read_context(), outbound.clone());
            }
            (None, "textDocument/didOpen") => {
                if let Err(err) = server.open_document(params) {
                    tracing::warn!(error = %err, "rototo LSP didOpen failed");
                }
                diagnostics.schedule(server.read_context(), outbound.clone());
            }
            (None, "textDocument/didChange") => {
                if let Err(err) = server.change_document(params) {
                    tracing::warn!(error = %err, "rototo LSP didChange failed");
                }
                diagnostics.schedule(server.read_context(), outbound.clone());
            }
            (None, "textDocument/didSave") | (None, "textDocument/didClose") => {
                if let Err(err) = server.remove_document_overlay(params) {
                    tracing::warn!(error = %err, "rototo LSP document close failed");
                }
                diagnostics.schedule(server.read_context(), outbound.clone());
            }
            (None, "exit") => {
                if server.shutdown_requested {
                    break Ok(());
                }
                break Err(RototoError::new("LSP exit received before shutdown"));
            }
            (None, _) => {}
        }
    };

    // Closing the outbound channel lets the writer drain what request tasks
    // already produced, then stop.
    drop(outbound);
    let _ = writer_task.await;
    // Release the transport's read half too, so an in-process host sees the
    // stream end; a detached reader would otherwise pin the session open.
    reader_task.abort();
    exit
}

fn send_outcome(
    outbound: &mpsc::UnboundedSender<JsonValue>,
    id: JsonValue,
    result: Result<JsonValue>,
) {
    let message = match result {
        Ok(result) => response_message(id, result),
        Err(err) => {
            error_response_message(id, -32603, &format!("rototo LSP request failed: {err}"))
        }
    };
    let _ = outbound.send(message);
}

/// Diagnostics recompute on a background task after edits settle. Every
/// mutation bumps the generation; a scheduled run that discovers a newer
/// generation (before or after its lint build) simply drops out, so only the
/// latest buffers are ever published, and publish order is guarded so a slow
/// stale build cannot overwrite a newer publication.
#[derive(Default)]
struct DiagnosticsScheduler {
    generation: Arc<AtomicU64>,
    published: Arc<AtomicU64>,
}

impl DiagnosticsScheduler {
    fn schedule(&self, context: ReadContext, outbound: mpsc::UnboundedSender<JsonValue>) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let current = Arc::clone(&self.generation);
        let published = Arc::clone(&self.published);
        tokio::spawn(async move {
            tokio::time::sleep(DIAGNOSTICS_DEBOUNCE).await;
            if current.load(Ordering::SeqCst) != generation {
                return;
            }
            let snapshot = match context.snapshot().await {
                Ok(Some(snapshot)) => snapshot,
                Ok(None) => return,
                Err(err) => {
                    tracing::warn!(error = %err, "rototo LSP diagnostics failed");
                    return;
                }
            };
            if current.load(Ordering::SeqCst) != generation {
                return;
            }
            // fetch_max returns the previous published generation; a newer one
            // already out means this build is stale.
            if published.fetch_max(generation, Ordering::SeqCst) > generation {
                return;
            }
            for publication in publish_diagnostics_params(&snapshot.lint) {
                let Ok(params) = serde_json::to_value(publication) else {
                    continue;
                };
                let _ = outbound.send(notification_message(
                    "textDocument/publishDiagnostics",
                    params,
                ));
            }
        });
    }
}

/// Everything a read request needs, captured at spawn time so concurrent
/// requests run against a consistent view while the session keeps mutating.
#[derive(Clone)]
pub(super) struct ReadContext {
    root: Option<PathBuf>,
    revision: u64,
    overlays: BTreeMap<String, OverlayDocument>,
    cache: Arc<SnapshotCache>,
}

impl ReadContext {
    async fn snapshot(&self) -> Result<Option<Arc<PackageLintSnapshot>>> {
        let Some(root) = &self.root else {
            return Ok(None);
        };
        self.cache
            .snapshot(root, self.revision, &self.overlays)
            .await
            .map(Some)
    }

    fn package_path_for_uri(&self, uri: &str) -> Result<String> {
        let Some(root) = &self.root else {
            return Err(RototoError::new("LSP package root is not initialized"));
        };
        let path = path_from_file_uri(uri)?;
        package_relative_path(root, &path)
    }
}

/// The revision-keyed snapshot store shared by every read. The async lock is
/// deliberate: concurrent requests against the same revision coalesce onto one
/// lint build instead of racing.
#[derive(Default)]
struct SnapshotCache {
    cache: tokio::sync::Mutex<Option<(u64, Arc<PackageLintSnapshot>)>>,
    build_count: AtomicUsize,
}

impl SnapshotCache {
    async fn snapshot(
        &self,
        root: &Path,
        revision: u64,
        overlays: &BTreeMap<String, OverlayDocument>,
    ) -> Result<Arc<PackageLintSnapshot>> {
        let mut cache = self.cache.lock().await;
        if let Some((cached_revision, snapshot)) = cache.as_ref()
            && *cached_revision == revision
        {
            return Ok(Arc::clone(snapshot));
        }
        let mut input = LintInput::new(root.to_path_buf());
        input.overlays = overlays.clone();
        self.build_count.fetch_add(1, Ordering::Relaxed);
        let snapshot = Arc::new(lint_package_snapshot(input).await?);
        // A read spawned before a newer mutation may finish after it; never
        // let its older snapshot displace a newer cached one.
        if cache
            .as_ref()
            .is_none_or(|(cached_revision, _)| *cached_revision < revision)
        {
            *cache = Some((revision, Arc::clone(&snapshot)));
        }
        Ok(snapshot)
    }
}

/// Dispatch one concurrent read request against a fixed session view.
async fn handle_read_request(
    context: ReadContext,
    method: &str,
    params: JsonValue,
) -> Result<JsonValue> {
    let into_json = |value: serde_json::Result<JsonValue>| {
        value.map_err(|err| RototoError::new(err.to_string()))
    };
    match method {
        "textDocument/documentSymbol" => into_json(serde_json::to_value(
            read_document_symbols(context, params).await?,
        )),
        "textDocument/completion" => into_json(serde_json::to_value(
            read_completion_items(context, params).await?,
        )),
        "textDocument/hover" => Ok(read_hover(context, params)
            .await?
            .map(serde_json::to_value)
            .transpose()
            .map_err(|err| RototoError::new(err.to_string()))?
            .unwrap_or(JsonValue::Null)),
        "textDocument/definition" => Ok(read_definition(context, params)
            .await?
            .map(serde_json::to_value)
            .transpose()
            .map_err(|err| RototoError::new(err.to_string()))?
            .unwrap_or(JsonValue::Null)),
        "textDocument/references" => into_json(serde_json::to_value(
            read_references(context, params).await?,
        )),
        _ => Err(RototoError::new("method not found")),
    }
}

async fn read_document_symbols(
    context: ReadContext,
    params: JsonValue,
) -> Result<Vec<LspDocumentSymbol>> {
    let uri = text_document_uri(&params, "documentSymbol")?;
    let path = context.package_path_for_uri(&uri)?;
    let Some(snapshot) = context.snapshot().await? else {
        return Ok(Vec::new());
    };
    Ok(snapshot
        .document_symbols(&path)
        .iter()
        .map(lsp_document_symbol)
        .collect())
}

async fn read_completion_items(
    context: ReadContext,
    params: JsonValue,
) -> Result<Vec<LspCompletionItem>> {
    let uri = text_document_uri(&params, "completion")?;
    let position = source_position_from_json(
        params
            .get("position")
            .ok_or_else(|| RototoError::new("completion missing position"))?,
    )?;
    let path = context.package_path_for_uri(&uri)?;
    let Some(snapshot) = context.snapshot().await? else {
        return Ok(Vec::new());
    };
    Ok(snapshot
        .completion_items(&path, position)
        .iter()
        .map(lsp_completion_item)
        .collect())
}

async fn read_hover(context: ReadContext, params: JsonValue) -> Result<Option<LspHover>> {
    let uri = text_document_uri(&params, "hover")?;
    let position = source_position_from_json(
        params
            .get("position")
            .ok_or_else(|| RototoError::new("hover missing position"))?,
    )?;
    let path = context.package_path_for_uri(&uri)?;
    let Some(snapshot) = context.snapshot().await? else {
        return Ok(None);
    };
    Ok(snapshot.hover(&path, position).map(lsp_hover))
}

async fn read_definition(context: ReadContext, params: JsonValue) -> Result<Option<LspLocation>> {
    let uri = text_document_uri(&params, "definition")?;
    let position = source_position_from_json(
        params
            .get("position")
            .ok_or_else(|| RototoError::new("definition missing position"))?,
    )?;
    let path = context.package_path_for_uri(&uri)?;
    let Some(snapshot) = context.snapshot().await? else {
        return Ok(None);
    };
    Ok(snapshot.definition(&path, position).map(lsp_location))
}

async fn read_references(context: ReadContext, params: JsonValue) -> Result<Vec<LspLocation>> {
    let uri = text_document_uri(&params, "references")?;
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
    let path = context.package_path_for_uri(&uri)?;
    let Some(snapshot) = context.snapshot().await? else {
        return Ok(Vec::new());
    };
    Ok(snapshot
        .references(&path, position, include_declaration)
        .iter()
        .map(lsp_reference)
        .collect())
}

fn text_document_uri(params: &JsonValue, method: &str) -> Result<String> {
    params
        .get("textDocument")
        .and_then(|text_document| text_document.get("uri"))
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
        .ok_or_else(|| RototoError::new(format!("{method} missing textDocument.uri")))
}

/// Route an incoming message: a `$/cancelRequest` cancels an in-flight request
/// task if one is running, otherwise records the id for the queued case;
/// everything else is queued.
fn intake(
    message: JsonValue,
    queue: &mut VecDeque<JsonValue>,
    cancellations: &mut PendingCancellations,
    inflight: &Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<()>>>>,
) {
    if message.get("method").and_then(JsonValue::as_str) == Some("$/cancelRequest") {
        if let Some(id) = message.get("params").and_then(|params| params.get("id")) {
            if let Some(sender) = inflight.lock().unwrap().remove(&id.to_string()) {
                let _ = sender.send(());
            } else {
                cancellations.record(id);
            }
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
    /// Last package snapshot, reused while `revision` is unchanged and shared
    /// with every concurrently running read. The lint pipeline reads the whole
    /// package from disk plus overlays, so without this every request and
    /// every keystroke would recompute the entire package.
    cache: Arc<SnapshotCache>,
}

impl LspServer {
    pub(super) fn new() -> Self {
        Self {
            package_root: None,
            overlays: BTreeMap::new(),
            shutdown_requested: false,
            revision: 0,
            cache: Arc::new(SnapshotCache::default()),
        }
    }

    /// An immutable view of the session for one read: buffers as they are at
    /// spawn time plus the shared snapshot cache.
    pub(super) fn read_context(&self) -> ReadContext {
        ReadContext {
            root: self.package_root.clone(),
            revision: self.revision,
            overlays: self.overlays.clone(),
            cache: Arc::clone(&self.cache),
        }
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

    #[cfg(test)]
    pub(super) async fn package_diagnostics(
        &self,
    ) -> Result<Vec<super::protocol::PublishDiagnosticsParams>> {
        let Some(snapshot) = self.read_context().snapshot().await? else {
            return Ok(Vec::new());
        };
        Ok(publish_diagnostics_params(&snapshot.lint))
    }

    #[cfg(test)]
    pub(super) async fn document_symbols(
        &self,
        params: JsonValue,
    ) -> Result<Vec<LspDocumentSymbol>> {
        read_document_symbols(self.read_context(), params).await
    }

    #[cfg(test)]
    pub(super) async fn completion_items(
        &self,
        params: JsonValue,
    ) -> Result<Vec<LspCompletionItem>> {
        read_completion_items(self.read_context(), params).await
    }

    #[cfg(test)]
    pub(super) async fn hover(&self, params: JsonValue) -> Result<Option<LspHover>> {
        read_hover(self.read_context(), params).await
    }

    #[cfg(test)]
    pub(super) async fn definition(&self, params: JsonValue) -> Result<Option<LspLocation>> {
        read_definition(self.read_context(), params).await
    }

    #[cfg(test)]
    pub(super) async fn references(&self, params: JsonValue) -> Result<Vec<LspLocation>> {
        read_references(self.read_context(), params).await
    }

    #[cfg(test)]
    pub(super) fn snapshot_build_count(&self) -> usize {
        self.cache.build_count.load(Ordering::Relaxed)
    }

    fn package_path_for_uri(&self, uri: &str) -> Result<String> {
        let Some(root) = &self.package_root else {
            return Err(RototoError::new("LSP package root is not initialized"));
        };
        let path = path_from_file_uri(uri)?;
        package_relative_path(root, &path)
    }

    #[cfg(test)]
    pub(super) fn overlay_text(&self, path: &str) -> Option<&str> {
        self.overlays
            .get(path)
            .map(|document| document.text.as_str())
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

        let inflight = Arc::new(std::sync::Mutex::new(HashMap::new()));
        intake(
            json!({"id": 1, "method": "textDocument/completion"}),
            &mut queue,
            &mut cancellations,
            &inflight,
        );
        intake(
            json!({"method": "$/cancelRequest", "params": {"id": 1}}),
            &mut queue,
            &mut cancellations,
            &inflight,
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
