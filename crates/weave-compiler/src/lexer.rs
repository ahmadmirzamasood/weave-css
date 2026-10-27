//! CSS-aware tokenizer.
//!
//! Implements the tokenization algorithm of CSS Syntax Module Level 3
//! (CRD 2026-10-01, §4) over a validated UTF-8 buffer, producing tokens with
//! byte spans. Preprocessing is implicit: CR, CRLF and FF are newlines and
//! U+0000 never occurs (rejected as E0006 when the buffer was registered).
//!
//! In [`Mode::Weave`] two extensions from LANGUAGE.md §7 are recognized:
//! `#{…}` is one [`TokenKind::Interpolation`] token (scanned to its matching
//! `}`, respecting strings and comments), and `\#` directly followed by `{` is
//! a [`TokenKind::LiteralHash`] that emits `#`. Interpolations inside quoted
//! strings and unquoted `url(…)` are part of those tokens and recorded in
//! [`Token::interpolation`]. [`Mode::Css`] is plain CSS tokenization, used for
//! verbatim regions (`opaque`, unknown at-rules), where `#{` is ordinary text.

/// Whether the Weave extensions are recognized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Css,
    Weave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Whitespace,
    /// `/* … */`; unterminated comments run to the end of input.
    Comment {
        terminated: bool,
    },
    Ident,
    /// An identifier immediately followed by `(`; the token includes the `(`.
    Function,
    AtKeyword,
    Hash,
    /// A quoted string; unterminated when the input ends inside it.
    String {
        terminated: bool,
    },
    /// A quoted string interrupted by an unescaped newline (the newline is
    /// not part of the token).
    BadString,
    /// An unquoted `url(…)`; unterminated when the input ends inside it.
    Url {
        terminated: bool,
    },
    BadUrl,
    Number,
    Percentage,
    Dimension,
    Delim(char),
    Cdo,
    Cdc,
    Colon,
    Semicolon,
    Comma,
    OpenSquare,
    CloseSquare,
    OpenParen,
    CloseParen,
    OpenBrace,
    CloseBrace,
    /// `#{ … }` including the closing brace (Weave mode only).
    Interpolation {
        terminated: bool,
    },
    /// `\#` immediately followed by `{` (Weave mode only); stands for `#`.
    LiteralHash,
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub start: u32,
    pub end: u32,
    /// For strings and URLs in Weave mode: the offset of the first `#{`
    /// interpolation they contain.
    pub interpolation: Option<u32>,
}

impl Token {
    pub fn text(self, source: &str) -> &str {
        source
            .get(self.start as usize..self.end as usize)
            .unwrap_or("")
    }

    pub fn is_trivia(self) -> bool {
        matches!(self.kind, TokenKind::Whitespace | TokenKind::Comment { .. })
    }
}

/// A cursor over one source text. Positions are byte offsets and always lie
/// on character boundaries.
#[derive(Debug)]
pub(crate) struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    /// Nesting allowed for interpolations inside strings inside
    /// interpolations, which the tokenizer scans recursively.
    max_depth: u32,
    depth: u32,
    too_deep: bool,
}

impl<'a> Lexer<'a> {
    /// Creates a lexer starting at `start` (for example after a byte-order
    /// mark).
    pub fn new(text: &'a str, start: usize, max_depth: u32) -> Self {
        Lexer {
            text,
            bytes: text.as_bytes(),
            pos: start.min(text.len()),
            max_depth,
            depth: 0,
            too_deep: false,
        }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos.min(self.text.len());
    }

    /// True once an interpolation was nested deeper than the limit. The
    /// offending token is reported unterminated; callers report E8002.
    pub fn too_deep(&self) -> bool {
        self.too_deep
    }

    fn byte(&self, at: usize) -> Option<u8> {
        self.bytes.get(at).copied()
    }

    fn peek(&self, ahead: usize) -> Option<u8> {
        self.byte(self.pos + ahead)
    }

    /// Advances over one character (one to four bytes).
    fn bump_char(&mut self) {
        let len = self
            .text
            .get(self.pos..)
            .and_then(|rest| rest.chars().next())
            .map_or(1, char::len_utf8);
        self.pos = (self.pos + len).min(self.text.len());
    }

    fn token(&self, kind: TokenKind, start: usize) -> Token {
        Token {
            kind,
            start: offset(start),
            end: offset(self.pos),
            interpolation: None,
        }
    }

    pub fn next(&mut self, mode: Mode) -> Token {
        let start = self.pos;
        let Some(b) = self.byte(start) else {
            return self.token(TokenKind::Eof, start);
        };
        let kind = match b {
            b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' => {
                while self.peek(0).is_some_and(is_whitespace) {
                    self.pos += 1;
                }
                TokenKind::Whitespace
            }
            b'"' | b'\'' => return self.string(b, mode),
            b'#' if mode == Mode::Weave && self.peek(1) == Some(b'{') => {
                return self.interpolation();
            }
            b'#' => {
                self.pos += 1;
                if self.peek(0).is_some_and(is_name_byte) || self.valid_escape(self.pos, mode) {
                    self.name(mode);
                    TokenKind::Hash
                } else {
                    TokenKind::Delim('#')
                }
            }
            b'(' => self.single(TokenKind::OpenParen),
            b')' => self.single(TokenKind::CloseParen),
            b'[' => self.single(TokenKind::OpenSquare),
            b']' => self.single(TokenKind::CloseSquare),
            b'{' => self.single(TokenKind::OpenBrace),
            b'}' => self.single(TokenKind::CloseBrace),
            b',' => self.single(TokenKind::Comma),
            b':' => self.single(TokenKind::Colon),
            b';' => self.single(TokenKind::Semicolon),
            b'+' | b'.' if self.starts_number(start) => self.numeric(mode),
            b'-' if self.starts_number(start) => self.numeric(mode),
            b'-' if self.peek(1) == Some(b'-') && self.peek(2) == Some(b'>') => {
                self.pos += 3;
                TokenKind::Cdc
            }
            b'-' if self.starts_ident(start, mode) => return self.ident_like(mode),
            b'<' if self
                .text
                .get(start..)
                .is_some_and(|r| r.starts_with("<!--")) =>
            {
                self.pos += 4;
                TokenKind::Cdo
            }
            b'@' if self.starts_ident(start + 1, mode) => {
                self.pos += 1;
                self.name(mode);
                TokenKind::AtKeyword
            }
            b'\\' if self.is_marker(start, mode) => {
                self.pos += 2;
                TokenKind::LiteralHash
            }
            b'\\' if self.valid_escape(start, mode) => return self.ident_like(mode),
            b'0'..=b'9' => self.numeric(mode),
            b'/' if self.peek(1) == Some(b'*') => return self.comment(),
            _ if is_name_start_byte(b) => return self.ident_like(mode),
            _ => {
                let c = self.text.get(start..).and_then(|r| r.chars().next());
                self.bump_char();
                TokenKind::Delim(c.unwrap_or('\u{fffd}'))
            }
        };
        self.token(kind, start)
    }

    fn single(&mut self, kind: TokenKind) -> TokenKind {
        self.pos += 1;
        kind
    }

    /// `\#{` in Weave mode: a literal marker, never an escape.
    fn is_marker(&self, at: usize, mode: Mode) -> bool {
        mode == Mode::Weave
            && self.byte(at) == Some(b'\\')
            && self.byte(at + 1) == Some(b'#')
            && self.byte(at + 2) == Some(b'{')
    }

    /// CSS Syntax §4.3.8, excluding Weave literal markers.
    fn valid_escape(&self, at: usize, mode: Mode) -> bool {
        self.byte(at) == Some(b'\\')
            && self.byte(at + 1).is_some_and(|b| !is_newline(b))
            && !self.is_marker(at, mode)
    }

    /// CSS Syntax §4.3.9.
    fn starts_ident(&self, at: usize, mode: Mode) -> bool {
        match self.byte(at) {
            Some(b'-') => match self.byte(at + 1) {
                Some(b'-') => true,
                Some(b) if is_name_start_byte(b) => true,
                _ => self.valid_escape(at + 1, mode),
            },
            Some(b'\\') => self.valid_escape(at, mode),
            Some(b) => is_name_start_byte(b),
            None => false,
        }
    }

    /// CSS Syntax §4.3.10.
    fn starts_number(&self, at: usize) -> bool {
        let digit = |i: usize| self.byte(i).is_some_and(|b| b.is_ascii_digit());
        match self.byte(at) {
            Some(b'+' | b'-') => {
                digit(at + 1) || (self.byte(at + 1) == Some(b'.') && digit(at + 2))
            }
            Some(b'.') => digit(at + 1),
            Some(b) => b.is_ascii_digit(),
            None => false,
        }
    }

    /// Consumes an escape whose backslash is at the current position.
    fn escape(&mut self) {
        self.pos += 1;
        match self.peek(0) {
            Some(b) if b.is_ascii_hexdigit() => {
                let mut count = 0;
                while count < 6 && self.peek(0).is_some_and(|b| b.is_ascii_hexdigit()) {
                    self.pos += 1;
                    count += 1;
                }
                self.newline_or_space();
            }
            Some(_) => self.bump_char(),
            None => {}
        }
    }

    /// Consumes one whitespace character (CRLF counts as one).
    fn newline_or_space(&mut self) {
        match self.peek(0) {
            Some(b'\r') if self.peek(1) == Some(b'\n') => self.pos += 2,
            Some(b) if is_whitespace(b) => self.pos += 1,
            _ => {}
        }
    }

    /// CSS Syntax §4.3.11: consumes an ident sequence.
    fn name(&mut self, mode: Mode) {
        loop {
            match self.peek(0) {
                Some(b) if is_name_byte(b) => self.bump_char(),
                Some(b'\\') if self.valid_escape(self.pos, mode) => self.escape(),
                _ => break,
            }
        }
    }

    /// CSS Syntax §4.3.4.
    fn ident_like(&mut self, mode: Mode) -> Token {
        let start = self.pos;
        self.name(mode);
        if self.peek(0) != Some(b'(') {
            return self.token(TokenKind::Ident, start);
        }
        let name = self.text.get(start..self.pos).unwrap_or("");
        self.pos += 1;
        if !ident_value(name).eq_ignore_ascii_case("url") {
            return self.token(TokenKind::Function, start);
        }
        let mut look = self.pos;
        while self.byte(look).is_some_and(is_whitespace) {
            look += 1;
        }
        if matches!(self.byte(look), Some(b'"' | b'\'')) {
            return self.token(TokenKind::Function, start);
        }
        self.url(start, mode)
    }

    /// CSS Syntax §4.3.6, after `url(`.
    fn url(&mut self, start: usize, mode: Mode) -> Token {
        let mut interpolation = None;
        while self.peek(0).is_some_and(is_whitespace) {
            self.pos += 1;
        }
        let kind = loop {
            match self.peek(0) {
                None => break TokenKind::Url { terminated: false },
                Some(b')') => {
                    self.pos += 1;
                    break TokenKind::Url { terminated: true };
                }
                Some(b) if is_whitespace(b) => {
                    while self.peek(0).is_some_and(is_whitespace) {
                        self.pos += 1;
                    }
                    match self.peek(0) {
                        None => break TokenKind::Url { terminated: false },
                        Some(b')') => {
                            self.pos += 1;
                            break TokenKind::Url { terminated: true };
                        }
                        _ => break self.bad_url(),
                    }
                }
                Some(b'#') if mode == Mode::Weave && self.peek(1) == Some(b'{') => {
                    interpolation.get_or_insert(offset(self.pos));
                    self.interpolation();
                }
                Some(b'"' | b'\'' | b'(') => break self.bad_url(),
                Some(b) if is_non_printable(b) => break self.bad_url(),
                Some(b'\\') if self.valid_escape(self.pos, Mode::Css) => self.escape(),
                Some(b'\\') => break self.bad_url(),
                Some(_) => self.bump_char(),
            }
        };
        let mut token = self.token(kind, start);
        token.interpolation = interpolation;
        token
    }

    /// CSS Syntax §4.3.14: consumes the remnants of a bad url.
    fn bad_url(&mut self) -> TokenKind {
        loop {
            match self.peek(0) {
                None => break,
                Some(b')') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') if self.valid_escape(self.pos, Mode::Css) => self.escape(),
                Some(_) => self.bump_char(),
            }
        }
        TokenKind::BadUrl
    }

    /// CSS Syntax §4.3.5.
    fn string(&mut self, quote: u8, mode: Mode) -> Token {
        let start = self.pos;
        self.pos += 1;
        let mut interpolation = None;
        let kind = loop {
            match self.peek(0) {
                None => break TokenKind::String { terminated: false },
                Some(b) if b == quote => {
                    self.pos += 1;
                    break TokenKind::String { terminated: true };
                }
                Some(b) if is_newline(b) => break TokenKind::BadString,
                Some(b'\\') => match self.peek(1) {
                    None => self.pos += 1,
                    Some(b'\r') if self.peek(2) == Some(b'\n') => self.pos += 3,
                    Some(b) if is_newline(b) => self.pos += 2,
                    Some(_) => self.escape(),
                },
                Some(b'#') if mode == Mode::Weave && self.peek(1) == Some(b'{') => {
                    interpolation.get_or_insert(offset(self.pos));
                    self.interpolation();
                }
                Some(_) => self.bump_char(),
            }
        };
        let mut token = self.token(kind, start);
        token.interpolation = interpolation;
        token
    }

    /// `#{ … }`: scans Weave-mode tokens up to the matching `}`.
    fn interpolation(&mut self) -> Token {
        let start = self.pos;
        self.pos += 2;
        if self.depth >= self.max_depth {
            self.too_deep = true;
            self.pos = self.text.len();
            return self.token(TokenKind::Interpolation { terminated: false }, start);
        }
        self.depth += 1;
        let mut braces: u64 = 1;
        let terminated = loop {
            let token = self.next(Mode::Weave);
            match token.kind {
                TokenKind::Eof => break false,
                TokenKind::OpenBrace => braces += 1,
                TokenKind::CloseBrace => {
                    braces -= 1;
                    if braces == 0 {
                        break true;
                    }
                }
                _ => {}
            }
            if self.too_deep {
                break false;
            }
        };
        self.depth -= 1;
        self.token(TokenKind::Interpolation { terminated }, start)
    }

    fn comment(&mut self) -> Token {
        let start = self.pos;
        let terminated = match self.text.get(start + 2..).and_then(|r| r.find("*/")) {
            Some(end) => {
                self.pos = start + 2 + end + 2;
                true
            }
            None => {
                self.pos = self.text.len();
                false
            }
        };
        self.token(TokenKind::Comment { terminated }, start)
    }

    /// CSS Syntax §4.3.3.
    fn numeric(&mut self, mode: Mode) -> TokenKind {
        if matches!(self.peek(0), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        self.digits();
        if self.peek(0) == Some(b'.') && self.peek(1).is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
            self.digits();
        }
        if matches!(self.peek(0), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(self.peek(1), Some(b'+' | b'-')));
            if self.peek(1 + sign).is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1 + sign;
                self.digits();
            }
        }
        if self.starts_ident(self.pos, mode) {
            self.name(mode);
            TokenKind::Dimension
        } else if self.peek(0) == Some(b'%') {
            self.pos += 1;
            TokenKind::Percentage
        } else {
            TokenKind::Number
        }
    }

    fn digits(&mut self) {
        while self.peek(0).is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
    }
}

fn offset(pos: usize) -> u32 {
    // Source files are at most `u32::MAX` bytes (`SourceManager::add`).
    u32::try_from(pos).unwrap_or(u32::MAX)
}

pub(crate) fn is_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

fn is_newline(b: u8) -> bool {
    matches!(b, b'\n' | b'\r' | b'\x0c')
}

/// Ident-start code point; every byte of a non-ASCII scalar is >= 0x80.
fn is_name_start_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn is_name_byte(b: u8) -> bool {
    is_name_start_byte(b) || b.is_ascii_digit() || b == b'-'
}

fn is_non_printable(b: u8) -> bool {
    matches!(b, 0x00..=0x08 | 0x0b | 0x0e..=0x1f | 0x7f)
}

/// The value of an ident sequence with CSS escapes decoded (CSS Syntax
/// §4.3.7). Invalid code points decode to U+FFFD.
pub(crate) fn ident_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 && chars.peek().is_some_and(char::is_ascii_hexdigit) {
            if let Some(h) = chars.next() {
                hex.push(h);
            }
        }
        if hex.is_empty() {
            if let Some(next) = chars.next() {
                out.push(next);
            }
            continue;
        }
        if chars.peek() == Some(&'\r') {
            chars.next();
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
        } else if chars
            .peek()
            .is_some_and(|c| matches!(c, ' ' | '\t' | '\n' | '\x0c'))
        {
            chars.next();
        }
        let value = u32::from_str_radix(&hex, 16).unwrap_or(0);
        let decoded = match value {
            0 => '\u{fffd}',
            v => char::from_u32(v).unwrap_or('\u{fffd}'),
        };
        out.push(decoded);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str, mode: Mode) -> Vec<(TokenKind, &str)> {
        let mut lexer = Lexer::new(text, 0, 16);
        let mut out = Vec::new();
        loop {
            let token = lexer.next(mode);
            if token.kind == TokenKind::Eof {
                return out;
            }
            out.push((token.kind, token.text(text)));
        }
    }

    use TokenKind as K;

    #[test]
    fn tokenizes_a_declaration() {
        assert_eq!(
            kinds("color: rgb(0 0 0 / 50%);", Mode::Css),
            [
                (K::Ident, "color"),
                (K::Colon, ":"),
                (K::Whitespace, " "),
                (K::Function, "rgb("),
                (K::Number, "0"),
                (K::Whitespace, " "),
                (K::Number, "0"),
                (K::Whitespace, " "),
                (K::Number, "0"),
                (K::Whitespace, " "),
                (K::Delim('/'), "/"),
                (K::Whitespace, " "),
                (K::Percentage, "50%"),
                (K::CloseParen, ")"),
                (K::Semicolon, ";"),
            ]
        );
    }

    #[test]
    fn strings_comments_and_urls_hide_punctuation() {
        let text = r#""a; } /*" /* ; } */ url(data:x;y{}) url( "q;" )"#;
        assert_eq!(
            kinds(text, Mode::Css),
            [
                (K::String { terminated: true }, r#""a; } /*""#),
                (K::Whitespace, " "),
                (K::Comment { terminated: true }, "/* ; } */"),
                (K::Whitespace, " "),
                (K::Url { terminated: true }, "url(data:x;y{})"),
                (K::Whitespace, " "),
                (K::Function, "url("),
                (K::Whitespace, " "),
                (K::String { terminated: true }, r#""q;""#),
                (K::Whitespace, " "),
                (K::CloseParen, ")"),
            ]
        );
    }

    #[test]
    fn bad_and_unterminated_tokens() {
        assert_eq!(kinds("\"ab\ncd", Mode::Css)[0], (K::BadString, "\"ab"));
        assert_eq!(
            kinds("'abc", Mode::Css),
            [(K::String { terminated: false }, "'abc")]
        );
        assert_eq!(kinds("url(a b) x", Mode::Css)[0], (K::BadUrl, "url(a b)"));
        assert_eq!(
            kinds("url(abc", Mode::Css),
            [(K::Url { terminated: false }, "url(abc")]
        );
        assert_eq!(
            kinds("/* x", Mode::Css),
            [(K::Comment { terminated: false }, "/* x")]
        );
    }

    #[test]
    fn escapes_numbers_and_identifiers() {
        assert_eq!(
            kinds(
                r"\72 ule --x -webkit-a _h é1 12px -.5e3% +3 #a1 #-",
                Mode::Css
            )
            .into_iter()
            .filter(|(k, _)| *k != K::Whitespace)
            .collect::<Vec<_>>(),
            [
                (K::Ident, r"\72 ule"),
                (K::Ident, "--x"),
                (K::Ident, "-webkit-a"),
                (K::Ident, "_h"),
                (K::Ident, "é1"),
                (K::Dimension, "12px"),
                (K::Percentage, "-.5e3%"),
                (K::Number, "+3"),
                (K::Hash, "#a1"),
                (K::Hash, "#-"),
            ]
        );
        assert_eq!(ident_value(r"\72 ule"), "rule");
        assert_eq!(ident_value(r"u\72 l"), "url");
        assert_eq!(ident_value(r"\0 x\110000 y\z"), "\u{fffd}x\u{fffd}yz");
    }

    #[test]
    fn escaped_url_name_is_a_url_token() {
        assert_eq!(
            kinds(r"u\72 l(a;b)", Mode::Css),
            [(K::Url { terminated: true }, r"u\72 l(a;b)")]
        );
    }

    #[test]
    fn weave_extensions_only_in_weave_mode() {
        assert_eq!(
            kinds("#{a + {b: 1}} \\#{x}", Mode::Weave),
            [
                (K::Interpolation { terminated: true }, "#{a + {b: 1}}"),
                (K::Whitespace, " "),
                (K::LiteralHash, "\\#"),
                (K::OpenBrace, "{"),
                (K::Ident, "x"),
                (K::CloseBrace, "}"),
            ]
        );
        assert_eq!(
            kinds("#{a}", Mode::Css),
            [
                (K::Delim('#'), "#"),
                (K::OpenBrace, "{"),
                (K::Ident, "a"),
                (K::CloseBrace, "}"),
            ]
        );
        assert_eq!(kinds(r"\#{", Mode::Css)[0], (K::Ident, r"\#"));
        assert_eq!(
            kinds("#{ \"}\" ", Mode::Weave),
            [(K::Interpolation { terminated: false }, "#{ \"}\" ")]
        );
    }

    #[test]
    fn interpolation_inside_strings_and_urls_is_recorded() {
        let text = r#""a #{"}" + b} c" url(x#{y}) "\#{n}""#;
        let mut lexer = Lexer::new(text, 0, 16);
        let s = lexer.next(Mode::Weave);
        assert_eq!(s.kind, K::String { terminated: true });
        assert_eq!(s.text(text), r#""a #{"}" + b} c""#);
        assert_eq!(s.interpolation, Some(3));
        lexer.next(Mode::Weave);
        let u = lexer.next(Mode::Weave);
        assert_eq!(u.kind, K::Url { terminated: true });
        assert_eq!(u.interpolation, Some(22));
        lexer.next(Mode::Weave);
        let literal = lexer.next(Mode::Weave);
        assert_eq!(literal.interpolation, None);
        assert_eq!(literal.kind, K::String { terminated: true });
    }

    #[test]
    fn deep_interpolation_nesting_stops() {
        let text = "#{\"#{\"#{\"#{x}\"}\"}\"}";
        let mut lexer = Lexer::new(text, 0, 2);
        let token = lexer.next(Mode::Weave);
        assert!(lexer.too_deep());
        assert_eq!(token.kind, K::Interpolation { terminated: false });
        assert_eq!(lexer.next(Mode::Weave).kind, K::Eof);
    }

    #[test]
    fn crlf_and_form_feed_are_whitespace() {
        assert_eq!(
            kinds("a\r\n\x0cb", Mode::Css),
            [
                (K::Ident, "a"),
                (K::Whitespace, "\r\n\x0c"),
                (K::Ident, "b")
            ]
        );
    }

    #[test]
    fn every_input_terminates_with_progress() {
        // Exhaustive over short strings of tricky bytes: each call advances.
        let alphabet = [
            "\\", "#", "{", "}", "\"", "u", "url(", "/*", "-", "1", "é", "\n", "@", "e",
        ];
        let mut stack = vec![(String::new(), 0)];
        while let Some((prefix, parts)) = stack.pop() {
            for mode in [Mode::Css, Mode::Weave] {
                let mut lexer = Lexer::new(&prefix, 0, 4);
                let mut guard = 0;
                loop {
                    let before = lexer.pos();
                    let token = lexer.next(mode);
                    if token.kind == K::Eof {
                        break;
                    }
                    assert!(lexer.pos() > before, "{prefix:?}");
                    assert!(prefix.is_char_boundary(lexer.pos()));
                    guard += 1;
                    assert!(guard <= prefix.len());
                }
            }
            if parts < 4 {
                for part in alphabet {
                    stack.push((format!("{prefix}{part}"), parts + 1));
                }
            }
        }
    }
}
