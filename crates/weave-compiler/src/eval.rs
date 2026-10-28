//! Evaluation (LANGUAGE.md §4–§10): lexical scopes, expressions,
//! interpolation, functions, control flow and recipes, producing the CSS
//! tree ([`crate::ir`]).
//!
//! - Scopes are environments linked to their parent. Function and recipe
//!   bodies start from the module environment (they never see the caller);
//!   a content block keeps the environment of its `apply` statement. A
//!   scope knows the names it binds before they are bound, so a use before
//!   the `bind` is E1003 rather than a silent reference to an outer name.
//! - A top-level binding whose expression failed is poisoned: later uses
//!   fail without further diagnostics, so one mistake is reported once.
//! - Nothing here recurses per call or per nesting level. Expressions,
//!   templates, argument binding and function bodies are tasks on one
//!   explicit stack (the *machine*), so a user function calling itself
//!   adds tasks, not Rust stack frames. CSS-emitting blocks, recipe
//!   expansions and content blocks are frames on a second explicit stack.
//! - Every expression operation, statement, loop iteration and emitted item
//!   costs an evaluation step, and producing text or collections costs one
//!   more step per KiB or per 64 elements (E8004). Recursion is bounded by
//!   the call-depth limit (E8003) and the CSS produced so far by the output
//!   limit (E8006), checked as nodes are created.
//! - After interpolation, every CSS region is re-checked: inserted text can
//!   never change the structure of the surrounding CSS (§7.3).

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::builtins::{self, Bounds, Builtin, Member, Prelude};
use crate::diagnostic::{Code, Diagnostic, Diagnostics};
use crate::expr::{BinOp, Call, Ctx, Interp, Op, Piece, Program, Template};
use crate::ir::{CallSite, Node, Origin};
use crate::lexer::{Lexer, Mode, TokenKind as K};
use crate::number::NumError;
use crate::options::{Limit, ResourceLimits};
use crate::source::{FileId, Span};
use crate::syntax::{Apply, Branch, Context, Each, FnDef, Item, Param, RecipeDef, VerbatimKind};
use crate::value::{
    Map, Type, Value, describe, equal_within, escape_ident, escape_string_contents, escape_url,
    trailing_name,
};

/// A failure that has already been reported.
#[derive(Clone, Copy, Debug)]
struct Fail;

type Eval<T> = Result<T, Fail>;

/// Callers see at most this many enclosing calls in a diagnostic.
const TRACE_LABELS: usize = 3;

// ----- environments ------------------------------------------------------------

#[derive(Debug)]
enum Slot<'s> {
    Value(Value),
    /// The binding's expression failed.
    Poisoned,
    Function(&'s FnDef),
    Recipe(&'s RecipeDef),
}

/// One lexical scope.
#[derive(Debug)]
struct Env<'s> {
    parent: Option<Rc<Env<'s>>>,
    /// Names this scope binds, bound or not yet.
    declared: HashSet<Rc<str>>,
    slots: RefCell<HashMap<Rc<str>, Slot<'s>>>,
}

impl<'s> Env<'s> {
    /// A scope for a block: its `bind` names plus `names` (parameters or
    /// loop variables).
    fn block(parent: Option<Rc<Env<'s>>>, items: &[Item], names: &[&Rc<str>]) -> Rc<Env<'s>> {
        let declared = items
            .iter()
            .filter_map(|item| match item {
                Item::Bind { name, .. } => Some(Rc::clone(name)),
                _ => None,
            })
            .chain(names.iter().map(|n| Rc::clone(n)))
            .collect();
        Rc::new(Env {
            parent,
            declared,
            slots: RefCell::new(HashMap::new()),
        })
    }

    fn set(&self, name: &Rc<str>, slot: Slot<'s>) {
        self.slots.borrow_mut().insert(Rc::clone(name), slot);
    }
}

impl Drop for Env<'_> {
    /// Releases a chain of parents iteratively rather than recursively.
    fn drop(&mut self) {
        let mut parent = self.parent.take();
        while let Some(env) = parent {
            match Rc::try_unwrap(env) {
                Ok(mut env) => parent = env.parent.take(),
                Err(_) => break,
            }
        }
    }
}

/// What a name refers to.
enum Found<'s> {
    Value(Value),
    Poisoned,
    Function(&'s FnDef),
    Recipe(&'s RecipeDef),
    /// Declared in a scope but not bound yet.
    Unbound,
    Missing,
}

fn find<'s>(env: &Rc<Env<'s>>, name: &str) -> Found<'s> {
    let mut current = Some(env);
    while let Some(scope) = current {
        if let Some(slot) = scope.slots.borrow().get(name) {
            return match slot {
                Slot::Value(value) => Found::Value(value.clone()),
                Slot::Poisoned => Found::Poisoned,
                Slot::Function(def) => Found::Function(def),
                Slot::Recipe(def) => Found::Recipe(def),
            };
        }
        if scope.declared.contains(name) {
            return Found::Unbound;
        }
        current = scope.parent.as_ref();
    }
    Found::Missing
}

// ----- loops ---------------------------------------------------------------------

#[derive(Debug)]
enum Sequence {
    List(Rc<Vec<Value>>),
    Map(Rc<Map>),
}

/// The state of an `each` statement.
#[derive(Debug)]
struct Loop<'s> {
    each: &'s Each,
    items: Sequence,
    next: usize,
    /// The scope the `each` statement is in.
    parent: Rc<Env<'s>>,
}

impl<'s> Loop<'s> {
    /// The scope of the next iteration, holding the loop variables.
    fn next_env(&mut self) -> Option<Rc<Env<'s>>> {
        let i = self.next;
        let (key, value) = match &self.items {
            Sequence::List(items) => (
                Value::Number(crate::number::Number::new(
                    i as f64,
                    crate::number::Unit::None,
                )),
                items.get(i)?.clone(),
            ),
            Sequence::Map(map) => {
                let (k, v) = map.entry_at(i)?;
                (Value::Str(Rc::clone(k)), v.clone())
            }
        };
        self.next += 1;
        let each = self.each;
        let names: Vec<&Rc<str>> = each
            .key
            .iter()
            .map(|(n, _)| n)
            .chain(std::iter::once(&each.value.0))
            .collect();
        let env = Env::block(Some(Rc::clone(&self.parent)), &each.body, &names);
        if let Some((name, _)) = &each.key {
            env.set(name, Slot::Value(key));
        }
        env.set(&each.value.0, Slot::Value(value));
        Some(env)
    }
}

// ----- the machine -------------------------------------------------------------

/// The result of a finished task, handed to the task below it.
#[derive(Debug)]
enum Out {
    Value(Value),
    /// Every value an argument list computed.
    Values(Vec<Value>),
    Text(String),
}

#[derive(Debug)]
struct ExprTask<'s> {
    program: &'s Program,
    pc: usize,
    stack: Vec<Value>,
    env: Rc<Env<'s>>,
    site: Span,
    /// Return every value left on the stack (argument lists).
    all: bool,
}

#[derive(Debug)]
struct TextTask<'s> {
    template: &'s Template,
    next: usize,
    out: String,
    /// The last interpolation if it was inserted outside a string or URL,
    /// for the comment-opener check of the next text.
    after: Option<Span>,
    /// Strings and fragments are bounded by the string limit.
    bounded: bool,
    site: Span,
    env: Rc<Env<'s>>,
}

/// Binding a user function's arguments, evaluating defaults left to right.
#[derive(Debug)]
struct CallTask<'s> {
    def: &'s FnDef,
    callee: Span,
    args: Vec<Option<(Value, Span)>>,
    next: usize,
    env: Rc<Env<'s>>,
}

/// What a function body waits for from the expression it started.
#[derive(Debug)]
enum Wait<'s> {
    Nothing,
    Bind(&'s Rc<str>),
    Condition {
        branches: &'s [Branch],
        index: usize,
        otherwise: Option<&'s [Item]>,
    },
    Iterable(&'s Each),
    Yield(Span),
    Report(bool, Span),
}

#[derive(Debug)]
struct Block<'s> {
    items: std::slice::Iter<'s, Item>,
    env: Rc<Env<'s>>,
    each: Option<Loop<'s>>,
}

#[derive(Debug)]
struct BodyTask<'s> {
    def: &'s FnDef,
    callee: Span,
    blocks: Vec<Block<'s>>,
    wait: Wait<'s>,
}

#[derive(Debug)]
enum Task<'s> {
    Expr(ExprTask<'s>),
    Text(TextTask<'s>),
    Call(CallTask<'s>),
    Body(BodyTask<'s>),
}

impl Task<'_> {
    /// Whether the task holds one level of call depth.
    fn is_call(&self) -> bool {
        matches!(self, Task::Call(_) | Task::Body(_))
    }
}

#[derive(Debug)]
enum Action<'s> {
    Push(Task<'s>),
    /// The task finished.
    Done(Out),
    /// Replace the task (argument binding becomes the body).
    Become(Task<'s>),
}

/// What a callee name denotes.
enum Callee<'s> {
    User(&'s FnDef),
    Builtin(Builtin),
}

// ----- CSS frames ----------------------------------------------------------------

/// The CSS context that emitted items land in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Stylesheet,
    Rule,
    Declarations,
    Keyframes,
    Page,
}

impl Place {
    fn of(context: Context) -> Place {
        match context {
            Context::Stylesheet => Place::Stylesheet,
            Context::Declarations => Place::Declarations,
            Context::Keyframes => Place::Keyframes,
            Context::Page => Place::Page,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Place::Stylesheet => "at stylesheet level",
            Place::Rule => "in a rule body",
            Place::Declarations => "in a declaration list",
            Place::Keyframes => "in `@keyframes`",
            Place::Page => "in `@page`",
        }
    }
}

/// A content block passed to a recipe, with the scope it was written in.
#[derive(Debug)]
struct Content<'s> {
    items: &'s [Item],
    env: Rc<Env<'s>>,
    /// The slot that a `slot;` inside the content block refers to.
    slot: Option<Rc<SlotTarget<'s>>>,
}

/// The slot of one recipe expansion.
#[derive(Debug)]
struct SlotTarget<'s> {
    content: Option<Content<'s>>,
    used: Cell<u32>,
    /// The `apply` keyword.
    apply: Span,
}

/// What a frame becomes when it closes; `None` when its header failed.
#[derive(Debug)]
enum Build {
    Top,
    Rule(Option<String>),
    AtRule(String, Option<String>),
    Keyframe(Option<String>),
    /// `when`/`each` blocks, recipe expansions and content blocks: their
    /// items go straight to the enclosing CSS block.
    Inline,
}

#[derive(Debug)]
struct Frame<'s> {
    items: std::slice::Iter<'s, Item>,
    env: Rc<Env<'s>>,
    out: Vec<Node>,
    build: Build,
    /// The frame whose output receives this frame's nodes.
    target: usize,
    place: Place,
    slot: Option<Rc<SlotTarget<'s>>>,
    each: Option<Loop<'s>>,
    /// For a recipe expansion: errors reported before it started.
    expansion: Option<usize>,
    /// Where limit errors about the frame point.
    site: Span,
}

/// The CSS region a template belongs to, for the structure check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    Value { custom: bool },
    Prelude,
    Selector,
    Name,
}

/// Evaluates a module's items into CSS nodes. The result is meaningful only
/// if no error was reported.
pub(crate) fn evaluate(
    items: &[Item],
    file: FileId,
    limits: &ResourceLimits,
    diags: &mut Diagnostics,
) -> Vec<Node> {
    let size = |limit| usize::try_from(limits.get(limit)).unwrap_or(usize::MAX);
    let module = Env::block(None, items, &[]);
    for item in items {
        match item {
            Item::Fn(def) => module.set(&def.name, Slot::Function(def)),
            Item::Recipe(def) => module.set(&def.name, Slot::Recipe(def)),
            _ => {}
        }
    }
    let mut evaluator = Evaluator {
        diags,
        reported: HashSet::new(),
        errors: 0,
        module: Rc::clone(&module),
        steps: 0,
        max_steps: limits.get(Limit::EvaluationSteps),
        depth: 0,
        max_depth: limits.get(Limit::CallDepth),
        trace: Vec::new(),
        calls: None,
        output: 0,
        max_output: limits.get(Limit::OutputBytes),
        top_blocked: false,
        bounds: Bounds {
            string_bytes: size(Limit::StringBytes),
            elements: size(Limit::CollectionElements),
        },
        aborted: false,
    };
    evaluator.run_items(items, module, Span::empty(file, 0))
}

struct Evaluator<'s, 'd> {
    diags: &'d mut Diagnostics,
    /// Errors already reported, so that a loop reports each once.
    reported: HashSet<(Code, Option<Span>, String)>,
    /// Errors reported, including repeated ones.
    errors: usize,
    module: Rc<Env<'s>>,
    steps: u64,
    max_steps: u64,
    /// Active function calls and recipe expansions.
    depth: u64,
    max_depth: u64,
    /// The active calls and expansions, innermost last, for diagnostics.
    trace: Vec<(String, Span)>,
    /// The active recipe applications, recorded in emitted nodes.
    calls: Option<Rc<CallSite>>,
    /// Bytes of CSS produced so far (an upper bound of the output).
    output: u64,
    max_output: u64,
    /// Whether a rule or block was emitted at the top level, which
    /// `@import` and `@namespace` may not follow.
    top_blocked: bool,
    bounds: Bounds,
    /// Set after a limit stops evaluation.
    aborted: bool,
}

impl<'s> Evaluator<'s, '_> {
    fn report(&mut self, diagnostic: Diagnostic) -> Fail {
        let error = diagnostic.is_error();
        if error {
            self.errors += 1;
            let key = (
                diagnostic.code(),
                diagnostic.primary(),
                diagnostic.message().to_string(),
            );
            if !self.reported.insert(key) {
                return Fail;
            }
        }
        let mut diagnostic = diagnostic;
        for (what, span) in self.trace.iter().rev().take(TRACE_LABELS) {
            diagnostic = diagnostic.with_label(*span, what.clone());
        }
        if self.trace.len() > TRACE_LABELS {
            diagnostic = diagnostic.with_note(format!(
                "{} more enclosing calls are not shown",
                self.trace.len() - TRACE_LABELS
            ));
        }
        self.diags.push(diagnostic);
        if error && self.diags.is_full() {
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

    /// A broken evaluator invariant. Never silent: output must not lose
    /// items without a diagnostic.
    fn internal(&mut self, site: Span) -> Fail {
        self.report(
            Diagnostic::error(Code::E5001, "internal compiler error: malformed program")
                .with_primary(site)
                .with_note("please report this with the input that triggered it"),
        )
    }

    // ----- limits -------------------------------------------------------------

    fn step(&mut self, site: Span) -> Eval<()> {
        self.charge(1, site)
    }

    /// Spends `units` evaluation steps (E8004 beyond the limit).
    fn charge(&mut self, units: u64, site: Span) -> Eval<()> {
        if self.aborted {
            return Err(Fail);
        }
        self.steps = self.steps.saturating_add(units);
        if self.steps > self.max_steps {
            let diagnostic = Diagnostic::error(
                Code::E8004,
                format!("evaluation exceeds the limit of {} steps", self.max_steps),
            )
            .with_primary(site)
            .with_note("raise it with the `evaluation-steps` limit");
            let fail = self.report(diagnostic);
            self.aborted = true;
            return Err(fail);
        }
        Ok(())
    }

    /// The extra cost of producing a value: one step per KiB of text or
    /// per 64 collection elements.
    fn charge_value(&mut self, value: &Value, site: Span) -> Eval<()> {
        let units = match value {
            Value::Str(s) | Value::Css(s) => s.len() / 1024,
            Value::List(l) => l.len() / 64,
            Value::Map(m) => m.len() / 64,
            _ => 0,
        };
        if units == 0 {
            Ok(())
        } else {
            self.charge(units as u64, site)
        }
    }

    /// The comparison budget left: 64 value pairs per remaining step.
    fn budget(&self) -> u64 {
        self.max_steps.saturating_sub(self.steps).saturating_mul(64)
    }

    /// Settles a comparison's cost; a budget that ran out is E8004.
    fn settle(&mut self, before: u64, after: u64, site: Span) -> Eval<()> {
        let used = before.saturating_sub(after);
        self.charge(used / 64, site)
    }

    fn enter(&mut self, what: String, site: Span) -> Eval<()> {
        if self.depth >= self.max_depth {
            let diagnostic = Diagnostic::error(
                Code::E8003,
                format!(
                    "call depth exceeds the limit of {} (function calls and recipe applications)",
                    self.max_depth
                ),
            )
            .with_primary(site)
            .with_note("check for unbounded recursion, or raise the `call-depth` limit");
            return Err(self.report(diagnostic));
        }
        self.depth += 1;
        self.trace.push((what, site));
        Ok(())
    }

    /// The origin of a node produced by the item at `span`.
    fn origin(&self, span: Span) -> Option<Origin> {
        Some(Origin {
            span,
            calls: self.calls.clone(),
        })
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        self.trace.pop();
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

    // ----- names ----------------------------------------------------------------

    fn lookup(&mut self, env: &Rc<Env<'s>>, name: &str, span: Span) -> Eval<Value> {
        let diagnostic = match find(env, name) {
            Found::Value(value) => return Ok(value),
            Found::Poisoned => return Err(Fail),
            Found::Unbound => Diagnostic::error(
                Code::E1003,
                format!("`{name}` is used before its `bind` statement"),
            )
            .with_note("bindings are not hoisted: within a block, a name refers to that block's binding from its `bind` statement on"),
            Found::Function(_) => Diagnostic::error(
                Code::E1004,
                format!("`{name}` is a function, not a value"),
            )
            .with_note("functions are not values; call it: `name(…)`"),
            Found::Recipe(_) => Diagnostic::error(
                Code::E1004,
                format!("`{name}` is a recipe, not a value"),
            )
            .with_note("recipes are expanded with `apply`"),
            Found::Missing => match builtins::prelude(name) {
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
            },
        };
        Err(self.report(diagnostic.with_primary(span)))
    }

    /// Resolves the target of a call expression.
    fn callee(&mut self, call: &Call, env: &Rc<Env<'s>>) -> Eval<Callee<'s>> {
        let callee = &call.callee;
        let Some((namespace, ns_span)) = &callee.namespace else {
            let name = &callee.name;
            let diagnostic = match find(env, name) {
                Found::Function(def) => return Ok(Callee::User(def)),
                Found::Value(_) | Found::Poisoned | Found::Unbound => {
                    Diagnostic::error(Code::E1004, format!("`{name}` is a value, not a function"))
                }
                Found::Recipe(_) => Diagnostic::error(
                    Code::E1004,
                    format!("`{name}` is a recipe, not a function"),
                )
                .with_note("recipes emit CSS and are expanded with `apply`"),
                Found::Missing => match builtins::function(name) {
                    Some(builtin) => return Ok(Callee::Builtin(builtin)),
                    None if builtins::prelude(name) == Some(Prelude::Namespace) => {
                        Diagnostic::error(
                            Code::E1004,
                            format!("`{name}` is a built-in namespace, not a function"),
                        )
                    }
                    None => Diagnostic::error(Code::E1002, format!("unknown function `{name}`"))
                        .with_note("calls are never passed through as CSS functions; write CSS function text as a fragment, for example `` `name(…)` ``"),
                },
            };
            return Err(self.report(diagnostic.with_primary(callee.span)));
        };
        let full = format!("{namespace}.{}", callee.name);
        if !matches!(find(env, namespace), Found::Missing) {
            return Err(self.error(
                Code::E1004,
                callee.span,
                format!("`{full}` is not a function: `{namespace}` is not a namespace"),
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
            Member::Implemented(builtin) => Ok(Callee::Builtin(builtin)),
            Member::Specified => Err(self.report(
                Diagnostic::error(
                    Code::E5001,
                    format!("built-in `{full}` is not implemented in this build"),
                )
                .with_primary(callee.span)
                .with_note("docs/BUILTINS.md lists the built-ins this build implements"),
            )),
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

    // ----- the machine ------------------------------------------------------------

    /// Runs a task and everything it starts to completion.
    fn machine(&mut self, root: Task<'s>) -> Eval<Out> {
        let mut tasks = vec![root];
        let mut input: Option<Out> = None;
        loop {
            let Some(task) = tasks.last_mut() else {
                return Err(Fail);
            };
            let action = match task {
                Task::Expr(t) => self.expr_task(t, input.take()),
                Task::Text(t) => self.text_task(t, input.take()),
                Task::Call(t) => self.call_task(t, input.take()),
                Task::Body(t) => self.body_task(t, input.take()),
            };
            match action {
                Ok(Action::Push(task)) => tasks.push(task),
                Ok(Action::Become(task)) => {
                    if let Some(slot) = tasks.last_mut() {
                        *slot = task;
                    }
                }
                Ok(Action::Done(out)) => {
                    if tasks.pop().is_some_and(|t| t.is_call()) {
                        self.leave();
                    }
                    if tasks.is_empty() {
                        return Ok(out);
                    }
                    input = Some(out);
                }
                Err(Fail) => {
                    for task in tasks.iter().rev() {
                        if task.is_call() {
                            self.leave();
                        }
                    }
                    return Err(Fail);
                }
            }
        }
    }

    fn value(&mut self, program: &'s Program, env: &Rc<Env<'s>>, site: Span) -> Eval<Value> {
        match self.machine(Task::Expr(ExprTask {
            program,
            pc: 0,
            stack: Vec::new(),
            env: Rc::clone(env),
            site,
            all: false,
        }))? {
            Out::Value(value) => Ok(value),
            _ => Err(self.internal(site)),
        }
    }

    fn values(&mut self, program: &'s Program, env: &Rc<Env<'s>>, site: Span) -> Eval<Vec<Value>> {
        match self.machine(Task::Expr(ExprTask {
            program,
            pc: 0,
            stack: Vec::new(),
            env: Rc::clone(env),
            site,
            all: true,
        }))? {
            Out::Values(values) => Ok(values),
            _ => Err(self.internal(site)),
        }
    }

    fn text(
        &mut self,
        template: &'s Template,
        env: &Rc<Env<'s>>,
        bounded: bool,
        site: Span,
    ) -> Eval<String> {
        match self.machine(Task::Text(TextTask {
            template,
            next: 0,
            out: String::new(),
            after: None,
            bounded,
            site,
            env: Rc::clone(env),
        }))? {
            Out::Text(text) => Ok(text),
            _ => Err(self.internal(site)),
        }
    }

    fn expr_task(&mut self, t: &mut ExprTask<'s>, input: Option<Out>) -> Eval<Action<'s>> {
        let site = t.site;
        if let Some(out) = input {
            // The result of the task started by the previous operation.
            let op = t.pc.checked_sub(1).and_then(|pc| t.program.ops.get(pc));
            match (op, out) {
                (Some(Op::Str(_)), Out::Text(text)) => t.stack.push(Value::Str(text.into())),
                (Some(Op::Fragment(_, span)), Out::Text(text)) => {
                    let text = text.trim_matches(|c| matches!(c, ' ' | '\t' | '\n'));
                    if let Some(problem) = builtins::fragment_problem(text) {
                        return Err(self.error(
                            Code::E2006,
                            *span,
                            format!("invalid CSS fragment after interpolation: {problem}"),
                        ));
                    }
                    t.stack.push(Value::Css(text.into()));
                }
                (Some(Op::Call(_)), Out::Value(value)) => t.stack.push(value),
                _ => return Err(self.internal(site)),
            }
        }
        while let Some(op) = t.program.ops.get(t.pc) {
            self.step(site)?;
            t.pc += 1;
            let stack = &mut t.stack;
            match op {
                Op::Const(value) => {
                    if let Value::Str(text) | Value::Css(text) = value {
                        if text.len() > self.bounds.string_bytes {
                            return Err(self.string_limit(site));
                        }
                    }
                    stack.push(value.clone());
                }
                Op::Str(template) | Op::Fragment(template, _) => {
                    let site = match op {
                        Op::Fragment(_, span) => *span,
                        _ => site,
                    };
                    return Ok(Action::Push(Task::Text(TextTask {
                        template,
                        next: 0,
                        out: String::new(),
                        after: None,
                        bounded: true,
                        site,
                        env: Rc::clone(&t.env),
                    })));
                }
                Op::Load(name, span) => {
                    let value = self.lookup(&t.env, name, *span)?;
                    stack.push(value);
                }
                Op::Neg(span) => match self.pop(stack, site)? {
                    Value::Number(ref n) => stack.push(Value::Number(n.neg())),
                    other => return Err(self.operand_type("-", "a number", &other, *span)),
                },
                Op::Not(span) => match self.pop(stack, site)? {
                    Value::Bool(b) => stack.push(Value::Bool(!b)),
                    other => return Err(self.operand_type("!", "a bool", &other, *span)),
                },
                Op::Binary(op, span) => {
                    let right = self.pop(stack, site)?;
                    let left = self.pop(stack, site)?;
                    let value = self.binary(*op, &left, &right, *span)?;
                    stack.push(value);
                }
                Op::JumpIfFalse(target, span) | Op::JumpIfTrue(target, span) => {
                    let skip_on = matches!(op, Op::JumpIfTrue(..));
                    let symbol = if skip_on { "||" } else { "&&" };
                    match self.pop(stack, site)? {
                        Value::Bool(b) if b == skip_on => {
                            stack.push(Value::Bool(b));
                            t.pc = *target;
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
                    let items = self.take(stack, *count, *span)?;
                    let value = Value::List(Rc::new(items));
                    self.charge_value(&value, *span)?;
                    t.stack.push(value);
                }
                Op::Map(keys, span) => {
                    let values = self.take(stack, keys.len(), *span)?;
                    let entries = keys.iter().map(|(k, _)| Rc::clone(k)).zip(values);
                    let value = Value::Map(Rc::new(Map::from_entries(entries)));
                    self.charge_value(&value, *span)?;
                    t.stack.push(value);
                }
                Op::Member(key, span) => {
                    let base = self.pop(stack, site)?;
                    let value = self.member(&base, key, *span)?;
                    stack.push(value);
                }
                Op::Index(span) => {
                    let index = self.pop(stack, site)?;
                    let base = self.pop(stack, site)?;
                    let value = self.index(&base, &index, *span)?;
                    stack.push(value);
                }
                Op::Call(call) => {
                    let args = self.take(stack, call.args.len(), call.callee.span)?;
                    match self.callee(call, &t.env)? {
                        Callee::Builtin(builtin) => {
                            let mut budget = self.budget();
                            let before = budget;
                            let result =
                                builtins::call(builtin, call, args, self.bounds, &mut budget);
                            self.settle(before, budget, call.callee.span)?;
                            let value = match result {
                                Ok(value) => value,
                                Err(d) => {
                                    let limit = d.code() == Code::E8004;
                                    let fail = self.report(d);
                                    if limit {
                                        self.aborted = true;
                                    }
                                    return Err(fail);
                                }
                            };
                            self.charge_value(&value, call.callee.span)?;
                            t.stack.push(value);
                        }
                        Callee::User(def) => {
                            return self.start_call(def, call, args).map(Action::Push);
                        }
                    }
                }
            }
        }
        if t.all {
            return Ok(Action::Done(Out::Values(std::mem::take(&mut t.stack))));
        }
        let value = self.pop(&mut t.stack, site)?;
        Ok(Action::Done(Out::Value(value)))
    }

    /// Matches a call's arguments to a user function's parameters and
    /// starts binding them.
    fn start_call(&mut self, def: &'s FnDef, call: &Call, args: Vec<Value>) -> Eval<Task<'s>> {
        let names: Vec<&str> = def.params.iter().map(|p| &*p.name).collect();
        let slots = builtins::match_args(&def.name, &names, call)
            .map_err(|d| self.report(d.with_label(def.span, "function defined here")))?;
        let mut args: Vec<Option<Value>> = args.into_iter().map(Some).collect();
        let bound = slots
            .into_iter()
            .map(|slot| {
                slot.and_then(|i| {
                    let span = call.args.get(i).map_or(call.callee.span, |a| a.span);
                    args.get_mut(i).and_then(Option::take).map(|v| (v, span))
                })
            })
            .collect();
        let params: Vec<&Rc<str>> = def.params.iter().map(|p| &p.name).collect();
        let env = Env::block(Some(Rc::clone(&self.module)), &def.body, &params);
        self.enter(format!("in a call to `{}`", def.name), call.callee.span)?;
        Ok(Task::Call(CallTask {
            def,
            callee: call.callee.span,
            args: bound,
            next: 0,
            env,
        }))
    }

    /// Checks a parameter's type annotation (E4005).
    fn check_param(
        &mut self,
        what: &str,
        param: &Param,
        value: &Value,
        span: Span,
        default: bool,
    ) -> Eval<()> {
        let Some(ty) = param.ty else {
            return Ok(());
        };
        if ty.accepts(value) {
            return Ok(());
        }
        let source = if default {
            "its default value is"
        } else {
            "got"
        };
        let mut diagnostic = Diagnostic::error(
            Code::E4005,
            format!(
                "`{what}` expects a {} for `{}`, {source} {}",
                ty.name(),
                param.name,
                describe(value)
            ),
        )
        .with_primary(span)
        .with_label(param.span, "parameter declared here");
        if ty == Type::Length && matches!(value, Value::Number(n) if n.unit.is_none()) {
            diagnostic = diagnostic
                .with_note("a unitless number is not a length: write `0px`, or multiply by `1px`");
        }
        Err(self.report(diagnostic))
    }

    fn call_task(&mut self, t: &mut CallTask<'s>, input: Option<Out>) -> Eval<Action<'s>> {
        let def = t.def;
        if let Some(out) = input {
            let (Out::Value(value), Some(param)) = (out, def.params.get(t.next)) else {
                return Err(self.internal(t.callee));
            };
            self.check_param(&def.name, param, &value, t.callee, true)?;
            t.env.set(&param.name, Slot::Value(value));
            t.next += 1;
        }
        while let Some(param) = def.params.get(t.next) {
            match t.args.get_mut(t.next).and_then(Option::take) {
                Some((value, span)) => {
                    self.check_param(&def.name, param, &value, span, false)?;
                    t.env.set(&param.name, Slot::Value(value));
                    t.next += 1;
                }
                None => match &param.default {
                    Some(default) => {
                        return Ok(Action::Push(Task::Expr(ExprTask {
                            program: default,
                            pc: 0,
                            stack: Vec::new(),
                            env: Rc::clone(&t.env),
                            site: param.span,
                            all: false,
                        })));
                    }
                    None => {
                        return Err(self.report(
                            Diagnostic::error(
                                Code::E4001,
                                format!("missing argument `{}` for `{}`", param.name, def.name),
                            )
                            .with_primary(t.callee)
                            .with_label(param.span, "parameter declared here"),
                        ));
                    }
                },
            }
        }
        Ok(Action::Become(Task::Body(BodyTask {
            def,
            callee: t.callee,
            blocks: vec![Block {
                items: def.body.iter(),
                env: Rc::clone(&t.env),
                each: None,
            }],
            wait: Wait::Nothing,
        })))
    }

    fn expr(program: &'s Program, env: &Rc<Env<'s>>, site: Span) -> Action<'s> {
        Action::Push(Task::Expr(ExprTask {
            program,
            pc: 0,
            stack: Vec::new(),
            env: Rc::clone(env),
            site,
            all: false,
        }))
    }

    fn body_task(&mut self, t: &mut BodyTask<'s>, input: Option<Out>) -> Eval<Action<'s>> {
        if let Some(out) = input {
            let Out::Value(value) = out else {
                return Err(self.internal(t.callee));
            };
            let wait = std::mem::replace(&mut t.wait, Wait::Nothing);
            let Some(env) = t.blocks.last().map(|b| Rc::clone(&b.env)) else {
                return Err(self.internal(t.callee));
            };
            match wait {
                Wait::Nothing => return Err(self.internal(t.callee)),
                Wait::Bind(name) => env.set(name, Slot::Value(value)),
                Wait::Condition {
                    branches,
                    index,
                    otherwise,
                } => {
                    let Some(branch) = branches.get(index) else {
                        return Err(self.internal(t.callee));
                    };
                    match self.condition(&value, branch.span)? {
                        true => t.blocks.push(Block {
                            items: branch.body.iter(),
                            env: Env::block(Some(env), &branch.body, &[]),
                            each: None,
                        }),
                        false => match branches.get(index + 1) {
                            Some(next) => {
                                t.wait = Wait::Condition {
                                    branches,
                                    index: index + 1,
                                    otherwise,
                                };
                                return Ok(Self::expr(&next.condition, &env, next.span));
                            }
                            None => {
                                if let Some(body) = otherwise {
                                    t.blocks.push(Block {
                                        items: body.iter(),
                                        env: Env::block(Some(env), body, &[]),
                                        each: None,
                                    });
                                }
                            }
                        },
                    }
                }
                Wait::Iterable(each) => {
                    let mut lp = self.start_loop(each, value, env)?;
                    if let Some(env) = lp.next_env() {
                        self.step(each.span)?;
                        t.blocks.push(Block {
                            items: each.body.iter(),
                            env,
                            each: Some(lp),
                        });
                    }
                }
                Wait::Yield(span) => {
                    if let Some((ty, ty_span)) = t.def.returns {
                        if !ty.accepts(&value) {
                            return Err(self.report(
                                Diagnostic::error(
                                    Code::E4006,
                                    format!(
                                        "`{}` must yield a {}, but this yields {}",
                                        t.def.name,
                                        ty.name(),
                                        describe(&value)
                                    ),
                                )
                                .with_primary(span)
                                .with_label(ty_span, "return type declared here"),
                            ));
                        }
                    }
                    return Ok(Action::Done(Out::Value(value)));
                }
                Wait::Report(warning, span) => self.user_report(warning, &value, span)?,
            }
        }
        loop {
            let Some(block) = t.blocks.last_mut() else {
                return Err(self.report(
                    Diagnostic::error(
                        Code::E4007,
                        format!("`{}` ended without `yield`", t.def.name),
                    )
                    .with_primary(t.callee)
                    .with_label(t.def.span, "function defined here")
                    .with_note("every path through a function body must reach `yield`"),
                ));
            };
            let Some(item) = block.items.next() else {
                let next = block.each.as_mut().and_then(Loop::next_env);
                match (next, block.each.as_ref()) {
                    (Some(env), Some(lp)) => {
                        let each = lp.each;
                        self.step(each.span)?;
                        block.env = env;
                        block.items = each.body.iter();
                    }
                    _ => {
                        t.blocks.pop();
                    }
                }
                continue;
            };
            let env = Rc::clone(&block.env);
            match item {
                Item::Bind { name, span, value } => {
                    self.step(*span)?;
                    t.wait = Wait::Bind(name);
                    return Ok(Self::expr(value, &env, *span));
                }
                Item::When {
                    branches,
                    otherwise,
                } => {
                    let Some(first) = branches.first() else {
                        continue;
                    };
                    self.step(first.span)?;
                    t.wait = Wait::Condition {
                        branches,
                        index: 0,
                        otherwise: otherwise.as_deref(),
                    };
                    return Ok(Self::expr(&first.condition, &env, first.span));
                }
                Item::Each(each) => {
                    self.step(each.span)?;
                    t.wait = Wait::Iterable(each);
                    return Ok(Self::expr(&each.iterable, &env, each.span));
                }
                Item::Yield { value, span } => {
                    self.step(*span)?;
                    t.wait = Wait::Yield(*span);
                    return Ok(Self::expr(value, &env, *span));
                }
                Item::Report {
                    warning,
                    value,
                    span,
                } => {
                    self.step(*span)?;
                    t.wait = Wait::Report(*warning, *span);
                    return Ok(Self::expr(value, &env, *span));
                }
                // The parser admits nothing else in a function body.
                _ => return Err(self.internal(t.callee)),
            }
        }
    }

    fn text_task(&mut self, t: &mut TextTask<'s>, input: Option<Out>) -> Eval<Action<'s>> {
        if let Some(out) = input {
            let interp = t.next.checked_sub(1).and_then(|i| t.template.pieces.get(i));
            let (Out::Value(value), Some(Piece::Interp(interp))) = (out, interp) else {
                return Err(self.internal(t.site));
            };
            self.inserted(t, interp, &value)?;
        }
        while let Some(piece) = t.template.pieces.get(t.next) {
            t.next += 1;
            match piece {
                Piece::Text(text) => {
                    if let Some(span) = t.after.take() {
                        if t.out.ends_with('/') && text.starts_with('*') {
                            return Err(self.comment_opener(span));
                        }
                    }
                    t.out.push_str(text);
                    if t.bounded && t.out.len() > self.bounds.string_bytes {
                        return Err(self.string_limit(t.site));
                    }
                }
                Piece::Interp(interp) => {
                    return Ok(Self::expr(&interp.program, &t.env, interp.span));
                }
            }
        }
        self.charge((t.out.len() / 1024) as u64, t.site)?;
        Ok(Action::Done(Out::Text(std::mem::take(&mut t.out))))
    }

    /// Appends an interpolated value to a template's text.
    fn inserted(&mut self, t: &mut TextTask<'s>, interp: &Interp, value: &Value) -> Eval<()> {
        let start = t.out.len();
        self.insert(value, interp.ctx, &mut t.out, interp.span)?;
        let structural = matches!(interp.ctx, Ctx::Value | Ctx::Selector | Ctx::Name);
        if structural {
            let inserted = t.out.get(start..).unwrap_or("");
            let before = t.out.get(..start).unwrap_or("");
            if before.ends_with('/') && inserted.starts_with('*') {
                return Err(self.comment_opener(interp.span));
            }
        }
        t.after = structural.then_some(interp.span);
        if t.bounded && t.out.len() > self.bounds.string_bytes {
            return Err(self.string_limit(t.site));
        }
        Ok(())
    }

    // ----- shared statement semantics -------------------------------------------------

    /// A `when` condition (E2002 unless a bool).
    fn condition(&mut self, value: &Value, span: Span) -> Eval<bool> {
        match value {
            Value::Bool(b) => Ok(*b),
            other => Err(self.report(
                Diagnostic::error(
                    Code::E2002,
                    format!("a condition must be a bool, got {}", describe(other)),
                )
                .with_primary(span)
                .with_note(
                    "there is no implicit truthiness: compare explicitly, for example `count > 0`",
                ),
            )),
        }
    }

    /// Starts an `each` loop over `value` (E2010 for other types or the
    /// wrong number of loop variables).
    fn start_loop(&mut self, each: &'s Each, value: Value, parent: Rc<Env<'s>>) -> Eval<Loop<'s>> {
        let items = match &value {
            Value::List(items) => Sequence::List(Rc::clone(items)),
            Value::Map(map) if each.key.is_some() => Sequence::Map(Rc::clone(map)),
            Value::Map(_) => {
                return Err(self.report(
                    Diagnostic::error(Code::E2010, "iterating a map needs two loop variables")
                        .with_primary(each.span)
                        .with_note("write `each key, value in map { … }`"),
                ));
            }
            other => {
                return Err(self.report(
                    Diagnostic::error(Code::E2010, format!("{} is not iterable", describe(other)))
                        .with_primary(each.span)
                        .with_note(
                            "`each` iterates lists and maps; use range(start, end) for numbers",
                        ),
                ));
            }
        };
        Ok(Loop {
            each,
            items,
            next: 0,
            parent,
        })
    }

    /// `fail` (E9001) or `warn` (W9001).
    fn user_report(&mut self, warning: bool, value: &Value, span: Span) -> Eval<()> {
        let Value::Str(message) = value else {
            return Err(self.error(
                Code::E2011,
                span,
                format!(
                    "`{}` needs a string message, got {}",
                    if warning { "warn" } else { "fail" },
                    describe(value)
                ),
            ));
        };
        if warning {
            self.report(Diagnostic::warning(Code::W9001, message.to_string()).with_primary(span));
            Ok(())
        } else {
            Err(self.error(Code::E9001, span, message.to_string()))
        }
    }

    // ----- CSS blocks ------------------------------------------------------------------

    fn run_items(&mut self, items: &'s [Item], module: Rc<Env<'s>>, origin: Span) -> Vec<Node> {
        let mut stack = vec![Frame {
            site: origin,
            items: items.iter(),
            env: module,
            out: Vec::new(),
            build: Build::Top,
            target: 0,
            place: Place::Stylesheet,
            slot: None,
            each: None,
            expansion: None,
        }];
        loop {
            let next = match stack.last_mut() {
                Some(frame) if !self.aborted => frame.items.next(),
                Some(_) => None,
                None => return Vec::new(),
            };
            match next {
                Some(item) => self.item(item, &mut stack),
                None => {
                    if self.aborted {
                        return Vec::new();
                    }
                    if let Some(done) = self.close(&mut stack) {
                        return done;
                    }
                }
            }
        }
    }

    /// Closes the innermost frame, or starts its loop's next iteration.
    /// Returns the output when the top frame closes.
    fn close(&mut self, stack: &mut Vec<Frame<'s>>) -> Option<Vec<Node>> {
        let frame = stack.last_mut()?;
        if let Some(lp) = frame.each.as_mut() {
            let each = lp.each;
            if let Some(env) = lp.next_env() {
                if self.step(each.span).is_ok() {
                    frame.env = env;
                    frame.items = each.body.iter();
                }
                return None;
            }
        }
        let done = stack.pop()?;
        if let (Some(errors), Some(slot)) = (done.expansion, &done.slot) {
            self.leave();
            let parent = self.calls.as_ref().and_then(|c| c.parent.clone());
            self.calls = parent;
            if slot.content.is_some() && slot.used.get() == 0 && self.errors == errors {
                self.report(
                    Diagnostic::error(
                        Code::E4010,
                        "the content block is never placed: the recipe did not execute `slot`",
                    )
                    .with_primary(slot.apply)
                    .with_note("a recipe given a content block must execute `slot;` exactly once"),
                );
            }
        }
        let site = done.site;
        if let Build::Top = done.build {
            return Some(done.out);
        }
        let node = match done.build {
            // A rule without declarations is not emitted (LANGUAGE.md §11).
            Build::Rule(Some(selector)) if done.out.iter().any(Node::emits) => Node::Rule {
                selector,
                body: done.out,
                origin: self.origin(site),
            },
            Build::AtRule(name, Some(prelude)) => Node::AtRule {
                name,
                prelude,
                body: Some(done.out),
                origin: self.origin(site),
            },
            Build::Keyframe(Some(selectors)) => Node::Keyframe {
                selectors,
                body: done.out,
                origin: self.origin(site),
            },
            _ => return None,
        };
        let target = stack.last().map_or(0, |f| f.target);
        let _ = self.emit(stack, target, node, false, site);
        None
    }

    /// Adds a node to the output of frame `target`, checking the output
    /// limit (E8006). `neutral` nodes may precede `@import`.
    fn emit(
        &mut self,
        stack: &mut [Frame<'s>],
        target: usize,
        node: Node,
        neutral: bool,
        site: Span,
    ) -> Eval<()> {
        self.step(site)?;
        // A lower bound of the bytes the emitter writes for the node, so the
        // check never rejects output that fits.
        let prelude = |p: &str| if p.is_empty() { 0 } else { p.len() + 1 };
        let bytes = match &node {
            Node::Rule { selector, .. } => selector.len() + 5,
            Node::Declaration {
                name,
                value,
                important,
                ..
            } => 2 + name.len() + 1 + prelude(value) + if *important { 11 } else { 0 } + 2,
            Node::AtRule {
                name,
                prelude: p,
                body,
                ..
            } => name.len() + prelude(p) + if body.is_some() { 5 } else { 2 },
            Node::Keyframe { selectors, .. } => selectors.len() + 5,
            Node::Verbatim { text, .. } => text.len() + 1,
        };
        self.output = self.output.saturating_add(bytes as u64);
        if self.output > self.max_output {
            let fail = self.report(
                Diagnostic::error(
                    Code::E8006,
                    format!(
                        "the emitted CSS exceeds the output limit of {} bytes",
                        self.max_output
                    ),
                )
                .with_primary(site)
                .with_note("raise it with the `output-bytes` limit"),
            );
            self.aborted = true;
            return Err(fail);
        }
        if target == 0 && !neutral {
            self.top_blocked = true;
        }
        if let Some(frame) = stack.get_mut(target) {
            frame.out.push(node);
        }
        Ok(())
    }

    /// Opens a frame for a block whose items go to the enclosing CSS block.
    fn inline(
        stack: &mut Vec<Frame<'s>>,
        items: &'s [Item],
        env: Rc<Env<'s>>,
        slot: Option<Rc<SlotTarget<'s>>>,
    ) {
        let Some(parent) = stack.last() else {
            return;
        };
        let (target, place, site) = (parent.target, parent.place, parent.site);
        stack.push(Frame {
            site,
            items: items.iter(),
            env,
            out: Vec::new(),
            build: Build::Inline,
            target,
            place,
            slot,
            each: None,
            expansion: None,
        });
    }

    /// Reports an item that its expansion placed where it is not allowed.
    fn misplaced(&mut self, code: Code, span: Span, what: &str, place: Place) {
        let mut diagnostic =
            Diagnostic::error(code, format!("{what} is not allowed {}", place.describe()))
                .with_primary(span);
        if code == Code::E6001 {
            diagnostic = diagnostic.with_note(
                "the recipe or content block is expanded at stylesheet level; apply it inside a rule",
            );
        }
        self.report(diagnostic);
    }

    fn item(&mut self, item: &'s Item, stack: &mut Vec<Frame<'s>>) {
        let Some(frame) = stack.last() else {
            return;
        };
        let env = Rc::clone(&frame.env);
        let (target, place, slot) = (frame.target, frame.place, frame.slot.clone());
        let index = stack.len() - 1;
        match item {
            Item::Rule {
                selector,
                body,
                span,
                dynamic,
            } => {
                if self.step(*span).is_err() {
                    return;
                }
                match place {
                    Place::Stylesheet => {}
                    Place::Rule => {
                        self.report(
                            Diagnostic::error(
                                Code::E5001,
                                "nested `rule`s (nesting) are not implemented in this build",
                            )
                            .with_primary(*span)
                            .with_note("the recipe or content block is expanded inside a rule; nesting arrives in a later phase"),
                        );
                        return;
                    }
                    Place::Declarations => {
                        return self.misplaced(Code::E6002, *span, "a `rule`", place);
                    }
                    Place::Keyframes | Place::Page => {
                        return self.misplaced(Code::E6005, *span, "a `rule`", place);
                    }
                }
                let check = dynamic.then_some(*span);
                let selector = self.region(selector, Region::Selector, &env, check).ok();
                stack.push(Frame {
                    site: *span,
                    items: body.iter(),
                    env: Env::block(Some(env), body, &[]),
                    out: Vec::new(),
                    build: Build::Rule(selector),
                    target: index + 1,
                    place: Place::Rule,
                    slot,
                    each: None,
                    expansion: None,
                });
            }
            Item::AtRule {
                name,
                prelude,
                body: Some(body),
                context,
                span,
            } => {
                if self.step(*span).is_err() {
                    return;
                }
                let prelude = self.region(prelude, Region::Prelude, &env, None).ok();
                stack.push(Frame {
                    site: *span,
                    items: body.iter(),
                    env: Env::block(Some(env), body, &[]),
                    out: Vec::new(),
                    build: Build::AtRule(name.clone(), prelude),
                    target: index + 1,
                    place: Place::of(*context),
                    slot,
                    each: None,
                    expansion: None,
                });
            }
            Item::Keyframe {
                selectors,
                body,
                span,
            } => {
                let selectors = self.region(selectors, Region::Prelude, &env, None).ok();
                stack.push(Frame {
                    site: *span,
                    items: body.iter(),
                    env: Env::block(Some(env), body, &[]),
                    out: Vec::new(),
                    build: Build::Keyframe(selectors),
                    target: index + 1,
                    place: Place::Declarations,
                    slot,
                    each: None,
                    expansion: None,
                });
            }
            Item::Declaration {
                name,
                value,
                important,
                custom,
                span,
            } => {
                match place {
                    Place::Stylesheet => {
                        return self.misplaced(Code::E6001, *span, "a declaration", place);
                    }
                    Place::Keyframes => {
                        return self.misplaced(Code::E6005, *span, "a declaration", place);
                    }
                    Place::Rule | Place::Declarations | Place::Page => {}
                }
                let name = self.region(name, Region::Name, &env, None).ok();
                let value = self
                    .region(value, Region::Value { custom: *custom }, &env, None)
                    .ok();
                if let (Some(name), Some(value)) = (name, value) {
                    let node = Node::Declaration {
                        name,
                        value,
                        important: *important,
                        origin: self.origin(*span),
                    };
                    let _ = self.emit(stack, target, node, false, *span);
                }
            }
            Item::AtRule {
                name,
                prelude,
                body: None,
                span,
                ..
            } => {
                let lower = name.to_ascii_lowercase();
                let ordered = matches!(lower.as_str(), "@import" | "@namespace");
                if ordered && target == 0 && self.top_blocked {
                    self.report(
                        Diagnostic::error(
                            Code::E7010,
                            format!("`{name}` must come before style rules and other at-rules; Weave does not reorder CSS"),
                        )
                        .with_primary(*span)
                        .with_note("an earlier `when`, `each` or `apply` emitted a rule or block; move this statement before it"),
                    );
                    return;
                }
                if let Ok(prelude) = self.region(prelude, Region::Prelude, &env, None) {
                    let node = Node::AtRule {
                        name: name.clone(),
                        prelude,
                        body: None,
                        origin: self.origin(*span),
                    };
                    let neutral = matches!(
                        lower.as_str(),
                        "@import" | "@namespace" | "@charset" | "@layer"
                    );
                    let _ = self.emit(stack, target, node, neutral, *span);
                }
            }
            Item::Verbatim { text, kind, span } => {
                if *kind == VerbatimKind::Opaque
                    && matches!(place, Place::Declarations | Place::Keyframes | Place::Page)
                {
                    return self.misplaced(Code::E6005, *span, "`opaque`", place);
                }
                let neutral = *kind == VerbatimKind::Comment;
                let node = Node::Verbatim {
                    text: text.clone(),
                    origin: self.origin(*span),
                };
                let _ = self.emit(stack, target, node, neutral, *span);
            }
            Item::Bind { name, span, value } => {
                if self.step(*span).is_err() {
                    return;
                }
                let slot = match self.value(value, &env, *span) {
                    Ok(value) => Slot::Value(value),
                    Err(Fail) => Slot::Poisoned,
                };
                env.set(name, slot);
            }
            Item::When {
                branches,
                otherwise,
            } => {
                for branch in branches {
                    if self.step(branch.span).is_err() {
                        return;
                    }
                    let Ok(value) = self.value(&branch.condition, &env, branch.span) else {
                        return;
                    };
                    match self.condition(&value, branch.span) {
                        Ok(true) => {
                            let scope = Env::block(Some(env), &branch.body, &[]);
                            return Self::inline(stack, &branch.body, scope, slot);
                        }
                        Ok(false) => {}
                        Err(Fail) => return,
                    }
                }
                if let Some(body) = otherwise {
                    let scope = Env::block(Some(env), body, &[]);
                    Self::inline(stack, body, scope, slot);
                }
            }
            Item::Each(each) => {
                if self.step(each.span).is_err() {
                    return;
                }
                let Ok(value) = self.value(&each.iterable, &env, each.span) else {
                    return;
                };
                let Ok(mut lp) = self.start_loop(each, value, env) else {
                    return;
                };
                if let Some(scope) = lp.next_env() {
                    if self.step(each.span).is_err() {
                        return;
                    }
                    Self::inline(stack, &each.body, scope, slot);
                    if let Some(frame) = stack.last_mut() {
                        frame.each = Some(lp);
                    }
                }
            }
            Item::Apply(apply) => {
                let _ = self.apply(apply, stack, env, slot);
            }
            Item::Slot(span) => {
                if self.step(*span).is_err() {
                    return;
                }
                let Some(target_slot) = slot else {
                    self.internal(*span);
                    return;
                };
                let used = target_slot.used.get().saturating_add(1);
                target_slot.used.set(used);
                let Some(content) = &target_slot.content else {
                    return;
                };
                if used > 1 {
                    self.report(
                        Diagnostic::error(
                            Code::E4011,
                            "`slot` is executed more than once for one content block",
                        )
                        .with_primary(*span)
                        .with_label(target_slot.apply, "content block passed here")
                        .with_note(
                            "a recipe given a content block must execute `slot;` exactly once",
                        ),
                    );
                    return;
                }
                let scope = Env::block(Some(Rc::clone(&content.env)), content.items, &[]);
                let inner = content.slot.clone();
                Self::inline(stack, content.items, scope, inner);
            }
            Item::Report {
                warning,
                value,
                span,
            } => {
                if self.step(*span).is_err() {
                    return;
                }
                if let Ok(value) = self.value(value, &env, *span) {
                    let _ = self.user_report(*warning, &value, *span);
                }
            }
            Item::Fn(_) | Item::Recipe(_) => {}
            Item::Yield { span, .. } => {
                self.internal(*span);
            }
        }
    }

    /// `apply`: binds the recipe's arguments in a scope of its own (the
    /// module scope plus its parameters) and expands its body in place.
    fn apply(
        &mut self,
        apply: &'s Apply,
        stack: &mut Vec<Frame<'s>>,
        env: Rc<Env<'s>>,
        slot: Option<Rc<SlotTarget<'s>>>,
    ) -> Eval<()> {
        self.step(apply.span)?;
        let call = &apply.call;
        let args = self.values(&apply.args, &env, apply.span)?;
        let def = self.recipe(call, &env)?;
        let names: Vec<&str> = def.params.iter().map(|p| &*p.name).collect();
        let slots = builtins::match_args(&def.name, &names, call)
            .map_err(|d| self.report(d.with_label(def.span, "recipe defined here")))?;
        let params: Vec<&Rc<str>> = def.params.iter().map(|p| &p.name).collect();
        let scope = Env::block(Some(Rc::clone(&self.module)), &def.body, &params);
        let mut args: Vec<Option<Value>> = args.into_iter().map(Some).collect();
        for (param, slot_index) in def.params.iter().zip(slots) {
            let given = slot_index.and_then(|i| {
                let span = call.args.get(i).map_or(call.callee.span, |a| a.span);
                args.get_mut(i).and_then(Option::take).map(|v| (v, span))
            });
            let value = match (given, &param.default) {
                (Some((value, span)), _) => {
                    self.check_param(&def.name, param, &value, span, false)?;
                    value
                }
                (None, Some(default)) => {
                    let value = self.value(default, &scope, param.span)?;
                    self.check_param(&def.name, param, &value, call.callee.span, true)?;
                    value
                }
                (None, None) => {
                    return Err(self.report(
                        Diagnostic::error(
                            Code::E4001,
                            format!("missing argument `{}` for `{}`", param.name, def.name),
                        )
                        .with_primary(call.callee.span)
                        .with_label(param.span, "parameter declared here"),
                    ));
                }
            };
            scope.set(&param.name, Slot::Value(value));
        }
        let content = apply
            .content
            .as_deref()
            .map(|items| Content { items, env, slot });
        let errors = self.errors;
        self.enter(format!("in the expansion of `{}`", def.name), apply.span)?;
        self.calls = Some(Rc::new(CallSite {
            span: apply.span,
            parent: self.calls.take(),
        }));
        let target = Rc::new(SlotTarget {
            content,
            used: Cell::new(0),
            apply: apply.span,
        });
        Self::inline(stack, &def.body, scope, Some(target));
        if let Some(frame) = stack.last_mut() {
            frame.expansion = Some(errors);
        }
        Ok(())
    }

    /// Resolves the recipe of an `apply` statement.
    fn recipe(&mut self, call: &Call, env: &Rc<Env<'s>>) -> Eval<&'s RecipeDef> {
        let callee = &call.callee;
        let shown = match &callee.namespace {
            Some((ns, _)) => format!("{ns}.{}", callee.name),
            None => callee.name.to_string(),
        };
        let diagnostic = match &callee.namespace {
            Some((namespace, ns_span)) => {
                if matches!(find(env, namespace), Found::Missing)
                    && builtins::prelude(namespace).is_none()
                {
                    Diagnostic::error(Code::E1002, format!("unresolved name `{namespace}`"))
                        .with_primary(*ns_span)
                } else {
                    Diagnostic::error(Code::E1004, format!("`{shown}` is not a recipe"))
                        .with_primary(callee.span)
                        .with_note("built-in namespaces contain functions, not recipes")
                }
            }
            None => match find(env, &callee.name) {
                Found::Recipe(def) => return Ok(def),
                Found::Function(_) => Diagnostic::error(
                    Code::E1004,
                    format!("`{shown}` is a function, not a recipe"),
                )
                .with_note("functions return values; call it inside an expression or `#{…}`"),
                Found::Value(_) | Found::Poisoned | Found::Unbound => {
                    Diagnostic::error(Code::E1004, format!("`{shown}` is a value, not a recipe"))
                }
                Found::Missing if builtins::prelude(&callee.name).is_some() => Diagnostic::error(
                    Code::E1004,
                    format!("`{shown}` is a built-in, not a recipe"),
                ),
                Found::Missing => {
                    Diagnostic::error(Code::E1002, format!("unknown recipe `{shown}`"))
                }
            }
            .with_primary(callee.span),
        };
        Err(self.report(diagnostic))
    }

    // ----- expressions ------------------------------------------------------------

    fn pop(&mut self, stack: &mut Vec<Value>, site: Span) -> Eval<Value> {
        match stack.pop() {
            Some(value) => Ok(value),
            None => Err(self.internal(site)),
        }
    }

    fn take(&mut self, stack: &mut Vec<Value>, count: usize, span: Span) -> Eval<Vec<Value>> {
        if count > self.bounds.elements {
            return Err(self.report(
                Diagnostic::error(
                    Code::E8007,
                    format!(
                        "the collection has {count} elements, more than the limit of {}",
                        self.bounds.elements
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
        if let BinOp::Eq | BinOp::Ne = op {
            let mut budget = self.budget();
            let before = budget;
            let result = equal_within(left, right, &mut budget);
            self.settle(before, budget, span)?;
            let Some(same) = result else {
                let fail = self.report(builtins::budget_error(span));
                self.aborted = true;
                return Err(fail);
            };
            return Ok(Value::Bool(same == (op == BinOp::Eq)));
        }
        let symbol = op.symbol();
        if let (BinOp::Add, Value::Str(a), Value::Str(b)) = (op, left, right) {
            let len = a.len() + b.len();
            if len > self.bounds.string_bytes {
                return Err(self.string_limit(span));
            }
            self.charge((len / 1024) as u64, span)?;
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
                BinOp::Add | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    "two numbers or two strings"
                }
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

    // ----- interpolation ------------------------------------------------------

    fn comment_opener(&mut self, span: Span) -> Fail {
        self.error(
            Code::E2003,
            span,
            "the interpolated text would form a comment opener `/*` with the adjacent text",
        )
    }

    /// Evaluates a CSS region and checks that interpolation left its
    /// structure intact. `check` forces the check for text without
    /// interpolations, reported at the given span: the selectors of rules
    /// in recipe bodies and content blocks, which the parser could not
    /// check.
    fn region(
        &mut self,
        template: &'s Template,
        region: Region,
        env: &Rc<Env<'s>>,
        check: Option<Span>,
    ) -> Eval<String> {
        let site = template.pieces.iter().find_map(|p| match p {
            Piece::Interp(i) => Some(i.span),
            Piece::Text(_) => None,
        });
        let text = match (template.static_text(), site) {
            (Some(text), _) if check.is_none() => return Ok(text.to_string()),
            (Some(text), _) => text.to_string(),
            (None, Some(site)) => self.text(template, env, false, site)?,
            (None, None) => template
                .pieces
                .iter()
                .filter_map(|p| match p {
                    Piece::Text(text) => Some(text.as_str()),
                    Piece::Interp(_) => None,
                })
                .collect(),
        };
        if site.is_none() && check.is_none() {
            return Ok(text);
        }
        if let Some((code, problem)) = structure_problem(&text, region) {
            let what = match region {
                Region::Value { .. } => "declaration value",
                Region::Prelude => "at-rule prelude",
                Region::Selector => "selector",
                Region::Name => "declaration name",
            };
            let when = if site.is_some() {
                " after interpolation"
            } else {
                ""
            };
            let mut diagnostic =
                Diagnostic::error(code, format!("invalid {what}{when}: {problem}"))
                    .with_note(format!("the result was `{}`", shorten(&text)));
            if let Some(site) = site.or(check) {
                diagnostic = diagnostic.with_primary(site);
            }
            return Err(self.report(diagnostic));
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

    /// Evaluates `text`, which must compile without errors.
    fn nodes(text: &str) -> Vec<Node> {
        let mut sources = crate::source::SourceManager::new();
        let file = sources.add("t.weave", text.as_bytes().to_vec()).unwrap();
        let mut diags = Diagnostics::new(100);
        let sheet = crate::parser::parse(text, 0, file, 128, &mut diags);
        let out = evaluate(&sheet.items, file, &ResourceLimits::default(), &mut diags);
        assert!(!diags.has_errors(), "{:?}", diags.into_vec());
        out
    }

    fn origin_of(node: &Node) -> (u32, Vec<u32>) {
        let origin = match node {
            Node::Rule { origin, .. }
            | Node::Declaration { origin, .. }
            | Node::AtRule { origin, .. }
            | Node::Keyframe { origin, .. }
            | Node::Verbatim { origin, .. } => origin.clone().unwrap(),
        };
        let mut calls = Vec::new();
        let mut site = origin.calls.clone();
        while let Some(s) = site {
            calls.push(s.span.start);
            site = s.parent.clone();
        }
        (origin.span.start, calls)
    }

    #[test]
    fn nodes_record_definition_and_invocation_locations() {
        let text = "recipe inner { a: b; slot; }\n\
                    recipe outer { apply inner { c: d; } }\n\
                    rule .x { apply outer; e: f; }";
        let out = nodes(text);
        let Some(Node::Rule { body, .. }) = out.first() else {
            panic!("{out:?}")
        };
        let at = |needle: &str| u32::try_from(text.find(needle).unwrap()).unwrap();
        let apply_outer = at("apply outer");
        let apply_inner = at("apply inner");
        // `a: b` comes from `inner`, expanded by `outer`, expanded in `.x`.
        assert_eq!(
            origin_of(&body[0]),
            (at("a: b"), vec![apply_inner, apply_outer])
        );
        // The content block's `c: d` is written in `outer`; it is placed by
        // `inner`'s slot, so both applications are on its path.
        assert_eq!(
            origin_of(&body[1]),
            (at("c: d"), vec![apply_inner, apply_outer])
        );
        // Outside any expansion there is no call chain.
        assert_eq!(origin_of(&body[2]), (at("e: f"), vec![]));
        assert_eq!(origin_of(&out[0]), (at("rule .x"), vec![]));
    }

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
