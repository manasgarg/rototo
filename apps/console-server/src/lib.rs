//! Internal console bindings: the surface the TypeScript console server
//! reaches the Rust core through. Everything speaks JSON and package roots.
//!
//! The contract with the server (design/console-git-ops.md): TypeScript
//! resolves refs and hands this layer pins; nothing here ever sees a branch
//! name. Staged trees come from [`JsPinStore`], and every other function
//! takes a package root inside such a tree (or any local package directory).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use rototo::edit::{EditOperation, EditOptions};
use rototo::fixtures::{FixtureGenerateSelection, FixtureTargetSelection};
use rototo::model::{InspectSelection, PackageInspectRequest};
use rototo::{PinStore, SourceAuth, SourceOptions};
use serde_json::Value as JsonValue;

#[napi]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `"release"` or `"debug"`. The latency harness asserts its budgets only
/// against release builds; a debug native module measures but does not gate.
#[napi(js_name = "buildProfile")]
pub fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// Pin-keyed staging: trees keyed by `(remote, pin)`, built once, reused
/// from disk, evicted only by the size budget. `pin` must be a full commit
/// SHA; resolving refs is the server's job.
#[napi(js_name = "_PinStore")]
pub struct JsPinStore {
    inner: Arc<PinStore>,
}

#[napi]
impl JsPinStore {
    #[napi(constructor)]
    pub fn new(root: String, max_bytes: Option<f64>) -> Result<Self> {
        let max_bytes = match max_bytes {
            Some(value) if value.is_finite() && value > 0.0 => value as u64,
            Some(_) => {
                return Err(Error::from_reason(
                    "maxBytes must be a positive finite number",
                ));
            }
            None => u64::MAX,
        };
        Ok(Self {
            inner: Arc::new(PinStore::new(root, max_bytes)),
        })
    }

    /// The staged tree for `(remote, pin)` as an absolute path.
    #[napi]
    pub async fn stage(
        &self,
        remote: String,
        pin: String,
        token: Option<String>,
    ) -> Result<String> {
        let options = match token {
            Some(token) => SourceOptions::new().with_auth(SourceAuth::Bearer(token)),
            None => SourceOptions::new(),
        };
        let tree = self
            .inner
            .stage(&remote, &pin, &options)
            .await
            .map_err(js_err)?;
        path_string(&tree)
    }
}

/// Package roots inside a staged tree: directories holding a
/// `rototo-package.toml`, as `/`-separated paths relative to `root` (`"."`
/// for the tree root itself). Rebuildable data for `discovered_packages`.
#[napi(js_name = "discoverPackages")]
pub async fn discover_packages(root: String) -> Result<Vec<String>> {
    let root = PathBuf::from(root);
    let mut found = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        if dir.join("rototo-package.toml").is_file() {
            let relative = dir
                .strip_prefix(&root)
                .map_err(|_| Error::from_reason("walked outside the discovery root"))?;
            found.push(relative_path_string(relative));
            // Packages do not nest: a package owns its subtree.
            continue;
        }
        let mut entries = tokio::fs::read_dir(&dir).await.map_err(|err| {
            Error::from_reason(format!("failed to read {}: {err}", dir.display()))
        })?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| Error::from_reason(format!("failed to read {}: {err}", dir.display())))?
        {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            let file_type = entry.file_type().await.map_err(|err| {
                Error::from_reason(format!("failed to inspect {}: {err}", dir.display()))
            })?;
            // Symlinks stay out of discovery: staged trees are plain files,
            // and following links out of the tree is never wanted.
            if file_type.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The package semantic model: entities, references (bidirectional via
/// `references_to`/`references_from` on the server side), extends edges,
/// lists, locations.
#[napi(js_name = "semanticModel")]
pub async fn semantic_model(root: String) -> Result<JsonValue> {
    let model = rototo::lint::package_semantic_model(Path::new(&root))
        .await
        .map_err(js_err)?;
    to_json(&model)
}

/// Stages the composed view of a package — its `extends` chain resolved
/// and layered, the same composition `Package::load` sees — into `dest`,
/// which must not exist yet. The console hands tree-staged package roots
/// here (ring 2 reads: fleet health, the cross-overlay matrix); remote
/// extends sources resolve through the ordinary source machinery.
#[napi(js_name = "stageComposed")]
pub async fn stage_composed(source: String, dest: String) -> Result<()> {
    let staged = rototo::stage_package_source(&source, &SourceOptions::default())
        .await
        .map_err(js_err)?;
    let from = staged.path().to_path_buf();
    let to = PathBuf::from(dest);
    tokio::task::spawn_blocking(move || copy_dir(&from, &to))
        .await
        .map_err(|error| Error::from_reason(error.to_string()))?
        .map_err(|error| Error::from_reason(error.to_string()))?;
    // `staged` may own a tempdir; it lives until here, after the copy.
    drop(staged);
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Full package lint: documents and diagnostics.
#[napi(js_name = "lintPackage")]
pub async fn lint_package(root: String) -> Result<JsonValue> {
    let lint = rototo::lint_package(Path::new(&root))
        .await
        .map_err(js_err)?;
    Ok(serde_json::json!({
        "documents": lint.documents,
        "diagnostics": lint.diagnostics,
    }))
}

/// The inspect report. `request` selects sections:
/// `{ variables?, catalogs?, lintRules?, lintAuthorities?, linters?, context? }`
/// where each selection is `"all"` or an array of ids, and `context` is an
/// evaluation context object for resolution sections.
#[napi(js_name = "inspectReport")]
pub async fn inspect_report(root: String, request: Option<JsonValue>) -> Result<JsonValue> {
    let request = inspect_request(request)?;
    let report = rototo::inspect_package_report(Path::new(&root), request)
        .await
        .map_err(js_err)?;
    to_json(&report)
}

/// The semantic diff between two staged package roots, with resolution
/// impacts when `context` is given.
#[napi(js_name = "diffPackages")]
pub async fn diff_packages(
    before_root: String,
    after_root: String,
    context: Option<JsonValue>,
) -> Result<JsonValue> {
    let diff = rototo::diff_packages(
        Path::new(&before_root),
        Path::new(&after_root),
        context.as_ref(),
    )
    .await
    .map_err(js_err)?;
    to_json(&diff)
}

/// The two-package diff evaluated under several labeled contexts at once:
/// one set of semantic changes plus lenient per-context resolution impacts.
/// `contexts` is `[{ label, context }, ...]`; this is the review panel's
/// execution delta with its denominator attached.
#[napi(js_name = "diffPackagesWithContexts")]
pub async fn diff_packages_with_contexts(
    before_root: String,
    after_root: String,
    contexts: JsonValue,
) -> Result<JsonValue> {
    let contexts = labeled_contexts(contexts)?;
    let diff = rototo::diff_packages_with_contexts(
        Path::new(&before_root),
        Path::new(&after_root),
        &contexts,
    )
    .await
    .map_err(js_err)?;
    to_json(&diff)
}

fn labeled_contexts(input: JsonValue) -> Result<Vec<rototo::model::LabeledContext>> {
    let JsonValue::Array(entries) = input else {
        return Err(Error::from_reason(
            "contexts must be an array of { label, context } objects",
        ));
    };
    entries
        .into_iter()
        .map(|entry| {
            let JsonValue::Object(mut fields) = entry else {
                return Err(Error::from_reason(
                    "each context must be a { label, context } object",
                ));
            };
            let label = match fields.get("label") {
                Some(JsonValue::String(label)) => label.clone(),
                _ => return Err(Error::from_reason("context label must be a string")),
            };
            let context = fields
                .remove("context")
                .filter(JsonValue::is_object)
                .ok_or_else(|| Error::from_reason("context must be a JSON object"))?;
            Ok(rototo::model::LabeledContext { label, context })
        })
        .collect()
}

/// Applies edit operations to the package at `root` and returns
/// `{ plan: { writes, deletes }, records }`. Pure: nothing is written; the
/// server turns the plan into one commit (or one local write).
/// `options.inherited` lists entity addresses the package does not own.
#[napi(js_name = "applyEdit")]
pub async fn apply_edit(
    root: String,
    operations: JsonValue,
    options: Option<JsonValue>,
) -> Result<JsonValue> {
    let operations: Vec<EditOperation> = serde_json::from_value(operations)
        .map_err(|err| Error::from_reason(format!("invalid edit operations: {err}")))?;
    let options = edit_options(options)?;
    let outcome = rototo::edit::apply_to_package(Path::new(&root), &operations, &options)
        .await
        .map_err(js_err)?;
    to_json(&outcome)
}

/// Traced resolution of every variable in the package under one context:
/// the batch call behind previews and the lit-up reference graph.
#[napi(js_name = "traceResolutions")]
pub async fn trace_resolutions(root: String, context: JsonValue) -> Result<JsonValue> {
    let traces = rototo::trace_variable_resolutions(Path::new(&root), &context)
        .await
        .map_err(js_err)?;
    to_json(&traces)
}

/// The lenient batch behind the lit-up graph: every variable resolves under
/// one shared state, and a variable that cannot resolve (a rule reading a
/// context key the caller did not supply) carries its error instead of
/// failing the whole batch.
#[napi(js_name = "traceResolutionOutcomes")]
pub async fn trace_resolution_outcomes(root: String, context: JsonValue) -> Result<JsonValue> {
    let outcomes = rototo::trace_variable_resolution_outcomes(Path::new(&root), &context)
        .await
        .map_err(js_err)?;
    to_json(&outcomes)
}

/// Behavior scheduled to change after `now`: rule and query expressions
/// comparing `env.now` against literal instants that have not passed yet.
#[napi(js_name = "upcomingChanges")]
pub async fn upcoming_changes(root: String, now: String) -> Result<JsonValue> {
    let changes = rototo::upcoming_changes(Path::new(&root), &now)
        .await
        .map_err(js_err)?;
    to_json(&changes)
}

/// Synthesized boundary contexts for the package's variables, from the
/// fixtures machinery: per behavior case, a context that exercises it and
/// the expected outcome. `variables` narrows the sweep; omitted means all.
#[napi(js_name = "resolveFixtures")]
pub async fn resolve_fixtures(root: String, variables: Option<Vec<String>>) -> Result<JsonValue> {
    let selection = FixtureGenerateSelection {
        variables: match variables {
            Some(ids) => FixtureTargetSelection::some(ids),
            None => FixtureTargetSelection::All,
        },
    };
    let invocations =
        rototo::fixtures::generate_resolve_invocations(&root, &SourceOptions::new(), selection)
            .await
            .map_err(js_err)?;
    let rendered = invocations
        .into_iter()
        .map(|invocation| {
            Ok(serde_json::json!({
                "target": { "kind": "variable", "id": invocation.target.id() },
                "caseId": invocation.case_id,
                "title": invocation.title,
                "because": invocation.because,
                "context": invocation.context,
                "expect": serde_json::to_value(&invocation.expect)
                    .map_err(|err| Error::from_reason(err.to_string()))?,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(JsonValue::Array(rendered))
}

/// One in-process LSP session over rototo's real language server: framed
/// JSON-RPC strings in via [`send`], framed messages out via [`receive`].
/// The TypeScript bridge owns id correlation and notification fan-out; this
/// object owns only the transport, so cancellation, debounced diagnostics,
/// and overlay handling behave exactly as they do over stdio.
#[napi(js_name = "LspSession")]
pub struct JsLspSession {
    // The server halves wait here until the first async call: the napi
    // constructor runs outside the tokio runtime, so spawning must be lazy.
    pending: std::sync::Mutex<
        Option<(
            tokio::io::ReadHalf<tokio::io::DuplexStream>,
            tokio::io::WriteHalf<tokio::io::DuplexStream>,
        )>,
    >,
    writer: tokio::sync::Mutex<tokio::io::WriteHalf<tokio::io::DuplexStream>>,
    reader: tokio::sync::Mutex<tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
}

#[napi]
impl JsLspSession {
    #[napi(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let (client_io, server_io) = tokio::io::duplex(1 << 20);
        let (server_read, server_write) = tokio::io::split(server_io);
        let (client_read, client_write) = tokio::io::split(client_io);
        Self {
            pending: std::sync::Mutex::new(Some((server_read, server_write))),
            writer: tokio::sync::Mutex::new(client_write),
            reader: tokio::sync::Mutex::new(tokio::io::BufReader::new(client_read)),
        }
    }

    fn ensure_started(&self) {
        let taken = self.pending.lock().expect("lsp session lock").take();
        if let Some((server_read, server_write)) = taken {
            tokio::spawn(async move {
                // The session ends when the client drops or sends exit;
                // either way the outcome is the client's to observe.
                let _ =
                    rototo::lsp::serve(tokio::io::BufReader::new(server_read), server_write).await;
            });
        }
    }

    /// Writes one JSON-RPC message (a serialized JSON string) to the server.
    #[napi]
    pub async fn send(&self, message: String) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        self.ensure_started();
        let mut writer = self.writer.lock().await;
        let framed = format!("Content-Length: {}\r\n\r\n{message}", message.len());
        writer
            .write_all(framed.as_bytes())
            .await
            .map_err(|err| Error::from_reason(format!("lsp write failed: {err}")))?;
        writer
            .flush()
            .await
            .map_err(|err| Error::from_reason(format!("lsp flush failed: {err}")))
    }

    /// Reads the next JSON-RPC message from the server, or `null` once the
    /// session has ended. Call from one reader loop; concurrent receives
    /// would interleave frames.
    #[napi]
    pub async fn receive(&self) -> Result<Option<String>> {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt};
        self.ensure_started();
        let mut reader = self.reader.lock().await;
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            let read = reader
                .read_line(&mut line)
                .await
                .map_err(|err| Error::from_reason(format!("lsp read failed: {err}")))?;
            if read == 0 {
                return Ok(None);
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                content_length = value.trim().parse().ok();
            }
        }
        let Some(length) = content_length else {
            return Err(Error::from_reason("lsp frame missing Content-Length"));
        };
        let mut body = vec![0; length];
        reader
            .read_exact(&mut body)
            .await
            .map_err(|err| Error::from_reason(format!("lsp read failed: {err}")))?;
        String::from_utf8(body)
            .map(Some)
            .map_err(|err| Error::from_reason(format!("lsp frame is not UTF-8: {err}")))
    }
}

/// Traced resolution of one variable under one context.
#[napi(js_name = "traceResolution")]
pub async fn trace_resolution(
    root: String,
    variable: String,
    context: JsonValue,
) -> Result<JsonValue> {
    let trace = rototo::trace_variable_resolution(Path::new(&root), &variable, &context)
        .await
        .map_err(js_err)?;
    to_json(&trace)
}

fn inspect_request(value: Option<JsonValue>) -> Result<PackageInspectRequest> {
    let Some(value) = value else {
        return Ok(PackageInspectRequest::default());
    };
    let JsonValue::Object(map) = value else {
        return Err(Error::from_reason("inspect request must be an object"));
    };
    let mut request = PackageInspectRequest::default();
    for (key, value) in map {
        match key.as_str() {
            "variables" => request.variables = selection(&key, value)?,
            "catalogs" => request.catalogs = selection(&key, value)?,
            "lintRules" => request.lint_rules = selection(&key, value)?,
            "lintAuthorities" => request.lint_authorities = selection(&key, value)?,
            "linters" => request.linters = selection(&key, value)?,
            "context" => request.context = Some(value),
            other => {
                return Err(Error::from_reason(format!(
                    "unknown inspect request field: {other}"
                )));
            }
        }
    }
    Ok(request)
}

fn selection(field: &str, value: JsonValue) -> Result<InspectSelection> {
    match value {
        JsonValue::Null => Ok(InspectSelection::None),
        JsonValue::String(text) if text == "all" => Ok(InspectSelection::All),
        JsonValue::Array(items) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    JsonValue::String(id) => ids.push(id),
                    _ => {
                        return Err(Error::from_reason(format!(
                            "inspect selection {field} must hold strings"
                        )));
                    }
                }
            }
            Ok(InspectSelection::Some(ids))
        }
        _ => Err(Error::from_reason(format!(
            "inspect selection {field} must be null, \"all\", or an array of ids"
        ))),
    }
}

fn edit_options(value: Option<JsonValue>) -> Result<EditOptions> {
    let mut options = EditOptions::default();
    let Some(value) = value else {
        return Ok(options);
    };
    let JsonValue::Object(map) = value else {
        return Err(Error::from_reason("edit options must be an object"));
    };
    for (key, value) in map {
        match key.as_str() {
            "inherited" => match value {
                JsonValue::Array(items) => {
                    for item in items {
                        match item {
                            JsonValue::String(address) => {
                                options.inherited.insert(address);
                            }
                            _ => {
                                return Err(Error::from_reason(
                                    "inherited must hold entity address strings",
                                ));
                            }
                        }
                    }
                }
                _ => {
                    return Err(Error::from_reason(
                        "inherited must be an array of entity addresses",
                    ));
                }
            },
            other => {
                return Err(Error::from_reason(format!("unknown edit option: {other}")));
            }
        }
    }
    Ok(options)
}

fn relative_path_string(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        return ".".to_owned();
    }
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<JsonValue> {
    serde_json::to_value(value).map_err(|err| Error::from_reason(err.to_string()))
}

fn path_string(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::from_reason(format!("path is not valid UTF-8: {}", path.display())))
}

fn js_err(err: rototo::RototoError) -> Error {
    Error::from_reason(err.to_string())
}
