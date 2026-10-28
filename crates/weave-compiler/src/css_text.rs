//! Rendering token sequences as output CSS text.
//!
//! - Every line break (CR, CRLF, FF) is written as LF (LANGUAGE.md §2).
//! - Literal markers `\#{` become `#{` (§7.4), outside strings via
//!   [`TokenKind::LiteralHash`] and inside strings and URLs textually.
//! - Selectors and known at-rule preludes are normalized (§12, §15);
//!   declaration values keep their text.
//!
//! Tokens that contain interpolation (`#{…}` itself, and strings and URLs
//! with `#{…}` inside) are not rendered: they are kept as [`Seg::Token`] so
//! that the parser can turn them into template interpolations.

use crate::lexer::{Token, TokenKind as K};

/// Rendered text, or a token carrying interpolation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Seg {
    Text(String),
    Token(Token),
}

fn has_interpolation(token: Token) -> bool {
    matches!(token.kind, K::Interpolation { .. }) || token.interpolation.is_some()
}

/// Appends text, merging it with a preceding text segment.
pub(crate) fn push_text(segs: &mut Vec<Seg>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(Seg::Text(last)) = segs.last_mut() {
        last.push_str(text);
    } else {
        segs.push(Seg::Text(text.to_string()));
    }
}

/// Appends a token: rendered, or kept if it carries interpolation.
pub(crate) fn push_token(segs: &mut Vec<Seg>, token: Token, source: &str) {
    if has_interpolation(token) {
        segs.push(Seg::Token(token));
    } else {
        let mut text = String::new();
        render_token(token, source, &mut text);
        push_text(segs, &text);
    }
}

/// Writes CR, CRLF and FF as LF.
pub(crate) fn normalize_newlines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\x0c' => out.push('\n'),
            _ => out.push(c),
        }
    }
    out
}

/// Replaces each literal marker `\#{` by `#{`, leaving every other escape
/// (including `\\`) untouched.
pub(crate) fn replace_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('#') => {
                chars.next();
                if chars.peek() != Some(&'{') {
                    out.push('\\');
                }
                out.push('#');
            }
            Some(_) => {
                out.push('\\');
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The output text of one token from a Weave-mode region.
pub(crate) fn render_token(token: Token, source: &str, out: &mut String) {
    let text = token.text(source);
    match token.kind {
        K::LiteralHash => out.push('#'),
        K::String { .. } | K::BadString | K::Url { .. } | K::BadUrl => {
            out.push_str(&replace_markers(&normalize_newlines(text)));
        }
        _ => out.push_str(&normalize_newlines(text)),
    }
}

/// Renders tokens exactly, apart from line breaks and literal markers.
pub(crate) fn render(tokens: &[Token], source: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    for &token in tokens {
        push_token(&mut out, token, source);
    }
    out
}

/// A known at-rule prelude or keyframe selector: comments removed, every run
/// of whitespace and comments written as one space, ends trimmed.
pub(crate) fn normalize_prelude(tokens: &[Token], source: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut space = false;
    for &token in tokens {
        if token.is_trivia() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            push_text(&mut out, " ");
        }
        space = false;
        push_token(&mut out, token, source);
    }
    out
}

fn closer(kind: K) -> Option<K> {
    match kind {
        K::OpenParen | K::Function => Some(K::CloseParen),
        K::OpenSquare => Some(K::CloseSquare),
        K::OpenBrace => Some(K::CloseBrace),
        _ => None,
    }
}

/// How whitespace is normalized inside one bracket level of a selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LevelKind {
    /// The selector list itself and pseudo-class arguments.
    Selector,
    /// `[…]`.
    Attribute,
    /// `{…}` (only possible inside parentheses): prelude rules.
    Brace,
}

#[derive(Debug)]
struct Level {
    kind: LevelKind,
    /// The closing token kind; `None` at the top level.
    closer: Option<K>,
    started: bool,
    space: bool,
    after_combinator: bool,
    seen_equals: bool,
    after_operator: bool,
}

impl Level {
    fn new(kind: LevelKind, closer: Option<K>) -> Self {
        Level {
            kind,
            closer,
            started: false,
            space: false,
            after_combinator: false,
            seen_equals: false,
            after_operator: false,
        }
    }

    /// Writes the separator before a component value at this level.
    fn before_value(&mut self, token: Token, out: &mut Vec<Seg>) {
        let space = match self.kind {
            LevelKind::Selector => self.space && self.started && !self.after_combinator,
            LevelKind::Attribute => {
                // Only a modifier after the value keeps its space.
                self.space && token.kind == K::Ident && self.seen_equals && !self.after_operator
            }
            LevelKind::Brace => self.space && self.started,
        };
        if space {
            push_text(out, " ");
        }
        self.started = true;
        self.space = false;
        self.after_combinator = false;
        self.after_operator = matches!(token.kind, K::Delim('=' | '~' | '|' | '^' | '$' | '*'));
        self.seen_equals |= token.kind == K::Delim('=');
    }
}

/// A `rule` selector list (LANGUAGE.md §15): selectors joined with `, `,
/// one space around combinators and for descendant combinators, no
/// whitespace inside `[…]` except before an attribute modifier, and
/// pseudo-class argument lists normalized the same way. Comments are removed.
///
/// One pass over the tokens with an explicit stack of bracket levels, so
/// deeply nested input cannot exhaust the thread stack. Unmatched closers are
/// written as ordinary tokens; unclosed brackets end with the input (the
/// parser has already reported both).
pub(crate) fn normalize_selector(tokens: &[Token], source: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut levels = vec![Level::new(LevelKind::Selector, None)];
    for &token in tokens {
        let nested = levels.len() > 1;
        let Some(level) = levels.last_mut() else {
            break;
        };
        if token.is_trivia() {
            level.space = true;
            continue;
        }
        if nested && level.closer == Some(token.kind) {
            levels.pop();
            push_token(&mut out, token, source);
            continue;
        }
        match (level.kind, token.kind) {
            (LevelKind::Selector, K::Comma) => {
                push_text(&mut out, ", ");
                *level = Level::new(LevelKind::Selector, level.closer);
            }
            (LevelKind::Selector, K::Delim('>' | '+' | '~')) => {
                if level.started {
                    push_text(&mut out, " ");
                }
                push_token(&mut out, token, source);
                push_text(&mut out, " ");
                level.started = true;
                level.space = false;
                level.after_combinator = true;
            }
            _ => {
                level.before_value(token, &mut out);
                let inner = match token.kind {
                    K::OpenSquare => LevelKind::Attribute,
                    _ if level.kind == LevelKind::Brace => LevelKind::Brace,
                    K::OpenBrace => LevelKind::Brace,
                    _ => LevelKind::Selector,
                };
                push_token(&mut out, token, source);
                if let Some(close) = closer(token.kind) {
                    levels.push(Level::new(inner, Some(close)));
                }
            }
        }
    }
    out
}

/// Trims CSS whitespace from both ends.
pub(crate) fn trim_css_whitespace(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{Lexer, Mode};

    fn tokens(text: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(text, 0, 16);
        let mut out = Vec::new();
        loop {
            let t = lexer.next(Mode::Weave);
            if t.kind == K::Eof {
                return out;
            }
            out.push(t);
        }
    }

    fn flat(segs: Vec<Seg>) -> String {
        segs.into_iter()
            .map(|seg| match seg {
                Seg::Text(text) => text,
                Seg::Token(_) => "<token>".to_string(),
            })
            .collect()
    }

    fn selector(text: &str) -> String {
        flat(normalize_selector(&tokens(text), text))
    }

    #[test]
    fn selectors_are_normalized() {
        let cases = [
            (".a,.b", ".a, .b"),
            ("  .a  ,\n\t.b  ", ".a, .b"),
            ("a>b", "a > b"),
            ("a  >  b+c~d", "a > b + c ~ d"),
            ("a /* c */ b", "a b"),
            ("a/**/b", "a b"),
            ("ul   li", "ul li"),
            ("[ href ^= \"x\" i ]", "[href^=\"x\" i]"),
            ("[data-a = b s]", "[data-a=b s]"),
            ("[ ns|attr ]", "[ns|attr]"),
            (":not( .a ,.b )", ":not(.a, .b)"),
            (":nth-child(2n+1 of .x,.y)", ":nth-child(2n+1 of .x, .y)"),
            (":is(a>b)", ":is(a > b)"),
            ("svg|a, *|*", "svg|a, *|*"),
            ("a::before", "a::before"),
            (".naïve-日本", ".naïve-日本"),
            ("[title=\"a, b\"]", "[title=\"a, b\"]"),
            (r".a\:b", r".a\:b"),
            ("> .x", "> .x"),
        ];
        for (input, expected) in cases {
            assert_eq!(selector(input), expected, "{input:?}");
        }
    }

    #[test]
    fn preludes_collapse_whitespace_and_drop_comments() {
        let text = "  screen  and/* c */(width\n>=\t768px)  ";
        assert_eq!(
            flat(normalize_prelude(&tokens(text), text)),
            "screen and (width >= 768px)"
        );
        let text = "\"a  b\"  ,  'c'";
        assert_eq!(
            flat(normalize_prelude(&tokens(text), text)),
            "\"a  b\" , 'c'"
        );
    }

    #[test]
    fn line_breaks_become_lf() {
        assert_eq!(normalize_newlines("a\r\nb\rc\x0cd\ne"), "a\nb\nc\nd\ne");
    }

    #[test]
    fn markers_are_replaced_only_as_markers() {
        assert_eq!(
            replace_markers(r##""\#{x} \# \\#{y} \23 ""##),
            r##""#{x} \# \\#{y} \23 ""##
        );
        assert_eq!(replace_markers("a\\"), "a\\");
        let text = r##"\#{a} "\#{b}" /* \#{c} */"##;
        assert_eq!(
            flat(render(&tokens(text), text)),
            r##"#{a} "#{b}" /* \#{c} */"##
        );
    }

    #[test]
    fn interpolation_tokens_are_kept_in_place() {
        let text = ".a-#{x}  >  [t=\"#{y}\"]";
        let segs = normalize_selector(&tokens(text), text);
        assert_eq!(flat(segs.clone()), ".a-<token> > [t=<token>]");
        assert_eq!(segs.len(), 5);
        let text = "a #{b} url(#{c}) d";
        assert_eq!(flat(render(&tokens(text), text)), "a <token> <token> d");
    }
}
