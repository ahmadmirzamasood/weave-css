//! Tokenizer and parser entry points for the fuzz targets in `fuzz/`
//! (feature `fuzzing`). Not a stable API.
//!
//! Each function checks the structural invariants of its stage and panics
//! with a description when one is broken, so a fuzzer reports the input.
//! [`crate::compile`] is fuzzed through the public API.

use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::lexer::{Lexer, Mode, TokenKind};
use crate::options::Limit;
use crate::parser;
use crate::source::SourceManager;

/// Tokenizes `text` in both modes and returns the number of tokens.
///
/// Checks that tokens are contiguous, non-empty (except the final `Eof`),
/// lie on character boundaries, cover the whole text and that the
/// tokenizer reaches `Eof` within one token per byte.
pub fn lex(text: &str, max_depth: u32) -> usize {
    let mut count = 0;
    for mode in [Mode::Weave, Mode::Css] {
        let mut lexer = Lexer::new(text, 0, max_depth);
        let mut at = 0usize;
        let mut reached_end = false;
        for _ in 0..=text.len() {
            let token = lexer.next(mode);
            let (start, end) = (token.start as usize, token.end as usize);
            assert_eq!(start, at, "token does not start where the last one ended");
            assert!(end <= text.len(), "token ends past the input");
            assert!(
                text.is_char_boundary(start) && text.is_char_boundary(end),
                "token splits a character"
            );
            count += 1;
            if token.kind == TokenKind::Eof {
                assert_eq!(start, text.len(), "Eof before the end of input");
                assert_eq!(start, end, "Eof is not empty");
                reached_end = true;
                break;
            }
            assert!(end > start, "empty token {:?} at {start}", token.kind);
            at = end;
        }
        assert!(reached_end, "tokenizer did not reach Eof");
    }
    count
}

/// Registers `bytes` as a source file (UTF-8 and U+0000 checks, byte-order
/// mark), parses it as an entry module and returns the number of
/// diagnostics.
///
/// Checks that every diagnostic span lies inside the file on character
/// boundaries.
pub fn parse(bytes: &[u8], max_depth: u32) -> usize {
    let mut sources = SourceManager::new();
    let file = match sources.add("fuzz.weave", bytes.to_vec()) {
        Ok(file) => file,
        Err(diag) => {
            // E0001/E0006: the file is registered with its valid prefix.
            let text = sources.files().next().map_or("", |file| file.text());
            check_spans(&sources, &diag, text);
            return 1;
        }
    };
    let Some(source) = sources.get(file) else {
        return 0;
    };
    let text = source.text();
    let limit = Limit::Diagnostics.default_value();
    let mut diags = Diagnostics::new(usize::try_from(limit).unwrap_or(usize::MAX));
    let start = source.content_start() as usize;
    let _ = parser::parse(text, start, file, true, max_depth, &mut diags);
    let items = diags.into_vec();
    for diag in &items {
        check_spans(&sources, diag, text);
    }
    items.len()
}

fn check_spans(sources: &SourceManager, diag: &Diagnostic, text: &str) {
    let spans = diag
        .primary()
        .into_iter()
        .chain(diag.labels().iter().map(|label| label.span));
    for span in spans {
        let (start, end) = (span.start as usize, span.end as usize);
        assert!(start <= end && end <= text.len(), "span outside the input");
        assert!(
            text.is_char_boundary(start) && text.is_char_boundary(end),
            "span splits a character"
        );
        assert!(sources.location(span).is_some(), "span has no location");
    }
    // Rendering must not fail and must not let control characters through.
    let rendered = diag.render(sources);
    assert!(
        !rendered.chars().any(|c| c.is_control() && c != '\n'),
        "control character in a rendered diagnostic"
    );
}
