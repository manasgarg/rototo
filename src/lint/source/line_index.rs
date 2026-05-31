use std::ops::Range;

use crate::diagnostics::{SourcePosition, SourceRange};

#[derive(Clone)]
pub(crate) struct LineIndex {
    text: String,
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
            text: text.to_owned(),
            line_starts,
            text_len: text.len(),
        }
    }

    pub(super) fn range(&self, range: Range<usize>) -> SourceRange {
        let start = self.floor_char_boundary(range.start.min(self.text_len));
        let end = self
            .ceil_char_boundary(range.end.min(self.text_len))
            .max(start);
        SourceRange {
            start: self.position(start),
            end: self.position(end),
        }
    }

    fn position(&self, offset: usize) -> SourcePosition {
        let offset = self.floor_char_boundary(offset.min(self.text_len));
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next_line) => next_line.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line).copied().unwrap_or(0);
        let character = self
            .text
            .get(line_start..offset)
            .map(|line_prefix| line_prefix.encode_utf16().count())
            .unwrap_or_default();
        SourcePosition { line, character }
    }

    pub(crate) fn offset_for_line_character(&self, line: usize, character: usize) -> usize {
        let line_start = self.line_starts.get(line).copied().unwrap_or(self.text_len);
        let line_end = self
            .line_starts
            .get(line.saturating_add(1))
            .copied()
            .unwrap_or(self.text_len);
        let Some(line_text) = self.text.get(line_start..line_end) else {
            return self.text_len;
        };
        let mut utf16_units = 0_usize;
        for (relative_offset, ch) in line_text.char_indices() {
            let next_units = utf16_units + ch.len_utf16();
            if next_units > character {
                return line_start + relative_offset;
            }
            if next_units == character {
                return line_start + relative_offset + ch.len_utf8();
            }
            utf16_units = next_units;
        }
        line_end
    }

    fn floor_char_boundary(&self, mut offset: usize) -> usize {
        while offset > 0 && !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    fn ceil_char_boundary(&self, mut offset: usize) -> usize {
        while offset < self.text_len && !self.text.is_char_boundary(offset) {
            offset += 1;
        }
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_positions_clamp_to_zero() {
        let index = LineIndex::new("");
        assert_eq!(
            index.range(0..10),
            SourceRange {
                start: SourcePosition {
                    line: 0,
                    character: 0
                },
                end: SourcePosition {
                    line: 0,
                    character: 0
                },
            }
        );
        assert_eq!(index.offset_for_line_character(20, 20), 0);
    }

    #[test]
    fn byte_ranges_render_as_utf16_positions() {
        let index = LineIndex::new("aé\nb😀c");

        assert_eq!(
            index.range(1..3),
            SourceRange {
                start: SourcePosition {
                    line: 0,
                    character: 1
                },
                end: SourcePosition {
                    line: 0,
                    character: 2
                },
            }
        );
        assert_eq!(
            index.range(5..9),
            SourceRange {
                start: SourcePosition {
                    line: 1,
                    character: 1
                },
                end: SourcePosition {
                    line: 1,
                    character: 3
                },
            }
        );
    }

    #[test]
    fn invalid_byte_range_boundaries_snap_to_character_boundaries() {
        let index = LineIndex::new("é😀");

        assert_eq!(
            index.range(1..5),
            SourceRange {
                start: SourcePosition {
                    line: 0,
                    character: 0
                },
                end: SourcePosition {
                    line: 0,
                    character: 3
                },
            }
        );
    }

    #[test]
    fn utf16_positions_convert_back_to_byte_offsets() {
        let index = LineIndex::new("aé\nb😀c");

        assert_eq!(index.offset_for_line_character(0, 0), 0);
        assert_eq!(index.offset_for_line_character(0, 1), 1);
        assert_eq!(index.offset_for_line_character(0, 2), 3);
        assert_eq!(index.offset_for_line_character(1, 0), 4);
        assert_eq!(index.offset_for_line_character(1, 1), 5);
        assert_eq!(index.offset_for_line_character(1, 2), 5);
        assert_eq!(index.offset_for_line_character(1, 3), 9);
        assert_eq!(index.offset_for_line_character(1, 99), 10);
    }
}
