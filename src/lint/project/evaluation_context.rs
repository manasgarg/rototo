use std::sync::Arc;

use super::super::index::{EvaluationContextNode, EvaluationContextSampleNode};
use super::super::source::SourceDocument;
use super::super::syntax::SyntaxIndex;

pub(crate) fn project_evaluation_context(
    document: &SourceDocument,
    syntax: &SyntaxIndex,
    id: &str,
) -> EvaluationContextNode {
    let json = syntax.json.get(&document.id).cloned();
    let (validator, invalid_message) = match &json {
        Some(json) => match jsonschema::options()
            .should_validate_formats(true)
            .build(json)
        {
            Ok(validator) => (Some(Arc::new(validator)), None),
            Err(err) => (None, Some(err.to_string())),
        },
        None => (None, None),
    };

    EvaluationContextNode {
        id: id.to_owned(),
        path: document.path.clone(),
        location: document.document_location(),
        json,
        validator,
        invalid_message,
    }
}

pub(crate) fn project_evaluation_context_sample(
    document: &SourceDocument,
    syntax: &SyntaxIndex,
    evaluation_context_id: &str,
    sample_id: &str,
) -> EvaluationContextSampleNode {
    EvaluationContextSampleNode {
        evaluation_context_id: evaluation_context_id.to_owned(),
        key: sample_id.to_owned(),
        path: document.path.clone(),
        location: document.document_location(),
        value: syntax.json.get(&document.id).cloned(),
    }
}
