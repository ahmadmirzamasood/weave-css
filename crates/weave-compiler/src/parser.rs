//! Parser for Weave modules.
//!
//! Implements item classification (LANGUAGE.md §3.1), declarations (§3.2),
//! `bind` statements (§4.2), `rule` at stylesheet level (§11), the at-rule
//! matrix and context matrix (CSS-COMPATIBILITY.md §2, §3), `opaque` blocks
//! and unknown at-rules (§13) and `/*!` comments. CSS regions that Weave
//! processes become templates whose `#{…}` interpolations are parsed as
//! expressions ([`crate::expr`]).
//!
//! Functions, recipes, `when`, `each`, `apply`, `slot`, `yield`, `fail` and
//! `warn` are parsed into their statements (§8–§10). Recipe bodies and
//! content blocks are parsed without knowing where they will be expanded:
//! the evaluator checks the placement of their declarations and rules.
//!
//! Nested `rule`s and group at-rules nested in rules are kept as items;
//! the evaluator lowers them ([`crate::selector`]). Known at-rules in
//! recipe bodies and content blocks are kept too, and their placement is
//! checked where they are expanded.
//!
//! `load` statements are collected separately from the items
//! (LANGUAGE.md §14): they must precede every other statement (E7005), and
//! the compiler resolves them before evaluation. `export` marks a top-level
//! `bind`, `fn` or `recipe` as visible through namespaces.
//!
//! Recovery is bounded: every step consumes at least one token, bracket and
//! block nesting share the nesting-depth limit (E8002, which stops parsing),
//! and parsing stops once the diagnostic limit is reached.

use std::collections::{HashMap, HashSet};

use crate::css_text::{
    Seg, normalize_newlines, normalize_prelude, normalize_selector, render, trim_css_whitespace,
};
use crate::diagnostic::{Code, Diagnostic, Diagnostics};
use std::rc::Rc;

use crate::expr::{self, Call, Cx, DefHead, EachHead, Program, Resume, StmtError, Template};
use crate::lexer::{Lexer, Mode, Token, TokenKind as K, ident_value};
use crate::source::{FileId, Span};
use crate::syntax::{
    Apply, Branch, Context, Each, FnDef, Item, Load, RecipeDef, Stylesheet, VerbatimKind,
};

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

const NOT_IMPLEMENTED_NOTE: &str = "this build implements the plain-CSS subset, bindings, expressions, interpolation, functions, control flow, recipes, nesting, modules and the built-ins of docs/BUILTINS.md; the `color` built-ins are not implemented yet";

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
    /// A recipe body or content block, or a `when`/`each` block in one: a
    /// rule body whose placement is known only when it is expanded.
    Dynamic,
    /// A function body.
    Function,
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
            Ctx::Dynamic => "in a recipe body or content block",
            Ctx::Function => "in a function body",
        }
    }

    /// The context of a `when` or `each` block in this context.
    fn block(self) -> Ctx {
        match self {
            Ctx::TopLevel => Ctx::Stylesheet,
            other => other,
        }
    }

    /// The context recorded for an at-rule body.
    fn context(self) -> Context {
        match self {
            Ctx::Declarations => Context::Declarations,
            Ctx::Keyframes => Context::Keyframes,
            Ctx::Page => Context::Page,
            _ => Context::Stylesheet,
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
    /// `@layer` statements.
    Neutral,
    /// `@charset` and `/*!` comments, which may also precede `load`.
    Comment,
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
        selector: Template,
        span: Span,
        dynamic: bool,
    },
    AtRule {
        name: String,
        prelude: Template,
        context: Context,
        top_only: bool,
        span: Span,
    },
    Keyframe {
        selectors: Template,
        span: Span,
    },
    /// A `when` branch; `condition` is `None` for `otherwise`.
    When {
        branches: Vec<Branch>,
        condition: Option<(Program, Span)>,
    },
    Each(Box<EachHead>),
    Apply {
        call: Box<Call>,
        args: Program,
        span: Span,
    },
    Fn(Box<DefHead>),
    Recipe(Box<DefHead>),
    /// A block parsed only for its diagnostics.
    Discard,
}

impl Pending {
    /// Names a block binds besides its `bind` statements: parameters and
    /// loop variables share the block's scope (LANGUAGE.md §4.3).
    fn names(&self) -> Vec<(Rc<str>, Span)> {
        match self {
            Pending::Fn(head) | Pending::Recipe(head) => head
                .params
                .iter()
                .map(|p| (Rc::clone(&p.name), p.span))
                .collect(),
            Pending::Each(head) => head
                .key
                .iter()
                .chain(std::iter::once(&head.value))
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// A block opened by an item; [`Parser::run`] parses its contents.
#[derive(Debug)]
struct Opened {
    open: Token,
    ctx: Ctx,
    pending: Pending,
    /// Whether the block lies lexically inside a recipe body.
    recipe: bool,
}

/// An open block on the parser's explicit stack.
#[derive(Debug)]
struct Frame {
    open: Token,
    pending: Pending,
    ctx: Ctx,
    parent_ctx: Ctx,
    parent_recipe: bool,
    parent_items: Vec<Item>,
}

/// A `{` token at `at`.
fn brace(at: usize) -> Token {
    let start = text_offset(at);
    Token {
        kind: K::OpenBrace,
        start,
        end: start.saturating_add(1),
        interpolation: None,
    }
}

/// Parses one module. Diagnostics go to `diags`; the tree is meaningful only
/// if no error was reported. `entry` marks the entry module, the only one
/// that may start with `@charset`.
pub(crate) fn parse(
    text: &str,
    start: usize,
    file: FileId,
    entry: bool,
    max_depth: u32,
    diags: &mut Diagnostics,
) -> Stylesheet {
    let mut parser = Parser {
        text,
        file,
        lexer: Lexer::new(text, start, max_depth),
        diags,
        reported: HashSet::new(),
        in_recipe: false,
        max_depth,
        depth: 0,
        aborted: false,
        top_items: 0,
        import_blocker: None,
        namespace_blocker: None,
        entry,
        body_started: false,
        loads: Vec::new(),
        exports: Vec::new(),
    };
    let items = parser.run();
    Stylesheet {
        loads: parser.loads,
        exports: parser.exports,
        items,
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
    /// Whether the current block lies lexically inside a recipe body.
    in_recipe: bool,
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
    /// Whether this is the entry module.
    entry: bool,
    /// Whether a top-level item other than `load`, `@charset` or a comment
    /// was seen, after which `load` is E7005.
    body_started: bool,
    loads: Vec<Load>,
    exports: Vec<(Rc<str>, Span)>,
}

impl<'a> Parser<'a, '_> {
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

    fn not_in_function(&mut self, span: Span, what: &str) {
        self.report(
            Diagnostic::error(Code::E4014, format!("{what} is not allowed in a function body"))
                .with_primary(span)
                .with_note("functions only compute values: their bodies may contain `bind`, `when`, `each`, `yield`, `fail` and `warn`; use a recipe to emit CSS"),
        );
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
            _ => {}
        }
    }

    // ----- templates ---------------------------------------------------

    fn cx(&self) -> Cx<'a> {
        Cx {
            text: self.text,
            file: self.file,
            max_depth: self.max_depth,
            depth: self.depth,
            nesting: 0,
        }
    }

    /// Builds the template of a CSS region; interpolation errors are
    /// reported and leave an empty template.
    fn template(&mut self, segs: Vec<Seg>, ctx: expr::Ctx) -> Template {
        match expr::css_template(self.cx(), segs, ctx) {
            Ok(template) => template,
            Err(diagnostic) => {
                self.report(diagnostic);
                Template::default()
            }
        }
    }

    /// E1001 for a name bound twice directly in one block: by `bind`, by a
    /// top-level `fn` or `recipe`, or as one of `names` (parameters and
    /// loop variables, which share the block's scope).
    fn check_bindings(&mut self, items: &[Item], names: &[(Rc<str>, Span)]) {
        let mut first: HashMap<&str, Span> = HashMap::new();
        for (name, span) in names {
            first.entry(name).or_insert(*span);
        }
        let mut duplicates = Vec::new();
        for item in items {
            let (name, span) = match item {
                Item::Bind { name, span, .. } => (name, *span),
                Item::Fn(def) => (&def.name, def.span),
                Item::Recipe(def) => (&def.name, def.span),
                _ => continue,
            };
            match first.get(&**name) {
                Some(&earlier) => duplicates.push((name.to_string(), span, earlier)),
                None => {
                    first.insert(name, span);
                }
            }
        }
        for (name, span, earlier) in duplicates {
            self.report(
                Diagnostic::error(
                    Code::E1001,
                    format!("duplicate name: `{name}` is already bound in this scope"),
                )
                .with_primary(span)
                .with_label(earlier, "first bound here")
                .with_note(
                    "bindings are immutable; use a new name, or shadow it in a nested block",
                ),
            );
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
                K::CloseBrace => match frames.pop() {
                    Some(frame) => {
                        let body = std::mem::replace(&mut items, frame.parent_items);
                        self.check_bindings(&body, &frame.pending.names());
                        ctx = frame.parent_ctx;
                        self.in_recipe = frame.parent_recipe;
                        self.depth -= 1;
                        self.finish(frame.pending, frame.ctx, body, &mut items)
                    }
                    None => {
                        self.error(Code::E0005, self.span(token), "unmatched `}`");
                        None
                    }
                },
                K::Comment { .. } => {
                    let text = self.text_of(token);
                    if text.starts_with("/*!") {
                        if ctx == Ctx::Function {
                            self.not_in_function(self.span(token), "a `/*!` comment");
                        } else {
                            items.push(Item::Verbatim {
                                text: normalize_newlines(text),
                                kind: VerbatimKind::Comment,
                                span: self.span(token),
                            });
                            self.top_item(ctx, TopClass::Comment, token);
                        }
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
                    ctx: opened.ctx,
                    parent_ctx: ctx,
                    parent_recipe: self.in_recipe,
                    parent_items: std::mem::take(&mut items),
                });
                ctx = opened.ctx;
                self.in_recipe = opened.recipe;
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
            self.check_bindings(&body, &frame.pending.names());
            self.in_recipe = frame.parent_recipe;
            // The input is exhausted, so no `otherwise` can follow.
            let _ = self.finish(frame.pending, frame.ctx, body, &mut items);
        }
        self.depth = 0;
        let names: Vec<(Rc<str>, Span)> = self
            .loads
            .iter()
            .map(|load| (Rc::clone(&load.name), load.name_span))
            .collect();
        self.check_bindings(&items, &names);
        items
    }

    /// Turns a closed block into its item. After a `when` branch, an
    /// `otherwise` continues the statement with another block.
    fn finish(
        &mut self,
        pending: Pending,
        ctx: Ctx,
        body: Vec<Item>,
        items: &mut Vec<Item>,
    ) -> Option<Opened> {
        match pending {
            Pending::Rule {
                selector,
                span,
                dynamic,
            } => items.push(Item::Rule {
                selector,
                body,
                span,
                dynamic,
            }),
            Pending::AtRule {
                name,
                prelude,
                context,
                top_only,
                span,
            } => items.push(Item::AtRule {
                name,
                prelude,
                body: Some(body),
                context,
                top_only,
                span,
            }),
            Pending::Keyframe { selectors, span } => items.push(Item::Keyframe {
                selectors,
                body,
                span,
            }),
            Pending::When {
                mut branches,
                condition,
            } => {
                let Some((condition, span)) = condition else {
                    items.push(Item::When {
                        branches,
                        otherwise: Some(body),
                    });
                    return None;
                };
                branches.push(Branch {
                    condition,
                    span,
                    body,
                });
                return self.otherwise(branches, ctx, items);
            }
            Pending::Each(head) => {
                let head = *head;
                items.push(Item::Each(Box::new(Each {
                    key: head.key,
                    value: head.value,
                    iterable: head.iterable,
                    span: head.span,
                    body,
                })));
            }
            Pending::Apply { call, args, span } => items.push(Item::Apply(Box::new(Apply {
                call: *call,
                args,
                content: Some(body),
                span,
            }))),
            Pending::Fn(head) => {
                let head = *head;
                items.push(Item::Fn(Box::new(FnDef {
                    name: head.name,
                    span: head.span,
                    params: head.params,
                    returns: head.returns,
                    body,
                })));
            }
            Pending::Recipe(head) => {
                let head = *head;
                items.push(Item::Recipe(Box::new(RecipeDef {
                    name: head.name,
                    span: head.span,
                    params: head.params,
                    body,
                })));
            }
            Pending::Discard => {}
        }
        None
    }

    /// After a `when` branch: `otherwise when … {`, `otherwise {` or the end
    /// of the statement.
    fn otherwise(
        &mut self,
        branches: Vec<Branch>,
        ctx: Ctx,
        items: &mut Vec<Item>,
    ) -> Option<Opened> {
        let save = self.lexer.pos();
        let next = self.next_significant(Mode::Weave);
        if self.aborted || !(next.kind == K::Ident && self.text_of(next) == "otherwise") {
            self.unread_to(text_offset(save));
            items.push(Item::When {
                branches,
                otherwise: None,
            });
            return None;
        }
        let after = self.next_significant(Mode::Weave);
        let when_start = match after.kind {
            K::Ident if self.text_of(after) == "when" => Some(after.end as usize),
            K::Function if self.text_of(after) == "when(" => Some(after.end as usize - 1),
            _ => None,
        };
        let recipe = self.in_recipe;
        if let Some(start) = when_start {
            return match expr::parse_when_head(self.cx(), start) {
                Ok(head) => {
                    self.unread_to(text_offset(head.open + 1));
                    Some(Opened {
                        open: brace(head.open),
                        ctx,
                        pending: Pending::When {
                            branches,
                            condition: Some((head.condition, head.span)),
                        },
                        recipe,
                    })
                }
                Err(error) => {
                    items.push(Item::When {
                        branches,
                        otherwise: None,
                    });
                    self.head_failed(*error, after, "when", ctx)
                }
            };
        }
        if after.kind == K::OpenBrace {
            return Some(Opened {
                open: after,
                ctx,
                pending: Pending::When {
                    branches,
                    condition: None,
                },
                recipe,
            });
        }
        self.error(
            Code::E0101,
            self.span(after),
            "expected `when` or a `{` block after `otherwise`",
        );
        self.unread(after);
        items.push(Item::When {
            branches,
            otherwise: None,
        });
        None
    }

    /// Reports a statement head that failed to parse and resumes after it:
    /// at an offset, by parsing its block (in `ctx`) only for diagnostics,
    /// or by skipping the statement.
    fn head_failed(
        &mut self,
        error: StmtError,
        keyword: Token,
        word: &str,
        ctx: Ctx,
    ) -> Option<Opened> {
        self.report(error.diagnostic);
        match error.resume {
            Resume::At(at) => {
                self.unread_to(text_offset(at));
                None
            }
            Resume::Block(open) => {
                self.unread_to(text_offset(open + 1));
                Some(Opened {
                    open: brace(open),
                    ctx,
                    pending: Pending::Discard,
                    recipe: self.in_recipe,
                })
            }
            Resume::Skip => {
                self.unread_to(keyword.end);
                self.skip_statement(word);
                None
            }
        }
    }

    fn top_item(&mut self, ctx: Ctx, class: TopClass, token: Token) {
        if ctx != Ctx::TopLevel {
            return;
        }
        self.top_items += 1;
        if class != TopClass::Comment {
            self.body_started = true;
        }
        let span = self.span(token);
        match class {
            TopClass::Other => {
                self.import_blocker.get_or_insert(span);
                self.namespace_blocker.get_or_insert(span);
            }
            TopClass::Namespace => {
                self.import_blocker.get_or_insert(span);
            }
            TopClass::Import | TopClass::Neutral | TopClass::Comment => {}
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
        if first.kind == K::Ident && self.text_of(first) == "otherwise" {
            self.error(
                Code::E0101,
                span,
                "`otherwise` must directly follow the block of a `when` statement",
            );
        } else if end == End::BadBlock && first.kind != K::OpenBrace {
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
            self.body_started = true;
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
            Ctx::Function => self.not_in_function(name_span, "a declaration"),
            Ctx::Rule | Ctx::Declarations | Ctx::Page | Ctx::Dynamic => {}
        }
        let (value, important) = split_important(&tokens, self.text);
        let name_segs = render(name, self.text);
        let name = self.template(name_segs, expr::Ctx::Name);
        let value_segs = render(value, self.text);
        let value = self.template(value_segs, expr::Ctx::Value);
        items.push(Item::Declaration {
            name,
            value,
            important,
            custom,
            span: name_span,
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
        if ctx == Ctx::TopLevel && !matches!(word.as_str(), "rule" | "opaque") {
            // Statements emit nothing themselves but still precede a later
            // `@charset`.
            self.top_items += 1;
            if word != "load" {
                self.body_started = true;
            }
        }
        match word.as_str() {
            "rule" | "opaque" | "apply" if ctx == Ctx::Function => {
                self.not_in_function(span, &format!("`{word}`"));
            }
            "rule" => return self.rule(keyword, ctx),
            "bind" => {
                self.bind(keyword, items);
                return None;
            }
            "opaque" => {
                self.opaque(keyword, ctx, items);
                return None;
            }
            "when" => return self.when(keyword, ctx),
            "each" => return self.each(keyword, ctx),
            "apply" if ctx == Ctx::Keyframes => {
                self.error(Code::E6005, span, "`apply` is not allowed in `@keyframes`");
            }
            "apply" => return self.apply(keyword, items),
            "yield" if ctx == Ctx::Function => {
                self.expr_statement(keyword, &word, items);
                return None;
            }
            "yield" => self.error(
                Code::E4013,
                span,
                "`yield` is allowed only in a function body",
            ),
            "fail" | "warn" => {
                self.expr_statement(keyword, &word, items);
                return None;
            }
            "slot" => {
                self.slot(keyword, ctx, items);
                return None;
            }
            "fn" | "recipe" | "load" | "export" if ctx != Ctx::TopLevel => {
                self.error(
                    Code::E0104,
                    span,
                    format!("`{word}` is allowed only at module top level"),
                );
            }
            "fn" | "recipe" => return self.definition(keyword, &word),
            "load" => {
                self.load(keyword);
                return None;
            }
            "export" => return self.export(keyword, items),
            _ => self.not_implemented(span, &format!("`{word}` statements are")),
        }
        self.skip_statement(&word);
        None
    }

    /// `yield`, `fail` or `warn` and its expression.
    fn expr_statement(&mut self, keyword: Token, word: &str, items: &mut Vec<Item>) {
        let span = self.span(keyword);
        match expr::parse_expr_stmt(self.cx(), keyword.end as usize, word) {
            Ok(stmt) => {
                self.unread_to(text_offset(stmt.end));
                items.push(if word == "yield" {
                    Item::Yield {
                        value: stmt.program,
                        span,
                    }
                } else {
                    Item::Report {
                        warning: word == "warn",
                        value: stmt.program,
                        span,
                    }
                });
            }
            // Expression statements never resume at a block, so the context
            // is unused.
            Err(error) => {
                let _ = self.head_failed(*error, keyword, word, Ctx::Stylesheet);
            }
        }
    }

    /// `slot;` (LANGUAGE.md §9).
    fn slot(&mut self, keyword: Token, ctx: Ctx, items: &mut Vec<Item>) {
        let span = self.span(keyword);
        let next = self.next_significant(Mode::Weave);
        if next.kind != K::Semicolon {
            self.error(
                Code::E0102,
                Span::new(self.file, keyword.end, keyword.end),
                "expected `;` after `slot`",
            );
            self.unread(next);
        }
        match ctx {
            Ctx::Function => self.not_in_function(span, "`slot`"),
            Ctx::Rule | Ctx::Dynamic if self.in_recipe => items.push(Item::Slot(span)),
            Ctx::TopLevel | Ctx::Stylesheet | Ctx::Rule | Ctx::Dynamic => {
                self.error(Code::E4012, span, "`slot` is allowed only in a recipe body");
            }
            _ => self.error(
                Code::E6005,
                span,
                format!("`slot` is not allowed {}", ctx.describe()),
            ),
        }
    }

    /// `when condition {` (LANGUAGE.md §10).
    fn when(&mut self, keyword: Token, ctx: Ctx) -> Option<Opened> {
        let ctx = ctx.block();
        match expr::parse_when_head(self.cx(), keyword.end as usize) {
            Ok(head) => {
                self.unread_to(text_offset(head.open + 1));
                Some(Opened {
                    open: brace(head.open),
                    ctx,
                    pending: Pending::When {
                        branches: Vec::new(),
                        condition: Some((head.condition, head.span)),
                    },
                    recipe: self.in_recipe,
                })
            }
            Err(error) => self.head_failed(*error, keyword, "when", ctx),
        }
    }

    /// `each [key,] value in expression {` (LANGUAGE.md §10).
    fn each(&mut self, keyword: Token, ctx: Ctx) -> Option<Opened> {
        let ctx = ctx.block();
        match expr::parse_each_head(self.cx(), keyword.end as usize) {
            Ok(head) => {
                self.unread_to(text_offset(head.open + 1));
                Some(Opened {
                    open: brace(head.open),
                    ctx,
                    pending: Pending::Each(Box::new(head)),
                    recipe: self.in_recipe,
                })
            }
            Err(error) => self.head_failed(*error, keyword, "each", ctx),
        }
    }

    /// `apply name(args);` or `apply name(args) { content }` (LANGUAGE.md §9).
    fn apply(&mut self, keyword: Token, items: &mut Vec<Item>) -> Option<Opened> {
        let span = self.span(keyword);
        match expr::parse_apply_head(self.cx(), keyword.end as usize) {
            Ok(head) => match head.open {
                Some(open) => {
                    self.unread_to(text_offset(open + 1));
                    Some(Opened {
                        open: brace(open),
                        ctx: Ctx::Dynamic,
                        pending: Pending::Apply {
                            call: Box::new(head.call),
                            args: head.args,
                            span,
                        },
                        recipe: self.in_recipe,
                    })
                }
                None => {
                    self.unread_to(text_offset(head.end));
                    items.push(Item::Apply(Box::new(Apply {
                        call: head.call,
                        args: head.args,
                        content: None,
                        span,
                    })));
                    None
                }
            },
            Err(error) => self.head_failed(*error, keyword, "apply", Ctx::Dynamic),
        }
    }

    /// `fn name(params) -> type {` or `recipe name(params) {` (LANGUAGE.md
    /// §8, §9).
    fn definition(&mut self, keyword: Token, word: &str) -> Option<Opened> {
        let function = word == "fn";
        let ctx = if function {
            Ctx::Function
        } else {
            Ctx::Dynamic
        };
        match expr::parse_def_head(self.cx(), keyword.end as usize, function) {
            Ok(head) => {
                self.unread_to(text_offset(head.open + 1));
                let open = brace(head.open);
                let head = Box::new(head);
                Some(Opened {
                    open,
                    ctx,
                    pending: if function {
                        Pending::Fn(head)
                    } else {
                        Pending::Recipe(head)
                    },
                    recipe: !function,
                })
            }
            Err(error) => self
                .head_failed(*error, keyword, word, ctx)
                .map(|opened| Opened {
                    recipe: !function,
                    ..opened
                }),
        }
    }

    /// `load "./path.weave" as name;` (LANGUAGE.md §14). The path is
    /// validated and resolved by the compiler, not here.
    fn load(&mut self, keyword: Token) {
        let span = self.span(keyword);
        if self.body_started {
            self.report(
                Diagnostic::error(
                    Code::E7005,
                    "`load` must come before every other statement of the module",
                )
                .with_primary(span)
                .with_note("only comments and an initial `@charset` may precede `load` statements; move it to the top"),
            );
        }
        let path = self.next_skipped(true);
        if path.kind != (K::String { terminated: true }) {
            self.report(
                Diagnostic::error(Code::E0101, "expected a quoted module path after `load`")
                    .with_primary(self.span(path))
                    .with_note("write `load \"./file.weave\" as name;`"),
            );
            self.unread(path);
            self.skip_statement("load");
            return;
        }
        let raw = self.text_of(path);
        let path_text = raw
            .get(1..raw.len().saturating_sub(1))
            .unwrap_or("")
            .to_string();
        let path_span = self.span(path);

        let as_token = self.next_skipped(true);
        if !(as_token.kind == K::Ident && self.text_of(as_token) == "as") {
            self.report(
                Diagnostic::error(
                    Code::E0101,
                    "expected `as` and a namespace name after the module path",
                )
                .with_primary(self.span(as_token))
                .with_note(
                    "every loaded module is bound to a namespace: `load \"./file.weave\" as name;`",
                ),
            );
            self.unread(as_token);
            self.skip_statement("load");
            return;
        }
        let name = self.next_skipped(true);
        let name_text = self.text_of(name).to_string();
        let name_span = self.span(name);
        if name.kind != K::Ident || !expr::is_identifier(&name_text) {
            self.error(
                Code::E0101,
                name_span,
                "expected a namespace name after `as`",
            );
            self.unread(name);
            self.skip_statement("load");
            return;
        }
        if expr::KEYWORDS.contains(&name_text.as_str()) {
            self.error(
                Code::E0103,
                name_span,
                format!("`{name_text}` is a keyword and cannot name a namespace"),
            );
        }
        let end = self.next_skipped(true);
        if end.kind != K::Semicolon {
            self.error(
                Code::E0102,
                self.span(end),
                "expected `;` after the `load` statement",
            );
            self.unread(end);
            self.skip_statement("load");
        }
        if let Some(earlier) = self
            .loads
            .iter()
            .find(|load| *load.name == *name_text)
            .map(|load| load.name_span)
        {
            self.report(
                Diagnostic::error(
                    Code::E1001,
                    format!("duplicate name: `{name_text}` is already bound in this scope"),
                )
                .with_primary(name_span)
                .with_label(earlier, "first bound here")
                .with_note(
                    "one module may be loaded under two names, but each name must be unique",
                ),
            );
            return;
        }
        self.loads.push(Load {
            path: path_text,
            path_span,
            name: name_text.into(),
            name_span,
            span,
        });
    }

    /// `export bind …;`, `export fn …` or `export recipe …`.
    fn export(&mut self, keyword: Token, items: &mut Vec<Item>) -> Option<Opened> {
        let next = self.next_skipped(true);
        let word = if next.kind == K::Ident {
            self.text_of(next).to_string()
        } else {
            String::new()
        };
        match word.as_str() {
            "bind" => {
                let before = items.len();
                self.bind(next, items);
                if items.len() > before {
                    if let Some(Item::Bind { name, span, .. }) = items.last() {
                        self.exports.push((Rc::clone(name), *span));
                    }
                }
                None
            }
            "fn" | "recipe" => {
                let opened = self.definition(next, &word);
                if let Some(Opened {
                    pending: Pending::Fn(head) | Pending::Recipe(head),
                    ..
                }) = &opened
                {
                    self.exports.push((Rc::clone(&head.name), head.span));
                }
                opened
            }
            _ => {
                let note = if word == "load" {
                    "re-exports are not part of Weave v0.1; load the module where it is needed"
                } else {
                    "only top-level `bind`, `fn` and `recipe` definitions can be exported"
                };
                self.report(
                    Diagnostic::error(
                        Code::E0101,
                        "`export` must be followed by `bind`, `fn` or `recipe`",
                    )
                    .with_primary(self.span(keyword))
                    .with_note(note),
                );
                self.unread(next);
                let first = self.next_skipped(true);
                self.skip(first, true);
                None
            }
        }
    }

    /// `bind name = expression;` (LANGUAGE.md §4.2).
    fn bind(&mut self, keyword: Token, items: &mut Vec<Item>) {
        match expr::parse_bind(self.cx(), keyword.end as usize) {
            Ok(stmt) => {
                self.unread_to(text_offset(stmt.end));
                items.push(Item::Bind {
                    name: stmt.name,
                    span: stmt.name_span,
                    value: stmt.program,
                });
            }
            Err(error) => {
                self.report(error.diagnostic);
                match error.resume {
                    Some(at) => self.unread_to(text_offset(at)),
                    None => {
                        self.unread_to(keyword.end);
                        self.skip_statement("bind");
                    }
                }
            }
        }
    }

    /// Skips the extent of a statement this build does not implement.
    fn skip_statement(&mut self, word: &str) {
        match word {
            "when" | "each" | "fn" | "recipe" | "rule" | "opaque" => {
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
            // Checked where the recipe or content block is expanded.
            Ctx::Dynamic => {}
            Ctx::Function => {
                self.not_in_function(span, "`rule`");
                keep = false;
            }
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
            // Lowered against the enclosing rule by the evaluator.
            Ctx::Rule => {}
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
            let segs = normalize_selector(&tokens, self.text);
            Pending::Rule {
                selector: self.template(segs, expr::Ctx::Selector),
                span,
                dynamic: ctx == Ctx::Dynamic,
            }
        } else {
            Pending::Discard
        };
        Some(Opened {
            open,
            ctx: Ctx::Rule,
            pending,
            recipe: self.in_recipe,
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
        // In recipe bodies and content blocks the placement is checked where
        // they are expanded.
        let allowed = ctx.is_stylesheet() || ctx == Ctx::Rule || ctx == Ctx::Dynamic;
        if ctx == Ctx::Function {
            self.not_in_function(span, "`opaque`");
        } else if !allowed {
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
            items.push(Item::Verbatim {
                text: content,
                kind: VerbatimKind::Opaque,
                span,
            });
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

        if ctx == Ctx::Function {
            self.not_in_function(span, &format!("`{raw}`"));
            self.verbatim_at_rule(at);
            return None;
        }
        if matches!(kind, AtKind::Unknown | AtKind::Preserved) {
            // In recipe bodies and content blocks, checked where expanded.
            if kind == AtKind::Preserved && !ctx.is_stylesheet() && ctx != Ctx::Dynamic {
                self.error(
                    Code::E6005,
                    span,
                    format!("`{raw}` is not allowed {}", ctx.describe()),
                );
                self.verbatim_at_rule(at);
                return None;
            }
            let verbatim = if kind == AtKind::Preserved && ctx == Ctx::Dynamic {
                VerbatimKind::TopAtRule
            } else {
                VerbatimKind::AtRule
            };
            if let Some(text) = self.verbatim_at_rule(at) {
                items.push(Item::Verbatim {
                    text,
                    kind: verbatim,
                    span,
                });
            }
            self.top_item(ctx, TopClass::Other, at);
            return None;
        }

        let (prelude_tokens, end) = self.scan(Scan::Prelude);
        let prelude = normalize_prelude(&prelude_tokens, self.text);
        let prelude = self.template(prelude, expr::Ctx::Value);
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
            let dynamic_layer = kind == AtKind::Layer && ctx == Ctx::Dynamic;
            if dynamic_layer || self.statement_at_rule_allowed(kind, at, ctx, &prelude_tokens) {
                items.push(Item::AtRule {
                    name: raw,
                    prelude,
                    body: None,
                    context: Context::Stylesheet,
                    top_only: dynamic_layer,
                    span,
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
        let mut context = None;
        let mut top_only = false;
        let (body_ctx, keep) = match (kind, ctx) {
            (AtKind::Group | AtKind::Layer, Ctx::TopLevel | Ctx::Stylesheet) => {
                (Ctx::Stylesheet, true)
            }
            // Lowered by wrapping the enclosing rule (CSS-COMPATIBILITY.md
            // §4.3); in recipe bodies and content blocks, wherever they are
            // expanded.
            (AtKind::Group | AtKind::Layer, Ctx::Rule | Ctx::Dynamic) => {
                context = Some(Context::Enclosing);
                (ctx, true)
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
            // Allowed only at stylesheet level: checked where the recipe or
            // content block is expanded.
            (
                AtKind::Scope | AtKind::Descriptors | AtKind::Keyframes | AtKind::Page,
                Ctx::Dynamic,
            ) => {
                top_only = true;
                (body_context(kind), true)
            }
            (_, _) => {
                self.error(
                    Code::E6005,
                    span,
                    format!("`{raw}` is not allowed {}", ctx.describe()),
                );
                (body_context(kind), false)
            }
        };
        let context = context.unwrap_or_else(|| body_ctx.context());
        self.top_item(ctx, TopClass::Other, at);
        let pending = if keep {
            Pending::AtRule {
                name: raw,
                prelude,
                context,
                top_only,
                span,
            }
        } else {
            Pending::Discard
        };
        Some(Opened {
            open,
            ctx: body_ctx,
            pending,
            recipe: self.in_recipe,
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
                self.top_item(ctx, TopClass::Comment, at);
                if first && !self.entry {
                    self.report(
                        Diagnostic::error(
                            Code::E6008,
                            "`@charset` is allowed only in the entry module",
                        )
                        .with_primary(span)
                        .with_note("a loaded module's CSS follows other CSS, where `@charset` is invalid; remove it"),
                    );
                    return false;
                }
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
            recipe: self.in_recipe,
            pending: Pending::Keyframe {
                span: self.span(first),
                selectors: {
                    let segs = normalize_prelude(&tokens, self.text);
                    self.template(segs, expr::Ctx::Value)
                },
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

fn text_offset(pos: usize) -> u32 {
    u32::try_from(pos).unwrap_or(u32::MAX)
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
