//! Evaluation (LANGUAGE.md §4–§7): lexical scopes, expressions and
//! interpolation, producing the CSS tree ([`crate::ir`]).
//!
//! - Every block opens a scope. A scope knows the names its `bind`
//!   statements declare before they execute, so a name used before its
//!   `bind` in the same block is E1003 rather than a silent reference to an
//!   outer binding.
//! - A binding whose expression failed is poisoned: later uses fail without
//!   further diagnostics, so one mistake is reported once.
//! - Blocks are walked with an explicit stack and expressions run on a
//!   value stack, so neither depends on the thread stack.
//! - After interpolation, every CSS region is re-checked: inserted text can
//!   never change the structure of the surrounding CSS (§7.3).

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::builtins::{self, Bounds, Member, Prelude};
use crate::diagnostic::{Code, Diagnostic, Diagnostics};
use crate::expr::{BinOp, Call, Ctx, Op, Piece, Program, Template};
use crate::ir::Node;
use crate::lexer::{Lexer, Mode, TokenKind as K};
use crate::number::NumError;
use crate::options::{Limit, ResourceLimits};
use crate::source::Span;
use crate::syntax::Item;
use crate::value::{
    Map, Value, equal, escape_ident, escape_string_contents, escape_url, trailing_name,
};

/// A failure that has already been reported.
#[derive(Clone, Copy, Debug)]
struct Fail;

type Eval<T> = Result<T, Fail>;

#[derive(Debug)]
enum Slot {
    Value(Value),
    /// The binding's expression failed.
    Poisoned,
}

#[derive(Debug, Default)]
struct Scope {
    /// Names bound by `bind` statements directly in this block.
    declared: HashSet<Rc<str>>,
    values: HashMap<Rc<str>, Slot>,
}

impl Scope {
    fn for_block(items: &[Item]) -> Scope {
        let declared = items
            .iter()
            .filter_map(|item| match item {
                Item::Bind { name, .. } => Some(Rc::clone(name)),
                _ => None,
            })
            .collect();
        Scope {
            declared,
            values: HashMap::new(),
        }
    }
}

/// The CSS region a template belongs to, for the structure check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    Value { custom: bool },
    Prelude,
    Selector,
    Name,
}

/// What a block becomes when it closes; `None` when its header failed.
#[derive(Debug)]
enum Build {
    Top,
    Rule(Option<String>),
    AtRule(String, Option<String>),
    Keyframe(Option<String>),
}

#[derive(Debug)]
struct Frame<'s> {
    items: std::slice::Iter<'s, Item>,
    out: Vec<Node>,
    build: Build,
}

/// Evaluates a module's items into CSS nodes. The result is meaningful only
/// if no error was reported.
pub(crate) fn evaluate(
    items: &[Item],
    limits: &ResourceLimits,
    diags: &mut Diagnostics,
) -> Vec<Node> {
    let size = |limit| usize::try_from(limits.get(limit)).unwrap_or(usize::MAX);
    let mut evaluator = Evaluator {
        diags,
        scopes: Vec::new(),
        steps: 0,
        max_steps: limits.get(Limit::EvaluationSteps),
        max_elements: size(Limit::CollectionElements),
        bounds: Bounds {
            string_bytes: size(Limit::StringBytes),
        },
        aborted: false,
    };
    evaluator.run_items(items)
}

struct Evaluator<'d> {
    diags: &'d mut Diagnostics,
    scopes: Vec<Scope>,
    steps: u64,
    max_steps: u64,
    max_elements: usize,
    bounds: Bounds,
    /// Set after a limit stops evaluation.
    aborted: bool,
}

impl Evaluator<'_> {
    fn report(&mut self, diagnostic: Diagnostic) -> Fail {
        self.diags.push(diagnostic);
        if self.diags.is_full() {
            self.aborted = true;
        }
        Fail
    }

    fn error(&mut self, code: Code, span: Span, message: impl Into<String>) -> Fail {
        self.report(Diagnostic::error(code, message).with_primary(span))
    }

    fn num_error(&mut self, err: NumError, span: Span) -> Fail {
        self.error(err.code, span, err.message)
    }

    // ----- blocks ---------------------------------------------------------

    fn run_items(&mut self, items: &[Item]) -> Vec<Node> {
        let mut stack = vec![Frame {
            items: items.iter(),
            out: Vec::new(),
            build: Build::Top,
        }];
        self.scopes.push(Scope::for_block(items));
        loop {
            let next = match stack.last_mut() {
                Some(frame) if !self.aborted => frame.items.next(),
                Some(_) => None,
                None => return Vec::new(),
            };
            let Some(item) = next else {
                let Some(done) = stack.pop() else {
                    return Vec::new();
                };
                self.scopes.pop();
                let node = match done.build {
                    Build::Top => return done.out,
                    Build::Rule(Some(selector)) => Some(Node::Rule {
                        selector,
                        body: done.out,
                    }),
                    Build::AtRule(name, Some(prelude)) => Some(Node::AtRule {
                        name,
                        prelude,
                        body: Some(done.out),
                    }),
                    Build::Keyframe(Some(selectors)) => Some(Node::Keyframe {
                        selectors,
                        body: done.out,
                    }),
                    _ => None,
                };
                if let (Some(node), Some(parent)) = (node, stack.last_mut()) {
                    parent.out.push(node);
                }
                continue;
            };
            let (body, build) = match item {
                Item::Rule { selector, body, .. } => {
                    let selector = self.template(selector, Region::Selector).ok();
                    (body, Build::Rule(selector))
                }
                Item::AtRule {
                    name,
                    prelude,
                    body: Some(body),
                } => {
                    let prelude = self.template(prelude, Region::Prelude).ok();
                    (body, Build::AtRule(name.clone(), prelude))
                }
                Item::Keyframe { selectors, body } => {
                    let selectors = self.template(selectors, Region::Prelude).ok();
                    (body, Build::Keyframe(selectors))
                }
                _ => {
                    let node = self.leaf(item);
                    if let (Some(node), Some(frame)) = (node, stack.last_mut()) {
                        frame.out.push(node);
                    }
                    continue;
                }
            };
            self.scopes.push(Scope::for_block(body));
            stack.push(Frame {
                items: body.iter(),
                out: Vec::new(),
                build,
            });
        }
    }

    /// Items without a body.
    fn leaf(&mut self, item: &Item) -> Option<Node> {
        match item {
            Item::Declaration {
                name,
                value,
                important,
                custom,
            } => {
                let name = self.template(name, Region::Name).ok();
                let value = self.template(value, Region::Value { custom: *custom }).ok();
                Some(Node::Declaration {
                    name: name?,
                    value: value?,
                    important: *important,
                })
            }
            Item::AtRule {
                name,
                prelude,
                body: None,
            } => Some(Node::AtRule {
                name: name.clone(),
                prelude: self.template(prelude, Region::Prelude).ok()?,
                body: None,
            }),
            Item::Verbatim(text) => Some(Node::Verbatim(text.clone())),
            Item::Bind { name, span, value } => {
                let slot = match self.run(value, *span) {
                    Ok(value) => Slot::Value(value),
                    Err(Fail) => Slot::Poisoned,
                };
                if let Some(scope) = self.scopes.last_mut() {
                    scope.values.insert(Rc::clone(name), slot);
                }
                None
            }
            Item::Rule { .. } | Item::AtRule { .. } | Item::Keyframe { .. } => None,
        }
    }

    // ----- names ------------------------------------------------------------

    /// Whether a user binding (initialized or not) named `name` is visible.
    fn is_bound(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .any(|s| s.values.contains_key(name) || s.declared.contains(name))
    }

    fn lookup(&mut self, name: &str, span: Span) -> Eval<Value> {
        for scope in self.scopes.iter().rev() {
            match scope.values.get(name) {
                Some(Slot::Value(value)) => return Ok(value.clone()),
                Some(Slot::Poisoned) => return Err(Fail),
                None => {}
            }
            if scope.declared.contains(name) {
                return Err(self.report(
                    Diagnostic::error(
                        Code::E1003,
                        format!("`{name}` is used before its `bind` statement"),
                    )
                    .with_primary(span)
                    .with_note("bindings are not hoisted: within a block, a name refers to that block's binding from its `bind` statement on"),
                ));
            }
        }
        let diagnostic = match builtins::prelude(name) {
            Some(Prelude::Namespace) => Diagnostic::error(
                Code::E1004,
                format!("`{name}` is a built-in namespace, not a value"),
            )
            .with_note(format!("call one of its members, for example `{name}.…(…)`")),
            Some(Prelude::Function) => Diagnostic::error(
                Code::E1004,
                format!("`{name}` is a function, not a value"),
            ),
            None => Diagnostic::error(Code::E1002, format!("unresolved name `{name}`"))
                .with_note("bind it first with `bind`; bare identifiers in CSS values are CSS and are not looked up"),
        };
        Err(self.report(diagnostic.with_primary(span)))
    }

    // ----- expressions ------------------------------------------------------

    fn step(&mut self, site: Span) -> Eval<()> {
        self.steps += 1;
        if self.steps > self.max_steps {
            self.aborted = true;
            return Err(self.report(
                Diagnostic::error(
                    Code::E8004,
                    format!("evaluation exceeds the limit of {} steps", self.max_steps),
                )
                .with_primary(site)
                .with_note("raise it with the `evaluation-steps` limit"),
            ));
        }
        Ok(())
    }

    /// Runs a program; `site` locates limit errors.
    fn run(&mut self, program: &Program, site: Span) -> Eval<Value> {
        let mut stack: Vec<Value> = Vec::new();
        let mut pc = 0;
        while let Some(op) = program.ops.get(pc) {
            if self.aborted {
                return Err(Fail);
            }
            self.step(site)?;
            pc += 1;
            match op {
                Op::Const(value) => {
                    if let Value::Str(text) | Value::Css(text) = value {
                        if text.len() > self.bounds.string_bytes {
                            return Err(self.string_limit(site));
                        }
                    }
                    stack.push(value.clone());
                }
                Op::Str(template) => {
                    let text = self.template_text(template, true, site)?;
                    stack.push(Value::Str(text.into()));
                }
                Op::Fragment(template, span) => {
                    let text = self.template_text(template, true, *span)?;
                    let text = text.trim_matches(|c| matches!(c, ' ' | '\t' | '\n'));
                    if let Some(problem) = builtins::fragment_problem(text) {
                        return Err(self.error(
                            Code::E2006,
                            *span,
                            format!("invalid CSS fragment after interpolation: {problem}"),
                        ));
                    }
                    stack.push(Value::Css(text.into()));
                }
                Op::Load(name, span) => {
                    let value = self.lookup(name, *span)?;
                    stack.push(value);
                }
                Op::Neg(span) => match self.pop(&mut stack, site)? {
                    Value::Number(ref n) => stack.push(Value::Number(n.neg())),
                    other => return Err(self.operand_type("-", "a number", &other, *span)),
                },
                Op::Not(span) => match self.pop(&mut stack, site)? {
                    Value::Bool(b) => stack.push(Value::Bool(!b)),
                    other => return Err(self.operand_type("!", "a bool", &other, *span)),
                },
                Op::Binary(op, span) => {
                    let right = self.pop(&mut stack, site)?;
                    let left = self.pop(&mut stack, site)?;
                    let value = self.binary(*op, &left, &right, *span)?;
                    stack.push(value);
                }
                Op::JumpIfFalse(target, span) | Op::JumpIfTrue(target, span) => {
                    let skip_on = matches!(op, Op::JumpIfTrue(..));
                    let symbol = if skip_on { "||" } else { "&&" };
                    match self.pop(&mut stack, site)? {
                        Value::Bool(b) if b == skip_on => {
                            stack.push(Value::Bool(b));
                            pc = *target;
                        }
                        Value::Bool(_) => {}
                        other => return Err(self.operand_type(symbol, "a bool", &other, *span)),
                    }
                }
                Op::AssertBool(span) => match stack.last() {
                    Some(Value::Bool(_)) => {}
                    Some(other) => {
                        let other = other.clone();
                        return Err(self.operand_type("&&` or `||", "a bool", &other, *span));
                    }
                    None => return Err(self.internal(site)),
                },
                Op::List(count, span) => {
                    let items = self.take(&mut stack, *count, *span)?;
                    stack.push(Value::List(Rc::new(items)));
                }
                Op::Map(keys, span) => {
                    let values = self.take(&mut stack, keys.len(), *span)?;
                    let entries = keys.iter().map(|(k, _)| Rc::clone(k)).zip(values);
                    stack.push(Value::Map(Rc::new(Map::from_entries(entries))));
                }
                Op::Member(key, span) => {
                    let base = self.pop(&mut stack, site)?;
                    let value = self.member(&base, key, *span)?;
                    stack.push(value);
                }
                Op::Index(span) => {
                    let index = self.pop(&mut stack, site)?;
                    let base = self.pop(&mut stack, site)?;
                    let value = self.index(&base, &index, *span)?;
                    stack.push(value);
                }
                Op::Call(call) => {
                    let args = self.take(&mut stack, call.args.len(), call.callee.span)?;
                    let value = self.call(call, args)?;
                    stack.push(value);
                }
            }
        }
        self.pop(&mut stack, site)
    }

    fn pop(&mut self, stack: &mut Vec<Value>, site: Span) -> Eval<Value> {
        match stack.pop() {
            Some(value) => Ok(value),
            None => Err(self.internal(site)),
        }
    }

    /// A broken evaluator invariant. Never silent: output must not lose
    /// items without a diagnostic.
    fn internal(&mut self, site: Span) -> Fail {
        self.report(
            Diagnostic::error(
                Code::E5001,
                "internal compiler error: malformed expression program",
            )
            .with_primary(site)
            .with_note("please report this with the input that triggered it"),
        )
    }

    fn take(&mut self, stack: &mut Vec<Value>, count: usize, span: Span) -> Eval<Vec<Value>> {
        if count > self.max_elements {
            return Err(self.report(
                Diagnostic::error(
                    Code::E8007,
                    format!(
                        "the collection has {count} elements, more than the limit of {}",
                        self.max_elements
                    ),
                )
                .with_primary(span)
                .with_note("raise it with the `collection-elements` limit"),
            ));
        }
        let Some(at) = stack.len().checked_sub(count) else {
            return Err(self.internal(span));
        };
        Ok(stack.split_off(at))
    }

    fn operand_type(&mut self, op: &str, expected: &str, got: &Value, span: Span) -> Fail {
        self.error(
            Code::E2001,
            span,
            format!("`{op}` needs {expected}, got a {}", got.type_name()),
        )
    }

    fn binary(&mut self, op: BinOp, left: &Value, right: &Value, span: Span) -> Eval<Value> {
        match op {
            BinOp::Eq => return Ok(Value::Bool(equal(left, right))),
            BinOp::Ne => return Ok(Value::Bool(!equal(left, right))),
            _ => {}
        }
        let symbol = op.symbol();
        if let (BinOp::Add, Value::Str(a), Value::Str(b)) = (op, left, right) {
            let len = a.len() + b.len();
            if len > self.bounds.string_bytes {
                return Err(self.string_limit(span));
            }
            let mut joined = String::with_capacity(len);
            joined.push_str(a);
            joined.push_str(b);
            return Ok(Value::Str(joined.into()));
        }
        if let (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge, Value::Str(a), Value::Str(b)) =
            (op, left, right)
        {
            return Ok(Value::Bool(ordering_holds(op, a.as_ref().cmp(b.as_ref()))));
        }
        let (Value::Number(a), Value::Number(b)) = (left, right) else {
            let expected = match op {
                BinOp::Add => "two numbers or two strings",
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => "two numbers or two strings",
                _ => "two numbers",
            };
            return Err(self.error(
                Code::E2001,
                span,
                format!(
                    "`{symbol}` needs {expected}, got a {} and a {}",
                    left.type_name(),
                    right.type_name()
                ),
            ));
        };
        let result = match op {
            BinOp::Add => a.add(b),
            BinOp::Sub => a.sub(b),
            BinOp::Mul => a.mul(b),
            BinOp::Div => a.div(b),
            BinOp::Rem => a.rem(b),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                return a
                    .compare(b)
                    .map(|ord| Value::Bool(ordering_holds(op, ord)))
                    .map_err(|e| self.num_error(e, span));
            }
            BinOp::Eq | BinOp::Ne => return Ok(Value::Bool(false)),
        };
        result
            .map(Value::Number)
            .map_err(|e| self.num_error(e, span))
    }

    fn string_limit(&mut self, span: Span) -> Fail {
        self.report(
            Diagnostic::error(
                Code::E8007,
                format!(
                    "the string exceeds the limit of {} bytes",
                    self.bounds.string_bytes
                ),
            )
            .with_primary(span)
            .with_note("raise it with the `string-bytes` limit"),
        )
    }

    fn member(&mut self, base: &Value, key: &str, span: Span) -> Eval<Value> {
        match base {
            Value::Map(map) => match map.get(key) {
                Some(value) => Ok(value.clone()),
                None => Err(self.missing_key(key, span)),
            },
            other => Err(self.error(
                Code::E2001,
                span,
                format!("`.{key}` needs a map, got a {}", other.type_name()),
            )),
        }
    }

    fn missing_key(&mut self, key: &str, span: Span) -> Fail {
        self.report(
            Diagnostic::error(Code::E2007, format!("the map has no key \"{key}\""))
                .with_primary(span)
                .with_note("use map.get(m, key, default) for optional keys"),
        )
    }

    fn index(&mut self, base: &Value, index: &Value, span: Span) -> Eval<Value> {
        match base {
            Value::List(items) => {
                let Value::Number(n) = index else {
                    return Err(self.error(
                        Code::E2009,
                        span,
                        format!(
                            "a list index must be an integer, got a {}",
                            index.type_name()
                        ),
                    ));
                };
                let position = match (n.unit.is_none(), n.as_integer()) {
                    (true, Some(i)) => i,
                    _ => {
                        return Err(self.error(
                            Code::E2009,
                            span,
                            format!(
                                "a list index must be a unitless integer, got `{}`",
                                n.serialize()
                            ),
                        ));
                    }
                };
                let item = usize::try_from(position).ok().and_then(|i| items.get(i));
                match item {
                    Some(value) => Ok(value.clone()),
                    None => Err(self.report(
                        Diagnostic::error(
                            Code::E2005,
                            format!(
                                "index {position} is out of range for a list of length {}",
                                items.len()
                            ),
                        )
                        .with_primary(span)
                        .with_note("list indices are zero-based"),
                    )),
                }
            }
            Value::Map(map) => {
                let Value::Str(key) = index else {
                    return Err(self.error(
                        Code::E2011,
                        span,
                        format!("map keys are strings, got a {}", index.type_name()),
                    ));
                };
                match map.get(key) {
                    Some(value) => Ok(value.clone()),
                    None => Err(self.missing_key(key, span)),
                }
            }
            other => Err(self.error(
                Code::E2001,
                span,
                format!("`[…]` needs a list or a map, got a {}", other.type_name()),
            )),
        }
    }

    fn call(&mut self, call: &Call, args: Vec<Value>) -> Eval<Value> {
        let callee = &call.callee;
        let Some((namespace, ns_span)) = &callee.namespace else {
            let name = &callee.name;
            let diagnostic = if self.is_bound(name) {
                Diagnostic::error(Code::E1004, format!("`{name}` is a value, not a function"))
            } else {
                match builtins::prelude(name) {
                    Some(Prelude::Function) => not_implemented(&format!("built-in `{name}`")),
                    Some(Prelude::Namespace) => Diagnostic::error(
                        Code::E1004,
                        format!("`{name}` is a built-in namespace, not a function"),
                    ),
                    None => Diagnostic::error(Code::E1002, format!("unknown function `{name}`"))
                        .with_note("calls are never passed through as CSS functions; write CSS function text as a fragment, for example `` `name(…)` ``"),
                }
            };
            return Err(self.report(diagnostic.with_primary(callee.span)));
        };
        let full = format!("{namespace}.{}", callee.name);
        if self.is_bound(namespace) {
            return Err(self.error(
                Code::E1004,
                callee.span,
                format!("`{full}` is not a function: `{namespace}` is a bound value"),
            ));
        }
        match builtins::prelude(namespace) {
            Some(Prelude::Namespace) => {}
            Some(Prelude::Function) => {
                return Err(self.error(
                    Code::E1004,
                    callee.span,
                    format!("`{namespace}` is a function, not a namespace"),
                ));
            }
            None => {
                return Err(self.error(
                    Code::E1002,
                    *ns_span,
                    format!("unresolved name `{namespace}`"),
                ));
            }
        }
        match builtins::member(namespace, &callee.name) {
            Member::Implemented(builtin) => {
                builtins::call(builtin, call, args, self.bounds).map_err(|d| self.report(d))
            }
            Member::Specified => Err(self
                .report(not_implemented(&format!("built-in `{full}`")).with_primary(callee.span))),
            Member::Unknown => Err(self.error(
                Code::E7003,
                callee.span,
                format!(
                    "`{}` is not a member of the built-in namespace `{namespace}`",
                    callee.name
                ),
            )),
        }
    }

    // ----- interpolation ------------------------------------------------------

    /// Evaluates a template's text. Interpolated text that would form a
    /// comment opener with adjacent text is rejected (E2003). Strings and
    /// fragments (`bounded`) are limited to the string size; CSS regions are
    /// bounded by the output limit instead.
    fn template_text(&mut self, template: &Template, bounded: bool, site: Span) -> Eval<String> {
        let mut out = String::new();
        // The span of the last interpolation if it was inserted outside a
        // string or URL, for the comment-opener check of the next text.
        let mut after: Option<Span> = None;
        for piece in &template.pieces {
            match piece {
                Piece::Text(text) => {
                    if let Some(span) = after.take() {
                        if out.ends_with('/') && text.starts_with('*') {
                            return Err(self.comment_opener(span));
                        }
                    }
                    out.push_str(text);
                }
                Piece::Interp(interp) => {
                    let value = self.run(&interp.program, interp.span)?;
                    let start = out.len();
                    self.insert(&value, interp.ctx, &mut out, interp.span)?;
                    let structural = matches!(interp.ctx, Ctx::Value | Ctx::Selector | Ctx::Name);
                    if structural {
                        let inserted = out.get(start..).unwrap_or("");
                        let before = out.get(..start).unwrap_or("");
                        if before.ends_with('/') && inserted.starts_with('*') {
                            return Err(self.comment_opener(interp.span));
                        }
                    }
                    after = structural.then_some(interp.span);
                }
            }
            if bounded && out.len() > self.bounds.string_bytes {
                return Err(self.string_limit(site));
            }
        }
        Ok(out)
    }

    fn comment_opener(&mut self, span: Span) -> Fail {
        self.error(
            Code::E2003,
            span,
            "the interpolated text would form a comment opener `/*` with the adjacent text",
        )
    }

    /// Evaluates a CSS region and checks that interpolation left its
    /// structure intact.
    fn template(&mut self, template: &Template, region: Region) -> Eval<String> {
        if let Some(text) = template.static_text() {
            return Ok(text.to_string());
        }
        let site = template.pieces.iter().find_map(|p| match p {
            Piece::Interp(i) => Some(i.span),
            Piece::Text(_) => None,
        });
        let Some(site) = site else {
            // Only text: nothing was interpolated.
            return Ok(template
                .pieces
                .iter()
                .filter_map(|p| match p {
                    Piece::Text(text) => Some(text.as_str()),
                    Piece::Interp(_) => None,
                })
                .collect());
        };
        let text = self.template_text(template, false, site)?;
        if let Some((code, problem)) = structure_problem(&text, region) {
            let what = match region {
                Region::Value { .. } => "declaration value",
                Region::Prelude => "at-rule prelude",
                Region::Selector => "selector",
                Region::Name => "declaration name",
            };
            return Err(self.report(
                Diagnostic::error(
                    code,
                    format!("invalid {what} after interpolation: {problem}"),
                )
                .with_primary(site)
                .with_note(format!("the result was `{}`", shorten(&text))),
            ));
        }
        Ok(text)
    }

    /// Inserts a value's text for its context (LANGUAGE.md §7.1).
    fn insert(&mut self, value: &Value, ctx: Ctx, out: &mut String, span: Span) -> Eval<()> {
        let Some(text) = value.plain_text() else {
            return Err(self.report(
                Diagnostic::error(
                    Code::E2004,
                    format!("a {} cannot be interpolated directly", value.type_name()),
                )
                .with_primary(span)
                .with_note("collections never serialize implicitly: build a fragment with css.space(…), css.comma(…) or css.slash(…)"),
            ));
        };
        match ctx {
            Ctx::Value => {
                if let Value::Str(s) = value {
                    out.push('"');
                    escape_string_contents(s, '"', out);
                    out.push('"');
                } else {
                    out.push_str(&text);
                }
            }
            Ctx::Text => out.push_str(&text),
            Ctx::Str(quote) => escape_string_contents(&text, quote, out),
            Ctx::Url => escape_url(&text, out),
            Ctx::Selector | Ctx::Name => {
                if let Value::Css(fragment) = value {
                    if ctx == Ctx::Name && !is_single_ident(fragment) {
                        return Err(self.error(
                            Code::E2003,
                            span,
                            "a CSS fragment in a declaration name must be a single identifier",
                        ));
                    }
                    out.push_str(fragment);
                } else {
                    let prefix = trailing_name(out).to_string();
                    escape_ident(&text, &prefix, out);
                }
            }
        }
        Ok(())
    }
}

fn ordering_holds(op: BinOp, ord: Ordering) -> bool {
    match op {
        BinOp::Lt => ord == Ordering::Less,
        BinOp::Le => ord != Ordering::Greater,
        BinOp::Gt => ord == Ordering::Greater,
        _ => ord != Ordering::Less,
    }
}

fn not_implemented(what: &str) -> Diagnostic {
    Diagnostic::error(Code::E5001, format!("{what} is not implemented in this build"))
        .with_note("this build implements the plain-CSS subset, bindings, expressions, interpolation and the css.* constructors")
}

fn shorten(text: &str) -> String {
    let mut short: String = text.chars().take(60).collect();
    if short.len() < text.len() {
        short.push('…');
    }
    short
}

fn is_single_ident(text: &str) -> bool {
    let mut lexer = Lexer::new(text, 0, 1);
    let first = lexer.next(Mode::Css);
    first.kind == K::Ident && first.start == 0 && lexer.next(Mode::Css).kind == K::Eof
}

/// Why interpolated text does not keep the structure of its region
/// (LANGUAGE.md §7.3), with the diagnostic code.
fn structure_problem(text: &str, region: Region) -> Option<(Code, &'static str)> {
    let mut lexer = Lexer::new(text, 0, 1);
    let mut stack: Vec<K> = Vec::new();
    let mut tokens = Vec::new();
    let code = match region {
        Region::Selector | Region::Name => Code::E6007,
        _ => Code::E2003,
    };
    loop {
        let token = lexer.next(Mode::Css);
        if token.kind == K::Eof {
            break;
        }
        tokens.push(token);
        let problem = match token.kind {
            K::String { terminated: false } | K::BadString => Some("a string is not terminated"),
            K::Url { terminated: false } => Some("a `url(` is not terminated"),
            K::Comment { terminated: false } => Some("a comment is not terminated"),
            K::Semicolon if stack.is_empty() || region == Region::Selector => {
                Some("`;` would end the item")
            }
            K::OpenBrace if region == Region::Selector => Some("`{` is not allowed"),
            K::OpenBrace if stack.is_empty() && region != (Region::Value { custom: true }) => {
                Some("`{` would open a block")
            }
            K::OpenBrace => {
                stack.push(K::CloseBrace);
                None
            }
            K::OpenParen | K::Function => {
                stack.push(K::CloseParen);
                None
            }
            K::OpenSquare => {
                stack.push(K::CloseSquare);
                None
            }
            K::CloseParen | K::CloseSquare | K::CloseBrace => {
                if stack.pop() == Some(token.kind) {
                    None
                } else {
                    Some("a closing bracket does not match")
                }
            }
            _ => None,
        };
        if let Some(problem) = problem {
            return Some((code, problem));
        }
    }
    if !stack.is_empty() {
        return Some((code, "a bracket is not closed"));
    }
    match region {
        Region::Name => {
            let single = matches!(tokens.as_slice(), [t] if t.kind == K::Ident);
            (!single).then_some((code, "a property name must be one identifier"))
        }
        Region::Selector => {
            if tokens.iter().all(|t| t.is_trivia()) {
                return Some((code, "the selector is empty"));
            }
            if tokens.iter().any(|t| t.kind == K::Delim('&')) {
                return Some((Code::E6003, "`&` has no parent rule here"));
            }
            let dangling = tokens.windows(2).any(|pair| {
                matches!(pair, [a, b] if matches!(a.kind, K::Delim('.') | K::Delim('#')) && b.kind != K::Ident)
            }) || tokens
                .last()
                .is_some_and(|t| matches!(t.kind, K::Delim('.') | K::Delim('#')));
            dangling.then_some((code, "`.` or `#` must be followed by a name"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structure_checks() {
        let value = Region::Value { custom: false };
        let custom = Region::Value { custom: true };
        assert_eq!(structure_problem("a \"b;c\" url(x;y)", value), None);
        assert!(structure_problem("a; b", value).is_some());
        assert!(structure_problem("a { b }", value).is_some());
        assert_eq!(structure_problem("{ a: [b] }", custom), None);
        assert!(structure_problem("a }", custom).is_some());
        assert!(structure_problem("(a", value).is_some());
        assert!(structure_problem("\"a", value).is_some());
        assert!(structure_problem("/* a", value).is_some());
        assert_eq!(structure_problem("x\\:\\ y", Region::Name), None);
        assert!(structure_problem("x y", Region::Name).is_some());
        assert!(structure_problem("", Region::Name).is_some());
        assert_eq!(
            structure_problem(".a-\\31 , #b > [c=\"{\"]", Region::Selector),
            None
        );
        assert!(structure_problem(".", Region::Selector).is_some());
        assert!(structure_problem(".a #", Region::Selector).is_some());
        assert!(structure_problem("a;b", Region::Selector).is_some());
        assert!(structure_problem("a{", Region::Selector).is_some());
        assert_eq!(
            structure_problem("& .a", Region::Selector).map(|(c, _)| c),
            Some(Code::E6003)
        );
        assert!(structure_problem(" ", Region::Selector).is_some());
    }
}
