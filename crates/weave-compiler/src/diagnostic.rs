//! Diagnostics: stable codes, severities, spans, labels and notes
//! (LANGUAGE.md §16).

use std::fmt::{self, Write as _};

use crate::source::{SourceManager, Span};

/// Whether a diagnostic stops output generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stable diagnostic code such as `E0001`. Codes are never reused for a
/// different meaning; the table lives in LANGUAGE.md §16.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Code(&'static str);

impl Code {
    /// Invalid UTF-8.
    pub const E0001: Code = Code("E0001");
    /// Unterminated string.
    pub const E0002: Code = Code("E0002");
    /// Unterminated comment.
    pub const E0003: Code = Code("E0003");
    /// Unterminated interpolation.
    pub const E0004: Code = Code("E0004");
    /// Unbalanced bracket.
    pub const E0005: Code = Code("E0005");
    /// U+0000 in source.
    pub const E0006: Code = Code("E0006");
    /// Unexpected token.
    pub const E0101: Code = Code("E0101");
    /// Expected `;`.
    pub const E0102: Code = Code("E0102");
    /// Keyword used as a name or declaration name.
    pub const E0103: Code = Code("E0103");
    /// Statement allowed only at top level.
    pub const E0104: Code = Code("E0104");
    /// Qualified rule without `rule`.
    pub const E0110: Code = Code("E0110");
    /// `{` in a non-custom declaration value.
    pub const E0111: Code = Code("E0111");
    /// `slot` outside a recipe.
    pub const E4012: Code = Code("E4012");
    /// `yield` outside a function.
    pub const E4013: Code = Code("E4013");
    /// Feature not implemented by this compiler build.
    pub const E5001: Code = Code("E5001");
    /// Declaration at stylesheet level.
    pub const E6001: Code = Code("E6001");
    /// Rule in a declaration-list context.
    pub const E6002: Code = Code("E6002");
    /// `&` without a parent rule.
    pub const E6003: Code = Code("E6003");
    /// Item not allowed in this at-rule context.
    pub const E6005: Code = Code("E6005");
    /// `@scope` nested in a rule.
    pub const E6006: Code = Code("E6006");
    /// Invalid `@charset`.
    pub const E6008: Code = Code("E6008");
    /// Module not found.
    pub const E7001: Code = Code("E7001");
    /// Invalid load path.
    pub const E7004: Code = Code("E7004");
    /// Path outside the module root.
    pub const E7006: Code = Code("E7006");
    /// Native `@import`/`@namespace` order violation.
    pub const E7010: Code = Code("E7010");
    /// Input size limit.
    pub const E8001: Code = Code("E8001");
    /// Nesting or parse depth limit.
    pub const E8002: Code = Code("E8002");
    /// Output size limit.
    pub const E8006: Code = Code("E8006");
    /// Module count limit.
    pub const E8008: Code = Code("E8008");

    /// Every code defined so far.
    pub const ALL: [Code; 29] = [
        Code::E0001,
        Code::E0002,
        Code::E0003,
        Code::E0004,
        Code::E0005,
        Code::E0006,
        Code::E0101,
        Code::E0102,
        Code::E0103,
        Code::E0104,
        Code::E0110,
        Code::E0111,
        Code::E4012,
        Code::E4013,
        Code::E5001,
        Code::E6001,
        Code::E6002,
        Code::E6003,
        Code::E6005,
        Code::E6006,
        Code::E6008,
        Code::E7001,
        Code::E7004,
        Code::E7006,
        Code::E7010,
        Code::E8001,
        Code::E8002,
        Code::E8006,
        Code::E8008,
    ];

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// A secondary span with an explanation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

/// One compiler message.
///
/// The primary span is optional only for diagnostics that have no source
/// location, such as an entry module that cannot be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    severity: Severity,
    code: Code,
    message: String,
    primary: Option<Span>,
    labels: Vec<Label>,
    notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: Code, message: impl Into<String>) -> Self {
        Diagnostic::new(Severity::Error, code, message)
    }

    pub fn warning(code: Code, message: impl Into<String>) -> Self {
        Diagnostic::new(Severity::Warning, code, message)
    }

    fn new(severity: Severity, code: Code, message: impl Into<String>) -> Self {
        Diagnostic {
            severity,
            code,
            message: message.into(),
            primary: None,
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn with_primary(mut self, span: Span) -> Self {
        self.primary = Some(span);
        self
    }

    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn severity(&self) -> Severity {
        self.severity
    }

    pub fn code(&self) -> Code {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn primary(&self) -> Option<Span> {
        self.primary
    }

    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// Renders the diagnostic as plain text. Every piece of user-controlled
    /// text (messages, file names, notes) passes through [`escape_control`].
    ///
    /// ```text
    /// error[E0006]: U+0000 is not allowed in source text
    ///   --> main.weave:2:2
    ///   = note: ...
    /// ```
    pub fn render(&self, sources: &SourceManager) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "{}[{}]: {}",
            self.severity,
            self.code,
            escape_control(&self.message)
        );
        if let Some(span) = self.primary {
            let _ = writeln!(out, "  --> {}", location(sources, span));
        }
        for label in &self.labels {
            let _ = writeln!(
                out,
                "  --> {}: {}",
                location(sources, label.span),
                escape_control(&label.message)
            );
        }
        for note in &self.notes {
            let _ = writeln!(out, "  = note: {}", escape_control(note));
        }
        out
    }
}

fn location(sources: &SourceManager, span: Span) -> String {
    match sources.location(span) {
        Some((name, lc)) => format!("{}:{lc}", escape_control(name)),
        None => format!("<unknown location {}..{}>", span.start, span.end),
    }
}

/// Escapes characters that could manipulate a terminal or hide text:
/// C0 and C1 controls (including newlines and ESC), DEL, and the bidirectional
/// formatting characters U+061C, U+200E, U+200F, U+202A–U+202E and
/// U+2066–U+2069. Each becomes `\u{XX}`; a backslash is left as is.
pub fn escape_control(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if needs_escape(c) {
            let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

fn needs_escape(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

/// Collects diagnostics up to a limit (LANGUAGE.md §17, default 100).
///
/// Diagnostics beyond the limit are counted but not stored. Errors are
/// tracked separately so that a full collector still reports failure.
#[derive(Debug)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
    limit: usize,
    dropped: usize,
    has_errors: bool,
}

impl Diagnostics {
    pub fn new(limit: usize) -> Self {
        Diagnostics {
            items: Vec::new(),
            limit,
            dropped: 0,
            has_errors: false,
        }
    }

    /// Records a diagnostic. Returns `false` if it was dropped because the
    /// limit was reached.
    pub fn push(&mut self, diagnostic: Diagnostic) -> bool {
        self.has_errors |= diagnostic.is_error();
        if self.items.len() < self.limit {
            self.items.push(diagnostic);
            true
        } else {
            self.dropped = self.dropped.saturating_add(1);
            false
        }
    }

    pub fn has_errors(&self) -> bool {
        self.has_errors
    }

    pub fn is_full(&self) -> bool {
        self.items.len() >= self.limit
    }

    /// The number of diagnostics dropped after the limit was reached.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_is_in_the_specification_table() {
        let spec = include_str!("../../../docs/LANGUAGE.md");
        let table = &spec[spec.find("## 16. Diagnostics").unwrap()..spec.find("## 17.").unwrap()];
        for code in Code::ALL {
            assert!(
                table.contains(&format!(" {code} ")),
                "{code} missing from LANGUAGE.md §16"
            );
        }
    }

    #[test]
    fn escapes_terminal_controls_and_bidi_overrides() {
        assert_eq!(escape_control("a\x1b[31mb"), "a\\u{1b}[31mb");
        assert_eq!(escape_control("line\nnext\r"), "line\\u{a}next\\u{d}");
        assert_eq!(escape_control("\u{7f}\u{9b}"), "\\u{7f}\\u{9b}");
        assert_eq!(escape_control("x\u{202e}y\u{2066}"), "x\\u{202e}y\\u{2066}");
        assert_eq!(escape_control("tab\there"), "tab\\u{9}here");
    }

    #[test]
    fn leaves_ordinary_unicode_alone() {
        assert_eq!(
            escape_control("café → 😀 \\ \u{200d}"),
            "café → 😀 \\ \u{200d}"
        );
    }

    #[test]
    fn render_includes_code_location_labels_and_notes() {
        let mut sources = SourceManager::new();
        let id = sources
            .add(
                "dir/ma\x1bin.weave",
                "bind a = 1;\nbind é = 2;".as_bytes().to_vec(),
            )
            .unwrap();
        let diag = Diagnostic::error(Code::E5001, "not \x07 here")
            .with_primary(Span::new(id, 17, 19))
            .with_label(Span::new(id, 5, 6), "first")
            .with_note("note\u{202e}");
        assert_eq!(
            diag.render(&sources),
            "error[E5001]: not \\u{7} here\n\
             \x20 --> dir/ma\\u{1b}in.weave:2:6\n\
             \x20 --> dir/ma\\u{1b}in.weave:1:6: first\n\
             \x20 = note: note\\u{202e}\n"
        );
    }

    #[test]
    fn render_without_location() {
        let sources = SourceManager::new();
        let diag = Diagnostic::warning(Code::E7001, "missing");
        assert_eq!(diag.render(&sources), "warning[E7001]: missing\n");
    }

    #[test]
    fn collector_stops_storing_at_the_limit_but_keeps_error_state() {
        let mut diags = Diagnostics::new(2);
        assert!(diags.push(Diagnostic::warning(Code::E5001, "w1")));
        assert!(diags.push(Diagnostic::warning(Code::E5001, "w2")));
        assert!(!diags.has_errors());
        assert!(diags.is_full());
        assert!(!diags.push(Diagnostic::error(Code::E5001, "e")));
        assert!(diags.has_errors());
        assert_eq!(diags.dropped(), 1);
        assert_eq!(diags.into_vec().len(), 2);
    }
}
