//! The syntax tree of one module.
//!
//! CSS text that Weave processes (selectors, declaration names and values,
//! known at-rule preludes, keyframe selectors) is held as a [`Template`]:
//! output-ready text — selectors and preludes normalized (LANGUAGE.md §12,
//! §15), values as written with line breaks as LF — with interpolations in
//! place. Verbatim regions are exact source text. The evaluator turns the
//! tree into CSS ([`crate::ir`]).

use std::rc::Rc;

use crate::expr::{Call, Program, Template};
use crate::source::Span;
use crate::value::Type;

/// One parsed module.
#[derive(Debug, Default)]
pub(crate) struct Stylesheet {
    pub items: Vec<Item>,
}

/// The kind of block an at-rule body is (CSS-COMPATIBILITY.md §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Context {
    Stylesheet,
    /// The body of a group at-rule written in a rule body, recipe body or
    /// content block: the context it is expanded in (stylesheet or rule
    /// body).
    Enclosing,
    Declarations,
    Keyframes,
    Page,
}

/// What produced a verbatim item; decides where it may be placed when it
/// comes from a recipe body or a content block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VerbatimKind {
    /// A `/*!` comment.
    Comment,
    /// The contents of an `opaque { }` block.
    Opaque,
    /// An unknown at-rule.
    AtRule,
    /// An at-rule preserved verbatim that is allowed only at stylesheet
    /// level (`@font-feature-values`), written in a recipe body or content
    /// block.
    TopAtRule,
}

#[derive(Debug)]
pub(crate) enum Item {
    /// `rule <selector list> { … }`. `dynamic` marks a rule written
    /// directly in a recipe body or content block: whether it is a
    /// top-level or a nested rule is known only where it is expanded.
    Rule {
        selector: Template,
        body: Vec<Item>,
        span: Span,
        dynamic: bool,
    },
    Declaration {
        name: Template,
        value: Template,
        important: bool,
        /// Whether the name, as written, is a custom property (`--*`).
        custom: bool,
        span: Span,
    },
    /// A known at-rule. `body` is `None` for statement at-rules such as
    /// `@import`. `name` includes the `@` and is written as in the source.
    AtRule {
        name: String,
        prelude: Template,
        body: Option<Vec<Item>>,
        context: Context,
        /// Written in a recipe body or content block, and allowed only at
        /// stylesheet level: checked where it is expanded.
        top_only: bool,
        span: Span,
    },
    /// A keyframe block inside `@keyframes`.
    Keyframe {
        selectors: Template,
        body: Vec<Item>,
        span: Span,
    },
    /// Text emitted exactly as given: `opaque` contents, unknown at-rules and
    /// `/*!` comments.
    Verbatim {
        text: String,
        kind: VerbatimKind,
        span: Span,
    },
    /// `bind name = expression;`.
    Bind {
        name: Rc<str>,
        span: Span,
        value: Program,
    },
    /// `when … { } otherwise when … { } otherwise { }`.
    When {
        branches: Vec<Branch>,
        otherwise: Option<Vec<Item>>,
    },
    Each(Box<Each>),
    Apply(Box<Apply>),
    /// `slot;`.
    Slot(Span),
    /// `yield expression;` (function bodies only).
    Yield {
        value: Program,
        span: Span,
    },
    /// `fail expression;` or `warn expression;`.
    Report {
        warning: bool,
        value: Program,
        span: Span,
    },
    /// `fn` (module top level only).
    Fn(Box<FnDef>),
    /// `recipe` (module top level only).
    Recipe(Box<RecipeDef>),
}

/// One `when` or `otherwise when` branch.
#[derive(Debug)]
pub(crate) struct Branch {
    pub condition: Program,
    /// The condition expression.
    pub span: Span,
    pub body: Vec<Item>,
}

/// `each value in …` or `each key, value in …`.
#[derive(Debug)]
pub(crate) struct Each {
    pub key: Option<(Rc<str>, Span)>,
    pub value: (Rc<str>, Span),
    pub iterable: Program,
    /// The iterated expression.
    pub span: Span,
    pub body: Vec<Item>,
}

/// `apply name(args);` or `apply name(args) { content }`.
#[derive(Debug)]
pub(crate) struct Apply {
    /// The recipe and its arguments; `args` computes one value per
    /// argument.
    pub call: Call,
    pub args: Program,
    pub content: Option<Vec<Item>>,
    /// The `apply` keyword.
    pub span: Span,
}

#[derive(Debug)]
pub(crate) struct Param {
    pub name: Rc<str>,
    pub span: Span,
    pub ty: Option<Type>,
    pub default: Option<Program>,
}

#[derive(Debug)]
pub(crate) struct FnDef {
    pub name: Rc<str>,
    pub span: Span,
    pub params: Vec<Param>,
    /// The return type and its span.
    pub returns: Option<(Type, Span)>,
    pub body: Vec<Item>,
}

#[derive(Debug)]
pub(crate) struct RecipeDef {
    pub name: Rc<str>,
    pub span: Span,
    pub params: Vec<Param>,
    pub body: Vec<Item>,
}
