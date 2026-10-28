//! The evaluated CSS tree that the emitter writes. Every text field is
//! output-ready CSS. Each node records where it came from, for source maps.

use std::rc::Rc;

use crate::source::Span;

/// One recipe application on the path to a node, innermost first.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CallSite {
    /// The `apply` keyword.
    pub span: Span,
    pub parent: Option<Rc<CallSite>>,
}

impl Drop for CallSite {
    /// Releases long chains iteratively rather than recursively.
    fn drop(&mut self) {
        let mut parent = self.parent.take();
        while let Some(site) = parent {
            match Rc::try_unwrap(site) {
                Ok(mut site) => parent = site.parent.take(),
                Err(_) => break,
            }
        }
    }
}

/// Where a node came from: the item that produced it (its definition
/// location, inside a recipe body or content block when expanded) and the
/// chain of recipe applications that expanded it (its invocation
/// locations).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Origin {
    pub span: Span,
    pub calls: Option<Rc<CallSite>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Rule {
        selector: String,
        body: Vec<Node>,
        origin: Option<Origin>,
    },
    Declaration {
        name: String,
        value: String,
        important: bool,
        origin: Option<Origin>,
    },
    /// `body` is `None` for statement at-rules. `name` includes the `@`.
    AtRule {
        name: String,
        prelude: String,
        body: Option<Vec<Node>>,
        origin: Option<Origin>,
    },
    Keyframe {
        selectors: String,
        body: Vec<Node>,
        origin: Option<Origin>,
    },
    Verbatim {
        text: String,
        origin: Option<Origin>,
    },
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
