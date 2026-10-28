//! CSS output (LANGUAGE.md §15).
//!
//! UTF-8, LF line breaks, two-space indentation, one empty line between
//! top-level items and none inside blocks, a single trailing LF when the
//! output is non-empty. Verbatim text is indented on its first line only.
//! Output is bounded by the `output-bytes` limit (E8006).
//!
//! When source maps are requested, the emitter records one mapping at the
//! first byte of every written node (after its indentation), pointing at the
//! node's origin; see `source_map.rs` for the provenance policy.

use crate::diagnostic::{Code, Diagnostic};
use crate::ir::{Node, Origin};

/// A generated position (0-based line, 0-based UTF-16 column) and the origin
/// of the node written there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawMapping {
    pub line: u32,
    pub column: u32,
    pub origin: Origin,
}

/// Emits a stylesheet, failing with E8006 when the output would exceed
/// `limit` bytes. With `map`, also returns the mappings in output order.
pub(crate) fn emit(
    nodes: &[Node],
    limit: u64,
    map: bool,
) -> Result<(String, Vec<RawMapping>), Diagnostic> {
    let mut writer = Writer {
        out: String::new(),
        limit: usize::try_from(limit).unwrap_or(usize::MAX),
        overflow: false,
        map,
        line: 0,
        column: 0,
        mappings: Vec::new(),
    };
    // An explicit stack of open blocks (not recursion), so the deepest
    // allowed nesting cannot exhaust the thread stack.
    let mut stack: Vec<std::slice::Iter<'_, Node>> = vec![nodes.iter()];
    let mut first = true;
    while !writer.overflow {
        let depth = stack.len().saturating_sub(1);
        let Some(level) = stack.last_mut() else {
            break;
        };
        let Some(item) = level.next() else {
            stack.pop();
            if let Some(parent) = depth.checked_sub(1) {
                writer.indent(parent);
                writer.push(
                    "}
",
                );
            }
            continue;
        };
        if !item.emits() {
            continue;
        }
        if depth == 0 {
            if !first {
                writer.push(
                    "
",
                );
            }
            first = false;
        }
        if let Some(body) = writer.item(item, depth) {
            stack.push(body.iter());
        }
    }
    if writer.overflow {
        return Err(Diagnostic::error(
            Code::E8006,
            format!("the emitted CSS exceeds the output limit of {limit} bytes"),
        )
        .with_note("raise it with the `output-bytes` limit"));
    }
    Ok((writer.out, writer.mappings))
}

struct Writer {
    out: String,
    limit: usize,
    overflow: bool,
    /// Whether to track positions and record mappings.
    map: bool,
    /// The current 0-based line and UTF-16 column, tracked only with `map`.
    line: u32,
    column: u32,
    mappings: Vec<RawMapping>,
}

impl Writer {
    fn push(&mut self, text: &str) {
        if self.overflow || self.out.len().saturating_add(text.len()) > self.limit {
            self.overflow = true;
            return;
        }
        self.out.push_str(text);
        if self.map {
            // Output is at most `u32::MAX` bytes (the limit's maximum), so
            // neither counter can overflow.
            for c in text.chars() {
                if c == '\n' {
                    self.line = self.line.saturating_add(1);
                    self.column = 0;
                } else {
                    let units = u32::try_from(c.len_utf16()).unwrap_or(2);
                    self.column = self.column.saturating_add(units);
                }
            }
        }
    }

    /// Records a mapping from the current position to `origin`.
    fn mark(&mut self, origin: Option<&Origin>) {
        if let (true, Some(origin)) = (self.map, origin) {
            self.mappings.push(RawMapping {
                line: self.line,
                column: self.column,
                origin: origin.clone(),
            });
        }
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.push("  ");
        }
    }

    /// Writes an item, or the header of a block item and returns its body.
    fn item<'a>(&mut self, item: &'a Node, depth: usize) -> Option<&'a [Node]> {
        self.indent(depth);
        match item {
            Node::Rule { origin, .. }
            | Node::Declaration { origin, .. }
            | Node::AtRule { origin, .. }
            | Node::Keyframe { origin, .. }
            | Node::Verbatim { origin, .. } => self.mark(origin.as_ref()),
        }
        match item {
            Node::Declaration {
                name,
                value,
                important,
                ..
            } => {
                self.push(name);
                self.push(":");
                if !value.is_empty() {
                    self.push(" ");
                    self.push(value);
                }
                if *important {
                    self.push(" !important");
                }
                self.push(
                    ";
",
                );
                None
            }
            Node::Verbatim { text, .. } => {
                self.push(text);
                self.push(
                    "
",
                );
                None
            }
            Node::Rule {
                selector: header,
                body,
                ..
            }
            | Node::Keyframe {
                selectors: header,
                body,
                ..
            } => {
                self.push(header);
                self.push(
                    " {
",
                );
                Some(body)
            }
            Node::AtRule {
                name,
                prelude,
                body,
                ..
            } => {
                self.push(name);
                if !prelude.is_empty() {
                    self.push(" ");
                    self.push(prelude);
                }
                match body {
                    Some(body) => {
                        self.push(
                            " {
",
                        );
                        Some(body)
                    }
                    None => {
                        self.push(
                            ";
",
                        );
                        None
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(name: &str, value: &str, important: bool) -> Node {
        Node::Declaration {
            name: name.into(),
            value: value.into(),
            important,
            origin: None,
        }
    }

    fn verbatim(text: &str) -> Node {
        Node::Verbatim {
            text: text.into(),
            origin: None,
        }
    }

    fn rule(selector: &str, body: Vec<Node>) -> Node {
        Node::Rule {
            selector: selector.into(),
            body,
            origin: None,
        }
    }

    fn at_rule(name: &str, prelude: &str, body: Option<Vec<Node>>) -> Node {
        Node::AtRule {
            name: name.into(),
            prelude: prelude.into(),
            body,
            origin: None,
        }
    }

    #[test]
    fn layout_and_separation() {
        let sheet: Vec<Node> = vec![
            verbatim("/*! banner */"),
            rule(".empty", vec![]),
            rule(
                ".a",
                vec![decl("color", "red", false), decl("--x", "", true)],
            ),
            at_rule(
                "@media",
                "print",
                Some(vec![
                    rule(
                        ".b",
                        vec![verbatim(
                            "x {
  y }",
                        )],
                    ),
                    rule(".c", vec![decl("--e", "", false)]),
                ]),
            ),
            at_rule("@layer", "a, b", None),
            at_rule("@font-face", "", Some(vec![])),
        ];
        assert_eq!(
            emit(&sheet, 1 << 20, false).unwrap().0,
            "/*! banner */\n\n.a {\n  color: red;\n  --x: !important;\n}\n\n\
             @media print {\n  .b {\n    x {\n  y }\n  }\n  .c {\n    --e:;\n  }\n}\n\n\
             @layer a, b;\n\n@font-face {\n}\n"
        );
    }

    #[test]
    fn empty_output_is_zero_bytes() {
        let sheet: Vec<Node> = vec![rule(".a", vec![])];
        assert_eq!(emit(&sheet, 1, false).unwrap().0, "");
    }

    #[test]
    fn output_limit_is_inclusive() {
        let sheet: Vec<Node> = vec![verbatim("abc")];
        assert_eq!(emit(&sheet, 4, false).unwrap().0, "abc\n");
        assert_eq!(emit(&sheet, 3, false).unwrap_err().code(), Code::E8006);
    }
}
