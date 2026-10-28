//! Expressions (LANGUAGE.md §5) and interpolation templates (§7).
//!
//! Expressions are tokenized with the expression-mode lexical grammar
//! (grammar.ebnf §2) and compiled by an operator-precedence (shunting-yard)
//! parser into a postfix [`Program`]. Neither parsing nor evaluation
//! recurses per bracket or operator, so the nesting limit, not the thread
//! stack, bounds how deep expressions may be. `&&` and `||` compile to
//! conditional jumps, which gives short-circuit evaluation.
//!
//! The only recursion is through templates: an interpolation inside a
//! string or CSS fragment inside an interpolation. That nesting is limited
//! to [`MAX_TEMPLATE_NESTING`] levels (E8002).
//!
//! A [`Template`] is CSS or string text with interpolations; each
//! interpolation records the context its value is inserted in.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::css_text::{Seg, push_quoted, render_token};
use crate::diagnostic::{Code, Diagnostic};
use crate::lexer::{Lexer, Mode, Token, TokenKind as K};
use crate::number::{Number, Unit};
use crate::source::{FileId, Span};
use crate::syntax::Param;
use crate::value::{Color, Type, Value};

/// How deeply interpolations may nest inside strings and fragments inside
/// interpolations. A reference-compiler bound (LANGUAGE.md §17) that keeps
/// the template recursion far from the thread stack's capacity.
pub(crate) const MAX_TEMPLATE_NESTING: u32 = 64;

/// The keywords of LANGUAGE.md §4.1.
pub(crate) const KEYWORDS: [&str; 19] = [
    "load",
    "as",
    "export",
    "bind",
    "fn",
    "recipe",
    "rule",
    "when",
    "otherwise",
    "each",
    "in",
    "apply",
    "slot",
    "yield",
    "fail",
    "warn",
    "opaque",
    "true",
    "false",
];

/// Where an interpolated value is inserted (LANGUAGE.md §7.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ctx {
    /// Declaration values, known preludes, keyframe selectors, fragments.
    Value,
    /// Inside a quoted CSS string with this delimiter.
    Str(char),
    /// Inside an unquoted `url(…)`.
    Url,
    Selector,
    /// A declaration name.
    Name,
    /// Inside a Weave string literal: the value's text becomes string data.
    Text,
}

#[derive(Debug)]
pub(crate) struct Interp {
    pub program: Program,
    pub ctx: Ctx,
    /// The whole `#{…}`.
    pub span: Span,
}

#[derive(Debug)]
pub(crate) enum Piece {
    Text(String),
    Interp(Box<Interp>),
}

/// Text with interpolations. Text pieces are output-ready (line breaks are
/// LF and literal markers are already `#{`).
#[derive(Debug, Default)]
pub(crate) struct Template {
    pub pieces: Vec<Piece>,
}

impl Template {
    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(Piece::Text(last)) = self.pieces.last_mut() {
            last.push_str(text);
        } else {
            self.pieces.push(Piece::Text(text.to_string()));
        }
    }

    /// [`Template::push_text`] for an owned string, moved rather than copied
    /// when it starts a text piece.
    fn push_owned(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        if let Some(Piece::Text(last)) = self.pieces.last_mut() {
            last.push_str(&text);
        } else {
            self.pieces.push(Piece::Text(text));
        }
    }

    /// The text of a template without interpolations.
    pub fn static_text(&self) -> Option<&str> {
        match self.pieces.as_slice() {
            [] => Some(""),
            [Piece::Text(text)] => Some(text),
            _ => None,
        }
    }
}

/// Binary operators other than `&&` and `||`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Mul,
    Div,
    Rem,
    Add,
    Sub,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl BinOp {
    fn prec(self) -> u8 {
        match self {
            BinOp::Mul | BinOp::Div | BinOp::Rem => 6,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
            BinOp::Eq | BinOp::Ne => 3,
        }
    }

    /// Non-associative operator classes (E0112).
    fn class(self) -> Option<u8> {
        match self {
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Some(1),
            BinOp::Eq | BinOp::Ne => Some(2),
            _ => None,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
        }
    }
}

/// A call target: a name or `namespace.member`.
#[derive(Debug)]
pub(crate) struct Callee {
    pub namespace: Option<(Rc<str>, Span)>,
    pub name: Rc<str>,
    /// From the first segment through the last.
    pub span: Span,
}

#[derive(Debug)]
pub(crate) struct Arg {
    pub name: Option<Rc<str>>,
    /// The argument name for named arguments, else the argument's first
    /// token.
    pub span: Span,
}

#[derive(Debug)]
pub(crate) struct Call {
    pub callee: Callee,
    pub args: Vec<Arg>,
}

/// One postfix instruction. Operands are taken from and results pushed to
/// the evaluator's value stack.
#[derive(Debug)]
pub(crate) enum Op {
    Const(Value),
    /// A string literal with interpolations.
    Str(Box<Template>),
    /// A CSS fragment literal; validated after interpolation (E2006).
    Fragment(Box<Template>, Span),
    Load(Rc<str>, Span),
    Neg(Span),
    Not(Span),
    Binary(BinOp, Span),
    /// `&&`: pops a bool; if false, pushes false and jumps.
    JumpIfFalse(usize, Span),
    /// `||`: pops a bool; if true, pushes true and jumps.
    JumpIfTrue(usize, Span),
    /// The right operand of `&&`/`||` must be a bool.
    AssertBool(Span),
    /// Pops this many values into a list.
    List(usize, Span),
    /// Pops one value per key into a map.
    Map(Rc<[(Rc<str>, Span)]>, Span),
    /// `.key` (span of the key).
    Member(Rc<str>, Span),
    /// `[index]` (span of `[`).
    Index(Span),
    /// Pops one value per argument.
    Call(Box<Call>),
}

#[derive(Debug, Default)]
pub(crate) struct Program {
    pub ops: Vec<Op>,
}

/// Shared parsing context.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cx<'a> {
    pub text: &'a str,
    pub file: FileId,
    /// The nesting limit (blocks, expressions and brackets combined).
    pub max_depth: u32,
    /// The enclosing block depth.
    pub depth: u32,
    /// Template nesting of the expression being parsed.
    pub nesting: u32,
}

impl Cx<'_> {
    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file, offset(start), offset(end))
    }

    fn token_span(&self, token: Token) -> Span {
        Span::new(self.file, token.start, token.end)
    }

    fn error(
        &self,
        code: Code,
        start: usize,
        end: usize,
        message: impl Into<String>,
    ) -> Diagnostic {
        Diagnostic::error(code, message).with_primary(self.span(start, end))
    }

    /// A lexer for strings and fragments inside expressions; its own
    /// interpolation nesting is bounded by the template limit.
    fn lexer(&self, at: usize) -> Lexer<'_> {
        let room = MAX_TEMPLATE_NESTING.saturating_sub(self.nesting).max(1);
        Lexer::new(self.text, at, room)
    }

    fn too_deep_templates(&self, start: usize, end: usize) -> Diagnostic {
        self.error(
            Code::E8002,
            start,
            end,
            format!(
                "interpolations nest more than {MAX_TEMPLATE_NESTING} levels deep inside strings and fragments"
            ),
        )
    }
}

fn offset(pos: usize) -> u32 {
    u32::try_from(pos).unwrap_or(u32::MAX)
}

// ----- tokens -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum P {
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Semicolon,
    Colon,
    Dot,
    Assign,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    Ne,
    AndAnd,
    OrOr,
}

const PUNCT: [(&str, P); 26] = [
    ("->", P::Arrow),
    ("<=", P::Le),
    (">=", P::Ge),
    ("==", P::EqEq),
    ("!=", P::Ne),
    ("&&", P::AndAnd),
    ("||", P::OrOr),
    ("(", P::LParen),
    (")", P::RParen),
    ("[", P::LBracket),
    ("]", P::RBracket),
    ("{", P::LBrace),
    ("}", P::RBrace),
    (",", P::Comma),
    (";", P::Semicolon),
    (":", P::Colon),
    (".", P::Dot),
    ("=", P::Assign),
    ("+", P::Plus),
    ("-", P::Minus),
    ("*", P::Star),
    ("/", P::Slash),
    ("%", P::Percent),
    ("!", P::Bang),
    ("<", P::Lt),
    (">", P::Gt),
];

#[derive(Debug)]
enum TK {
    Number(Number),
    Ident,
    /// A quoted string token (CSS lexer, Weave mode).
    Str(Token),
    /// A fragment literal: the CSS tokens between the backticks.
    Fragment(Vec<Token>),
    Color(Color),
    P(P),
}

#[derive(Debug)]
struct Tok {
    kind: TK,
    start: usize,
    end: usize,
}

/// Where scanning stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    /// The end of the scanned region.
    End(usize),
    /// A `;` at depth 0; the position after it.
    Semicolon(usize),
    /// A `}` at depth 0 (not consumed); its position.
    CloseBrace(usize),
    /// A `{` at depth 0 after a complete operand (statement heads only); its
    /// position.
    OpenBrace(usize),
}

/// What ends a scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanMode {
    /// Only the end of the region (interpolations).
    Region,
    /// A `;`, or a `}` at depth 0 (`bind`, `yield`, `fail`, `warn`).
    Statement,
    /// Also a `{` at depth 0 that follows a complete operand: the block of
    /// `when`, `each`, `apply`, `fn` and `recipe` (grammar.ebnf §8). A `{`
    /// where an operand is expected starts a map literal instead.
    Head,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn is_ident_cont(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit() || c == '-'
}

/// Whether `text` is one Weave identifier (LANGUAGE.md §4.1).
pub(crate) fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(is_ident_start) && chars.all(is_ident_cont)
}

struct Scanner<'a> {
    cx: Cx<'a>,
    pos: usize,
    end: usize,
}

impl Scanner<'_> {
    fn byte(&self, at: usize) -> Option<u8> {
        if at < self.end {
            self.cx.text.as_bytes().get(at).copied()
        } else {
            None
        }
    }

    fn char_at(&self, at: usize) -> Option<char> {
        if at < self.end {
            self.cx.text.get(at..).and_then(|r| r.chars().next())
        } else {
            None
        }
    }

    /// Tokenizes up to `end`, or (for statements and heads) up to the stop
    /// that `mode` defines.
    fn scan(&mut self, mode: ScanMode) -> Result<(Vec<Tok>, Stop), Diagnostic> {
        let mut toks = Vec::new();
        let mut depth = 0usize;
        // Whether the last token completes an operand (heads only).
        let mut operand_end = false;
        loop {
            self.skip_trivia()?;
            let start = self.pos;
            let Some(c) = self.char_at(start) else {
                return Ok((toks, Stop::End(start)));
            };
            let kind = match c {
                '0'..='9' => self.number()?,
                '.' if self.byte(start + 1).is_some_and(|b| b.is_ascii_digit()) => self.number()?,
                '"' | '\'' => self.string()?,
                '`' => self.fragment()?,
                '#' => self.color()?,
                c if is_ident_start(c) => {
                    while self.char_at(self.pos).is_some_and(is_ident_cont) {
                        self.pos += self.char_at(self.pos).map_or(1, char::len_utf8);
                    }
                    TK::Ident
                }
                _ => {
                    let rest = self.cx.text.get(start..self.end).unwrap_or("");
                    let Some(&(text, p)) = PUNCT.iter().find(|(t, _)| rest.starts_with(t)) else {
                        let what = if c == '\\' {
                            "CSS escapes are not allowed in expressions".to_string()
                        } else {
                            format!("unexpected `{c}` in an expression")
                        };
                        return Err(self
                            .cx
                            .error(Code::E0101, start, start + c.len_utf8(), what));
                    };
                    if mode != ScanMode::Region {
                        match p {
                            P::LBrace if depth == 0 && operand_end && mode == ScanMode::Head => {
                                return Ok((toks, Stop::OpenBrace(start)));
                            }
                            P::LParen | P::LBracket | P::LBrace => depth += 1,
                            P::RParen | P::RBracket => depth = depth.saturating_sub(1),
                            P::RBrace if depth == 0 => return Ok((toks, Stop::CloseBrace(start))),
                            P::RBrace => depth -= 1,
                            // `;` never occurs inside an expression, so it
                            // ends the statement even inside brackets.
                            P::Semicolon => return Ok((toks, Stop::Semicolon(start + 1))),
                            _ => {}
                        }
                    }
                    self.pos += text.len();
                    TK::P(p)
                }
            };
            operand_end = match &kind {
                TK::Ident => {
                    let word = self.cx.text.get(start..self.pos).unwrap_or("");
                    !KEYWORDS.contains(&word) || word == "true" || word == "false"
                }
                TK::P(p) => matches!(p, P::RParen | P::RBracket | P::RBrace),
                _ => true,
            };
            toks.push(Tok {
                kind,
                start,
                end: self.pos,
            });
        }
    }

    fn skip_trivia(&mut self) -> Result<(), Diagnostic> {
        loop {
            match self.byte(self.pos) {
                Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0c') => self.pos += 1,
                Some(b'/') if self.byte(self.pos + 1) == Some(b'*') => {
                    let body = self.cx.text.get(self.pos + 2..self.end).unwrap_or("");
                    match body.find("*/") {
                        Some(at) => self.pos += 2 + at + 2,
                        None => {
                            return Err(self.cx.error(
                                Code::E0003,
                                self.pos,
                                self.end,
                                "unterminated comment: `/*` has no `*/`",
                            ));
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// `NUMBER = (DIGITS ["." DIGITS] | "." DIGITS) [UNIT]`, no exponent.
    fn number(&mut self) -> Result<TK, Diagnostic> {
        let start = self.pos;
        let digits = |s: &mut Self| {
            while s.byte(s.pos).is_some_and(|b| b.is_ascii_digit()) {
                s.pos += 1;
            }
        };
        digits(self);
        if self.byte(self.pos) == Some(b'.')
            && self.byte(self.pos + 1).is_some_and(|b| b.is_ascii_digit())
        {
            self.pos += 1;
            digits(self);
        }
        let number_end = self.pos;
        if self.byte(self.pos) == Some(b'%') {
            self.pos += 1;
        } else {
            while self.byte(self.pos).is_some_and(|b| b.is_ascii_alphabetic()) {
                self.pos += 1;
            }
        }
        if self
            .byte(self.pos)
            .is_some_and(|b| b.is_ascii_digit() || b == b'.')
        {
            return Err(self
                .cx
                .error(
                    Code::E0101,
                    start,
                    self.pos + 1,
                    "a number cannot be followed directly by a digit or `.` after its unit",
                )
                .with_note("there is no exponent notation; write `10 % 3` for a remainder"));
        }
        let digits = self.cx.text.get(start..number_end).unwrap_or("");
        let unit = Unit::parse(self.cx.text.get(number_end..self.pos).unwrap_or(""));
        Number::literal(digits, unit)
            .map(TK::Number)
            .map_err(|e| self.cx.error(e.code, start, self.pos, e.message))
    }

    fn string(&mut self) -> Result<TK, Diagnostic> {
        let start = self.pos;
        let mut lexer = self.cx.lexer(start);
        let token = lexer.next(Mode::Weave);
        if lexer.too_deep() {
            return Err(self.cx.too_deep_templates(start, start + 1));
        }
        let end = token.end as usize;
        match token.kind {
            K::String { terminated: true } if end <= self.end => {
                self.pos = end;
                Ok(TK::Str(token))
            }
            K::BadString => Err(self
                .cx
                .error(
                    Code::E0002,
                    start,
                    end,
                    "unterminated string: line break inside a string",
                )
                .with_note("escape the line break with `\\` or close the string")),
            _ => Err(self
                .cx
                .error(Code::E0002, start, start + 1, "unterminated string")),
        }
    }

    fn fragment(&mut self) -> Result<TK, Diagnostic> {
        let open = self.pos;
        let mut lexer = self.cx.lexer(open + 1);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next(Mode::Weave);
            if lexer.too_deep() {
                return Err(self.cx.too_deep_templates(open, open + 1));
            }
            if token.kind == K::Eof || token.end as usize > self.end {
                return Err(self
                    .cx
                    .error(Code::E0007, open, open + 1, "unterminated CSS fragment: missing closing backtick")
                    .with_note("a backtick inside a CSS string does not end the fragment; write `\\60 ` for a literal backtick"));
            }
            if token.kind == K::Delim('`') {
                self.pos = token.end as usize;
                return Ok(TK::Fragment(tokens));
            }
            tokens.push(token);
        }
    }

    /// `#` + 3, 4, 6 or 8 hex digits, not followed by an identifier code
    /// point.
    fn color(&mut self) -> Result<TK, Diagnostic> {
        let start = self.pos;
        if self.byte(start + 1) == Some(b'{') {
            return Err(self
                .cx
                .error(
                    Code::E0101,
                    start,
                    start + 2,
                    "`#{` cannot appear inside an expression",
                )
                .with_note("refer to the name directly: `#{a + b}`, not `#{a + #{b}}`"));
        }
        let mut end = start + 1;
        while self.byte(end).is_some_and(|b| b.is_ascii_hexdigit()) {
            end += 1;
        }
        let digits = self.cx.text.get(start + 1..end).unwrap_or("");
        let color = Color::from_hex(digits);
        match color {
            Some(color) if !self.char_at(end).is_some_and(is_ident_cont) => {
                self.pos = end;
                Ok(TK::Color(color))
            }
            _ => {
                let mut stop = end;
                while self.char_at(stop).is_some_and(is_ident_cont) {
                    stop += self.char_at(stop).map_or(1, char::len_utf8);
                }
                Err(self
                    .cx
                    .error(Code::E0101, start, stop.max(start + 1), "invalid color literal")
                    .with_note("colors are `#` followed by 3, 4, 6 or 8 hex digits; write other colors as a CSS fragment such as `` `rebeccapurple` ``"))
            }
        }
    }
}

// ----- parsing ------------------------------------------------------------------

/// A pending operator on the operator stack.
#[derive(Debug)]
enum Pend {
    Neg(Span),
    Not(Span),
    Bin(BinOp, Span),
    /// `&&` and `||` with the index of their jump instruction.
    And(usize, Span),
    Or(usize, Span),
}

impl Pend {
    fn prec(&self) -> u8 {
        match self {
            Pend::Neg(_) | Pend::Not(_) => 7,
            Pend::Bin(op, _) => op.prec(),
            Pend::And(..) => 2,
            Pend::Or(..) => 1,
        }
    }
}

#[derive(Debug)]
enum BKind {
    Top,
    Paren,
    List,
    /// The keys in source order, and the same keys for duplicate detection.
    Map(Vec<(Rc<str>, Span)>, HashSet<Rc<str>>),
    Index,
    /// The callee, the arguments so far and the names of the named ones.
    Call(Callee, Vec<Arg>, HashSet<Rc<str>>),
}

#[derive(Debug)]
struct Bracket {
    kind: BKind,
    /// The opening bracket token.
    open: Span,
    /// Operator-stack height when the bracket opened.
    base: usize,
    /// Completed items (before the current one).
    count: usize,
    /// Whether the current item has started.
    pending: bool,
}

struct ExprParser<'a, 't> {
    cx: Cx<'a>,
    toks: &'t [Tok],
    next: usize,
    /// Offset used for errors at the end of the expression.
    end: usize,
    ops: Vec<Op>,
    stack: Vec<Pend>,
    brackets: Vec<Bracket>,
}

impl<'t> ExprParser<'_, 't> {
    fn peek(&self) -> Option<&'t Tok> {
        self.toks.get(self.next)
    }

    fn bump(&mut self) -> Option<&'t Tok> {
        let tok = self.toks.get(self.next)?;
        self.next += 1;
        Some(tok)
    }

    fn span(&self, tok: &Tok) -> Span {
        self.cx.span(tok.start, tok.end)
    }

    fn text(&self, tok: &Tok) -> &str {
        self.cx.text.get(tok.start..tok.end).unwrap_or("")
    }

    fn error(&self, code: Code, tok: &Tok, message: impl Into<String>) -> Diagnostic {
        Diagnostic::error(code, message).with_primary(self.span(tok))
    }

    fn at_end(&self, message: &str) -> Diagnostic {
        self.cx.error(Code::E0101, self.end, self.end, message)
    }

    fn unexpected(&self, tok: &Tok, expected: &str) -> Diagnostic {
        let shown: String = self.text(tok).chars().take(32).collect();
        self.error(Code::E0101, tok, format!("{expected}, found `{shown}`"))
    }

    fn top(&mut self) -> Option<&mut Bracket> {
        self.brackets.last_mut()
    }

    fn parse(mut self) -> Result<Program, Diagnostic> {
        self.brackets.push(Bracket {
            kind: BKind::Top,
            open: self.cx.span(self.end, self.end),
            base: 0,
            count: 0,
            pending: false,
        });
        let mut operand = true;
        loop {
            if operand {
                let Some(tok) = self.bump() else {
                    return Err(self.at_end("expected an expression"));
                };
                let closes_empty = matches!(tok.kind, TK::P(P::RParen | P::RBracket))
                    && self.brackets.last().is_some_and(|b| {
                        !b.pending && matches!(b.kind, BKind::List | BKind::Call(..))
                    });
                if closes_empty {
                    self.close(tok)?;
                    operand = false;
                    continue;
                }
                if let Some(b) = self.top() {
                    b.pending = true;
                }
                operand = self.operand(tok)?;
            } else {
                let Some(tok) = self.bump() else {
                    break;
                };
                operand = self.operator(tok)?;
            }
        }
        self.pop_to_base();
        if self.brackets.len() > 1 {
            if let Some(open) = self.brackets.last().map(|b| b.open) {
                return Err(
                    Diagnostic::error(Code::E0005, "unclosed bracket in expression")
                        .with_primary(open),
                );
            }
        }
        Ok(Program { ops: self.ops })
    }

    /// Handles a token where an operand is expected. Returns whether an
    /// operand is still expected.
    fn operand(&mut self, tok: &'t Tok) -> Result<bool, Diagnostic> {
        let span = self.span(tok);
        let op = match &tok.kind {
            TK::Number(n) => Op::Const(Value::Number(n.clone())),
            TK::Color(c) => Op::Const(Value::Color(*c)),
            TK::Ident => {
                let name = self.text(tok);
                match name {
                    "true" => Op::Const(Value::Bool(true)),
                    "false" => Op::Const(Value::Bool(false)),
                    _ if KEYWORDS.contains(&name) => {
                        return Err(self.error(
                            Code::E0103,
                            tok,
                            format!("`{name}` is a keyword and cannot be used as a name"),
                        ));
                    }
                    _ => Op::Load(name.into(), span),
                }
            }
            TK::Str(token) => {
                let template = string_template(self.cx, *token, true)?;
                match template.static_text() {
                    Some(text) => Op::Const(Value::Str(text.into())),
                    None => Op::Str(Box::new(template)),
                }
            }
            TK::Fragment(tokens) => {
                Op::Fragment(Box::new(fragment_template(self.cx, tokens)?), span)
            }
            TK::P(P::LParen) => {
                self.open(BKind::Paren, tok)?;
                return Ok(true);
            }
            TK::P(P::LBracket) => {
                self.open(BKind::List, tok)?;
                return Ok(true);
            }
            TK::P(P::LBrace) => {
                self.open(BKind::Map(Vec::new(), HashSet::new()), tok)?;
                return Ok(!self.map_key()?);
            }
            TK::P(P::Minus) => {
                self.stack.push(Pend::Neg(span));
                return Ok(true);
            }
            TK::P(P::Bang) => {
                self.stack.push(Pend::Not(span));
                return Ok(true);
            }
            TK::P(_) => return Err(self.unexpected(tok, "expected an expression")),
        };
        self.ops.push(op);
        Ok(false)
    }

    /// Handles a token after a complete operand. Returns whether an operand
    /// is expected next.
    fn operator(&mut self, tok: &'t Tok) -> Result<bool, Diagnostic> {
        let span = self.span(tok);
        let TK::P(p) = tok.kind else {
            return Err(self
                .unexpected(tok, "expected an operator")
                .with_note("names containing `-` are one identifier: write `a - b` to subtract"));
        };
        let bin = match p {
            P::Star => Some(BinOp::Mul),
            P::Slash => Some(BinOp::Div),
            P::Percent => Some(BinOp::Rem),
            P::Plus => Some(BinOp::Add),
            P::Minus => Some(BinOp::Sub),
            P::Lt => Some(BinOp::Lt),
            P::Le => Some(BinOp::Le),
            P::Gt => Some(BinOp::Gt),
            P::Ge => Some(BinOp::Ge),
            P::EqEq => Some(BinOp::Eq),
            P::Ne => Some(BinOp::Ne),
            _ => None,
        };
        if let Some(op) = bin {
            self.binary(op, span)?;
            return Ok(true);
        }
        match p {
            P::AndAnd | P::OrOr => {
                let and = p == P::AndAnd;
                self.pop_while(if and { 2 } else { 1 });
                let jump = self.ops.len();
                self.ops.push(if and {
                    Op::JumpIfFalse(0, span)
                } else {
                    Op::JumpIfTrue(0, span)
                });
                self.stack.push(if and {
                    Pend::And(jump, span)
                } else {
                    Pend::Or(jump, span)
                });
                Ok(true)
            }
            P::Dot => match self.bump() {
                Some(key) if matches!(key.kind, TK::Ident) => {
                    let name: Rc<str> = self.text(key).into();
                    self.ops.push(Op::Member(name, self.span(key)));
                    Ok(false)
                }
                Some(other) => Err(self.unexpected(other, "expected a name after `.`")),
                None => Err(self.at_end("expected a name after `.`")),
            },
            P::LBracket => {
                self.open(BKind::Index, tok)?;
                Ok(true)
            }
            P::LParen => {
                let callee = self.take_callee(span)?;
                self.open(BKind::Call(callee, Vec::new(), HashSet::new()), tok)?;
                self.arg_start()?;
                Ok(true)
            }
            P::Comma => self.comma(tok),
            P::RParen | P::RBracket | P::RBrace => {
                self.close(tok)?;
                Ok(false)
            }
            _ => Err(self.unexpected(tok, "expected an operator")),
        }
    }

    fn open(&mut self, kind: BKind, tok: &Tok) -> Result<(), Diagnostic> {
        // `brackets` includes the top level, so its length is the number of
        // open brackets once this one is pushed.
        let depth = u64::from(self.cx.depth) + self.brackets.len() as u64;
        if depth > u64::from(self.cx.max_depth) {
            return Err(self
                .error(
                    Code::E8002,
                    tok,
                    format!(
                        "nesting exceeds the depth limit of {} (blocks and brackets combined)",
                        self.cx.max_depth
                    ),
                )
                .with_note("raise it with the `nesting-depth` limit"));
        }
        self.brackets.push(Bracket {
            kind,
            open: self.span(tok),
            base: self.stack.len(),
            count: 0,
            pending: false,
        });
        Ok(())
    }

    fn emit(&mut self, pend: Pend) {
        match pend {
            Pend::Neg(span) => self.ops.push(Op::Neg(span)),
            Pend::Not(span) => self.ops.push(Op::Not(span)),
            Pend::Bin(op, span) => self.ops.push(Op::Binary(op, span)),
            Pend::And(jump, span) | Pend::Or(jump, span) => {
                let and = matches!(pend, Pend::And(..));
                self.ops.push(Op::AssertBool(span));
                let target = self.ops.len();
                if let Some(slot) = self.ops.get_mut(jump) {
                    *slot = if and {
                        Op::JumpIfFalse(target, span)
                    } else {
                        Op::JumpIfTrue(target, span)
                    };
                }
            }
        }
    }

    fn base(&self) -> usize {
        self.brackets.last().map_or(0, |b| b.base)
    }

    /// Emits pending operators of the current bracket with precedence of
    /// at least `prec`.
    fn pop_while(&mut self, prec: u8) {
        while self.stack.len() > self.base() && self.stack.last().is_some_and(|p| p.prec() >= prec)
        {
            if let Some(pend) = self.stack.pop() {
                self.emit(pend);
            }
        }
    }

    fn pop_to_base(&mut self) {
        self.pop_while(0);
    }

    fn binary(&mut self, op: BinOp, span: Span) -> Result<(), Diagnostic> {
        while self.stack.len() > self.base()
            && self.stack.last().is_some_and(|p| p.prec() >= op.prec())
        {
            if let Some(Pend::Bin(prev, _)) = self.stack.last() {
                if prev.class().is_some() && prev.class() == op.class() {
                    return Err(Diagnostic::error(
                        Code::E0112,
                        format!(
                            "`{}` cannot be chained with `{}`: comparisons are non-associative",
                            op.symbol(),
                            prev.symbol()
                        ),
                    )
                    .with_primary(span)
                    .with_note("combine comparisons with `&&`, for example `a < b && b < c`"));
                }
            }
            if let Some(pend) = self.stack.pop() {
                self.emit(pend);
            }
        }
        self.stack.push(Pend::Bin(op, span));
        Ok(())
    }

    /// Turns the operand just compiled into a call target.
    fn take_callee(&mut self, paren: Span) -> Result<Callee, Diagnostic> {
        let len = self.ops.len();
        match (self.ops.get(len.wrapping_sub(2)), self.ops.last()) {
            (Some(Op::Load(ns, ns_span)), Some(Op::Member(name, name_span))) => {
                let callee = Callee {
                    namespace: Some((Rc::clone(ns), *ns_span)),
                    name: Rc::clone(name),
                    span: Span::new(self.cx.file, ns_span.start, name_span.end),
                };
                self.ops.truncate(len - 2);
                Ok(callee)
            }
            (_, Some(Op::Load(name, span))) => {
                let callee = Callee {
                    namespace: None,
                    name: Rc::clone(name),
                    span: *span,
                };
                self.ops.truncate(len - 1);
                Ok(callee)
            }
            _ => Err(Diagnostic::error(
                Code::E1004,
                "only functions can be called; functions are not values",
            )
            .with_primary(paren)),
        }
    }

    /// At the start of a call argument: records it, with its name if it is
    /// written `name: value`.
    fn arg_start(&mut self) -> Result<(), Diagnostic> {
        let Some(first) = self.peek() else {
            return Ok(());
        };
        if matches!(first.kind, TK::P(P::RParen)) {
            return Ok(());
        }
        let named = matches!(first.kind, TK::Ident)
            && matches!(
                self.toks.get(self.next + 1).map(|t| &t.kind),
                Some(TK::P(P::Colon))
            );
        let span = self.span(first);
        let name: Option<Rc<str>> = named.then(|| self.text(first).into());
        if named {
            self.next += 2;
        }
        let Some(Bracket {
            kind: BKind::Call(_, args, names),
            ..
        }) = self.brackets.last_mut()
        else {
            return Ok(());
        };
        match &name {
            Some(n) => {
                if !names.insert(Rc::clone(n)) {
                    return Err(Diagnostic::error(
                        Code::E4003,
                        format!("argument `{n}` is passed more than once"),
                    )
                    .with_primary(span));
                }
            }
            None => {
                // Named arguments only ever follow positional ones, so the
                // last argument tells whether any is named.
                if args.last().is_some_and(|a| a.name.is_some()) {
                    return Err(Diagnostic::error(
                        Code::E0113,
                        "a positional argument cannot follow a named argument",
                    )
                    .with_primary(span));
                }
            }
        }
        args.push(Arg { name, span });
        Ok(())
    }

    /// At the start of a map entry: consumes `key :`. Returns true if the
    /// map was closed instead (`{}` or a trailing comma).
    fn map_key(&mut self) -> Result<bool, Diagnostic> {
        let Some(tok) = self.bump() else {
            return Err(self.at_end("expected a map key or `}`"));
        };
        let key: Rc<str> = match &tok.kind {
            TK::P(P::RBrace) => {
                self.close(tok)?;
                return Ok(true);
            }
            TK::Ident => self.text(tok).into(),
            TK::Str(token) => string_key(self.cx, *token).into(),
            _ => return Err(self.unexpected(tok, "expected a map key (a name or a string)")),
        };
        match self.bump() {
            Some(colon) if matches!(colon.kind, TK::P(P::Colon)) => {}
            Some(other) => return Err(self.unexpected(other, "expected `:` after the map key")),
            None => return Err(self.at_end("expected `:` after the map key")),
        }
        let span = self.span(tok);
        if let Some(Bracket {
            kind: BKind::Map(keys, seen),
            ..
        }) = self.brackets.last_mut()
        {
            if !seen.insert(Rc::clone(&key)) {
                return Err(
                    Diagnostic::error(Code::E2008, format!("duplicate map key `{key}`"))
                        .with_primary(span),
                );
            }
            keys.push((key, span));
        }
        Ok(false)
    }

    fn comma(&mut self, tok: &Tok) -> Result<bool, Diagnostic> {
        self.pop_to_base();
        let Some(bracket) = self.brackets.last_mut() else {
            return Ok(true);
        };
        if !matches!(bracket.kind, BKind::List | BKind::Map(..) | BKind::Call(..)) {
            return Err(self.unexpected(tok, "expected an operator"));
        }
        bracket.count += 1;
        bracket.pending = false;
        match bracket.kind {
            BKind::Map(..) => Ok(!self.map_key()?),
            BKind::Call(..) => {
                self.arg_start()?;
                Ok(true)
            }
            _ => Ok(true),
        }
    }

    fn close(&mut self, tok: &Tok) -> Result<(), Diagnostic> {
        self.pop_to_base();
        let closer = match tok.kind {
            TK::P(P::RParen) => ")",
            TK::P(P::RBracket) => "]",
            _ => "}",
        };
        let Some(bracket) = self.brackets.pop() else {
            return Err(self.error(Code::E0005, tok, format!("unmatched `{closer}`")));
        };
        let expected = match bracket.kind {
            BKind::Top => {
                return Err(self.error(Code::E0005, tok, format!("unmatched `{closer}`")));
            }
            BKind::Paren | BKind::Call(..) => ")",
            BKind::List | BKind::Index => "]",
            BKind::Map(..) => "}",
        };
        if expected != closer {
            return Err(Diagnostic::error(
                Code::E0005,
                format!("unclosed bracket: expected `{expected}` before `{closer}`"),
            )
            .with_primary(bracket.open));
        }
        let items = bracket.count + usize::from(bracket.pending);
        match bracket.kind {
            BKind::Top | BKind::Paren => {}
            BKind::List => self.ops.push(Op::List(items, bracket.open)),
            BKind::Index => self.ops.push(Op::Index(bracket.open)),
            BKind::Map(keys, _) => self.ops.push(Op::Map(keys.into(), bracket.open)),
            BKind::Call(callee, args, _) => {
                self.ops.push(Op::Call(Box::new(Call { callee, args })))
            }
        }
        Ok(())
    }
}

fn compile(cx: Cx<'_>, toks: &[Tok], end: usize) -> Result<Program, Diagnostic> {
    ExprParser {
        cx,
        toks,
        next: 0,
        end,
        ops: Vec::new(),
        stack: Vec::new(),
        brackets: Vec::new(),
    }
    .parse()
}

// ----- entry points ---------------------------------------------------------------

/// A parsed `bind name = expression;`.
#[derive(Debug)]
pub(crate) struct BindStmt {
    pub name: Rc<str>,
    pub name_span: Span,
    pub program: Program,
    /// The offset after the terminating `;`.
    pub end: usize,
}

/// A `bind` statement that failed to parse.
#[derive(Debug)]
pub(crate) struct BindError {
    pub diagnostic: Diagnostic,
    /// Where parsing can resume: after the statement's `;`, when the
    /// statement was tokenized completely.
    pub resume: Option<usize>,
}

/// Parses the rest of a `bind` statement starting at `start` (after the
/// keyword).
pub(crate) fn parse_bind(cx: Cx<'_>, start: usize) -> Result<BindStmt, Box<BindError>> {
    let mut scanner = Scanner {
        cx,
        pos: start,
        end: cx.text.len(),
    };
    let (toks, stop) = scanner.scan(ScanMode::Statement).map_err(|diagnostic| {
        Box::new(BindError {
            diagnostic,
            resume: None,
        })
    })?;
    let resume = match stop {
        Stop::Semicolon(after) => Some(after),
        Stop::End(_) | Stop::CloseBrace(_) | Stop::OpenBrace(_) => None,
    };
    bind_statement(cx, &toks, stop).map_err(|diagnostic| Box::new(BindError { diagnostic, resume }))
}

fn bind_statement(cx: Cx<'_>, toks: &[Tok], stop: Stop) -> Result<BindStmt, Diagnostic> {
    // Where a missing item is reported: the `;` or `}` that ended the
    // statement, or the end of its last token.
    let missing_at = match stop {
        Stop::Semicolon(after) => after - 1,
        Stop::CloseBrace(at) | Stop::OpenBrace(at) => at,
        Stop::End(at) => toks.last().map_or(at, |t| t.end),
    };
    let (name_tok, rest) = match toks.split_first() {
        Some((tok, rest)) if matches!(tok.kind, TK::Ident) => (tok, rest),
        Some((tok, _)) => {
            return Err(
                Diagnostic::error(Code::E0101, "expected a name after `bind`")
                    .with_primary(cx.span(tok.start, tok.end)),
            );
        }
        None => {
            return Err(cx.error(
                Code::E0101,
                missing_at,
                missing_at,
                "expected a name after `bind`",
            ));
        }
    };
    let name = cx.text.get(name_tok.start..name_tok.end).unwrap_or("");
    let name_span = cx.span(name_tok.start, name_tok.end);
    if KEYWORDS.contains(&name) {
        return Err(Diagnostic::error(
            Code::E0103,
            format!("`{name}` is a keyword and cannot be bound"),
        )
        .with_primary(name_span));
    }
    let rest = match rest.split_first() {
        Some((eq, rest)) if matches!(eq.kind, TK::P(P::Assign)) => rest,
        Some((tok, _)) => {
            return Err(
                Diagnostic::error(Code::E0101, "expected `=` after the bound name")
                    .with_primary(cx.span(tok.start, tok.end)),
            );
        }
        None => {
            return Err(cx.error(
                Code::E0101,
                missing_at,
                missing_at,
                "expected `=` after the bound name",
            ));
        }
    };
    let program = compile(cx, rest, missing_at)?;
    match stop {
        Stop::Semicolon(after) => Ok(BindStmt {
            name: name.into(),
            name_span,
            program,
            end: after,
        }),
        Stop::End(_) | Stop::CloseBrace(_) | Stop::OpenBrace(_) => Err(cx.error(
            Code::E0102,
            missing_at,
            missing_at,
            "expected `;` after the bound expression",
        )),
    }
}

/// How the parser resumes after a statement that failed to parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resume {
    /// Continue at this offset.
    At(usize),
    /// The statement's block starts with the `{` at this offset: parse the
    /// block for its own diagnostics and discard it.
    Block(usize),
    /// The statement could not be tokenized; skip its extent.
    Skip,
}

/// A statement that failed to parse.
#[derive(Debug)]
pub(crate) struct StmtError {
    pub diagnostic: Diagnostic,
    pub resume: Resume,
}

type Stmt<T> = Result<T, Box<StmtError>>;

fn stmt_error(diagnostic: Diagnostic, resume: Resume) -> Box<StmtError> {
    Box::new(StmtError { diagnostic, resume })
}

/// Where a statement that stopped at `stop` resumes.
fn resume_at(stop: Stop) -> Resume {
    match stop {
        Stop::Semicolon(at) | Stop::CloseBrace(at) | Stop::End(at) => Resume::At(at),
        Stop::OpenBrace(at) => Resume::Block(at),
    }
}

/// The span from the first to the last token.
fn toks_span(cx: Cx<'_>, toks: &[Tok]) -> Option<Span> {
    Some(cx.span(toks.first()?.start, toks.last()?.end))
}

/// Where an item missing at the end of a statement is reported.
fn missing_at(toks: &[Tok], stop: Stop) -> usize {
    match stop {
        Stop::Semicolon(after) => after - 1,
        Stop::CloseBrace(at) | Stop::OpenBrace(at) => at,
        Stop::End(at) => toks.last().map_or(at, |t| t.end),
    }
}

/// The expression of `yield`, `fail` or `warn`.
#[derive(Debug)]
pub(crate) struct ExprStmt {
    pub program: Program,
    /// The offset after the terminating `;`.
    pub end: usize,
}

/// Parses `expression ;` starting at `start` (after `keyword`).
pub(crate) fn parse_expr_stmt(cx: Cx<'_>, start: usize, keyword: &str) -> Stmt<ExprStmt> {
    let mut scanner = Scanner {
        cx,
        pos: start,
        end: cx.text.len(),
    };
    let (toks, stop) = scanner
        .scan(ScanMode::Statement)
        .map_err(|d| stmt_error(d, Resume::Skip))?;
    let resume = resume_at(stop);
    let at = missing_at(&toks, stop);
    if toks.is_empty() {
        return Err(stmt_error(
            cx.error(
                Code::E0101,
                at,
                at,
                format!("expected an expression after `{keyword}`"),
            ),
            resume,
        ));
    }
    let program = compile(cx, &toks, at).map_err(|d| stmt_error(d, resume))?;
    match stop {
        Stop::Semicolon(end) => Ok(ExprStmt { program, end }),
        _ => Err(stmt_error(
            cx.error(
                Code::E0102,
                at,
                at,
                format!("expected `;` after the `{keyword}` expression"),
            ),
            resume,
        )),
    }
}

/// Scans a statement head up to its block (or `;` for `apply`).
fn scan_head(cx: Cx<'_>, start: usize) -> Stmt<(Vec<Tok>, Stop)> {
    let mut scanner = Scanner {
        cx,
        pos: start,
        end: cx.text.len(),
    };
    scanner
        .scan(ScanMode::Head)
        .map_err(|d| stmt_error(d, Resume::Skip))
}

/// The offset of the head's `{`, or an error that the block is missing.
fn head_block(cx: Cx<'_>, toks: &[Tok], stop: Stop, what: &str) -> Stmt<usize> {
    match stop {
        Stop::OpenBrace(at) => Ok(at),
        _ => {
            let at = missing_at(toks, stop);
            Err(stmt_error(
                cx.error(Code::E0101, at, at, format!("expected a `{{` block {what}")),
                resume_at(stop),
            ))
        }
    }
}

/// A `when` condition up to its block.
#[derive(Debug)]
pub(crate) struct WhenHead {
    pub condition: Program,
    pub span: Span,
    /// The offset of the block's `{`.
    pub open: usize,
}

/// Parses a condition and its `{` starting at `start` (after `when`).
pub(crate) fn parse_when_head(cx: Cx<'_>, start: usize) -> Stmt<WhenHead> {
    let (toks, stop) = scan_head(cx, start)?;
    let open = head_block(cx, &toks, stop, "after the `when` condition")?;
    let Some(span) = toks_span(cx, &toks) else {
        return Err(stmt_error(
            cx.error(
                Code::E0101,
                open,
                open + 1,
                "expected a condition after `when`",
            ),
            Resume::Block(open),
        ));
    };
    let condition = compile(cx, &toks, open).map_err(|d| stmt_error(d, Resume::Block(open)))?;
    Ok(WhenHead {
        condition,
        span,
        open,
    })
}

/// `each value in expression {` or `each key, value in expression {`.
#[derive(Debug)]
pub(crate) struct EachHead {
    pub key: Option<(Rc<str>, Span)>,
    pub value: (Rc<str>, Span),
    pub iterable: Program,
    pub span: Span,
    pub open: usize,
}

/// A name that a statement binds: a loop variable or a parameter.
fn bound_name(
    cx: Cx<'_>,
    tok: Option<&Tok>,
    what: &str,
    at: usize,
) -> Result<(Rc<str>, Span), Diagnostic> {
    match tok {
        Some(tok) if matches!(tok.kind, TK::Ident) => {
            let name = cx.text.get(tok.start..tok.end).unwrap_or("");
            let span = cx.span(tok.start, tok.end);
            if KEYWORDS.contains(&name) {
                return Err(Diagnostic::error(
                    Code::E0103,
                    format!("`{name}` is a keyword and cannot be used as a name"),
                )
                .with_primary(span));
            }
            Ok((name.into(), span))
        }
        Some(tok) => Err(cx.error(Code::E0101, tok.start, tok.end, format!("expected {what}"))),
        None => Err(cx.error(Code::E0101, at, at, format!("expected {what}"))),
    }
}

pub(crate) fn duplicate_name(name: &str, span: Span, earlier: Span) -> Diagnostic {
    Diagnostic::error(
        Code::E1001,
        format!("duplicate name: `{name}` is already bound in this scope"),
    )
    .with_primary(span)
    .with_label(earlier, "first bound here")
}

/// Parses loop variables, `in`, the iterated expression and the `{`
/// starting at `start` (after `each`).
pub(crate) fn parse_each_head(cx: Cx<'_>, start: usize) -> Stmt<EachHead> {
    let (toks, stop) = scan_head(cx, start)?;
    let open = head_block(cx, &toks, stop, "after the `each` expression")?;
    let fail = |d: Diagnostic| stmt_error(d, Resume::Block(open));
    let first =
        bound_name(cx, toks.first(), "a loop variable name after `each`", open).map_err(fail)?;
    let mut next = 1;
    let mut key = None;
    let mut value = first;
    if toks
        .get(1)
        .is_some_and(|t| matches!(t.kind, TK::P(P::Comma)))
    {
        let second =
            bound_name(cx, toks.get(2), "a second loop variable name", open).map_err(fail)?;
        if second.0 == value.0 {
            return Err(fail(duplicate_name(&second.0, second.1, value.1)));
        }
        key = Some(value);
        value = second;
        next = 3;
    }
    let expected_in = |start: usize, end: usize| {
        fail(cx.error(
            Code::E0101,
            start,
            end,
            "expected `in` after the loop variables",
        ))
    };
    match toks.get(next) {
        Some(tok)
            if matches!(tok.kind, TK::Ident) && cx.text.get(tok.start..tok.end) == Some("in") => {}
        Some(tok) => return Err(expected_in(tok.start, tok.end)),
        None => return Err(expected_in(open, open + 1)),
    }
    let rest = toks.get(next + 1..).unwrap_or(&[]);
    let Some(span) = toks_span(cx, rest) else {
        return Err(fail(cx.error(
            Code::E0101,
            open,
            open + 1,
            "expected an expression after `in`",
        )));
    };
    let iterable = compile(cx, rest, open).map_err(fail)?;
    Ok(EachHead {
        key,
        value,
        iterable,
        span,
        open,
    })
}

/// `apply name(args)` followed by `;` or a content block.
#[derive(Debug)]
pub(crate) struct ApplyHead {
    pub call: Call,
    /// Computes one value per argument.
    pub args: Program,
    /// The offset of the content block's `{`, if there is one.
    pub open: Option<usize>,
    /// The offset after the `;` when there is no content block.
    pub end: usize,
}

/// Parses the recipe, its arguments and the `;` or `{` that follows,
/// starting at `start` (after `apply`).
pub(crate) fn parse_apply_head(cx: Cx<'_>, start: usize) -> Stmt<ApplyHead> {
    let (toks, stop) = scan_head(cx, start)?;
    let (open, end, resume) = match stop {
        Stop::OpenBrace(at) => (Some(at), at, Resume::Block(at)),
        Stop::Semicolon(after) => (None, after, Resume::At(after)),
        Stop::CloseBrace(_) | Stop::End(_) => {
            let at = missing_at(&toks, stop);
            return Err(stmt_error(
                cx.error(
                    Code::E0102,
                    at,
                    at,
                    "expected `;` or a content block after `apply`",
                ),
                resume_at(stop),
            ));
        }
    };
    let fail = |d: Diagnostic| stmt_error(d, resume);
    let at = missing_at(&toks, stop);
    let (Some(first), Some(last)) = (toks.first(), toks.last()) else {
        return Err(fail(cx.error(
            Code::E0101,
            at,
            at,
            "expected a recipe name after `apply`",
        )));
    };
    let not_recipe = || {
        cx.error(
            Code::E0101,
            first.start,
            last.end,
            "expected a recipe name, optionally followed by arguments",
        )
        .with_note("write `apply name;`, `apply name(args);` or `apply namespace.name(args);`")
    };
    let mut program = compile(cx, &toks, at).map_err(fail)?;
    let call = match program.ops.pop() {
        Some(Op::Call(call)) => *call,
        Some(Op::Load(name, span)) if program.ops.is_empty() => Call {
            callee: Callee {
                namespace: None,
                name,
                span,
            },
            args: Vec::new(),
        },
        Some(Op::Member(name, name_span)) => match program.ops.pop() {
            Some(Op::Load(namespace, ns_span)) if program.ops.is_empty() => Call {
                callee: Callee {
                    namespace: Some((namespace, ns_span)),
                    name,
                    span: Span::new(cx.file, ns_span.start, name_span.end),
                },
                args: Vec::new(),
            },
            _ => return Err(fail(not_recipe())),
        },
        _ => return Err(fail(not_recipe())),
    };
    Ok(ApplyHead {
        call,
        args: program,
        open,
        end,
    })
}

/// The head of a `fn` or `recipe` definition up to its block.
#[derive(Debug)]
pub(crate) struct DefHead {
    pub name: Rc<str>,
    pub span: Span,
    pub params: Vec<Param>,
    pub returns: Option<(Type, Span)>,
    pub open: usize,
}

fn type_name(cx: Cx<'_>, tok: Option<&Tok>, at: usize) -> Result<(Type, Span), Diagnostic> {
    let Some(tok) = tok else {
        return Err(cx.error(Code::E0101, at, at, "expected a type name"));
    };
    let text = cx.text.get(tok.start..tok.end).unwrap_or("");
    let span = cx.span(tok.start, tok.end);
    match (&tok.kind, Type::parse(text)) {
        (TK::Ident, Some(ty)) => Ok((ty, span)),
        _ => Err(Diagnostic::error(
            Code::E0101,
            format!(
                "unknown type `{}`",
                text.chars().take(32).collect::<String>()
            ),
        )
        .with_primary(span)
        .with_note(
            "the types are any, number, unitless, length, angle, time, percent, string, color, bool, list, map and css",
        )),
    }
}

/// Parses `fn name(params) -> type {` (`function`) or `recipe name(params)
/// {` (parameter list optional) starting after the keyword.
pub(crate) fn parse_def_head(cx: Cx<'_>, start: usize, function: bool) -> Stmt<DefHead> {
    let (toks, stop) = scan_head(cx, start)?;
    let what = if function { "function" } else { "recipe" };
    let open = head_block(cx, &toks, stop, &format!("for the {what} body"))?;
    let fail = |d: Diagnostic| stmt_error(d, Resume::Block(open));
    let unexpected = |tok: Option<&Tok>, message: &str| {
        let (start, end) = tok.map_or((open, open + 1), |t| (t.start, t.end));
        fail(cx.error(Code::E0101, start, end, message))
    };
    let (name, span) =
        bound_name(cx, toks.first(), &format!("a {what} name"), open).map_err(fail)?;
    let mut params: Vec<Param> = Vec::new();
    let mut seen: HashMap<Rc<str>, Span> = HashMap::new();
    let mut i = 1;
    if toks
        .get(i)
        .is_some_and(|t| matches!(t.kind, TK::P(P::LParen)))
    {
        i += 1;
        loop {
            if toks
                .get(i)
                .is_some_and(|t| matches!(t.kind, TK::P(P::RParen)))
            {
                i += 1;
                break;
            }
            let (pname, pspan) =
                bound_name(cx, toks.get(i), "a parameter name or `)`", open).map_err(fail)?;
            if let Some(&earlier) = seen.get(&pname) {
                return Err(fail(duplicate_name(&pname, pspan, earlier)));
            }
            seen.insert(Rc::clone(&pname), pspan);
            i += 1;
            let mut ty = None;
            if toks
                .get(i)
                .is_some_and(|t| matches!(t.kind, TK::P(P::Colon)))
            {
                ty = Some(type_name(cx, toks.get(i + 1), open).map_err(fail)?.0);
                i += 2;
            }
            let mut default = None;
            if toks
                .get(i)
                .is_some_and(|t| matches!(t.kind, TK::P(P::Assign)))
            {
                // The default runs to a `,` or `)` at depth 0.
                i += 1;
                let from = i;
                let mut depth = 0usize;
                while let Some(t) = toks.get(i) {
                    match t.kind {
                        TK::P(P::LParen | P::LBracket | P::LBrace) => depth += 1,
                        TK::P(P::RParen | P::Comma) if depth == 0 => break,
                        TK::P(P::RParen | P::RBracket | P::RBrace) => {
                            depth = depth.saturating_sub(1);
                        }
                        _ => {}
                    }
                    i += 1;
                }
                let slice = toks.get(from..i).unwrap_or(&[]);
                let at = toks.get(i).map_or(open, |t| t.start);
                default = Some(compile(cx, slice, at).map_err(fail)?);
            }
            params.push(Param {
                name: pname,
                span: pspan,
                ty,
                default,
            });
            match toks.get(i).map(|t| &t.kind) {
                Some(TK::P(P::Comma)) => i += 1,
                Some(TK::P(P::RParen)) => {
                    i += 1;
                    break;
                }
                _ => {
                    return Err(unexpected(
                        toks.get(i),
                        "expected `,` or `)` after the parameter",
                    ));
                }
            }
        }
    } else if function {
        return Err(unexpected(
            toks.get(i),
            "expected `(` after the function name",
        ));
    }
    let mut returns = None;
    if function
        && toks
            .get(i)
            .is_some_and(|t| matches!(t.kind, TK::P(P::Arrow)))
    {
        returns = Some(type_name(cx, toks.get(i + 1), open).map_err(fail)?);
        i += 2;
    }
    if let Some(t) = toks.get(i) {
        let expected = if function {
            "expected `->` or the function body"
        } else {
            "expected the recipe body"
        };
        return Err(unexpected(Some(t), expected));
    }
    Ok(DefHead {
        name,
        span,
        params,
        returns,
        open,
    })
}

/// Parses a terminated `#{…}` token.
pub(crate) fn interpolation(cx: Cx<'_>, token: Token, ctx: Ctx) -> Result<Interp, Diagnostic> {
    let span = cx.token_span(token);
    let start = token.start as usize;
    if cx.nesting >= MAX_TEMPLATE_NESTING {
        return Err(cx.too_deep_templates(start, start + 2));
    }
    let inner = Cx {
        nesting: cx.nesting + 1,
        ..cx
    };
    let end = (token.end as usize).saturating_sub(1);
    let mut scanner = Scanner {
        cx: inner,
        pos: start + 2,
        end,
    };
    let (toks, _) = scanner.scan(ScanMode::Region)?;
    if toks.is_empty() {
        return Err(
            Diagnostic::error(Code::E0101, "empty interpolation: expected an expression")
                .with_primary(span),
        );
    }
    let program = compile(inner, &toks, end)?;
    Ok(Interp { program, ctx, span })
}

/// Decodes a Weave string literal (TYPES.md §3). With `interpolate`, each
/// `#{…}` becomes an interpolation in the text context; otherwise (map
/// keys) it is ordinary text.
fn string_template(cx: Cx<'_>, token: Token, interpolate: bool) -> Result<Template, Diagnostic> {
    let text = cx.text;
    let start = token.start as usize + 1;
    let end = (token.end as usize).saturating_sub(1).max(start);
    let mut template = Template::default();
    let mut buf = String::new();
    let mut i = start;
    while i < end {
        let Some(c) = text.get(i..end).and_then(|r| r.chars().next()) else {
            break;
        };
        match c {
            '\\' => {
                i += 1;
                let Some(next) = text.get(i..end).and_then(|r| r.chars().next()) else {
                    break;
                };
                if next.is_ascii_hexdigit() {
                    let mut value = 0u32;
                    let mut count = 0;
                    while count < 6 {
                        match text.as_bytes().get(i).filter(|_| i < end) {
                            Some(&b) if b.is_ascii_hexdigit() => {
                                value = value * 16 + char::from(b).to_digit(16).unwrap_or(0);
                                i += 1;
                                count += 1;
                            }
                            _ => break,
                        }
                    }
                    match text.get(i..end) {
                        Some(r) if r.starts_with("\r\n") => i += 2,
                        Some(r) if r.starts_with([' ', '\t', '\n', '\r', '\x0c']) => i += 1,
                        _ => {}
                    }
                    buf.push(match value {
                        0 => '\u{fffd}',
                        v => char::from_u32(v).unwrap_or('\u{fffd}'),
                    });
                } else if let Some(r) = text.get(i..end).filter(|r| r.starts_with("\r\n")) {
                    i += r.len().min(2);
                } else if matches!(next, '\n' | '\r' | '\x0c') {
                    i += 1;
                } else {
                    buf.push(next);
                    i += next.len_utf8();
                }
            }
            '#' if interpolate && text.as_bytes().get(i + 1) == Some(&b'{') => {
                let mut lexer = cx.lexer(i);
                let tok = lexer.next(Mode::Weave);
                if lexer.too_deep() {
                    return Err(cx.too_deep_templates(i, i + 2));
                }
                if tok.kind != (K::Interpolation { terminated: true }) {
                    return Err(cx.error(
                        Code::E0004,
                        i,
                        i + 2,
                        "unterminated interpolation: `#{` has no `}`",
                    ));
                }
                template.push_text(&buf);
                buf.clear();
                template
                    .pieces
                    .push(Piece::Interp(Box::new(interpolation(cx, tok, Ctx::Text)?)));
                i = tok.end as usize;
            }
            c => {
                buf.push(c);
                i += c.len_utf8();
            }
        }
    }
    template.push_text(&buf);
    Ok(template)
}

/// A map-literal string key: decoded, never interpolated.
fn string_key(cx: Cx<'_>, token: Token) -> String {
    string_template(cx, token, false)
        .ok()
        .and_then(|t| t.static_text().map(str::to_string))
        .unwrap_or_default()
}

/// A fragment literal (TYPES.md §7): checked for well-formed CSS, trimmed,
/// interpolated in the value context.
fn fragment_template(cx: Cx<'_>, tokens: &[Token]) -> Result<Template, Diagnostic> {
    let mut stack: Vec<Token> = Vec::new();
    for &token in tokens {
        let span = || cx.token_span(token);
        let invalid = |what: &str| {
            Diagnostic::error(Code::E2006, format!("invalid CSS fragment: {what}"))
                .with_primary(span())
        };
        match token.kind {
            K::OpenBrace | K::CloseBrace => {
                return Err(invalid("`{` and `}` are not allowed in a fragment"));
            }
            K::Semicolon => return Err(invalid("`;` is not allowed in a fragment")),
            K::OpenParen | K::Function | K::OpenSquare => stack.push(token),
            K::CloseParen | K::CloseSquare => {
                let open = if token.kind == K::CloseParen {
                    matches!(
                        stack.last().map(|t| t.kind),
                        Some(K::OpenParen | K::Function)
                    )
                } else {
                    matches!(stack.last().map(|t| t.kind), Some(K::OpenSquare))
                };
                if open {
                    stack.pop();
                } else if let Some(inner) = stack.last() {
                    return Err(
                        Diagnostic::error(Code::E0005, "unclosed bracket in fragment")
                            .with_primary(cx.token_span(*inner)),
                    );
                } else {
                    return Err(Diagnostic::error(
                        Code::E0005,
                        "unmatched closing bracket in fragment",
                    )
                    .with_primary(span()));
                }
            }
            K::BadString => {
                return Err(Diagnostic::error(
                    Code::E0002,
                    "unterminated string: line break inside a string",
                )
                .with_primary(span()));
            }
            K::BadUrl => return Err(invalid("malformed `url(…)`")),
            K::Delim('\\') => return Err(invalid("a lone `\\` is not a valid escape")),
            K::Interpolation { terminated: false } => {
                return Err(Diagnostic::error(
                    Code::E0004,
                    "unterminated interpolation: `#{` has no `}`",
                )
                .with_primary(span()));
            }
            _ => {}
        }
    }
    if let Some(open) = stack.last() {
        return Err(
            Diagnostic::error(Code::E0005, "unclosed bracket in fragment")
                .with_primary(cx.token_span(*open)),
        );
    }
    let first = tokens.iter().position(|t| t.kind != K::Whitespace);
    let last = tokens.iter().rposition(|t| t.kind != K::Whitespace);
    let trimmed = match (first, last) {
        (Some(a), Some(b)) => tokens.get(a..=b).unwrap_or(&[]),
        _ => &[],
    };
    let mut segs = Vec::new();
    for &token in trimmed {
        crate::css_text::push_token(&mut segs, token, cx.text);
    }
    css_template(cx, segs, Ctx::Value)
}

/// Builds the template of a CSS region (declaration value, name, selector,
/// prelude, fragment) from its segments. Interpolations take `ctx`, except
/// inside quoted strings (string context) and `url(…)` (url context).
pub(crate) fn css_template(cx: Cx<'_>, segs: Vec<Seg>, ctx: Ctx) -> Result<Template, Diagnostic> {
    let mut template = Template::default();
    for seg in segs {
        match seg {
            Seg::Text(text) => template.push_owned(text),
            Seg::Token(token) => match token.kind {
                K::Interpolation { terminated: true } => {
                    let interp = interpolation(cx, token, ctx)?;
                    template.pieces.push(Piece::Interp(Box::new(interp)));
                }
                K::Interpolation { terminated: false } => {
                    return Err(Diagnostic::error(
                        Code::E0004,
                        "unterminated interpolation: `#{` has no `}`",
                    )
                    .with_primary(cx.token_span(token)));
                }
                K::String { .. } | K::BadString => {
                    let quote = if cx.text.as_bytes().get(token.start as usize) == Some(&b'\'') {
                        '\''
                    } else {
                        '"'
                    };
                    split_quoted(cx, token, Ctx::Str(quote), &mut template)?;
                }
                K::Url { .. } => split_quoted(cx, token, Ctx::Url, &mut template)?,
                _ => {
                    let mut text = String::new();
                    render_token(token, cx.text, &mut text);
                    template.push_text(&text);
                }
            },
        }
    }
    Ok(template)
}

/// Splits a CSS string or URL token at its interpolations. The text around
/// them is kept as written (escapes included), with line breaks written as
/// LF and literal markers as `#{`.
fn split_quoted(
    cx: Cx<'_>,
    token: Token,
    ctx: Ctx,
    template: &mut Template,
) -> Result<(), Diagnostic> {
    let text = cx.text;
    let end = token.end as usize;
    let mut i = token.start as usize;
    let mut seg_start = i;
    let flush = |template: &mut Template, from: usize, to: usize| {
        let raw = text.get(from..to).unwrap_or("");
        let mut piece = String::new();
        push_quoted(&mut piece, raw);
        template.push_owned(piece);
    };
    while i < end {
        let Some(c) = text.get(i..end).and_then(|r| r.chars().next()) else {
            break;
        };
        if c == '\\' {
            i += 1 + text
                .get(i + 1..end)
                .and_then(|r| r.chars().next())
                .map_or(0, char::len_utf8);
        } else if c == '#' && text.as_bytes().get(i + 1) == Some(&b'{') {
            flush(template, seg_start, i);
            let mut lexer = cx.lexer(i);
            let tok = lexer.next(Mode::Weave);
            if lexer.too_deep() {
                return Err(cx.too_deep_templates(i, i + 2));
            }
            if tok.kind != (K::Interpolation { terminated: true }) || tok.end as usize > end {
                return Err(cx.error(
                    Code::E0004,
                    i,
                    i + 2,
                    "unterminated interpolation: `#{` has no `}`",
                ));
            }
            template
                .pieces
                .push(Piece::Interp(Box::new(interpolation(cx, tok, ctx)?)));
            i = tok.end as usize;
            seg_start = i;
        } else {
            i += c.len_utf8();
        }
    }
    flush(template, seg_start, end);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx(text: &str) -> Cx<'_> {
        Cx {
            text,
            file: crate::source::SourceManager::new()
                .add("t.weave", Vec::new())
                .unwrap(),
            max_depth: 128,
            depth: 0,
            nesting: 0,
        }
    }

    /// The program for `bind x = <expr>;` in a compact postfix notation.
    fn rpn(expr: &str) -> Result<String, Code> {
        let source = format!("bind x = {expr};");
        let stmt = parse_bind(cx(&source), 4).map_err(|e| e.diagnostic.code())?;
        let parts: Vec<String> = stmt
            .program
            .ops
            .iter()
            .map(|op| match op {
                Op::Const(v) => v.css_text().unwrap_or_else(|| v.type_name().to_string()),
                Op::Str(_) => "str".into(),
                Op::Fragment(..) => "frag".into(),
                Op::Load(n, _) => n.to_string(),
                Op::Neg(_) => "neg".into(),
                Op::Not(_) => "not".into(),
                Op::Binary(op, _) => op.symbol().into(),
                Op::JumpIfFalse(t, _) => format!("jf{t}"),
                Op::JumpIfTrue(t, _) => format!("jt{t}"),
                Op::AssertBool(_) => "bool".into(),
                Op::List(n, _) => format!("list{n}"),
                Op::Map(k, _) => format!(
                    "map[{}]",
                    k.iter()
                        .map(|(k, _)| k.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                Op::Member(k, _) => format!(".{k}"),
                Op::Index(_) => "[]".into(),
                Op::Call(call) => format!(
                    "call:{}{}/{}",
                    call.callee
                        .namespace
                        .as_ref()
                        .map(|(n, _)| format!("{n}."))
                        .unwrap_or_default(),
                    call.callee.name,
                    call.args
                        .iter()
                        .map(|a| a.name.as_deref().unwrap_or("_").to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            })
            .collect();
        Ok(parts.join(" "))
    }

    #[test]
    fn precedence_and_associativity() {
        assert_eq!(rpn("2 + 3 * 4").unwrap(), "2 3 4 * +");
        assert_eq!(rpn("(2 + 3) * 4").unwrap(), "2 3 + 4 *");
        assert_eq!(rpn("10 - 4 - 3").unwrap(), "10 4 - 3 -");
        assert_eq!(rpn("-2 * -3").unwrap(), "2 neg 3 neg *");
        assert_eq!(rpn("-a.b").unwrap(), "a .b neg");
        assert_eq!(rpn("!a == b").unwrap(), "a not b ==");
        assert_eq!(rpn("a < b == c").unwrap(), "a b < c ==");
        assert_eq!(rpn("a == b < c").unwrap(), "a b c < ==");
        assert_eq!(rpn("x[1 + 2].k").unwrap(), "x 1 2 + [] .k");
    }

    #[test]
    fn short_circuit_compiles_to_jumps() {
        // Jumps skip the right operand and its bool check.
        assert_eq!(rpn("a && b").unwrap(), "a jf4 b bool");
        assert_eq!(rpn("a || b && c").unwrap(), "a jt7 b jf6 c bool bool");
        assert_eq!(rpn("a && b || c").unwrap(), "a jf4 b bool jt7 c bool");
    }

    #[test]
    fn collections_and_calls() {
        assert_eq!(rpn("[]").unwrap(), "list0");
        assert_eq!(rpn("[1, [2,], ]").unwrap(), "1 2 list1 list2");
        assert_eq!(rpn("{}").unwrap(), "map[]");
        assert_eq!(rpn("{a: 1, \"b-c\": 2,}").unwrap(), "1 2 map[a,b-c]");
        assert_eq!(
            rpn("css.space([1px])").unwrap(),
            "1px list1 call:css.space/_"
        );
        assert_eq!(rpn("f()").unwrap(), "call:f/");
        assert_eq!(rpn("f(1, b: 2,)").unwrap(), "1 2 call:f/_,b");
        assert_eq!(rpn("a && f(x)").unwrap(), "a jf5 x call:f/_ bool");
        assert_eq!(rpn("{\"#{x}\": 1}").unwrap(), "1 map[#{x}]");
    }

    #[test]
    fn literals() {
        assert_eq!(rpn("#2563EB").unwrap(), "#2563eb");
        assert_eq!(rpn(".5em").unwrap(), "0.5em");
        assert_eq!(rpn("10 % 3").unwrap(), "10 3 %");
        assert_eq!(rpn("50%").unwrap(), "50%");
        assert_eq!(rpn("'it\\27 s'").unwrap(), "\"it's\"");
        assert_eq!(rpn("\"a\\\nb\\#{c}\"").unwrap(), "\"ab#{c}\"");
        assert_eq!(rpn("\"x#{y}\"").unwrap(), "str");
        assert_eq!(rpn("`1px solid`").unwrap(), "frag");
        assert_eq!(rpn("gap-1 - 1").unwrap(), "gap-1 1 -");
        assert_eq!(rpn("ünits").unwrap(), "ünits");
    }

    #[test]
    fn syntax_errors() {
        let cases = [
            ("a < b < c", Code::E0112),
            ("a == b != c", Code::E0112),
            ("a < b + 1 <= c", Code::E0112),
            ("f(a: 1, 2)", Code::E0113),
            ("f(a: 1, a: 2)", Code::E4003),
            ("{a: 1, a: 2}", Code::E2008),
            ("10%3", Code::E0101),
            ("1e3", Code::E0101),
            ("1.5.5", Code::E0101),
            ("#abcde", Code::E0101),
            ("#fffz", Code::E0101),
            ("#{a}", Code::E0101),
            ("a b", Code::E0101),
            ("(1", Code::E0005),
            ("[1)", Code::E0005),
            ("1)", Code::E0005),
            ("", Code::E0101),
            ("()", Code::E0101),
            ("[,]", Code::E0101),
            ("(1, 2)", Code::E0101),
            ("{a 1}", Code::E0101),
            ("rule", Code::E0103),
            ("x[0](1)", Code::E1004),
            ("\"abc", Code::E0002),
            ("`a", Code::E0007),
            ("`{}`", Code::E2006),
            ("`a;b`", Code::E2006),
            ("`(a`", Code::E0005),
            ("`a)`", Code::E0005),
            // `\` + backtick is a CSS escape, so the fragment never ends.
            ("`a\\`", Code::E0007),
            ("`a\\\n`", Code::E2006),
            ("1 /* x", Code::E0003),
            ("a & b", Code::E0101),
            ("\\61", Code::E0101),
            ("1000000000001", Code::E3002),
        ];
        for (expr, code) in cases {
            assert_eq!(rpn(expr), Err(code), "{expr:?}");
        }
    }

    #[test]
    fn bind_statement_shape() {
        let source = "bind gap = 4px; rest";
        let stmt = parse_bind(cx(source), 4).unwrap();
        assert_eq!(&*stmt.name, "gap");
        assert_eq!(stmt.end, 15);
        let code = |s: &str| parse_bind(cx(s), 4).unwrap_err().diagnostic.code();
        assert_eq!(code("bind true = 1;"), Code::E0103);
        assert_eq!(code("bind 1 = 1;"), Code::E0101);
        assert_eq!(code("bind x 1;"), Code::E0101);
        assert_eq!(code("bind x = 1 }"), Code::E0102);
        assert_eq!(code("bind x = 1"), Code::E0102);
        let ok = parse_bind(cx("bind m = {a: \";}\"}; x"), 4).unwrap();
        assert_eq!(ok.end, 19);
        // A complete statement resumes after its `;`; a tokenizer error
        // leaves recovery to the caller.
        let err = parse_bind(cx("bind x = `a;b`; y"), 4).unwrap_err();
        assert_eq!((err.diagnostic.code(), err.resume), (Code::E2006, Some(15)));
        let err = parse_bind(cx("bind x = 10%3; y"), 4).unwrap_err();
        assert_eq!(err.resume, None);
        let err = parse_bind(cx("bind x = 1\n"), 4).unwrap_err();
        assert_eq!(err.diagnostic.primary().map(|s| s.start), Some(10));
    }

    #[test]
    fn deep_brackets_use_the_nesting_limit() {
        let deep = |n: usize| format!("bind x = {}1{};", "(".repeat(n), ")".repeat(n));
        let mut context = cx("");
        context.max_depth = 8;
        let text = deep(8);
        assert!(
            parse_bind(
                Cx {
                    text: &text,
                    ..context
                },
                4
            )
            .is_ok()
        );
        let text = deep(9);
        assert_eq!(
            parse_bind(
                Cx {
                    text: &text,
                    ..context
                },
                4
            )
            .unwrap_err()
            .diagnostic
            .code(),
            Code::E8002
        );
        // The parser itself does not recurse per bracket.
        let text = deep(100_000);
        context.max_depth = u32::MAX;
        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                parse_bind(
                    Cx {
                        text: &text,
                        ..context
                    },
                    4,
                )
                .is_ok()
            })
            .unwrap();
        assert!(handle.join().unwrap());
    }

    #[test]
    fn template_nesting_is_bounded() {
        let nested = |n: usize| {
            let mut s = String::from("1");
            for _ in 0..n {
                s = format!("\"#{{{s}}}\"");
            }
            format!("bind x = {s};")
        };
        let ok = nested(MAX_TEMPLATE_NESTING as usize);
        assert!(parse_bind(cx(&ok), 4).is_ok());
        let deep = nested(MAX_TEMPLATE_NESTING as usize + 1);
        assert_eq!(
            parse_bind(cx(&deep), 4).unwrap_err().diagnostic.code(),
            Code::E8002
        );
    }
}
