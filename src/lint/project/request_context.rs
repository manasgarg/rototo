use std::sync::Arc;

use super::super::index::{RequestContextEntryNode, RequestContextNode};
use super::super::source::SourceDocument;
use super::super::syntax::SyntaxIndex;

pub(crate) fn project_request_context(
    document: &SourceDocument,
    syntax: &SyntaxIndex,
    id: &str,
) -> RequestContextNode {
    let json = syntax.json.get(&document.id).cloned();
    let (validator, invalid_message) = match &json {
        Some(json) => match jsonschema::validator_for(json) {
            Ok(validator) => (Some(Arc::new(validator)), None),
            Err(err) => (None, Some(err.to_string())),
        },
        None => (None, None),
    };

    RequestContextNode {
        id: id.to_owned(),
        path: document.path.clone(),
        location: document.document_location(),
        json,
        validator,
        invalid_message,
    }
}

pub(crate) fn project_request_context_entry(
    document: &SourceDocument,
    syntax: &SyntaxIndex,
    request_context_id: &str,
    entry_id: &str,
) -> RequestContextEntryNode {
    RequestContextEntryNode {
        request_context_id: request_context_id.to_owned(),
        key: entry_id.to_owned(),
        path: document.path.clone(),
        location: document.document_location(),
        value: syntax.json.get(&document.id).cloned(),
    }
}
