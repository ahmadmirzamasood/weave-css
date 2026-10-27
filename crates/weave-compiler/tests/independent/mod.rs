//! Independent validation of emitted CSS with `cssparser` (Servo's CSS
//! Syntax Level 3 implementation), a parser that shares no code with Weave.
//!
//! The emitted CSS is parsed as a stylesheet; known at-rule bodies are
//! parsed with the grammar CSS gives them (rule lists, declaration lists,
//! keyframe lists, page bodies). The result is a structural tree for semantic
//! assertions plus a list of problems: rules or declarations the parser
//! rejects, bad strings, bad URLs, stray closing brackets and `{}` blocks in
//! non-custom property values. Unknown at-rules are kept as raw text; the
//! parser may lag new CSS, so syntax it does not know is not checked here
//! (preservation tests cover it separately).

#![allow(clippy::expect_used, clippy::panic, dead_code)]

use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, ParseErrorKind, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Rule {
        prelude: String,
        body: Vec<Node>,
    },
    AtRule {
        name: String,
        prelude: String,
        body: Body,
    },
    Declaration {
        name: String,
        value: String,
        important: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// A statement at-rule (`;`).
    None,
    Nodes(Vec<Node>),
    /// The block of an at-rule this validator does not interpret.
    Raw(String),
}

impl Node {
    /// All declarations in this subtree, depth first.
    pub fn declarations(&self) -> Vec<(&str, &str, bool)> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect<'a>(&'a self, out: &mut Vec<(&'a str, &'a str, bool)>) {
        match self {
            Node::Declaration {
                name,
                value,
                important,
            } => out.push((name, value, *important)),
            Node::Rule { body, .. }
            | Node::AtRule {
                body: Body::Nodes(body),
                ..
            } => body.iter().for_each(|n| n.collect(out)),
            Node::AtRule { .. } => {}
        }
    }
}

/// What a block contains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Top level and group at-rules: rules and at-rules.
    RuleList,
    /// Style rules: declarations, nested rules and at-rules.
    StyleBody,
    /// Descriptor at-rules and page margins: declarations only.
    Declarations,
    /// `@page`: declarations and margin at-rules.
    Page,
    /// `@keyframes`: keyframe rules with declaration bodies.
    Keyframes,
}

fn block_kind(at_rule: &str) -> Option<Kind> {
    let name = at_rule.to_ascii_lowercase();
    Some(match name.as_str() {
        "media" | "supports" | "container" | "layer" | "starting-style" | "scope" => Kind::RuleList,
        "font-face"
        | "font-palette-values"
        | "counter-style"
        | "property"
        | "view-transition"
        | "position-try"
        | "top-left-corner"
        | "top-left"
        | "top-center"
        | "top-right"
        | "top-right-corner"
        | "bottom-left-corner"
        | "bottom-left"
        | "bottom-center"
        | "bottom-right"
        | "bottom-right-corner"
        | "left-top"
        | "left-middle"
        | "left-bottom"
        | "right-top"
        | "right-middle"
        | "right-bottom" => Kind::Declarations,
        "page" => Kind::Page,
        "keyframes" => Kind::Keyframes,
        _ => return None,
    })
}

type Error<'i> = ParseError<String>;

fn custom<'i>(message: impl Into<String>) -> Error<'i> {
    ParseError {
        kind: ParseErrorKind::Custom(message.into()),
    }
}

struct Validator {
    kind: Kind,
    problems: Vec<String>,
}

impl<'i> Validator {
    /// Consumes the rest of `input`, recursing into blocks, and records
    /// tokens that make CSS invalid. Returns whether a `{}` block occurred
    /// at the top level of `input`.
    fn scan(&mut self, input: &mut Parser<'i>, context: &str) -> bool {
        let mut top_level_brace = false;
        loop {
            let token = match input.next_including_whitespace_and_comments() {
                Ok(token) => token.clone(),
                Err(_) => return top_level_brace,
            };
            match token {
                Token::BadString(_) => self.problems.push(format!("bad string in {context}")),
                Token::BadUrl(_) => self.problems.push(format!("bad url in {context}")),
                Token::CloseParenthesis | Token::CloseSquareBracket | Token::CloseCurlyBracket => {
                    self.problems.push(format!("stray {token:?} in {context}"));
                }
                Token::CurlyBracketBlock
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::Function(_) => {
                    if matches!(token, Token::CurlyBracketBlock) {
                        top_level_brace = true;
                    }
                    let _ = input.parse_nested_block(|nested| {
                        self.scan(nested, context);
                        Ok::<(), Error<'i>>(())
                    });
                }
                _ => {}
            }
        }
    }

    fn body(&mut self, kind: Kind, input: &mut Parser<'i>) -> Vec<Node> {
        let mut inner = Validator {
            kind,
            problems: Vec::new(),
        };
        let mut nodes = Vec::new();
        if kind == Kind::RuleList {
            let mut parser = StyleSheetParser::new(input, &mut inner);
            let mut results = Vec::new();
            for result in &mut parser {
                results.push(result.map_err(|(err, text, _)| format!("{err:?} at `{text}`")));
            }
            collect(results, &mut nodes, &mut self.problems);
        } else {
            let mut parser = RuleBodyParser::new(input, &mut inner);
            let mut results = Vec::new();
            for result in &mut parser {
                results.push(result.map_err(|(err, text, _)| format!("{err:?} at `{text}`")));
            }
            collect(results, &mut nodes, &mut self.problems);
        }
        self.problems.append(&mut inner.problems);
        nodes
    }
}

fn collect(results: Vec<Result<Node, String>>, nodes: &mut Vec<Node>, problems: &mut Vec<String>) {
    for result in results {
        match result {
            Ok(node) => nodes.push(node),
            Err(problem) => problems.push(problem),
        }
    }
}

fn trimmed(text: &str) -> String {
    text.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c'))
        .to_string()
}

impl<'i> QualifiedRuleParser<'i> for Validator {
    type Prelude = String;
    type QualifiedRule = Node;
    type Error = String;

    fn parse_prelude(&mut self, input: &mut Parser<'i>) -> Result<String, Error<'i>> {
        if matches!(self.kind, Kind::Declarations | Kind::Page) {
            return Err(custom("qualified rule in a declaration list"));
        }
        if self.kind == Kind::StyleBody {
            // Valid CSS Nesting, but Weave lowers nesting: its output is flat.
            self.problems
                .push("qualified rule nested in a style rule".to_string());
        }
        let start = input.position();
        self.scan(input, "selector");
        Ok(trimmed(input.slice_from(start)))
    }

    fn parse_block(
        &mut self,
        prelude: String,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<Node, Error<'i>> {
        let kind = if self.kind == Kind::Keyframes {
            Kind::Declarations
        } else {
            Kind::StyleBody
        };
        let body = self.body(kind, input);
        Ok(Node::Rule { prelude, body })
    }
}

impl<'i> AtRuleParser<'i> for Validator {
    type Prelude = (String, String);
    type AtRule = Node;
    type Error = String;

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
    ) -> Result<(String, String), Error<'i>> {
        let start = input.position();
        self.scan(input, "at-rule prelude");
        Ok((name.to_string(), trimmed(input.slice_from(start))))
    }

    fn rule_without_block(
        &mut self,
        (name, prelude): (String, String),
        _start: &ParserState,
    ) -> Result<Node, ()> {
        Ok(Node::AtRule {
            name,
            prelude,
            body: Body::None,
        })
    }

    fn parse_block(
        &mut self,
        (name, prelude): (String, String),
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<Node, Error<'i>> {
        let body = match block_kind(&name) {
            Some(kind) => Body::Nodes(self.body(kind, input)),
            None => {
                let start = input.position();
                self.scan(input, "unknown at-rule block");
                Body::Raw(trimmed(input.slice_from(start)))
            }
        };
        Ok(Node::AtRule {
            name,
            prelude,
            body,
        })
    }
}

impl<'i> DeclarationParser<'i> for Validator {
    type Declaration = Node;
    type Error = String;

    fn parse_value(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
        _start: &ParserState,
    ) -> Result<Node, Error<'i>> {
        if matches!(self.kind, Kind::RuleList | Kind::Keyframes) {
            return Err(custom("declaration in a rule list"));
        }
        let start = input.position();
        let mut value_end = start.byte_index();
        let mut important = false;
        let mut brace = false;
        loop {
            if input
                .try_parse(|i| {
                    cssparser::parse_important(i)?;
                    i.expect_exhausted()
                })
                .is_ok()
            {
                important = true;
                break;
            }
            // One component value at a time, so that `!important` is
            // recognized only at the end.
            let token = match input.next_including_whitespace_and_comments() {
                Ok(token) => token.clone(),
                Err(_) => break,
            };
            match token {
                Token::BadString(_) => self.problems.push("bad string in value".into()),
                Token::BadUrl(_) => self.problems.push("bad url in value".into()),
                Token::CloseParenthesis | Token::CloseSquareBracket | Token::CloseCurlyBracket => {
                    self.problems.push(format!("stray {token:?} in value"));
                }
                Token::CurlyBracketBlock
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::Function(_) => {
                    brace |= matches!(token, Token::CurlyBracketBlock);
                    let _ = input.parse_nested_block(|nested| {
                        self.scan(nested, "value");
                        Ok::<(), Error<'i>>(())
                    });
                }
                _ => {}
            }
            value_end = input.position().byte_index();
        }
        if brace && !name.starts_with("--") {
            return Err(custom(format!("`{{}}` block in the value of `{name}`")));
        }
        let source = input.slice_from(start);
        let value = source
            .get(..value_end.saturating_sub(start.byte_index()))
            .unwrap_or(source);
        Ok(Node::Declaration {
            name: name.to_string(),
            value: trimmed(value),
            important,
        })
    }
}

impl<'i> RuleBodyItemParser<'i, Node, String> for Validator {
    fn parse_declarations(&self) -> bool {
        !matches!(self.kind, Kind::RuleList | Kind::Keyframes)
    }

    fn parse_qualified(&self) -> bool {
        matches!(
            self.kind,
            Kind::StyleBody | Kind::Keyframes | Kind::RuleList
        )
    }
}

/// Parses `css` as a stylesheet. Returns the tree and every problem found.
pub fn parse(css: &str) -> (Vec<Node>, Vec<String>) {
    let mut input = Parser::new(css);
    let mut validator = Validator {
        kind: Kind::RuleList,
        problems: Vec::new(),
    };
    let nodes = validator.body(Kind::RuleList, &mut input);
    (nodes, validator.problems)
}

/// Parses `css` and fails the test if the independent parser reports any
/// problem.
pub fn validate(css: &str) -> Vec<Node> {
    let (nodes, problems) = parse(css);
    assert!(
        problems.is_empty(),
        "independent CSS parser rejected the output:\n{}\n--- css ---\n{css}",
        problems.join("\n")
    );
    nodes
}
