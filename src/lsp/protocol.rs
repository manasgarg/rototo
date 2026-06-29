use serde::Serialize;
use serde_json::{Value as JsonValue, json};

const TEXT_DOCUMENT_SYNC_KIND_INCREMENTAL: u8 = 2;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PublishDiagnosticsParams {
    pub(super) uri: String,
    pub(super) version: Option<i32>,
    pub(super) diagnostics: Vec<LspDiagnostic>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LspDiagnostic {
    pub(super) range: LspRange,
    pub(super) severity: u8,
    pub(super) source: &'static str,
    pub(super) code: String,
    pub(super) message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) related_information: Vec<LspDiagnosticRelatedInformation>,
    pub(super) data: LspDiagnosticData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LspDiagnosticRelatedInformation {
    pub(super) location: LspLocation,
    pub(super) message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LspDiagnosticData {
    pub(super) rule: String,
    pub(super) stage: String,
    pub(super) help: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LspDocumentSymbol {
    pub(super) name: String,
    pub(super) kind: u8,
    pub(super) range: LspRange,
    pub(super) selection_range: LspRange,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) children: Vec<LspDocumentSymbol>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LspCompletionItem {
    pub(super) label: String,
    pub(super) kind: u8,
    pub(super) detail: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) insert_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) text_edit: Option<LspTextEdit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) filter_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LspTextEdit {
    pub(super) range: LspRange,
    pub(super) new_text: String,
}

#[derive(Debug, Serialize)]
pub(super) struct LspHover {
    pub(super) contents: LspMarkupContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) range: Option<LspRange>,
}

#[derive(Debug, Serialize)]
pub(super) struct LspLocation {
    pub(super) uri: String,
    pub(super) range: LspRange,
}

#[derive(Debug, Serialize)]
pub(super) struct LspMarkupContent {
    pub(super) kind: &'static str,
    pub(super) value: String,
}

#[derive(Debug, Serialize, Clone, Copy)]
pub(super) struct LspRange {
    pub(super) start: LspPosition,
    pub(super) end: LspPosition,
}

#[derive(Debug, Serialize, Clone, Copy)]
pub(super) struct LspPosition {
    pub(super) line: usize,
    pub(super) character: usize,
}

pub(super) fn initialize_result() -> JsonValue {
    json!({
        "capabilities": {
            "positionEncoding": "utf-16",
            "textDocumentSync": {
                "openClose": true,
                "change": TEXT_DOCUMENT_SYNC_KIND_INCREMENTAL,
                "save": {
                    "includeText": false
                }
            },
            "documentSymbolProvider": true,
            "completionProvider": {
                "resolveProvider": false,
                "triggerCharacters": [".", "\"", "&", "|"]
            },
            "hoverProvider": true,
            "definitionProvider": true,
            "referencesProvider": true
        },
        "serverInfo": {
            "name": "rototo",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}
