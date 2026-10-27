//! Parser for the plain-CSS subset of Weave.
//!
//! Implements item classification (LANGUAGE.md §3.1), declarations (§3.2),
//! `rule` at stylesheet level (§11), the at-rule matrix and context matrix
//! (CSS-COMPATIBILITY.md §2, §3), `opaque` blocks and unknown at-rules (§13)
//! and `/*!` comments.
//!
//! Compile-time constructs that this build does not implement — `bind`,
//! `when`, `each`, `apply`, `fn`, `recipe`, `load`, `export`, `fail`, `warn`,
//! `#{…}` interpolation, nested `rule`s and group at-rules nested in rules —
//! are reported as E5001 and never silently dropped. Their extent is skipped
//! so that parsing continues and later independent errors are reported.
//!
//! Recovery is bounded: every step consumes at least one token, bracket and
//! block nesting share the nesting-depth limit (E8002, which stops parsing),
//! and parsing stops once the diagnostic limit is reached.

use std::collections::HashSet;

use crate::css_text::{
    normalize_newlines, normalize_prelude, normalize_selector, render, trim_css_whitespace,
};
use crate::diagnostic::{Code, Diagnostic, Diagnostics};
use crate::lexer::{Lexer, Mode, Token, TokenKind as K, ident_value};
use crate::source::{FileId, Span};
use crate::syntax::{Item, Stylesheet};

/// LANGUAGE.md §3.1 rule 4.
const STATEMENT_KEYWORDS: [&str; 14] = [
    "load", "export", "bind", "fn", "recipe", "rule", "when", "each", "apply", "slot", "yield",
    "fail", "warn", "opaque",
];

/// Keywords that can reach declaration classification (§3.1 rule 5).
const NAME_KEYWORDS: [&str; 5] = ["as", "in", "otherwise", "true", "false"];

/// The 16 page-margin at-rules of CSS Paged Media.
const PAGE_MARGINS: [&str; 16] = [
    "top-left-corner",
    "top-left",
    "top-center",
    "top-right",
    "top-right-corner",
    "bottom-left-corner",
    "bottom-left",
    "bottom-center",
    "bottom-right",
    "bottom-right-corner",
    "left-top",
    "left-middle",
    "left-bottom",
    "right-top",
    "right-middle",
    "right-bottom",
];

const NOT_IMPLEMENTED_NOTE: &str = "this build compiles the plain-CSS subset of Weave; the rest of the language is not implemented yet";

/// The kind of block being parsed (CSS-COMPATIBILITY.md §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ctx {
    /// The module top level.
    TopLevel,
    /// A stylesheet context opened by a group at-rule or `@scope`.
    Stylesheet,
    Rule,
    Declarations,
    Keyframes,
    Page,
}

impl Ctx {
    fn is_stylesheet(self) -> bool {
        matches!(self, Ctx::TopLevel | Ctx::Stylesheet)
    }

    fn describe(self) -> &'static str {
        match self {
            Ctx::TopLevel | Ctx::Stylesheet => "at stylesheet level",
            Ctx::Rule => "in a rule body",
            Ctx::Declarations => "in a declaration list",
            Ctx::Keyframes => "in `@keyframes`",
            Ctx::Page => "in `@page`",
        }
    }
}

/// How a known at-rule is parsed (CSS-COMPATIBILITY.md §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtKind {
    Charset,
    Import,
    Namespace,
    Layer,
    Group,
    Descriptors,
    Keyframes,
    Page,
    Scope,
    PageMargin,
    /// `@font-feature-values`: known, preserved verbatim.
    Preserved,
    Unknown,
}

fn at_kind(name: &str) -> AtKind {
    match name {
        "charset" => AtKind::Charset,
        "import" => AtKind::Import,
        "namespace" => AtKind::Namespace,
        "layer" => AtKind::Layer,
        "media" | "supports" | "container" | "starting-style" => AtKind::Group,
        "font-face"
        | "font-palette-values"
        | "counter-style"
        | "property"
        | "view-transition"
        | "position-try" => AtKind::Descriptors,
        "keyframes" => AtKind::Keyframes,
        "page" => AtKind::Page,
        "scope" => AtKind::Scope,
        "font-feature-values" => AtKind::Preserved,
        _ if PAGE_MARGINS.contains(&name) => AtKind::PageMargin,
        _ => AtKind::Unknown,
    }
}

/// Top-level items relevant to `@charset`, `@import` and `@namespace`
/// ordering (CSS-COMPATIBILITY.md §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TopClass {
    Import,
    Namespace,
    /// `@charset`, `@layer` statements and `/*!` comments.
    Neutral,
    Other,
}

/// What ended a scan of component values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    Semicolon,
    /// A `{` at depth 0 (preludes only); the token is consumed.
    OpenBrace(Token),
    /// A `}` at depth 0; left unconsumed for the enclosing block.
    CloseBrace,
    /// A depth-0 `{` block in a declaration value was consumed (E0111).
    BadBlock,
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scan {
    /// Stop at a depth-0 `{` (selectors, at-rule preludes, keyframes).
    Prelude,
    /// A declaration value; custom properties may contain `{}` blocks.
    Value { custom: bool },
}

/// What a block becomes once its `}` is reached.
#[derive(Debug)]
enum Pending {
    Rule {
        selector: String,
    },
    AtRule {
        name: String,
        prelude: String,
    },
    Keyframe {
        selectors: String,
    },
    /// A block parsed only for its diagnostics.
    Discard,
}

/// A block opened by an item; [`Parser::run`] parses its contents.
#[derive(Debug)]
struct Opened {
    open: Token,
    ctx: Ctx,
    pending: Pending,
}

/// An open block on the parser's explicit stack.
#[derive(Debug)]
struct Frame {
    open: Token,
    pending: Pending,
    parent_ctx: Ctx,
    parent_items: Vec<Item>,
}

fn finish(pending: Pending, body: Vec<Item>, items: &mut Vec<Item>) {
    match pending {
        Pending::Rule { selector } => items.push(Item::Rule { selector, body }),
        Pending::AtRule { name, prelude } => items.push(Item::AtRule {
            name,
            prelude,
            body: Some(body),
        }),
        Pending::Keyframe { selectors } => items.push(Item::Keyframe { selectors, body }),
        Pending::Discard => {}
    }
}

/// Parses one module. Diagnostics go to `diags`; the tree is meaningful only
/// if no error was reported.
pub(crate) fn parse(
    text: &str,
    start: usize,
    file: FileId,
    max_depth: u32,
    diags: &mut Diagnostics,
) -> Stylesheet {
    let mut parser = Parser {
        text,
        file,
        lexer: Lexer::new(text, start, max_depth),
        diags,
        reported: HashSet::new(),
        max_depth,
        depth: 0,
        aborted: false,
        top_items: 0,
        import_blocker: None,
        namespace_blocker: None,
    };
    Stylesheet {
        items: parser.run(),
    }
}

struct Parser<'a, 'd> {
    text: &'a str,
    file: FileId,
    lexer: Lexer<'a>,
    diags: &'d mut Diagnostics,
    /// (code, offset) pairs already reported, so that re-scanned tokens are
    /// reported once.
    reported: HashSet<(Code, u32)>,
    max_depth: u32,
    /// Current block depth.
    depth: u32,
    /// Set after E8002 or when the diagnostic limit is reached.
    aborted: bool,
    /// Top-level items seen so far, for `@charset`.
    top_items: usize,
    /// The first top-level item after which `@import` is no longer allowed.
    import_blocker: Option<Span>,
    /// The first top-level item after which `@namespace` is no longer allowed.
    namespace_blocker: Option<Span>,
}

impl Parser<'_, '_> {
    // ----- diagnostics -------------------------------------------------

    fn span(&self, token: Token) -> Span {
        Span::new(self.file, token.start, token.end)
    }

    fn text_of(&self, token: Token) -> &str {
        token.text(self.text)
    }

    fn report(&mut self, diagnostic: Diagnostic) {
        let key = diagnostic.primary().map_or(u32::MAX, |s| s.start);
        if !self.reported.insert((diagnostic.code(), key)) {
            return;
        }
        self.diags.push(diagnostic);
        if self.diags.is_full() {
            self.aborted = true;
        }
    }

    fn error(&mut self, code: Code, span: Span, message: impl Into<String>) {
        self.report(Diagnostic::error(code, message).with_primary(span));
    }

    fn not_implemented(&mut self, span: Span, what: &str) {
        self.report(
            Diagnostic::error(Code::E5001, format!("{what} not implemented in this build"))
                .with_primary(span)
                .with_note(NOT_IMPLEMENTED_NOTE),
        );
    }

    fn too_deep(&mut self, span: Span) {
        if !self.aborted {
            self.report(
                Diagnostic::error(
                    Code::E8002,
                    format!(
                        "nesting exceeds the depth limit of {} (blocks and brackets combined)",
                        self.max_depth
                    ),
                )
                .with_primary(span)
                .with_note("raise it with the `nesting-depth` limit"),
            );
        }
        self.aborted = true;
    }

    /// Reports lexical errors carried by a token (LANGUAGE.md §16 E00xx).
    /// `weave` marks regions that Weave processes (values, selectors,
    /// preludes); elsewhere (verbatim regions, skipped statements) only
    /// errors that make the input end early are reported.
    fn check_token(&mut self, token: Token, weave: bool) {
        let span = self.span(token);
        match token.kind {
            K::Comment { terminated: false } => {
                self.error(Code::E0003, span, "unterminated comment: `/*` has no `*/`");
            }
            K::String { terminated: false } => {
                self.error(Code::E0002, span, "unterminated string");
            }
            K::BadString if weave => self.report(
                Diagnostic::error(
                    Code::E0002,
                    "unterminated string: line break inside a string",
                )
                .with_primary(span)
                .with_note("escape the line break with `\\` or close the string"),
            ),
            K::Url { terminated: false } => {
                self.error(Code::E0005, span, "unterminated `url(`: missing `)`");
            }
            K::Interpolation { terminated: false } => {
                self.error(
                    Code::E0004,
                    span,
                    "unterminated interpolation: `#{` has no `}`",
                );
            }
            K::Interpolation { terminated: true } if weave => {
                self.not_implemented(span, "`#{…}` interpolation is");
            }
            _ => {}
        }
        if let Some(at) = token.interpolation {
            if weave {
                let span = Span::new(self.file, at, at.saturating_add(2));
                self.not_implemented(span, "`#{…}` interpolation is");
            }
        }
    }

    // ----- token access ------------------------------------------------

    fn raw_next(&mut self, mode: Mode) -> Token {
        if self.aborted {
            let end = text_len(self.text);
            return Token {
                kind: K::Eof,
                start: end,
                end,
                interpolation: None,
            };
        }
        let token = self.lexer.next(mode);
        if self.lexer.too_deep() {
            self.too_deep(self.span(token));
        }
        token
    }

    fn next(&mut self, mode: Mode) -> Token {
        let token = self.raw_next(mode);
        self.check_token(token, mode == Mode::Weave);
        token
    }

    /// The next token of a region that is skipped after an error (only
    /// end-of-input errors are reported), optionally past trivia.
    fn next_skipped(&mut self, significant: bool) -> Token {
        loop {
            let token = self.raw_next(Mode::Weave);
            self.check_token(token, false);
            if !(significant && token.is_trivia()) {
                return token;
            }
        }
    }

    /// The next token that is not whitespace or a comment.
    fn next_significant(&mut self, mode: Mode) -> Token {
        loop {
            let token = self.next(mode);
            if !token.is_trivia() {
                return token;
            }
        }
    }

    fn unread_to(&mut self, offset: u32) {
        self.lexer.set_pos(offset as usize);
    }

    fn unread(&mut self, token: Token) {
        self.lexer.set_pos(token.start as usize);
    }

    /// Enters a nested block or bracket; false (after E8002) when the
    /// combined depth would exceed the limit.
    fn within_depth(&mut self, extra: usize, at: Token) -> bool {
        let depth = u64::from(self.depth).saturating_add(extra as u64);
        if depth > u64::from(self.max_depth) {
            self.too_deep(self.span(at));
            return false;
        }
        true
    }

    // ----- blocks and items --------------------------------------------

    /// Parses the module. Open blocks live on an explicit stack instead of
    /// the call stack, so even the largest nesting limit (4096) cannot
    /// exhaust the thread stack.
    fn run(&mut self) -> Vec<Item> {
        let mut frames: Vec<Frame> = Vec::new();
        let mut ctx = Ctx::TopLevel;
        let mut items = Vec::new();
        while !self.aborted {
            let token = self.next(Mode::Weave);
            let opened = match token.kind {
                K::Eof => break,
                K::Whitespace | K::Semicolon => None,
                K::CloseBrace => {
                    match frames.pop() {
                        Some(frame) => {
                            let body = std::mem::replace(&mut items, frame.parent_items);
                            finish(frame.pending, body, &mut items);
                            ctx = frame.parent_ctx;
                            self.depth -= 1;
                        }
                        None => self.error(Code::E0005, self.span(token), "unmatched `}`"),
                    }
                    None
                }
                K::Comment { .. } => {
                    let text = self.text_of(token);
                    if text.starts_with("/*!") {
                        items.push(Item::Verbatim(normalize_newlines(text)));
                        self.top_item(ctx, TopClass::Neutral, token);
                    }
                    None
                }
                K::AtKeyword => self.at_rule(token, ctx, &mut items),
                K::Ident if STATEMENT_KEYWORDS.contains(&self.text_of(token)) => {
                    self.statement(token, ctx, &mut items)
                }
                K::Function if is_keyword_function(self.text_of(token)) => {
                    // `when(…)`: the keyword still starts its statement.
                    let keyword = Token {
                        kind: K::Ident,
                        end: token.end - 1,
                        ..token
                    };
                    self.unread_to(keyword.end);
                    self.statement(keyword, ctx, &mut items)
                }
                _ => self.other_item(token, ctx, &mut items),
            };
            if let Some(opened) = opened {
                if !self.within_depth(1, opened.open) {
                    break;
                }
                self.depth += 1;
                frames.push(Frame {
                    open: opened.open,
                    pending: opened.pending,
                    parent_ctx: ctx,
                    parent_items: std::mem::take(&mut items),
                });
                ctx = opened.ctx;
            }
        }
        // End of input (or an aborted parse): close open blocks, innermost
        // first.
        while let Some(frame) = frames.pop() {
            if !self.aborted {
                self.error(
                    Code::E0005,
                    self.span(frame.open),
                    "unclosed `{`: missing `}`",
                );
            }
            let body = std::mem::replace(&mut items, frame.parent_items);
            finish(frame.pending, body, &mut items);
        }
        self.depth = 0;
        items
    }

    fn top_item(&mut self, ctx: Ctx, class: TopClass, token: Token) {
        if ctx != Ctx::TopLevel {
            return;
        }
        self.top_items += 1;
        let span = self.span(token);
        match class {
            TopClass::Other => {
                self.import_blocker.get_or_insert(span);
                self.namespace_blocker.get_or_insert(span);
            }
            TopClass::Namespace => {
                self.import_blocker.get_or_insert(span);
            }
            TopClass::Import | TopClass::Neutral => {}
        }
    }

    /// Declarations, keyframe blocks and errors (§3.1 rules 5–7).
    fn other_item(&mut self, first: Token, ctx: Ctx, items: &mut Vec<Item>) -> Option<Opened> {
        if matches!(first.kind, K::Ident | K::Interpolation { .. }) {
            if let Some(name) = self.declaration_name(first) {
                self.declaration(&name, ctx, items);
                return None;
            }
        }
        if ctx == Ctx::Keyframes {
            return self.keyframe_block(first);
        }
        let end = self.skip(first, true);
        let span = self.span(first);
        if end == End::BadBlock && first.kind != K::OpenBrace {
            self.report(
                Diagnostic::error(
                    Code::E0110,
                    "qualified rules must be introduced with `rule`",
                )
                .with_primary(span)
                .with_note("write `rule <selector> { … }`; Weave never guesses selectors"),
            );
        } else {
            let shown = self.text_of(first).chars().take(32).collect::<String>();
            self.error(Code::E0101, span, format!("unexpected `{shown}`"));
        }
        if ctx == Ctx::TopLevel {
            self.top_items += 1;
        }
        None
    }

    /// Skips the rest of a malformed item that started with `first`: up to
    /// a `;` at depth 0, past a depth-0 `{…}` block when `block_ends`, or up
    /// to (not including) a depth-0 `}`.
    fn skip(&mut self, first: Token, block_ends: bool) -> End {
        let mut stack: Vec<K> = Vec::new();
        let mut token = first;
        loop {
            match token.kind {
                K::Eof => return End::Eof,
                K::Semicolon if stack.is_empty() => return End::Semicolon,
                K::OpenBrace | K::OpenParen | K::Function | K::OpenSquare => {
                    if !self.within_depth(stack.len() + 1, token) {
                        return End::Eof;
                    }
                    stack.push(closer_of(token.kind));
                }
                K::CloseBrace if !stack.contains(&K::CloseBrace) => {
                    self.unread(token);
                    return End::CloseBrace;
                }
                K::CloseParen | K::CloseSquare | K::CloseBrace => {
                    if let Some(at) = stack.iter().rposition(|&k| k == token.kind) {
                        stack.truncate(at);
                        if stack.is_empty() && token.kind == K::CloseBrace && block_ends {
                            return End::BadBlock;
                        }
                    }
                }
                _ => {}
            }
            token = self.raw_next(Mode::Weave);
            self.check_token(token, false);
        }
    }

    /// If `first` starts a declaration name followed by `:`, consumes
    /// through the `:` and returns the name tokens.
    fn declaration_name(&mut self, first: Token) -> Option<Vec<Token>> {
        let save = self.lexer.pos();
        let mut name = vec![first];
        let mut end = first.end;
        let after = loop {
            let token = self.raw_next(Mode::Weave);
            if token.start == end && matches!(token.kind, K::Ident | K::Interpolation { .. }) {
                end = token.end;
                name.push(token);
            } else {
                break token;
            }
        };
        let mut token = after;
        while token.is_trivia() {
            token = self.raw_next(Mode::Weave);
        }
        if token.kind == K::Colon {
            // Report lexical errors of the skipped trivia and name parts.
            self.lexer.set_pos(after.start as usize);
            loop {
                let t = self.next(Mode::Weave);
                if t.kind == K::Colon || t.kind == K::Eof {
                    break;
                }
            }
            for &part in name.iter().skip(1) {
                self.check_token(part, true);
            }
            return Some(name);
        }
        self.lexer.set_pos(save);
        None
    }

    fn declaration(&mut self, name: &[Token], ctx: Ctx, items: &mut Vec<Item>) {
        let (Some(&first), Some(&last)) = (name.first(), name.last()) else {
            return;
        };
        let name_span = Span::new(self.file, first.start, last.end);
        let name_text = self
            .text
            .get(first.start as usize..last.end as usize)
            .unwrap_or("");
        let custom = ident_value(name_text).starts_with("--");
        let (tokens, end) = self.scan(Scan::Value { custom });

        if name.len() == 1 && first.kind == K::Ident && NAME_KEYWORDS.contains(&name_text) {
            self.report(
                Diagnostic::error(
                    Code::E0103,
                    format!("`{name_text}` is a Weave keyword and cannot be a declaration name"),
                )
                .with_primary(name_span)
                .with_note(format!(
                    "write the name with a CSS escape, for example `{}`",
                    escaped_keyword(name_text)
                )),
            );
        }
        if end == End::BadBlock {
            // E0111 was reported; a selector is the usual cause.
            return;
        }
        match ctx {
            Ctx::TopLevel | Ctx::Stylesheet => {
                self.report(
                    Diagnostic::error(Code::E6001, "a declaration is not allowed at stylesheet level")
                        .with_primary(name_span)
                        .with_note("declarations belong inside `rule <selector> { … }` or a declaration at-rule"),
                );
            }
            Ctx::Keyframes => {
                self.error(
                    Code::E6005,
                    name_span,
                    "a declaration is not allowed directly in `@keyframes`; put it in a keyframe block",
                );
            }
            Ctx::Rule | Ctx::Declarations | Ctx::Page => {}
        }
        let (value, important) = split_important(&tokens, self.text);
        items.push(Item::Declaration {
            name: normalize_newlines(name_text),
            value: render(value, self.text),
            important,
        });
    }

    /// Scans component values in Weave mode (LANGUAGE.md §3.2): strings,
    /// comments, URLs, escapes and brackets are respected. Mismatched
    /// closing brackets are E0005.
    fn scan(&mut self, mode: Scan) -> (Vec<Token>, End) {
        let mut tokens = Vec::new();
        let mut stack: Vec<Token> = Vec::new();
        let mut bad_block = false;
        loop {
            let token = self.next(Mode::Weave);
            match token.kind {
                K::Eof => {
                    if let Some(&open) = stack.last() {
                        self.unclosed(open);
                    }
                    return (tokens, End::Eof);
                }
                K::Semicolon if stack.is_empty() => return (tokens, End::Semicolon),
                K::OpenBrace if stack.is_empty() && mode == Scan::Prelude => {
                    return (tokens, End::OpenBrace(token));
                }
                K::OpenBrace | K::OpenParen | K::Function | K::OpenSquare => {
                    if !self.within_depth(stack.len() + 1, token) {
                        return (tokens, End::Eof);
                    }
                    if token.kind == K::OpenBrace
                        && stack.is_empty()
                        && mode == (Scan::Value { custom: false })
                    {
                        self.report(
                            Diagnostic::error(
                                Code::E0111,
                                "`{` is not allowed in the value of a non-custom property",
                            )
                            .with_primary(self.span(token))
                            .with_note("a `;` may be missing, or this is a selector: style rules are written `rule <selector> { … }`"),
                        );
                        bad_block = true;
                    }
                    stack.push(token);
                    tokens.push(token);
                }
                K::CloseParen | K::CloseSquare | K::CloseBrace => {
                    let matching = stack
                        .iter()
                        .rposition(|open| closer_of(open.kind) == token.kind);
                    match matching {
                        Some(at) => {
                            if at + 1 < stack.len() {
                                if let Some(&inner) = stack.last() {
                                    self.unclosed(inner);
                                }
                            }
                            stack.truncate(at);
                            tokens.push(token);
                            if bad_block && stack.is_empty() && token.kind == K::CloseBrace {
                                return (tokens, End::BadBlock);
                            }
                        }
                        None if token.kind == K::CloseBrace => {
                            if let Some(&open) = stack.last() {
                                self.unclosed(open);
                            }
                            self.unread(token);
                            return (tokens, End::CloseBrace);
                        }
                        None => {
                            let shown = self.text_of(token).to_string();
                            self.error(
                                Code::E0005,
                                self.span(token),
                                format!("unmatched `{shown}`"),
                            );
                            tokens.push(token);
                        }
                    }
                }
                _ => tokens.push(token),
            }
        }
    }

    fn unclosed(&mut self, open: Token) {
        let shown = match open.kind {
            K::Function => "(",
            _ => self.text_of(open),
        }
        .to_string();
        self.error(Code::E0005, self.span(open), format!("unclosed `{shown}`"));
    }

    // ----- statements --------------------------------------------------

    fn statement(&mut self, keyword: Token, ctx: Ctx, items: &mut Vec<Item>) -> Option<Opened> {
        let span = self.span(keyword);
        let word = self.text_of(keyword).to_string();
        match word.as_str() {
            "rule" => return self.rule(keyword, ctx),
            "opaque" => {
                self.opaque(keyword, ctx, items);
                return None;
            }
            "fn" | "recipe" | "load" | "export" if ctx != Ctx::TopLevel => {
                self.error(
                    Code::E0104,
                    span,
                    format!("`{word}` is allowed only at module top level"),
                );
            }
            "yield" => self.error(
                Code::E4013,
                span,
                "`yield` is allowed only in a function body",
            ),
            "slot" if ctx.is_stylesheet() || ctx == Ctx::Rule => {
                self.error(Code::E4012, span, "`slot` is allowed only in a recipe body");
            }
            "slot" => {
                self.error(
                    Code::E6005,
                    span,
                    format!("`slot` is not allowed {}", ctx.describe()),
                );
            }
            "apply" if ctx == Ctx::Keyframes => {
                self.error(Code::E6005, span, "`apply` is not allowed in `@keyframes`");
            }
            _ => self.not_implemented(span, &format!("`{word}` statements are")),
        }
        if ctx == Ctx::TopLevel {
            // Statements emit nothing but still precede a later `@charset`.
            self.top_items += 1;
        }
        self.skip_statement(&word);
        None
    }

    /// Skips the extent of a statement this build does not implement.
    fn skip_statement(&mut self, word: &str) {
        match word {
            "when" | "each" | "fn" | "recipe" => {
                let first = self.next_skipped(true);
                self.skip_through_block(first);
                if word == "when" {
                    loop {
                        let next = self.next_skipped(true);
                        if next.kind == K::Ident && self.text_of(next) == "otherwise" {
                            let first = self.next_skipped(true);
                            self.skip_through_block(first);
                        } else {
                            self.unread(next);
                            break;
                        }
                    }
                }
            }
            "apply" => {
                let first = self.next_skipped(true);
                self.skip(first, true);
            }
            "export" => {
                let next = self.next_skipped(true);
                let inner = self.text_of(next).to_string();
                if next.kind == K::Ident && matches!(inner.as_str(), "fn" | "recipe") {
                    self.skip_statement(&inner);
                } else {
                    self.skip(next, false);
                }
            }
            _ => {
                let first = self.next_skipped(false);
                self.skip(first, false);
            }
        }
    }

    /// Skips up to and including the first depth-0 `{…}` block.
    fn skip_through_block(&mut self, first: Token) {
        let mut token = first;
        let mut stack = 0usize;
        loop {
            match token.kind {
                K::Eof => return,
                K::OpenBrace if stack == 0 => {
                    self.skip(token, true);
                    return;
                }
                K::Semicolon | K::CloseBrace if stack == 0 => {
                    if token.kind == K::CloseBrace {
                        self.unread(token);
                    }
                    return;
                }
                K::OpenParen | K::Function | K::OpenSquare => {
                    if !self.within_depth(stack + 1, token) {
                        return;
                    }
                    stack += 1;
                }
                K::CloseParen | K::CloseSquare => stack = stack.saturating_sub(1),
                _ => {}
            }
            token = self.next_skipped(false);
        }
    }

    fn rule(&mut self, keyword: Token, ctx: Ctx) -> Option<Opened> {
        let span = self.span(keyword);
        let (tokens, end) = self.scan(Scan::Prelude);
        let End::OpenBrace(open) = end else {
            self.error(
                Code::E0101,
                span,
                "expected a `{` block after the `rule` selector",
            );
            return None;
        };
        if tokens.iter().all(|t| t.is_trivia()) {
            self.error(Code::E0101, span, "expected a selector after `rule`");
        }
        let mut keep = true;
        match ctx {
            Ctx::TopLevel | Ctx::Stylesheet => {
                for &token in &tokens {
                    if token.kind == K::Delim('&') {
                        self.report(
                            Diagnostic::error(Code::E6003, "`&` has no parent rule here")
                                .with_primary(self.span(token))
                                .with_note("at stylesheet level and directly in `@scope`, use a full selector (or `:scope`)"),
                        );
                    }
                }
            }
            Ctx::Rule => {
                self.not_implemented(span, "nested `rule`s (nesting) are");
                keep = false;
            }
            Ctx::Declarations => {
                self.error(
                    Code::E6002,
                    span,
                    "a `rule` is not allowed in a declaration list",
                );
                keep = false;
            }
            Ctx::Keyframes | Ctx::Page => {
                self.error(
                    Code::E6005,
                    span,
                    format!("a `rule` is not allowed {}", ctx.describe()),
                );
                keep = false;
            }
        }
        self.top_item(ctx, TopClass::Other, keyword);
        let pending = if keep {
            Pending::Rule {
                selector: normalize_selector(&tokens, self.text),
            }
        } else {
            Pending::Discard
        };
        Some(Opened {
            open,
            ctx: Ctx::Rule,
            pending,
        })
    }

    fn opaque(&mut self, keyword: Token, ctx: Ctx, items: &mut Vec<Item>) {
        let span = self.span(keyword);
        let open = self.next_significant(Mode::Weave);
        if open.kind != K::OpenBrace {
            self.error(Code::E0101, self.span(open), "expected `{` after `opaque`");
            self.skip(open, true);
            return;
        }
        let allowed = ctx.is_stylesheet() || ctx == Ctx::Rule;
        if !allowed {
            self.error(
                Code::E6005,
                span,
                format!("`opaque` is not allowed {}", ctx.describe()),
            );
        }
        let Some(close) = self.balanced(open) else {
            return;
        };
        let inner = self
            .text
            .get(open.end as usize..close as usize)
            .unwrap_or("");
        let content = normalize_newlines(trim_css_whitespace(inner));
        if allowed && !content.is_empty() {
            items.push(Item::Verbatim(content));
        }
        self.top_item(ctx, TopClass::Other, keyword);
    }

    /// Scans plain CSS (no Weave extensions) after `open` (`{`) to its
    /// matching `}`, following CSS Syntax: only a closer matching the
    /// innermost open bracket closes it. Returns the offset of the `}`.
    fn balanced(&mut self, open: Token) -> Option<u32> {
        let mut stack = vec![K::CloseBrace];
        loop {
            let token = self.next(Mode::Css);
            match token.kind {
                K::Eof => {
                    if !self.aborted {
                        self.error(Code::E0005, self.span(open), "unclosed `{`: missing `}`");
                    }
                    return None;
                }
                K::OpenBrace | K::OpenParen | K::Function | K::OpenSquare => {
                    if !self.within_depth(stack.len() + 1, token) {
                        return None;
                    }
                    stack.push(closer_of(token.kind));
                }
                K::CloseParen | K::CloseSquare | K::CloseBrace
                    if stack.last() == Some(&token.kind) =>
                {
                    stack.pop();
                    if stack.is_empty() {
                        return Some(token.start);
                    }
                }
                _ => {}
            }
        }
    }

    // ----- at-rules ----------------------------------------------------

    fn at_rule(&mut self, at: Token, ctx: Ctx, items: &mut Vec<Item>) -> Option<Opened> {
        let raw = self.text_of(at).to_string();
        let name = ident_value(raw.get(1..).unwrap_or("")).to_ascii_lowercase();
        let kind = at_kind(&name);
        let span = self.span(at);

        if matches!(kind, AtKind::Unknown | AtKind::Preserved) {
            if kind == AtKind::Preserved && !ctx.is_stylesheet() {
                self.error(
                    Code::E6005,
                    span,
                    format!("`{raw}` is not allowed {}", ctx.describe()),
                );
                self.verbatim_at_rule(at);
                return None;
            }
            if let Some(text) = self.verbatim_at_rule(at) {
                items.push(Item::Verbatim(text));
            }
            self.top_item(ctx, TopClass::Other, at);
            return None;
        }

        let (prelude_tokens, end) = self.scan(Scan::Prelude);
        let prelude = normalize_prelude(&prelude_tokens, self.text);
        let statement = match kind {
            AtKind::Charset | AtKind::Import | AtKind::Namespace => true,
            AtKind::Layer => !matches!(end, End::OpenBrace(_)),
            _ => false,
        };

        if statement {
            match end {
                End::Semicolon => {}
                End::OpenBrace(open) => {
                    self.error(
                        Code::E0102,
                        self.span(open),
                        format!("expected `;` after the `{raw}` prelude"),
                    );
                    self.skip(open, true);
                    return None;
                }
                _ => {
                    self.error(
                        Code::E0102,
                        span,
                        format!("expected `;` after the `{raw}` prelude"),
                    );
                    return None;
                }
            }
            if self.statement_at_rule_allowed(kind, at, ctx, &prelude_tokens) {
                items.push(Item::AtRule {
                    name: raw,
                    prelude,
                    body: None,
                });
            }
            return None;
        }

        let End::OpenBrace(open) = end else {
            self.error(
                Code::E0101,
                span,
                format!("expected a `{{` block after the `{raw}` prelude"),
            );
            return None;
        };
        let (body_ctx, keep) = match (kind, ctx) {
            (AtKind::Group | AtKind::Layer, Ctx::TopLevel | Ctx::Stylesheet) => {
                (Ctx::Stylesheet, true)
            }
            (AtKind::Group | AtKind::Layer, Ctx::Rule) => {
                self.not_implemented(
                    span,
                    &format!("`{raw}` nested in a rule (group at-rule lowering) is"),
                );
                (Ctx::Rule, false)
            }
            (AtKind::Scope, Ctx::TopLevel | Ctx::Stylesheet) => (Ctx::Stylesheet, true),
            (AtKind::Scope, Ctx::Rule) => {
                self.error(
                    Code::E6006,
                    span,
                    "`@scope` nested in a rule is not supported in Weave v0.1",
                );
                (Ctx::Stylesheet, false)
            }
            (AtKind::Descriptors, Ctx::TopLevel | Ctx::Stylesheet) => (Ctx::Declarations, true),
            (AtKind::Keyframes, Ctx::TopLevel | Ctx::Stylesheet) => (Ctx::Keyframes, true),
            (AtKind::Page, Ctx::TopLevel | Ctx::Stylesheet) => (Ctx::Page, true),
            (AtKind::PageMargin, Ctx::Page) => (Ctx::Declarations, true),
            (_, _) => {
                self.error(
                    Code::E6005,
                    span,
                    format!("`{raw}` is not allowed {}", ctx.describe()),
                );
                (body_context(kind), false)
            }
        };
        self.top_item(ctx, TopClass::Other, at);
        let pending = if keep {
            Pending::AtRule { name: raw, prelude }
        } else {
            Pending::Discard
        };
        Some(Opened {
            open,
            ctx: body_ctx,
            pending,
        })
    }

    /// Context and ordering checks for `@charset`, `@import`, `@namespace`
    /// and `@layer` statements.
    fn statement_at_rule_allowed(
        &mut self,
        kind: AtKind,
        at: Token,
        ctx: Ctx,
        prelude: &[Token],
    ) -> bool {
        let span = self.span(at);
        let raw = self.text_of(at).to_string();
        match kind {
            AtKind::Charset => {
                let significant: Vec<Token> =
                    prelude.iter().copied().filter(|t| !t.is_trivia()).collect();
                let utf8 = matches!(significant.as_slice(), [t] if self.text_of(*t) == "\"UTF-8\"");
                let first = ctx == Ctx::TopLevel && self.top_items == 0;
                self.top_item(ctx, TopClass::Neutral, at);
                if !first {
                    self.report(
                        Diagnostic::error(
                            Code::E6008,
                            "`@charset` must be the first item of the entry module",
                        )
                        .with_primary(span),
                    );
                    return false;
                }
                if !utf8 {
                    self.report(
                        Diagnostic::error(Code::E6008, "`@charset` accepts only \"UTF-8\"")
                            .with_primary(span)
                            .with_note("Weave sources and output are always UTF-8; write `@charset \"UTF-8\";` or remove it"),
                    );
                    return false;
                }
                true
            }
            AtKind::Import | AtKind::Namespace => {
                if ctx != Ctx::TopLevel {
                    self.error(
                        Code::E6005,
                        span,
                        format!("`{raw}` is allowed only at module top level"),
                    );
                    return false;
                }
                let (blocker, class) = if kind == AtKind::Import {
                    (self.import_blocker, TopClass::Import)
                } else {
                    (self.namespace_blocker, TopClass::Namespace)
                };
                self.top_item(ctx, class, at);
                if let Some(earlier) = blocker {
                    self.report(
                        Diagnostic::error(
                            Code::E7010,
                            format!("`{raw}` must come before style rules and other at-rules; Weave does not reorder CSS"),
                        )
                        .with_primary(span)
                        .with_label(earlier, "this item comes first")
                        .with_note(format!("move `{raw}` before the earlier item")),
                    );
                    return false;
                }
                true
            }
            _ => {
                // `@layer a, b;`
                if !ctx.is_stylesheet() {
                    self.error(
                        Code::E6005,
                        span,
                        format!("a `{raw}` statement is not allowed {}", ctx.describe()),
                    );
                    return false;
                }
                self.top_item(ctx, TopClass::Neutral, at);
                true
            }
        }
    }

    fn keyframe_block(&mut self, first: Token) -> Option<Opened> {
        self.unread(first);
        let (tokens, end) = self.scan(Scan::Prelude);
        let End::OpenBrace(open) = end else {
            self.error(
                Code::E0101,
                self.span(first),
                "expected a `{` block after the keyframe selector",
            );
            return None;
        };
        Some(Opened {
            open,
            ctx: Ctx::Declarations,
            pending: Pending::Keyframe {
                selectors: normalize_prelude(&tokens, self.text),
            },
        })
    }

    /// An unknown at-rule: the exact source text from `@` through its `;`
    /// or matching `}` (LANGUAGE.md §13), scanned as plain CSS. An at-rule
    /// cut short by the enclosing `}` or the end of input ends at its last
    /// token.
    fn verbatim_at_rule(&mut self, at: Token) -> Option<String> {
        let start = at.start as usize;
        let mut end = at.end as usize;
        let mut stack: Vec<K> = Vec::new();
        loop {
            let token = self.next(Mode::Css);
            match token.kind {
                K::Eof => {
                    if !stack.is_empty() {
                        let raw = self.text_of(at).to_string();
                        self.error(
                            Code::E0005,
                            self.span(at),
                            format!("`{raw}` has an unclosed bracket"),
                        );
                        return None;
                    }
                    break;
                }
                K::Semicolon if stack.is_empty() => {
                    end = token.end as usize;
                    break;
                }
                K::OpenBrace if stack.is_empty() => {
                    let close = self.balanced(token)?;
                    end = close as usize + 1;
                    break;
                }
                K::CloseBrace if stack.is_empty() => {
                    self.unread(token);
                    break;
                }
                K::OpenBrace | K::OpenParen | K::Function | K::OpenSquare => {
                    if !self.within_depth(stack.len() + 1, token) {
                        return None;
                    }
                    stack.push(closer_of(token.kind));
                    end = token.end as usize;
                }
                K::CloseParen | K::CloseSquare | K::CloseBrace => {
                    if stack.last() == Some(&token.kind) {
                        stack.pop();
                    }
                    end = token.end as usize;
                }
                _ if token.is_trivia() => {}
                _ => end = token.end as usize,
            }
        }
        Some(normalize_newlines(self.text.get(start..end).unwrap_or("")))
    }
}

fn is_keyword_function(text: &str) -> bool {
    text.strip_suffix('(')
        .is_some_and(|name| STATEMENT_KEYWORDS.contains(&name))
}

/// `as` -> `\61 s`: the keyword with its first letter hex-escaped.
fn escaped_keyword(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => format!("\\{:x} {}", u32::from(first), chars.as_str()),
        None => String::new(),
    }
}

fn body_context(kind: AtKind) -> Ctx {
    match kind {
        AtKind::Descriptors | AtKind::PageMargin => Ctx::Declarations,
        AtKind::Keyframes => Ctx::Keyframes,
        AtKind::Page => Ctx::Page,
        _ => Ctx::Stylesheet,
    }
}

fn closer_of(open: K) -> K {
    match open {
        K::OpenSquare => K::CloseSquare,
        K::OpenBrace => K::CloseBrace,
        _ => K::CloseParen,
    }
}

fn text_len(text: &str) -> u32 {
    u32::try_from(text.len()).unwrap_or(u32::MAX)
}

/// Trims whitespace from a value and splits off a trailing `!important`
/// (`!`, optional whitespace, `important` in any ASCII case, at depth 0).
fn split_important<'t>(tokens: &'t [Token], source: &str) -> (&'t [Token], bool) {
    let trimmed = trim_whitespace(tokens);
    if let [rest @ .., last] = trimmed {
        if last.kind == K::Ident && last.text(source).eq_ignore_ascii_case("important") {
            let before = trim_end(rest);
            if let [value @ .., bang] = before {
                if bang.kind == K::Delim('!') {
                    return (trim_end(value), true);
                }
            }
        }
    }
    (trimmed, false)
}

fn trim_whitespace(tokens: &[Token]) -> &[Token] {
    let start = tokens
        .iter()
        .position(|t| t.kind != K::Whitespace)
        .unwrap_or(tokens.len());
    trim_end(tokens.get(start..).unwrap_or(&[]))
}

fn trim_end(tokens: &[Token]) -> &[Token] {
    let end = tokens
        .iter()
        .rposition(|t| t.kind != K::Whitespace)
        .map_or(0, |i| i + 1);
    tokens.get(..end).unwrap_or(&[])
}
