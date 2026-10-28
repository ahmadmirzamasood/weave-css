//! The syntax tree of one module.
//!
//! CSS text that Weave processes (selectors, declaration names and values,
//! known at-rule preludes, keyframe selectors) is held as a [`Template`]:
//! output-ready text — selectors and preludes normalized (LANGUAGE.md §12,
//! §15), values as written with line breaks as LF — with interpolations in
//! place. Verbatim regions are exact source text. The evaluator turns the
//! tree into CSS ([`crate::ir`]).

use std::rc::Rc;

use crate::expr::{Program, Template};
use crate::source::Span;

/// One parsed module.
#[derive(Debug, Default)]
pub(crate) struct Stylesheet {
    pub items: Vec<Item>,
}

#[derive(Debug)]
pub(crate) enum Item {
    /// `rule <selector list> { … }`.
    Rule { selector: Template, body: Vec<Item> },
    Declaration {
        name: Template,
        value: Template,
        important: bool,
        /// Whether the name, as written, is a custom property (`--*`).
        custom: bool,
    },
    /// A known at-rule. `body` is `None` for statement at-rules such as
    /// `@import`. `name` includes the `@` and is written as in the source.
    AtRule {
        name: String,
        prelude: Template,
        body: Option<Vec<Item>>,
    },
    /// A keyframe block inside `@keyframes`.
    Keyframe {
        selectors: Template,
        body: Vec<Item>,
    },
    /// Text emitted exactly as given: `opaque` contents, unknown at-rules and
    /// `/*!` comments.
    Verbatim(String),
    /// `bind name = expression;`.
    Bind {
        name: Rc<str>,
        span: Span,
        value: Program,
    },
}
