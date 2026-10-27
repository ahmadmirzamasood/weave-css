//! The syntax tree of the supported plain-CSS subset.
//!
//! Text fields hold output-ready CSS: selectors and known at-rule preludes
//! are normalized (LANGUAGE.md §12, §15), values keep their source text with
//! line breaks written as LF, and verbatim regions are exact source text.

/// One parsed module.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stylesheet {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    /// `rule <selector list> { … }`.
    Rule { selector: String, body: Vec<Item> },
    Declaration {
        name: String,
        value: String,
        important: bool,
    },
    /// A known at-rule. `body` is `None` for statement at-rules such as
    /// `@import`. `name` includes the `@` and is written as in the source.
    AtRule {
        name: String,
        prelude: String,
        body: Option<Vec<Item>>,
    },
    /// A keyframe block inside `@keyframes`.
    Keyframe { selectors: String, body: Vec<Item> },
    /// Text emitted exactly as given: `opaque` contents, unknown at-rules and
    /// `/*!` comments.
    Verbatim(String),
}

impl Item {
    /// Whether emitting this item writes anything. A style rule without
    /// declarations or verbatim items is omitted (LANGUAGE.md §11); every
    /// other item, including an empty at-rule block, is written.
    pub fn emits(&self) -> bool {
        match self {
            Item::Rule { body, .. } => body.iter().any(Item::emits),
            _ => true,
        }
    }
}
