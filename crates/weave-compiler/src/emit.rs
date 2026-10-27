//! CSS output (LANGUAGE.md §15).
//!
//! UTF-8, LF line breaks, two-space indentation, one empty line between
//! top-level items and none inside blocks, a single trailing LF when the
//! output is non-empty. Verbatim text is indented on its first line only.
//! Output is bounded by the `output-bytes` limit (E8006).

use crate::diagnostic::{Code, Diagnostic};
use crate::syntax::{Item, Stylesheet};

/// Emits a stylesheet, failing with E8006 when the output would exceed
/// `limit` bytes.
pub(crate) fn emit(sheet: &Stylesheet, limit: u64) -> Result<String, Diagnostic> {
    let mut writer = Writer {
        out: String::new(),
        limit: usize::try_from(limit).unwrap_or(usize::MAX),
        overflow: false,
    };
    // An explicit stack of open blocks (not recursion), so the deepest
    // allowed nesting cannot exhaust the thread stack.
    let mut stack: Vec<std::slice::Iter<'_, Item>> = vec![sheet.items.iter()];
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
    Ok(writer.out)
}

struct Writer {
    out: String,
    limit: usize,
    overflow: bool,
}

impl Writer {
    fn push(&mut self, text: &str) {
        if self.overflow || self.out.len().saturating_add(text.len()) > self.limit {
            self.overflow = true;
            return;
        }
        self.out.push_str(text);
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.push("  ");
        }
    }

    /// Writes an item, or the header of a block item and returns its body.
    fn item<'a>(&mut self, item: &'a Item, depth: usize) -> Option<&'a [Item]> {
        self.indent(depth);
        match item {
            Item::Declaration {
                name,
                value,
                important,
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
            Item::Verbatim(text) => {
                self.push(text);
                self.push(
                    "
",
                );
                None
            }
            Item::Rule {
                selector: header,
                body,
            }
            | Item::Keyframe {
                selectors: header,
                body,
            } => {
                self.push(header);
                self.push(
                    " {
",
                );
                Some(body)
            }
            Item::AtRule {
                name,
                prelude,
                body,
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

    fn decl(name: &str, value: &str, important: bool) -> Item {
        Item::Declaration {
            name: name.into(),
            value: value.into(),
            important,
        }
    }

    #[test]
    fn layout_and_separation() {
        let sheet = Stylesheet {
            items: vec![
                Item::Verbatim("/*! banner */".into()),
                Item::Rule {
                    selector: ".empty".into(),
                    body: vec![],
                },
                Item::Rule {
                    selector: ".a".into(),
                    body: vec![decl("color", "red", false), decl("--x", "", true)],
                },
                Item::AtRule {
                    name: "@media".into(),
                    prelude: "print".into(),
                    body: Some(vec![
                        Item::Rule {
                            selector: ".b".into(),
                            body: vec![Item::Verbatim("x {\n  y }".into())],
                        },
                        Item::Rule {
                            selector: ".c".into(),
                            body: vec![decl("--e", "", false)],
                        },
                    ]),
                },
                Item::AtRule {
                    name: "@layer".into(),
                    prelude: "a, b".into(),
                    body: None,
                },
                Item::AtRule {
                    name: "@font-face".into(),
                    prelude: String::new(),
                    body: Some(vec![]),
                },
            ],
        };
        assert_eq!(
            emit(&sheet, 1 << 20).unwrap(),
            "/*! banner */\n\n.a {\n  color: red;\n  --x: !important;\n}\n\n\
             @media print {\n  .b {\n    x {\n  y }\n  }\n  .c {\n    --e:;\n  }\n}\n\n\
             @layer a, b;\n\n@font-face {\n}\n"
        );
    }

    #[test]
    fn empty_output_is_zero_bytes() {
        let sheet = Stylesheet {
            items: vec![Item::Rule {
                selector: ".a".into(),
                body: vec![],
            }],
        };
        assert_eq!(emit(&sheet, 1).unwrap(), "");
    }

    #[test]
    fn output_limit_is_inclusive() {
        let sheet = Stylesheet {
            items: vec![Item::Verbatim("abc".into())],
        };
        assert_eq!(emit(&sheet, 4).unwrap(), "abc\n");
        assert_eq!(emit(&sheet, 3).unwrap_err().code(), Code::E8006);
    }
}
