//! The evaluated CSS tree that the emitter writes. Every text field is
//! output-ready CSS.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Rule {
        selector: String,
        body: Vec<Node>,
    },
    Declaration {
        name: String,
        value: String,
        important: bool,
    },
    /// `body` is `None` for statement at-rules. `name` includes the `@`.
    AtRule {
        name: String,
        prelude: String,
        body: Option<Vec<Node>>,
    },
    Keyframe {
        selectors: String,
        body: Vec<Node>,
    },
    Verbatim(String),
}

impl Node {
    /// Whether emitting this node writes anything. A style rule without
    /// declarations or verbatim items is omitted (LANGUAGE.md §11); every
    /// other node, including an empty at-rule block, is written.
    pub fn emits(&self) -> bool {
        match self {
            Node::Rule { body, .. } => body.iter().any(Node::emits),
            _ => true,
        }
    }
}
