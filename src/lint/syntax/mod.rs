mod diagnostics;
mod json;
mod location;
mod toml;

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::diagnostics::{DocId, LintDiagnostic};

use super::source::{DocumentKind, SourceStore};

pub(super) use location::{item_location, table_location, value_location};

#[derive(Default)]
pub(super) struct SyntaxIndex {
    pub(super) toml: BTreeMap<DocId, ParsedToml>,
    pub(super) json: BTreeMap<DocId, ::serde_json::Value>,
}

pub(super) struct ParsedToml {
    pub(super) spanned_root: SpannedTomlValue,
}

pub(super) struct SpannedTomlValue {
    inner: ::toml_span::Value<'static>,
}

impl ParsedToml {
    pub(super) fn new(value: ::toml_span::Value<'_>) -> Self {
        Self {
            spanned_root: SpannedTomlValue {
                inner: own_toml_span_value(value),
            },
        }
    }

    pub(super) fn root(&self) -> &::toml_span::Value<'static> {
        &self.spanned_root.inner
    }

    pub(super) fn root_table(&self) -> Option<&::toml_span::value::Table<'static>> {
        self.root().as_table()
    }

    pub(super) fn to_plain_toml(&self) -> ::toml::Value {
        plain_toml_from_span_value(self.root())
    }
}

pub(super) fn parse_sources(
    source: &SourceStore,
    diagnostics: &mut Vec<LintDiagnostic>,
) -> SyntaxIndex {
    let mut syntax = SyntaxIndex::default();
    for document in source.documents.values() {
        if let Some(read_error) = &document.read_error {
            if !matches!(&document.kind, DocumentKind::CustomLint) {
                diagnostics.push(diagnostics::read_error_diagnostic(document, read_error));
            }
            continue;
        }

        match &document.kind {
            DocumentKind::Manifest
            | DocumentKind::Variable { .. }
            | DocumentKind::EnumDeclaration { .. }
            | DocumentKind::EnumMembers { .. }
            | DocumentKind::CatalogEntry { .. } => {
                toml::parse_toml_document(document, &mut syntax, diagnostics);
            }
            DocumentKind::Catalog { .. }
            | DocumentKind::EvaluationContext { .. }
            | DocumentKind::EvaluationContextSample { .. } => {
                json::parse_json_document(document, &mut syntax, diagnostics);
            }
            DocumentKind::CustomLint => {}
        }
    }
    syntax
}

fn own_toml_span_value(value: ::toml_span::Value<'_>) -> ::toml_span::Value<'static> {
    let mut value = value;
    let span = value.span;
    let owned = match value.take() {
        ::toml_span::value::ValueInner::String(value) => {
            ::toml_span::value::ValueInner::String(Cow::Owned(value.into_owned()))
        }
        ::toml_span::value::ValueInner::Integer(value) => {
            ::toml_span::value::ValueInner::Integer(value)
        }
        ::toml_span::value::ValueInner::Float(value) => {
            ::toml_span::value::ValueInner::Float(value)
        }
        ::toml_span::value::ValueInner::Boolean(value) => {
            ::toml_span::value::ValueInner::Boolean(value)
        }
        ::toml_span::value::ValueInner::Array(values) => ::toml_span::value::ValueInner::Array(
            values.into_iter().map(own_toml_span_value).collect(),
        ),
        ::toml_span::value::ValueInner::Table(table) => ::toml_span::value::ValueInner::Table(
            table
                .into_iter()
                .map(|(key, value)| {
                    (
                        ::toml_span::value::Key {
                            name: Cow::Owned(key.name.into_owned()),
                            span: key.span,
                        },
                        own_toml_span_value(value),
                    )
                })
                .collect(),
        ),
    };

    ::toml_span::Value::with_span(owned, span)
}

pub(super) fn plain_toml_from_span_value(value: &::toml_span::Value<'_>) -> ::toml::Value {
    match value.as_ref() {
        ::toml_span::value::ValueInner::String(value) => ::toml::Value::String(value.to_string()),
        ::toml_span::value::ValueInner::Integer(value) => ::toml::Value::Integer(*value),
        ::toml_span::value::ValueInner::Float(value) => ::toml::Value::Float(*value),
        ::toml_span::value::ValueInner::Boolean(value) => ::toml::Value::Boolean(*value),
        ::toml_span::value::ValueInner::Array(values) => ::toml::Value::Array(
            values
                .iter()
                .map(plain_toml_from_span_value)
                .collect::<Vec<_>>(),
        ),
        ::toml_span::value::ValueInner::Table(table) => {
            let mut plain = ::toml::map::Map::new();
            for (key, value) in table {
                plain.insert(key.name.to_string(), plain_toml_from_span_value(value));
            }
            ::toml::Value::Table(plain)
        }
    }
}
