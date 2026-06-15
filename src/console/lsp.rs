use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use tokio::io::{AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

use crate::error::{Result, RototoError};
use crate::sdk::Workspace;

use super::store::WorkspaceRecord;

/* Bridges the console editor to the real rototo language server. The console
never reimplements lint, completion, or hover semantics: a draft editing
session stages the draft branch and runs the LSP server in-process over a
duplex pipe, forwarding the editor's unsaved text as document overlays.
Calls are serialized per session so the single JSON-RPC stream stays
ordered. */

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const IDLE_SESSION: Duration = Duration::from_secs(10 * 60);

/// Diagnostic shape returned from the console LSP endpoint.
///
/// The language server publishes full LSP diagnostics; the console simplifies
/// them into this stable browser-facing shape for one editor update response.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDiagnosticWire {
    pub message: String,
    pub severity: &'static str,
    pub rule: Option<String>,
    pub help: Option<String>,
    pub range: JsonValue,
}

/// Completion item shape returned from the console LSP endpoint.
///
/// It is a trimmed JSON-RPC completion item built per request so the frontend
/// does not depend on the full LSP schema.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspCompletionWire {
    pub label: String,
    pub kind: i64,
    pub detail: Option<String>,
}

/// Hover response shape returned from the console LSP endpoint.
///
/// The value is extracted from the LSP server response for one hover request
/// and discarded after serialization.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspHoverWire {
    pub value: String,
    pub range: Option<JsonValue>,
}

/// Outstanding JSON-RPC requests for one language-server session.
///
/// Request ids are allocated monotonically and mapped to one-shot senders.
/// The reader task removes entries as responses arrive; timeouts remove them
/// when the server does not answer.
struct PendingRequests {
    next_id: i64,
    by_id: HashMap<i64, oneshot::Sender<std::result::Result<JsonValue, String>>>,
}

/// State shared between the request writer and JSON-RPC reader task.
///
/// It owns pending request channels and the latest diagnostics published per
/// document URI. The session drops this state when its tasks are aborted.
struct SessionShared {
    pending: Mutex<PendingRequests>,
    diagnostics_by_uri: Mutex<HashMap<String, Vec<JsonValue>>>,
}

/// Live in-process language-server session for one user and draft.
///
/// A session owns the staged checkout handle, duplex writer, server task,
/// reader task, open document overlays, and idle timestamp. It is created on
/// demand, reused across editor requests, and dropped on draft invalidation or
/// after the idle window.
struct LspSession {
    /// Keeps the staged checkout's temp directory alive for the session.
    _staged: Arc<Workspace>,
    root: PathBuf,
    writer: WriteHalf<DuplexStream>,
    shared: Arc<SessionShared>,
    open_documents: HashMap<String, (i64, String)>,
    server_task: JoinHandle<()>,
    reader_task: JoinHandle<()>,
    last_used: Instant,
    closed: bool,
}

impl LspSession {
    fn shutdown(&mut self) {
        self.closed = true;
        self.server_task.abort();
        self.reader_task.abort();
    }
}

impl Drop for LspSession {
    fn drop(&mut self) {
        self.server_task.abort();
        self.reader_task.abort();
    }
}

/// Shared mutable holder for a single user/draft LSP session.
///
/// The slot lets concurrent editor requests serialize access while background
/// cleanup or draft invalidation can take and shut down the session.
type SessionSlot = Arc<Mutex<Option<LspSession>>>;

/// Per-(user, draft) language server session registry.
///
/// The registry lives in `ConsoleState` for the process lifetime. Individual
/// sessions are created on demand, refreshed when stale, and explicitly dropped
/// whenever a draft save changes the staged branch content.
#[derive(Clone, Default)]
pub struct LspSessions {
    slots: Arc<Mutex<HashMap<String, SessionSlot>>>,
}

impl LspSessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// The staged checkout goes stale once a save commits to the draft
    /// branch; drop the session so the next request restages.
    pub async fn drop_sessions_for_draft(&self, draft_id: &str) {
        let suffix = format!(":{draft_id}");
        let mut slots = self.slots.lock().await;
        let keys: Vec<String> = slots
            .keys()
            .filter(|key| key.ends_with(&suffix))
            .cloned()
            .collect();
        for key in keys {
            if let Some(slot) = slots.remove(&key)
                && let Some(mut session) = slot.lock().await.take()
            {
                session.shutdown();
            }
        }
    }

    pub async fn update(
        &self,
        user_id: &str,
        draft_id: &str,
        staged: Arc<Workspace>,
        workspace: &WorkspaceRecord,
        path: &str,
        text: &str,
    ) -> Result<Vec<LspDiagnosticWire>> {
        let slot = self.slot(user_id, draft_id).await;
        let mut guard = slot.lock().await;
        let session = self.session(&mut guard, staged).await?;
        let uri = sync_document(session, workspace, path, text).await?;
        // documentSymbol acts as a barrier: the server publishes diagnostics
        // for the didChange before it answers the next request on the same
        // stream.
        request(
            session,
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
        )
        .await?;
        let diagnostics = session
            .shared
            .diagnostics_by_uri
            .lock()
            .await
            .get(&uri)
            .cloned()
            .unwrap_or_default();
        Ok(diagnostics.iter().map(simplify_diagnostic).collect())
    }

    #[expect(clippy::too_many_arguments)]
    pub async fn completion(
        &self,
        user_id: &str,
        draft_id: &str,
        staged: Arc<Workspace>,
        workspace: &WorkspaceRecord,
        path: &str,
        text: &str,
        position: JsonValue,
    ) -> Result<Vec<LspCompletionWire>> {
        let slot = self.slot(user_id, draft_id).await;
        let mut guard = slot.lock().await;
        let session = self.session(&mut guard, staged).await?;
        let uri = sync_document(session, workspace, path, text).await?;
        let result = request(
            session,
            "textDocument/completion",
            json!({ "textDocument": { "uri": uri }, "position": position }),
        )
        .await?;
        let items = result.as_array().cloned().unwrap_or_default();
        Ok(items
            .iter()
            .map(|item| LspCompletionWire {
                label: item
                    .get("label")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                kind: item.get("kind").and_then(JsonValue::as_i64).unwrap_or(0),
                detail: item
                    .get("detail")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned),
            })
            .collect())
    }

    #[expect(clippy::too_many_arguments)]
    pub async fn hover(
        &self,
        user_id: &str,
        draft_id: &str,
        staged: Arc<Workspace>,
        workspace: &WorkspaceRecord,
        path: &str,
        text: &str,
        position: JsonValue,
    ) -> Result<Option<LspHoverWire>> {
        let slot = self.slot(user_id, draft_id).await;
        let mut guard = slot.lock().await;
        let session = self.session(&mut guard, staged).await?;
        let uri = sync_document(session, workspace, path, text).await?;
        let result = request(
            session,
            "textDocument/hover",
            json!({ "textDocument": { "uri": uri }, "position": position }),
        )
        .await?;
        let Some(value) = result
            .get("contents")
            .and_then(|contents| contents.get("value"))
            .and_then(JsonValue::as_str)
        else {
            return Ok(None);
        };
        Ok(Some(LspHoverWire {
            value: value.to_owned(),
            range: result
                .get("range")
                .cloned()
                .filter(|range| !range.is_null()),
        }))
    }

    async fn slot(&self, user_id: &str, draft_id: &str) -> SessionSlot {
        let key = format!("{user_id}:{draft_id}");
        let mut slots = self.slots.lock().await;
        // Opportunistically reap idle sessions; the map stays small.
        for slot in slots.values() {
            if let Ok(mut guard) = slot.try_lock()
                && let Some(session) = guard.as_mut()
                && session.last_used.elapsed() > IDLE_SESSION
            {
                session.shutdown();
                *guard = None;
            }
        }
        slots.entry(key).or_default().clone()
    }

    async fn session<'a>(
        &self,
        guard: &'a mut Option<LspSession>,
        staged: Arc<Workspace>,
    ) -> Result<&'a mut LspSession> {
        let stale = guard
            .as_ref()
            .map(|session| {
                session.closed
                    || !Arc::ptr_eq(&session._staged, &staged)
                    || session.root != staged.root()
            })
            .unwrap_or(true);
        if stale {
            if let Some(mut session) = guard.take() {
                session.shutdown();
            }
            *guard = Some(create_session(staged).await?);
        }
        let session = guard.as_mut().expect("session was just created");
        session.last_used = Instant::now();
        Ok(session)
    }
}

async fn create_session(staged: Arc<Workspace>) -> Result<LspSession> {
    let root = staged.root().to_path_buf();
    let (console_io, server_io) = tokio::io::duplex(4 * 1024 * 1024);
    let (server_read, server_write) = tokio::io::split(server_io);
    let server_task = tokio::spawn(async move {
        if let Err(err) = crate::lsp::serve(BufReader::new(server_read), server_write).await {
            tracing::warn!(error = %err, "console LSP server session ended with an error");
        }
    });

    let (console_read, console_write) = tokio::io::split(console_io);
    let shared = Arc::new(SessionShared {
        pending: Mutex::new(PendingRequests {
            next_id: 1,
            by_id: HashMap::new(),
        }),
        diagnostics_by_uri: Mutex::new(HashMap::new()),
    });
    let reader_shared = shared.clone();
    let reader_task = tokio::spawn(async move {
        read_loop(console_read, reader_shared).await;
    });

    let mut session = LspSession {
        _staged: staged,
        root: root.clone(),
        writer: console_write,
        shared,
        open_documents: HashMap::new(),
        server_task,
        reader_task,
        last_used: Instant::now(),
        closed: false,
    };

    request(
        &mut session,
        "initialize",
        json!({
            "rootUri": file_uri(&root, None),
            "capabilities": {},
        }),
    )
    .await?;
    crate::lsp::write_notification(&mut session.writer, "initialized", json!({})).await?;
    Ok(session)
}

async fn read_loop(console_read: ReadHalf<DuplexStream>, shared: Arc<SessionShared>) {
    let mut reader = BufReader::new(console_read);
    loop {
        let message = match crate::lsp::read_message(&mut reader).await {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(error = %err, "console LSP stream read failed");
                break;
            }
        };
        if let Some(id) = message.get("id").and_then(JsonValue::as_i64) {
            let sender = shared.pending.lock().await.by_id.remove(&id);
            if let Some(sender) = sender {
                let outcome = match message.get("error") {
                    Some(error) => Err(error
                        .get("message")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("rototo lsp request failed")
                        .to_owned()),
                    None => Ok(message.get("result").cloned().unwrap_or(JsonValue::Null)),
                };
                let _ = sender.send(outcome);
            }
            continue;
        }
        if message.get("method").and_then(JsonValue::as_str)
            == Some("textDocument/publishDiagnostics")
            && let Some(params) = message.get("params")
            && let Some(uri) = params.get("uri").and_then(JsonValue::as_str)
        {
            let diagnostics = params
                .get("diagnostics")
                .and_then(JsonValue::as_array)
                .cloned()
                .unwrap_or_default();
            shared
                .diagnostics_by_uri
                .lock()
                .await
                .insert(uri.to_owned(), diagnostics);
        }
    }
    // The stream is gone; fail any requests still waiting on it.
    let mut pending = shared.pending.lock().await;
    for (_, sender) in pending.by_id.drain() {
        let _ = sender.send(Err("rototo lsp session is closed".to_owned()));
    }
}

async fn request(session: &mut LspSession, method: &str, params: JsonValue) -> Result<JsonValue> {
    if session.closed {
        return Err(RototoError::new("rototo lsp session is closed"));
    }
    let started = std::time::Instant::now();
    let (sender, receiver) = oneshot::channel();
    let id = {
        let mut pending = session.shared.pending.lock().await;
        let id = pending.next_id;
        pending.next_id += 1;
        pending.by_id.insert(id, sender);
        id
    };
    if let Err(err) = crate::lsp::write_request(&mut session.writer, id, method, params).await {
        session.shared.pending.lock().await.by_id.remove(&id);
        session.closed = true;
        return Err(err);
    }
    let _ = session.writer.flush().await;
    let result = match tokio::time::timeout(REQUEST_TIMEOUT, receiver).await {
        Ok(Ok(Ok(result))) => Ok(result),
        Ok(Ok(Err(message))) => Err(RototoError::new(message)),
        Ok(Err(_)) => {
            session.closed = true;
            Err(RototoError::new("rototo lsp session is closed"))
        }
        Err(_) => {
            session.shared.pending.lock().await.by_id.remove(&id);
            Err(RototoError::new(format!("rototo lsp {method} timed out")))
        }
    };
    tracing::info!(
        operation = "lsp.request",
        method = %method,
        ok = result.is_ok(),
        latency_ms = started.elapsed().as_millis(),
        "console LSP request completed"
    );
    result
}

async fn sync_document(
    session: &mut LspSession,
    workspace: &WorkspaceRecord,
    repo_path: &str,
    text: &str,
) -> Result<String> {
    let relative = workspace_relative_path(&workspace.path, repo_path);
    let uri = file_uri(&session.root, Some(&relative));
    match session.open_documents.get_mut(&uri) {
        None => {
            session
                .open_documents
                .insert(uri.clone(), (1, text.to_owned()));
            crate::lsp::write_notification(
                &mut session.writer,
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": language_id(&relative),
                        "version": 1,
                        "text": text,
                    }
                }),
            )
            .await?;
        }
        Some((version, open_text)) if open_text.as_str() != text => {
            *version += 1;
            *open_text = text.to_owned();
            let version = *version;
            crate::lsp::write_notification(
                &mut session.writer,
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": version },
                    "contentChanges": [{ "text": text }],
                }),
            )
            .await?;
        }
        Some(_) => {}
    }
    Ok(uri)
}

fn simplify_diagnostic(raw: &JsonValue) -> LspDiagnosticWire {
    let data = raw.get("data");
    let rule = data
        .and_then(|data| data.get("rule"))
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
        .or_else(|| {
            raw.get("code")
                .and_then(JsonValue::as_str)
                .map(str::to_owned)
        });
    let help = data
        .and_then(|data| data.get("help"))
        .and_then(JsonValue::as_str)
        .map(str::to_owned);
    LspDiagnosticWire {
        message: raw
            .get("message")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_owned(),
        severity: if raw.get("severity").and_then(JsonValue::as_i64) == Some(1) {
            "error"
        } else {
            "warning"
        },
        rule,
        help,
        range: raw.get("range").cloned().unwrap_or_else(|| {
            json!({
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 0 },
            })
        }),
    }
}

fn workspace_relative_path(workspace_path: &str, repo_path: &str) -> String {
    if workspace_path == "." {
        return repo_path.to_owned();
    }
    repo_path
        .strip_prefix(&format!("{workspace_path}/"))
        .unwrap_or(repo_path)
        .to_owned()
}

fn file_uri(root: &Path, relative: Option<&str>) -> String {
    let path = match relative {
        Some(relative) => format!("{}/{relative}", root.display()),
        None => root.display().to_string(),
    };
    let encoded: Vec<String> = path
        .split('/')
        .map(|segment| {
            let mut out = String::with_capacity(segment.len());
            for byte in segment.bytes() {
                match byte {
                    b'A'..=b'Z'
                    | b'a'..=b'z'
                    | b'0'..=b'9'
                    | b'-'
                    | b'_'
                    | b'.'
                    | b'~'
                    | b'!'
                    | b'*'
                    | b'\''
                    | b'('
                    | b')' => out.push(byte as char),
                    other => out.push_str(&format!("%{other:02X}")),
                }
            }
            out
        })
        .collect();
    format!("file://{}", encoded.join("/"))
}

fn language_id(path: &str) -> &'static str {
    if path.ends_with(".json") {
        "json"
    } else if path.ends_with(".lua") {
        "lua"
    } else {
        "toml"
    }
}
