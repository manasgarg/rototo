use std::ops::Range;

use crate::diagnostics::{SourcePosition, SourceRange};

#[derive(Clone)]
pub(crate) struct LineIndex {
    line_starts: Vec<usize>,
    text_len: usize,
}

impl LineIndex {
    pub(super) fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self {
            line_starts,
            text_len: text.len(),
        }
    }

    pub(super) fn range(&self, range: Range<usize>) -> SourceRange {
        let start = range.start.min(self.text_len);
        let end = range.end.min(self.text_len).max(start);
        SourceRange {
            start: self.position(start),
            end: self.position(end),
        }
    }

    fn position(&self, offset: usize) -> SourcePosition {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next_line) => next_line.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line).copied().unwrap_or(0);
        SourcePosition {
            line,
            character: offset.saturating_sub(line_start),
        }
    }

    pub(crate) fn offset_for_line_character(&self, line: usize, character: usize) -> usize {
        let line_start = self.line_starts.get(line).copied().unwrap_or(self.text_len);
        line_start.saturating_add(character).min(self.text_len)
    }
}
