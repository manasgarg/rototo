#![allow(dead_code, unused_imports)]

pub(crate) use crate::diagnostics::{SourceSpan, TextRange};

#[derive(Clone, Debug)]
pub(crate) struct Spanned<T> {
    pub(crate) value: T,
    pub(crate) span: SourceSpan,
}
