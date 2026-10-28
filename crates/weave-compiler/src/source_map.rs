//! Source maps (ECMA-426, 1st edition, December 2024).
//!
//! Provenance policy (docs/SOURCE-MAPS.md): every emitted rule, declaration,
//! at-rule, keyframe block and verbatim item gets one mapping, from the first
//! byte of its text in the output (after indentation) to its *definition
//! location*: the start of the item that produced it, in the module that
//! contains it. For an item expanded by recipe applications this is the
//! position inside the recipe body or content block; the chain of `apply`
//! sites that expanded it is kept in [`Mapping::expansion`] (innermost
//! first) but is not part of the JSON, which has no standard place for it.
//! A value computed by a function call maps to the declaration that called
//! it. Lowered nested rules and rule segments map to the `rule` item that
//! wrote their selector.
//!
//! Lines are 0-based and columns are 0-based UTF-16 code units, both in the
//! generated CSS and in the original sources (ECMA-426 §4.1).

use std::fmt::Write as _;

use crate::emit::RawMapping;
use crate::source::{SourceManager, Span};

/// A 0-based line and 0-based column in UTF-16 code units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

/// A position in one source, identified by its index in
/// [`SourceMap::sources`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OriginalPosition {
    pub source: u32,
    pub position: Position,
}

/// One mapping from a generated position to its definition location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub generated: Position,
    pub original: OriginalPosition,
    /// The `apply` keywords of the recipe applications that expanded this
    /// item, innermost first; empty outside any expansion.
    pub expansion: Vec<OriginalPosition>,
}

/// A source map for one compiled stylesheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMap {
    /// Module names (root-relative paths), one per source file of the build
    /// in discovery order; the entry module first.
    sources: Vec<String>,
    /// Source text of each source, without a leading byte-order mark.
    contents: Vec<String>,
    /// Mappings in generated order.
    mappings: Vec<Mapping>,
}

/// How [`SourceMap::to_json`] writes the map.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonOptions<'a> {
    /// The `file` field: the generated file's name, usually relative to the
    /// map. Omitted when `None`.
    pub file: Option<&'a str>,
    /// Replaces the module names in `sources`, for example with paths
    /// relative to the map file. Must have one entry per source; otherwise
    /// the module names are written.
    pub sources: Option<&'a [String]>,
    /// Embeds `sourcesContent`. Off by default: maps then do not publish the
    /// source text.
    pub sources_content: bool,
}

impl SourceMap {
    /// Resolves raw emitter mappings against the build's sources.
    pub(crate) fn build(raw: Vec<RawMapping>, sources: &SourceManager) -> SourceMap {
        let mut names = Vec::new();
        let mut contents = Vec::new();
        for file in sources.files() {
            names.push(file.name().to_string());
            let start = file.content_start() as usize;
            contents.push(file.text().get(start..).unwrap_or_default().to_string());
        }
        let mappings = raw
            .into_iter()
            .filter_map(|raw| {
                let original = original_of(sources, raw.origin.span)?;
                let mut expansion = Vec::new();
                let mut site = raw.origin.calls.as_deref();
                while let Some(call) = site {
                    if let Some(position) = original_of(sources, call.span) {
                        expansion.push(position);
                    }
                    site = call.parent.as_deref();
                }
                Some(Mapping {
                    generated: Position {
                        line: raw.line,
                        column: raw.column,
                    },
                    original,
                    expansion,
                })
            })
            .collect();
        SourceMap {
            sources: names,
            contents,
            mappings,
        }
    }

    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    pub fn contents(&self) -> &[String] {
        &self.contents
    }

    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }

    /// The `mappings` field: Base64 VLQ segments of four fields each.
    pub fn encoded_mappings(&self) -> String {
        let mut out = String::new();
        let mut line = 0;
        let mut first_in_line = true;
        let mut column = 0i64;
        let (mut source, mut orig_line, mut orig_column) = (0i64, 0i64, 0i64);
        for mapping in &self.mappings {
            while line < mapping.generated.line {
                out.push(';');
                line += 1;
                column = 0;
                first_in_line = true;
            }
            if !first_in_line {
                out.push(',');
            }
            first_in_line = false;
            let generated = i64::from(mapping.generated.column);
            let original = mapping.original;
            let fields = [
                (generated, &mut column),
                (i64::from(original.source), &mut source),
                (i64::from(original.position.line), &mut orig_line),
                (i64::from(original.position.column), &mut orig_column),
            ];
            for (value, previous) in fields {
                encode_vlq(value - *previous, &mut out);
                *previous = value;
            }
        }
        out
    }

    /// Serializes the map as ECMA-426 JSON on one line, with the fields in a
    /// fixed order: `version`, `file`, `sources`, `sourcesContent`, `names`,
    /// `mappings`.
    pub fn to_json(&self, options: &JsonOptions<'_>) -> String {
        let sources = match options.sources {
            Some(paths) if paths.len() == self.sources.len() => paths,
            _ => &self.sources,
        };
        let mut out = String::from("{\"version\":3");
        if let Some(file) = options.file {
            out.push_str(",\"file\":");
            json_string(file, &mut out);
        }
        out.push_str(",\"sources\":");
        json_array(sources, &mut out);
        if options.sources_content {
            out.push_str(",\"sourcesContent\":");
            json_array(&self.contents, &mut out);
        }
        out.push_str(",\"names\":[],\"mappings\":\"");
        out.push_str(&self.encoded_mappings());
        out.push_str("\"}");
        out
    }
}

fn original_of(sources: &SourceManager, span: Span) -> Option<OriginalPosition> {
    let file = sources.get(span.file)?;
    let line = file.line_col(span.start)?.line.checked_sub(1)?;
    let column = file.utf16_column(span.start)?;
    Some(OriginalPosition {
        source: span.file.index(),
        position: Position { line, column },
    })
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Appends `value` as a Base64 VLQ (sign in the lowest bit, 5-bit groups,
/// least significant first, continuation bit 0x20).
pub fn encode_vlq(value: i64, out: &mut String) {
    let mut rest: u64 = if value < 0 {
        (value.unsigned_abs() << 1) | 1
    } else {
        value.unsigned_abs() << 1
    };
    loop {
        let mut digit = (rest & 0x1f) as usize;
        rest >>= 5;
        if rest != 0 {
            digit |= 0x20;
        }
        if let Some(&c) = BASE64.get(digit) {
            out.push(char::from(c));
        }
        if rest == 0 {
            break;
        }
    }
}

/// Appends `text` as a JSON string. Control characters, `"` and `\` are
/// escaped; U+2028 and U+2029 are escaped too so the result is also valid
/// JavaScript.
pub fn json_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 || matches!(c, '\u{2028}' | '\u{2029}') => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_array(items: &[String], out: &mut String) {
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_string(item, out);
    }
    out.push(']');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vlq(value: i64) -> String {
        let mut out = String::new();
        encode_vlq(value, &mut out);
        out
    }

    #[test]
    fn vlq_matches_known_encodings() {
        // Expected strings computed with a separate implementation of the
        // ECMA-426 Base64 VLQ rules (sign bit lowest, 5-bit groups).
        assert_eq!(vlq(0), "A");
        assert_eq!(vlq(1), "C");
        assert_eq!(vlq(-1), "D");
        assert_eq!(vlq(15), "e");
        assert_eq!(vlq(16), "gB");
        assert_eq!(vlq(-16), "hB");
        assert_eq!(vlq(123), "2H");
        assert_eq!(vlq(1_000_000), "gkh9B");
        assert_eq!(vlq(i64::from(u32::MAX)), "+/////H");
    }

    #[test]
    fn json_strings_are_escaped() {
        let mut out = String::new();
        json_string("a\"b\\c\nd\u{1}\u{2028}é", &mut out);
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\u0001\\u2028é\"");
    }
}
