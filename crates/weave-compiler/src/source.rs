//! Source files, byte spans and line/column positions.
//!
//! Spans are byte offsets into the original buffer (LANGUAGE.md §2). Positions
//! shown to users are 1-based lines and 1-based columns counted in Unicode
//! scalar values. LF, CRLF, CR and FF each count as one line break, and a
//! leading byte-order mark does not count as a column.

use std::fmt;

use crate::diagnostic::{Code, Diagnostic};

const BOM: char = '\u{feff}';

/// Identifies a file registered in a [`SourceManager`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(u32);

impl FileId {
    /// The index of this file in registration order.
    pub fn index(self) -> u32 {
        self.0
    }
}

/// A half-open byte range `start..end` in one source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// Creates a span; `start` and `end` are swapped if given in reverse.
    pub fn new(file: FileId, start: u32, end: u32) -> Self {
        Span {
            file,
            start: start.min(end),
            end: start.max(end),
        }
    }

    /// An empty span at `offset`.
    pub fn empty(file: FileId, offset: u32) -> Self {
        Span::new(file, offset, offset)
    }

    pub fn len(self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// A 1-based line and 1-based column counted in Unicode scalar values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for LineCol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// An immutable UTF-8 source buffer with a precomputed line table.
#[derive(Debug)]
pub struct SourceFile {
    id: FileId,
    name: String,
    text: String,
    /// Byte offset where line 1 content starts (after a byte-order mark).
    content_start: u32,
    /// Byte offset of the start of every line; line 1 starts at 0.
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn id(&self) -> FileId {
        self.id
    }

    /// The display name used in diagnostics, for example `main.weave`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The full text, including any leading byte-order mark.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Byte offset where content starts: 3 when the file begins with a
    /// byte-order mark, otherwise 0.
    pub fn content_start(&self) -> u32 {
        self.content_start
    }

    pub fn len(&self) -> u32 {
        // `SourceManager::add` rejects buffers longer than `u32::MAX`.
        u32::try_from(self.text.len()).unwrap_or(u32::MAX)
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn line_count(&self) -> u32 {
        u32::try_from(self.line_starts.len()).unwrap_or(u32::MAX)
    }

    /// The text covered by `span`, or `None` if it belongs to another file,
    /// is out of range or does not fall on character boundaries.
    pub fn slice(&self, span: Span) -> Option<&str> {
        if span.file != self.id {
            return None;
        }
        self.text.get(span.start as usize..span.end as usize)
    }

    /// The line and column of a byte offset. Returns `None` if the offset is
    /// past the end of the text or not on a character boundary. An offset
    /// inside a CRLF pair or inside the byte-order mark maps to the position
    /// of the CR or of the first column.
    pub fn line_col(&self, offset: u32) -> Option<LineCol> {
        let (line_index, line_start) = self.locate(offset)?;
        let prefix = self.text.get(line_start as usize..offset as usize)?;
        let column = u32::try_from(prefix.chars().count()).ok()?.checked_add(1)?;
        Some(LineCol {
            line: line_index.checked_add(1)?,
            column,
        })
    }

    /// The 0-based column of a byte offset in UTF-16 code units, as required
    /// by source maps (ECMA-426).
    pub fn utf16_column(&self, offset: u32) -> Option<u32> {
        let (_, line_start) = self.locate(offset)?;
        let prefix = self.text.get(line_start as usize..offset as usize)?;
        u32::try_from(prefix.encode_utf16().count()).ok()
    }

    /// Returns the 0-based line index and the effective start of that line.
    fn locate(&self, offset: u32) -> Option<(u32, u32)> {
        if offset as usize > self.text.len() || !self.text.is_char_boundary(offset as usize) {
            return None;
        }
        let line_index = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .checked_sub(1)?;
        let mut start = *self.line_starts.get(line_index)?;
        if line_index == 0 {
            // Offsets inside the byte-order mark map to column 1.
            start = self.content_start.min(offset);
        }
        Some((u32::try_from(line_index).ok()?, start))
    }
}

/// Owns every source buffer of one compilation.
#[derive(Debug, Default)]
pub struct SourceManager {
    files: Vec<SourceFile>,
    total_bytes: u64,
}

impl SourceManager {
    pub fn new() -> Self {
        SourceManager::default()
    }

    /// Decodes and registers a source buffer.
    ///
    /// Invalid UTF-8 is E0001 and U+0000 is E0006 (LANGUAGE.md §2). On
    /// E0001 the file is still registered, holding only the valid prefix, so
    /// that the diagnostic can be rendered with a line and column. Buffers
    /// longer than `u32::MAX` bytes are E8001 and are not registered.
    pub fn add(&mut self, name: impl Into<String>, bytes: Vec<u8>) -> Result<FileId, Diagnostic> {
        let name = name.into();
        let len = u32::try_from(bytes.len()).map_err(|_| {
            Diagnostic::error(
                Code::E8001,
                format!(
                    "`{name}` is {} bytes, more than spans can address",
                    bytes.len()
                ),
            )
        })?;
        let id = FileId(u32::try_from(self.files.len()).map_err(|_| {
            Diagnostic::error(Code::E8008, "too many source files registered".to_string())
        })?);

        let (text, invalid_at) = match String::from_utf8(bytes) {
            Ok(text) => (text, None),
            Err(err) => {
                let valid = err.utf8_error().valid_up_to();
                let mut bytes = err.into_bytes();
                bytes.truncate(valid);
                // The prefix up to `valid_up_to` is valid UTF-8 by definition.
                let text = String::from_utf8(bytes).unwrap_or_default();
                (text, Some(valid))
            }
        };

        self.total_bytes = self.total_bytes.saturating_add(u64::from(len));
        let file = SourceFile::build(id, name, text);
        let nul_at = file.text.find('\0');
        let name = file.name.clone();
        self.files.push(file);

        if let Some(offset) = invalid_at {
            let offset = u32::try_from(offset).unwrap_or(len);
            return Err(Diagnostic::error(
                Code::E0001,
                format!("`{name}` is not valid UTF-8 (first invalid byte at offset {offset})"),
            )
            .with_primary(Span::new(id, offset, offset)));
        }
        if let Some(offset) = nul_at {
            let offset = u32::try_from(offset).unwrap_or(len);
            return Err(
                Diagnostic::error(Code::E0006, "U+0000 is not allowed in source text")
                    .with_primary(Span::new(id, offset, offset.saturating_add(1))),
            );
        }
        Ok(id)
    }

    pub fn get(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    pub fn files(&self) -> impl Iterator<Item = &SourceFile> {
        self.files.iter()
    }

    /// The sum of the original byte lengths of all registered buffers.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// The file name and position of the start of `span`.
    pub fn location(&self, span: Span) -> Option<(&str, LineCol)> {
        let file = self.get(span.file)?;
        Some((file.name(), file.line_col(span.start)?))
    }
}

impl SourceFile {
    fn build(id: FileId, name: String, text: String) -> Self {
        let content_start = if text.starts_with(BOM) {
            BOM.len_utf8() as u32
        } else {
            0
        };
        let mut line_starts = vec![0];
        let bytes = text.as_bytes();
        let mut index = content_start as usize;
        while let Some(&byte) = bytes.get(index) {
            index += 1;
            match byte {
                b'\r' => {
                    if bytes.get(index) == Some(&b'\n') {
                        index += 1;
                    }
                    line_starts.push(index as u32);
                }
                b'\n' | b'\x0c' => line_starts.push(index as u32),
                _ => {}
            }
        }
        SourceFile {
            id,
            name,
            text,
            content_start,
            line_starts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> SourceFile {
        let mut sources = SourceManager::new();
        let id = sources.add("main.weave", text.as_bytes().to_vec()).unwrap();
        sources.files.swap_remove(id.0 as usize)
    }

    fn pos(file: &SourceFile, offset: u32) -> (u32, u32) {
        let lc = file.line_col(offset).unwrap();
        (lc.line, lc.column)
    }

    #[test]
    fn every_line_break_form_counts_once() {
        let f = file("a\nb\r\nc\rd\x0ce");
        assert_eq!(f.line_count(), 5);
        assert_eq!(pos(&f, 0), (1, 1));
        assert_eq!(pos(&f, 2), (2, 1));
        assert_eq!(pos(&f, 5), (3, 1));
        assert_eq!(pos(&f, 7), (4, 1));
        assert_eq!(pos(&f, 9), (5, 1));
    }

    #[test]
    fn crlf_offsets_stay_on_the_line_they_end() {
        let f = file("ab\r\ncd");
        assert_eq!(pos(&f, 2), (1, 3)); // the CR
        assert_eq!(pos(&f, 3), (1, 4)); // the LF inside CRLF
        assert_eq!(pos(&f, 4), (2, 1));
    }

    #[test]
    fn columns_count_unicode_scalar_values() {
        // "é" is 2 bytes, "→" 3 bytes, "😀" 4 bytes.
        let f = file("é→😀x");
        assert_eq!(pos(&f, 2), (1, 2));
        assert_eq!(pos(&f, 5), (1, 3));
        assert_eq!(pos(&f, 9), (1, 4));
        assert_eq!(pos(&f, 10), (1, 5));
    }

    #[test]
    fn utf16_columns_count_surrogate_pairs() {
        let f = file("é→😀x\n😀");
        assert_eq!(f.utf16_column(9), Some(4));
        assert_eq!(f.utf16_column(11), Some(0));
        assert_eq!(f.utf16_column(15), Some(2));
    }

    #[test]
    fn offsets_inside_a_character_are_rejected() {
        let f = file("é😀");
        assert_eq!(f.line_col(1), None);
        assert_eq!(f.line_col(3), None);
        assert_eq!(f.utf16_column(4), None);
    }

    #[test]
    fn end_of_file_is_a_position_but_past_it_is_not() {
        let f = file("ab\n");
        assert_eq!(pos(&f, 3), (2, 1));
        assert_eq!(f.line_col(4), None);
        let empty = file("");
        assert_eq!(pos(&empty, 0), (1, 1));
    }

    #[test]
    fn leading_bom_is_not_a_column() {
        let f = file("\u{feff}ab\nc");
        assert_eq!(f.content_start(), 3);
        assert_eq!(pos(&f, 3), (1, 1));
        assert_eq!(pos(&f, 4), (1, 2));
        assert_eq!(pos(&f, 0), (1, 1));
        assert_eq!(pos(&f, 6), (2, 1));
        assert_eq!(f.utf16_column(4), Some(1));
    }

    #[test]
    fn bom_after_the_start_is_ordinary_text() {
        let f = file("a\u{feff}b");
        assert_eq!(f.content_start(), 0);
        assert_eq!(pos(&f, 4), (1, 3));
    }

    #[test]
    fn invalid_utf8_reports_e0001_at_the_first_bad_byte() {
        let mut sources = SourceManager::new();
        let err = sources
            .add("bad.weave", b"a\n\xc3\xa9\xff rest".to_vec())
            .unwrap_err();
        assert_eq!(err.code(), Code::E0001);
        let span = err.primary().unwrap();
        assert_eq!((span.start, span.end), (4, 4));
        let (name, lc) = sources.location(span).unwrap();
        assert_eq!(name, "bad.weave");
        assert_eq!((lc.line, lc.column), (2, 2));
        // Only the valid prefix is retained.
        assert_eq!(sources.get(span.file).unwrap().text(), "a\né");
        assert_eq!(sources.total_bytes(), 10);
    }

    #[test]
    fn truncated_multibyte_sequence_is_e0001() {
        let mut sources = SourceManager::new();
        let err = sources.add("t.weave", b"ab\xe2\x82".to_vec()).unwrap_err();
        assert_eq!(err.code(), Code::E0001);
        assert_eq!(err.primary().unwrap().start, 2);
    }

    #[test]
    fn nul_reports_e0006() {
        let mut sources = SourceManager::new();
        let err = sources.add("n.weave", b"a\nb\0c".to_vec()).unwrap_err();
        assert_eq!(err.code(), Code::E0006);
        let span = err.primary().unwrap();
        assert_eq!((span.start, span.end), (3, 4));
        assert_eq!(
            sources.location(span).unwrap().1,
            LineCol { line: 2, column: 2 }
        );
    }

    #[test]
    fn slices_respect_file_and_boundaries() {
        let mut sources = SourceManager::new();
        let a = sources.add("a.weave", "é!".as_bytes().to_vec()).unwrap();
        let b = sources.add("b.weave", b"xyz".to_vec()).unwrap();
        let fa = sources.get(a).unwrap();
        assert_eq!(fa.slice(Span::new(a, 0, 2)), Some("é"));
        assert_eq!(fa.slice(Span::new(a, 1, 2)), None);
        assert_eq!(fa.slice(Span::new(b, 0, 1)), None);
        assert_eq!(Span::new(a, 5, 2), Span::new(a, 2, 5));
    }
}
