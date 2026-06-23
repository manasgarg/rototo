use toml_span::value::Table;

use super::super::index::*;
use super::super::source::SourceDocument;
use super::super::syntax::{ParsedToml, item_location};
use super::fields::{expression_field, integer_field, optional_string_field};

pub(crate) fn project_qualifier(
    document: &SourceDocument,
    toml: &ParsedToml,
    id: &str,
) -> QualifierNode {
    let root = toml.root_table();
    let location = document.document_location();
    let schema_version = root
        .map(|root| integer_field(document, root, "schema_version", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });
    let description = root.and_then(|root| optional_string_field(document, root, "description"));
    let when = root
        .map(|root| expression_field(document, root, "when", location.clone()))
        .unwrap_or_else(|| ProjectField::Missing {
            location: location.clone(),
        });
    let predicates = project_predicates(document, root);

    QualifierNode {
        doc: document.id,
        id: id.to_owned(),
        location,
        schema_version,
        description,
        when,
        predicates,
    }
}

fn project_predicates(document: &SourceDocument, root: Option<&Table<'_>>) -> PredicateCollection {
    let Some(root) = root else {
        return PredicateCollection::Absent;
    };
    let Some(item) = root.get("predicate") else {
        return PredicateCollection::Absent;
    };
    PredicateCollection::Invalid {
        location: item_location(document, item),
    }
}
