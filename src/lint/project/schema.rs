use std::sync::Arc;

use super::super::index::SchemaNode;
use super::super::source::SourceDocument;
use super::super::syntax::SyntaxIndex;

pub(crate) fn project_schema(document: &SourceDocument, syntax: &SyntaxIndex) -> SchemaNode {
    let json = syntax.json.get(&document.id).cloned();
    let (validator, invalid_message) = match &json {
        Some(json) => match jsonschema::validator_for(json) {
            Ok(validator) => (Some(Arc::new(validator)), None),
            Err(err) => (None, Some(err.to_string())),
        },
        None => (None, None),
    };

    SchemaNode {
        doc: document.id,
        path: document.path.clone(),
        location: document.document_location(),
        json,
        validator,
        invalid_message,
    }
}
