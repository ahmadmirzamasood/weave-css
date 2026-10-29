//! The prelude (LANGUAGE.md §4.3, TYPES.md §9, docs/BUILTINS.md).
//!
//! This build implements `range` and the `css`, `math`, `list`, `map`,
//! `string`, `color` and `meta` namespaces. Every built-in is deterministic: none reads
//! files, the network, the clock or the environment, and each result is
//! bounded by the string and collection limits.

use std::collections::HashMap;
use std::rc::Rc;

use crate::diagnostic::{Code, Diagnostic};
use crate::expr::Call;
use crate::lexer::{Lexer, Mode, TokenKind as K};
use crate::number::{Number, Unit, checked};
use crate::source::Span;
use crate::value::{Color, Map, Value, describe, equal_within, escape_ident};

/// What a prelude name denotes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prelude {
    Namespace,
    /// `range`.
    Function,
}

/// The built-in namespaces.
const NAMESPACES: [&str; 7] = ["css", "math", "list", "map", "string", "color", "meta"];

pub(crate) fn prelude(name: &str) -> Option<Prelude> {
    if name == "range" {
        Some(Prelude::Function)
    } else if NAMESPACES.contains(&name) {
        Some(Prelude::Namespace)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Range,
    CssSpace,
    CssComma,
    CssSlash,
    CssIdent,
    CssFragment,
    MathRound,
    MathFloor,
    MathCeil,
    MathAbs,
    MathMin,
    MathMax,
    MathClamp,
    MathUnit,
    MathValue,
    MathConvert,
    ListLen,
    ListAppend,
    ListConcat,
    ListSlice,
    ListContains,
    MapLen,
    MapHas,
    MapGet,
    MapSet,
    MapRemove,
    MapMerge,
    MapKeys,
    MapValues,
    StringLen,
    StringSlice,
    StringContains,
    StringUpper,
    StringLower,
    StringJoin,
    ColorRgb,
    ColorMix,
    ColorRed,
    ColorGreen,
    ColorBlue,
    ColorAlpha,
    MetaType,
}

use Builtin as B;

/// Every implemented built-in: namespace (empty for prelude functions),
/// member name, parameters. A parameter written `name?` is optional.
const TABLE: &[(&str, &str, Builtin, &[&str])] = &[
    ("", "range", B::Range, &["start", "end", "step?"]),
    ("css", "space", B::CssSpace, &["items"]),
    ("css", "comma", B::CssComma, &["items"]),
    ("css", "slash", B::CssSlash, &["left", "right"]),
    ("css", "ident", B::CssIdent, &["text"]),
    ("css", "fragment", B::CssFragment, &["text"]),
    ("math", "round", B::MathRound, &["x"]),
    ("math", "floor", B::MathFloor, &["x"]),
    ("math", "ceil", B::MathCeil, &["x"]),
    ("math", "abs", B::MathAbs, &["x"]),
    ("math", "min", B::MathMin, &["a", "b"]),
    ("math", "max", B::MathMax, &["a", "b"]),
    ("math", "clamp", B::MathClamp, &["low", "x", "high"]),
    ("math", "unit", B::MathUnit, &["x"]),
    ("math", "value", B::MathValue, &["x"]),
    ("math", "convert", B::MathConvert, &["x", "unit"]),
    ("list", "len", B::ListLen, &["l"]),
    ("list", "append", B::ListAppend, &["l", "value"]),
    ("list", "concat", B::ListConcat, &["a", "b"]),
    ("list", "slice", B::ListSlice, &["l", "start", "end"]),
    ("list", "contains", B::ListContains, &["l", "value"]),
    ("map", "len", B::MapLen, &["m"]),
    ("map", "has", B::MapHas, &["m", "key"]),
    ("map", "get", B::MapGet, &["m", "key", "default"]),
    ("map", "set", B::MapSet, &["m", "key", "value"]),
    ("map", "remove", B::MapRemove, &["m", "key"]),
    ("map", "merge", B::MapMerge, &["a", "b"]),
    ("map", "keys", B::MapKeys, &["m"]),
    ("map", "values", B::MapValues, &["m"]),
    ("string", "len", B::StringLen, &["s"]),
    ("string", "slice", B::StringSlice, &["s", "start", "end"]),
    ("string", "contains", B::StringContains, &["s", "needle"]),
    ("string", "upper", B::StringUpper, &["s"]),
    ("string", "lower", B::StringLower, &["s"]),
    ("string", "join", B::StringJoin, &["items", "separator"]),
    ("color", "rgb", B::ColorRgb, &["r", "g", "b", "alpha?"]),
    ("color", "mix", B::ColorMix, &["a", "b", "weight?"]),
    ("color", "red", B::ColorRed, &["c"]),
    ("color", "green", B::ColorGreen, &["c"]),
    ("color", "blue", B::ColorBlue, &["c"]),
    ("color", "alpha", B::ColorAlpha, &["c"]),
    ("meta", "type", B::MetaType, &["x"]),
];

fn entry(builtin: Builtin) -> (&'static str, &'static str, &'static [&'static str]) {
    TABLE
        .iter()
        .find(|(_, _, b, _)| *b == builtin)
        .map_or(("", "", &[][..]), |(ns, name, _, params)| {
            (*ns, *name, *params)
        })
}

impl Builtin {
    /// The name as written in a call, such as `math.round`.
    pub fn name(self) -> String {
        match entry(self) {
            ("", name, _) => name.to_string(),
            (ns, name, _) => format!("{ns}.{name}"),
        }
    }

    /// Parameter names and whether each is required.
    fn params(self) -> Vec<(&'static str, bool)> {
        entry(self)
            .2
            .iter()
            .map(|p| match p.strip_suffix('?') {
                Some(name) => (name, false),
                None => (*p, true),
            })
            .collect()
    }
}

/// A namespace member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Member {
    Implemented(Builtin),
    Unknown,
}

pub(crate) fn member(namespace: &str, name: &str) -> Member {
    if namespace.is_empty() {
        return Member::Unknown;
    }
    TABLE
        .iter()
        .find(|(ns, n, _, _)| *ns == namespace && *n == name)
        .map_or(Member::Unknown, |(_, _, builtin, _)| {
            Member::Implemented(*builtin)
        })
}

/// The prelude function `name` (`range`).
pub(crate) fn function(name: &str) -> Option<Builtin> {
    TABLE
        .iter()
        .find(|(ns, n, _, _)| ns.is_empty() && *n == name)
        .map(|(_, _, b, _)| *b)
}

/// Size bounds for built-in results.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds {
    pub string_bytes: usize,
    pub elements: usize,
}

fn at(span: Span, code: Code, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, message).with_primary(span)
}

/// Maps call arguments to parameters (LANGUAGE.md §5.4): for each
/// parameter, the index of its argument. Too many positional arguments
/// (E4004), unknown names (E4002) and a parameter passed twice (E4003) are
/// reported at the argument; missing arguments are left to the caller.
pub(crate) fn match_args(
    what: &str,
    params: &[&str],
    call: &Call,
) -> Result<Vec<Option<usize>>, Diagnostic> {
    let mut slots: Vec<Option<usize>> = vec![None; params.len()];
    let mut position = 0;
    // Named lookups scan short parameter lists and hash long ones, so a
    // call never costs parameters × arguments.
    let index: Option<HashMap<&str, usize>> = (params.len() > 16).then(|| {
        params
            .iter()
            .enumerate()
            .map(|(i, p)| (*p, i))
            .rev()
            .collect()
    });
    let find = |name: &str| match &index {
        Some(index) => index.get(name).copied(),
        None => params.iter().position(|p| *p == name),
    };
    for (i, arg) in call.args.iter().enumerate() {
        let index = match &arg.name {
            None => {
                if position >= params.len() {
                    return Err(at(
                        arg.span,
                        Code::E4004,
                        format!(
                            "too many arguments: `{what}` takes {} argument{}",
                            params.len(),
                            if params.len() == 1 { "" } else { "s" }
                        ),
                    ));
                }
                position += 1;
                position - 1
            }
            Some(name) => match find(name) {
                Some(index) => index,
                None => {
                    let mut diagnostic = at(
                        arg.span,
                        Code::E4002,
                        format!("`{what}` has no parameter named `{name}`"),
                    );
                    if !params.is_empty() {
                        diagnostic =
                            diagnostic.with_note(format!("parameters: {}", params.join(", ")));
                    }
                    return Err(diagnostic);
                }
            },
        };
        match slots.get_mut(index) {
            Some(slot @ None) => *slot = Some(i),
            _ => {
                let param = params.get(index).copied().unwrap_or("");
                return Err(at(
                    arg.span,
                    Code::E4003,
                    format!("argument `{param}` is passed more than once"),
                ));
            }
        }
    }
    Ok(slots)
}

/// Binds the arguments of a built-in call: one entry per parameter,
/// `None` for an omitted optional parameter.
fn bind_args(
    builtin: Builtin,
    call: &Call,
    args: Vec<Value>,
) -> Result<Vec<Option<(Value, Span)>>, Diagnostic> {
    let params = builtin.params();
    let names: Vec<&str> = params.iter().map(|(n, _)| *n).collect();
    let slots = match_args(&builtin.name(), &names, call)?;
    let mut args: Vec<Option<Value>> = args.into_iter().map(Some).collect();
    let mut bound = Vec::with_capacity(params.len());
    for ((name, required), slot) in params.iter().zip(slots) {
        match slot {
            Some(i) => {
                let value = args.get_mut(i).and_then(Option::take);
                let span = call.args.get(i).map_or(call.callee.span, |a| a.span);
                match value {
                    Some(value) => bound.push(Some((value, span))),
                    None => return Err(at(span, Code::E4001, "missing argument")),
                }
            }
            None if *required => {
                return Err(at(
                    call.callee.span,
                    Code::E4001,
                    format!("missing argument `{name}` for `{}`", builtin.name()),
                ));
            }
            None => bound.push(None),
        }
    }
    Ok(bound)
}

/// A built-in call's bound arguments, taken in parameter order.
struct Args {
    builtin: Builtin,
    callee: Span,
    values: std::vec::IntoIter<Option<(Value, Span)>>,
    params: std::vec::IntoIter<(&'static str, bool)>,
}

impl Args {
    /// The next parameter's argument, `None` if it was omitted.
    fn optional(&mut self) -> Result<Option<(Value, Span, &'static str)>, Diagnostic> {
        let (name, _) = self
            .params
            .next()
            .ok_or_else(|| at(self.callee, Code::E4001, "missing argument"))?;
        match self.values.next() {
            Some(Some((value, span))) => Ok(Some((value, span, name))),
            Some(None) => Ok(None),
            None => Err(at(self.callee, Code::E4001, "missing argument")),
        }
    }

    fn any(&mut self) -> Result<(Value, Span), Diagnostic> {
        match self.optional()? {
            Some((value, span, _)) => Ok((value, span)),
            None => Err(at(self.callee, Code::E4001, "missing argument")),
        }
    }

    fn mismatch(&self, name: &str, expected: &str, got: &Value, span: Span) -> Diagnostic {
        at(
            span,
            Code::E4005,
            format!(
                "`{}` expects {expected} for `{name}`, got {}",
                self.builtin.name(),
                describe(got)
            ),
        )
    }

    fn typed<T>(
        &mut self,
        expected: &str,
        take: impl FnOnce(&Value) -> Option<T>,
    ) -> Result<(T, Span), Diagnostic> {
        let Some((value, span, name)) = self.optional()? else {
            return Err(at(self.callee, Code::E4001, "missing argument"));
        };
        match take(&value) {
            Some(t) => Ok((t, span)),
            None => Err(self.mismatch(name, expected, &value, span)),
        }
    }

    fn number(&mut self) -> Result<(Number, Span), Diagnostic> {
        self.typed("a number", |v| match v {
            Value::Number(n) => Some(n.clone()),
            _ => None,
        })
    }

    fn color(&mut self) -> Result<(Color, Span), Diagnostic> {
        self.typed("a color", |v| match v {
            Value::Color(c) => Some(*c),
            _ => None,
        })
    }

    /// The next argument as a number with its parameter name; `None` when
    /// an optional argument was omitted.
    fn named_number(&mut self) -> Result<Option<(Number, Span, &'static str)>, Diagnostic> {
        match self.optional()? {
            Some((value, span, name)) => match &value {
                Value::Number(n) => Ok(Some((n.clone(), span, name))),
                other => Err(self.mismatch(name, "a number", other, span)),
            },
            None => Ok(None),
        }
    }

    fn string(&mut self) -> Result<(Rc<str>, Span), Diagnostic> {
        self.typed("a string", |v| match v {
            Value::Str(s) => Some(Rc::clone(s)),
            _ => None,
        })
    }

    fn list(&mut self) -> Result<(Rc<Vec<Value>>, Span), Diagnostic> {
        self.typed("a list", |v| match v {
            Value::List(l) => Some(Rc::clone(l)),
            _ => None,
        })
    }

    fn map(&mut self) -> Result<(Rc<Map>, Span), Diagnostic> {
        self.typed("a map", |v| match v {
            Value::Map(m) => Some(Rc::clone(m)),
            _ => None,
        })
    }

    /// A unitless integer (E2009 otherwise).
    fn integer(&mut self) -> Result<(i64, Span), Diagnostic> {
        let (n, span) = self.number()?;
        match (n.unit.is_none(), n.as_integer()) {
            (true, Some(i)) => Ok((i, span)),
            _ => Err(at(
                span,
                Code::E2009,
                format!(
                    "`{}` needs a unitless integer here, got `{}`",
                    self.builtin.name(),
                    n.serialize()
                ),
            )),
        }
    }
}

fn collection_error(builtin: Builtin, value: &Value, span: Span) -> Diagnostic {
    at(
        span,
        Code::E2004,
        format!(
            "`{}` cannot serialize a nested {}",
            builtin.name(),
            value.type_name()
        ),
    )
    .with_note("build the inner value into a fragment first, for example with css.space(…)")
}

fn string_limit(bounds: Bounds, span: Span) -> Diagnostic {
    at(
        span,
        Code::E8007,
        format!(
            "the result exceeds the string limit of {} bytes",
            bounds.string_bytes
        ),
    )
    .with_note("raise it with the `string-bytes` limit")
}

fn element_limit(bounds: Bounds, count: usize, span: Span) -> Diagnostic {
    at(
        span,
        Code::E8007,
        format!(
            "the result would have {count} elements, more than the limit of {}",
            bounds.elements
        ),
    )
    .with_note("raise it with the `collection-elements` limit")
}

fn fragment(text: String, bounds: Bounds, span: Span) -> Result<Value, Diagnostic> {
    if text.len() > bounds.string_bytes {
        return Err(string_limit(bounds, span));
    }
    Ok(Value::Css(text.into()))
}

fn list_value(items: Vec<Value>, bounds: Bounds, span: Span) -> Result<Value, Diagnostic> {
    if items.len() > bounds.elements {
        return Err(element_limit(bounds, items.len(), span));
    }
    Ok(Value::List(Rc::new(items)))
}

fn number_value(value: f64, unit: Unit, span: Span) -> Result<Value, Diagnostic> {
    checked(value)
        .map(|v| Value::Number(Number::new(v, unit)))
        .map_err(|e| at(span, e.code, e.message))
}

fn index(i: usize) -> Value {
    Value::Number(Number::new(i as f64, Unit::None))
}

/// `round`, `floor` and `ceil` on the quantized value (TYPES.md §2.1), so
/// that they agree with serialization: `2.4999999` serializes as `2.5`
/// and rounds to `3`. `round` takes ties toward +∞, as CSS `round()`.
fn round(n: &Number, builtin: Builtin) -> Number {
    const SCALE: i64 = 1_000_000;
    let q = n.quantized();
    let r = match builtin {
        B::MathFloor => q.div_euclid(SCALE),
        B::MathCeil => -(q.wrapping_neg().div_euclid(SCALE)),
        _ => q.saturating_add(SCALE / 2).div_euclid(SCALE),
    };
    Number::new(r as f64, n.unit.clone())
}

const SCALE: i128 = 1_000_000;

/// `num / den` rounded to the nearest integer, ties toward +infinity (`den > 0`).
fn round_div(num: i128, den: i128) -> i128 {
    (2 * num + den).div_euclid(2 * den)
}

/// A color channel or alpha from `n`: unitless or a percentage, clamped to
/// its range, scaled so that the full range is 255, then rounded
/// (TYPES.md section 9). `full` is the unitless value that means "full".
fn channel(
    n: &Number,
    full: i128,
    builtin: Builtin,
    name: &str,
    span: Span,
) -> Result<u8, Diagnostic> {
    let q = i128::from(n.quantized());
    let whole = if n.unit.is_none() {
        full
    } else if n.unit == Unit::parse("%") {
        100
    } else {
        return Err(at(
            span,
            Code::E3001,
            format!(
                "`{}` needs a unitless number or a percentage for `{name}`, got `{}`",
                builtin.name(),
                n.serialize()
            ),
        ));
    };
    let q = q.clamp(0, whole * SCALE);
    let value = round_div(q * 255, whole * SCALE);
    Ok(u8::try_from(value).unwrap_or(u8::MAX))
}

/// A mix weight as `(numerator, denominator)`: unitless 0-1 or a
/// percentage; anything else, or a value outside the range, is an error.
fn weight(n: &Number, builtin: Builtin, span: Span) -> Result<(i128, i128), Diagnostic> {
    let q = i128::from(n.quantized());
    let den = if n.unit.is_none() {
        SCALE
    } else if n.unit == Unit::parse("%") {
        100 * SCALE
    } else {
        return Err(at(
            span,
            Code::E3001,
            format!(
                "`{}` needs a unitless number or a percentage for `weight`, got `{}`",
                builtin.name(),
                n.serialize()
            ),
        ));
    };
    if !(0..=den).contains(&q) {
        return Err(at(
            span,
            Code::E2003,
            format!(
                "`{}` needs a weight between 0 and 1 (or 0% and 100%), got `{}`",
                builtin.name(),
                n.serialize()
            ),
        ));
    }
    Ok((q, den))
}

fn mix_channel(a: u8, b: u8, (w, den): (i128, i128)) -> u8 {
    let value = round_div(i128::from(a) * w + i128::from(b) * (den - w), den);
    u8::try_from(value).unwrap_or(u8::MAX)
}

/// `a` converted to `unit` (callers checked compatibility by comparing).
fn in_unit(value: &Number, unit: &Unit, span: Span) -> Result<Value, Diagnostic> {
    match value.convert_to(unit) {
        Some(n) => number_value(n.value, n.unit, span),
        None => Err(at(span, Code::E3001, "incompatible units")),
    }
}

fn compare(a: &Number, b: &Number, span: Span) -> Result<std::cmp::Ordering, Diagnostic> {
    a.compare(b).map_err(|e| at(span, e.code, e.message))
}

/// Character offsets `start..end` of `s` as byte offsets, if in range.
fn char_range(s: &str, start: i64, end: i64) -> Option<(usize, usize)> {
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    if start > end {
        return None;
    }
    let mut offsets = s
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(s.len()));
    let from = offsets.nth(start)?;
    let to = if end == start {
        from
    } else {
        offsets.nth(end - start - 1)?
    };
    Some((from, to))
}

/// Calls a built-in. Failures are reported at the callee unless they
/// concern one argument. `budget` bounds the value comparisons of
/// `list.contains`; running out of it is E8004.
pub(crate) fn call(
    builtin: Builtin,
    call: &Call,
    args: Vec<Value>,
    bounds: Bounds,
    budget: &mut u64,
) -> Result<Value, Diagnostic> {
    let span = call.callee.span;
    let params = builtin.params();
    let mut args = Args {
        builtin,
        callee: span,
        values: bind_args(builtin, call, args)?.into_iter(),
        params: params.into_iter(),
    };
    let name = builtin.name();
    match builtin {
        B::Range => {
            let (start, _) = args.integer()?;
            let (end, _) = args.integer()?;
            let step = match args.optional()? {
                Some((Value::Number(ref n), s, _)) => match (n.unit.is_none(), n.as_integer()) {
                    (true, Some(i)) => (i, s),
                    _ => {
                        return Err(at(
                            s,
                            Code::E2009,
                            format!(
                                "`range` needs a unitless integer step, got `{}`",
                                n.serialize()
                            ),
                        ));
                    }
                },
                Some((other, s, p)) => return Err(args.mismatch(p, "a number", &other, s)),
                None => (1, span),
            };
            if step.0 == 0 {
                return Err(at(step.1, Code::E3003, "`range` needs a non-zero step"));
            }
            // At most 2e12 / 1, so the count fits comfortably in an i64.
            let distance = end - start;
            let count = if (distance > 0) == (step.0 > 0) && distance != 0 {
                (distance.abs() + step.0.abs() - 1) / step.0.abs()
            } else {
                0
            };
            let count = usize::try_from(count).unwrap_or(usize::MAX);
            if count > bounds.elements {
                return Err(element_limit(bounds, count, span));
            }
            let items = (0..count)
                .map(|i| {
                    let v = start + step.0 * i as i64;
                    Value::Number(Number::new(v as f64, Unit::None))
                })
                .collect();
            Ok(Value::List(Rc::new(items)))
        }
        B::CssSpace | B::CssComma => {
            let (items, _) = args.list()?;
            let separator = if builtin == B::CssSpace { " " } else { ", " };
            let mut out = String::new();
            for (i, item) in items.iter().enumerate() {
                let Some(text) = item.css_text() else {
                    return Err(collection_error(builtin, item, span));
                };
                if i > 0 {
                    out.push_str(separator);
                }
                out.push_str(&text);
                if out.len() > bounds.string_bytes {
                    return Err(string_limit(bounds, span));
                }
            }
            fragment(out, bounds, span)
        }
        B::CssSlash => {
            let (left, _) = args.any()?;
            let (right, _) = args.any()?;
            let mut out = String::new();
            for (i, value) in [&left, &right].into_iter().enumerate() {
                let Some(text) = value.css_text() else {
                    return Err(collection_error(builtin, value, span));
                };
                if i > 0 {
                    out.push_str(" / ");
                }
                out.push_str(&text);
            }
            fragment(out, bounds, span)
        }
        B::CssIdent => {
            let (text, _) = args.string()?;
            if text.is_empty() {
                return Err(at(
                    span,
                    Code::E2006,
                    "invalid CSS fragment: `css.ident` needs a non-empty string",
                ));
            }
            let mut out = String::new();
            escape_ident(&text, "", &mut out);
            fragment(out, bounds, span)
        }
        B::CssFragment => {
            let (text, _) = args.string()?;
            if text.len() > bounds.string_bytes {
                return Err(string_limit(bounds, span));
            }
            if let Some(problem) = fragment_problem(&text) {
                return Err(at(
                    span,
                    Code::E2006,
                    format!("invalid CSS fragment: {problem}"),
                )
                .with_note(
                    "a fragment must be balanced CSS component values without `{`, `}` or `;`",
                ));
            }
            let trimmed = text.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c'));
            Ok(Value::Css(Rc::from(crate::css_text::normalize_newlines(
                trimmed,
            ))))
        }
        B::MathRound | B::MathFloor | B::MathCeil => {
            let (x, _) = args.number()?;
            Ok(Value::Number(round(&x, builtin)))
        }
        B::ColorRgb => {
            let next = |args: &mut Args| match args.named_number()? {
                Some(found) => Ok(found),
                None => Err(at(span, Code::E4001, "missing argument")),
            };
            let (r, rs, rn) = next(&mut args)?;
            let (g, gs, gn) = next(&mut args)?;
            let (b, bs, bn) = next(&mut args)?;
            let alpha = args.named_number()?;
            let r = channel(&r, 255, builtin, rn, rs)?;
            let g = channel(&g, 255, builtin, gn, gs)?;
            let b = channel(&b, 255, builtin, bn, bs)?;
            let a = match alpha {
                Some((n, s, name)) => channel(&n, 1, builtin, name, s)?,
                None => 255,
            };
            Ok(Value::Color(Color { r, g, b, a }))
        }
        B::ColorMix => {
            let (a, _) = args.color()?;
            let (b, _) = args.color()?;
            let w = match args.named_number()? {
                Some((n, s, _)) => weight(&n, builtin, s)?,
                None => (1, 2),
            };
            Ok(Value::Color(Color {
                r: mix_channel(a.r, b.r, w),
                g: mix_channel(a.g, b.g, w),
                b: mix_channel(a.b, b.b, w),
                a: mix_channel(a.a, b.a, w),
            }))
        }
        B::ColorRed | B::ColorGreen | B::ColorBlue => {
            let (c, _) = args.color()?;
            let value = match builtin {
                B::ColorRed => c.r,
                B::ColorGreen => c.g,
                _ => c.b,
            };
            Ok(Value::Number(Number::new(f64::from(value), Unit::None)))
        }
        B::ColorAlpha => {
            let (c, _) = args.color()?;
            number_value(f64::from(c.a) / 255.0, Unit::None, span)
        }
        B::MathAbs => {
            let (x, _) = args.number()?;
            Ok(Value::Number(Number::new(x.value.abs(), x.unit)))
        }
        B::MathMin | B::MathMax => {
            let (a, _) = args.number()?;
            let (b, _) = args.number()?;
            let ord = compare(&a, &b, span)?;
            let pick_b = if builtin == B::MathMin {
                ord == std::cmp::Ordering::Greater
            } else {
                ord == std::cmp::Ordering::Less
            };
            if pick_b {
                in_unit(&b, &a.unit, span)
            } else {
                Ok(Value::Number(a))
            }
        }
        B::MathClamp => {
            let (low, _) = args.number()?;
            let (x, _) = args.number()?;
            let (high, _) = args.number()?;
            if compare(&low, &high, span)? == std::cmp::Ordering::Greater {
                return Err(at(
                    span,
                    Code::E2003,
                    format!(
                        "`math.clamp` needs `low` ≤ `high`, got {} and {}",
                        low.serialize(),
                        high.serialize()
                    ),
                ));
            }
            if compare(&x, &low, span)? == std::cmp::Ordering::Less {
                in_unit(&low, &x.unit, span)
            } else if compare(&x, &high, span)? == std::cmp::Ordering::Greater {
                in_unit(&high, &x.unit, span)
            } else {
                Ok(Value::Number(x))
            }
        }
        B::MathUnit => {
            let (x, _) = args.number()?;
            Ok(Value::Str(x.unit.name().into()))
        }
        B::MathValue => {
            let (x, _) = args.number()?;
            Ok(Value::Number(Number::new(x.value, Unit::None)))
        }
        B::MathConvert => {
            let (x, _) = args.number()?;
            let (unit_text, unit_span) = args.string()?;
            let unit = Unit::parse(&unit_text);
            let converted = if unit.is_known() {
                x.convert_to(&unit)
            } else {
                None
            };
            match converted {
                Some(n) => number_value(n.value, n.unit, span),
                None => Err(at(
                    unit_span,
                    Code::E3005,
                    format!(
                        "cannot convert {} to \"{unit_text}\": the unit is unknown or not convertible",
                        x.unit.describe()
                    ),
                )
                .with_note("conversions exist within absolute lengths, angles, times, frequencies and resolutions")),
            }
        }
        B::ListLen => {
            let (l, _) = args.list()?;
            Ok(index(l.len()))
        }
        B::ListAppend => {
            let (l, _) = args.list()?;
            let (value, _) = args.any()?;
            if l.len() >= bounds.elements {
                return Err(element_limit(bounds, l.len().saturating_add(1), span));
            }
            let mut items = Vec::with_capacity(l.len() + 1);
            items.extend(l.iter().cloned());
            items.push(value);
            list_value(items, bounds, span)
        }
        B::ListConcat => {
            let (a, _) = args.list()?;
            let (b, _) = args.list()?;
            let count = a.len().saturating_add(b.len());
            if count > bounds.elements {
                return Err(element_limit(bounds, count, span));
            }
            let items = a.iter().chain(b.iter()).cloned().collect();
            list_value(items, bounds, span)
        }
        B::ListSlice => {
            let (l, _) = args.list()?;
            let (start, _) = args.integer()?;
            let (end, _) = args.integer()?;
            let len = l.len() as i64;
            if start < 0 || start > end || end > len {
                return Err(at(
                    span,
                    Code::E2005,
                    format!("`list.slice` range {start}..{end} is out of range for a list of length {len}"),
                )
                .with_note("indices are zero-based; the range is half-open: 0 ≤ start ≤ end ≤ length"));
            }
            let items = l
                .get(start as usize..end as usize)
                .map(<[Value]>::to_vec)
                .unwrap_or_default();
            list_value(items, bounds, span)
        }
        B::ListContains => {
            let (l, _) = args.list()?;
            let (value, _) = args.any()?;
            for item in l.iter() {
                match equal_within(item, &value, budget) {
                    Some(true) => return Ok(Value::Bool(true)),
                    Some(false) => {}
                    None => return Err(budget_error(span)),
                }
            }
            Ok(Value::Bool(false))
        }
        B::MapLen => {
            let (m, _) = args.map()?;
            Ok(index(m.len()))
        }
        B::MapHas => {
            let (m, _) = args.map()?;
            let (key, _) = args.string()?;
            Ok(Value::Bool(m.get(&key).is_some()))
        }
        B::MapGet => {
            let (m, _) = args.map()?;
            let (key, _) = args.string()?;
            let (default, _) = args.any()?;
            Ok(m.get(&key).cloned().unwrap_or(default))
        }
        B::MapSet => {
            let (m, _) = args.map()?;
            let (key, _) = args.string()?;
            let (value, _) = args.any()?;
            if m.get(&key).is_none() && m.len() >= bounds.elements {
                return Err(element_limit(bounds, m.len().saturating_add(1), span));
            }
            let entries = m
                .entries()
                .map(|(k, v)| (Rc::clone(k), v.clone()))
                .chain(std::iter::once((key, value)));
            Ok(Value::Map(Rc::new(Map::from_entries(entries))))
        }
        B::MapRemove => {
            let (m, _) = args.map()?;
            let (key, _) = args.string()?;
            if m.get(&key).is_none() {
                return Ok(Value::Map(m));
            }
            let entries = m
                .entries()
                .filter(|(k, _)| ***k != *key)
                .map(|(k, v)| (Rc::clone(k), v.clone()));
            Ok(Value::Map(Rc::new(Map::from_entries(entries))))
        }
        B::MapMerge => {
            let (a, _) = args.map()?;
            let (b, _) = args.map()?;
            let added = b.entries().filter(|(k, _)| a.get(k).is_none()).count();
            let count = a.len().saturating_add(added);
            if count > bounds.elements {
                return Err(element_limit(bounds, count, span));
            }
            // `a`'s keys in order with `b`'s values where present, then
            // `b`'s new keys in order.
            let entries = a
                .entries()
                .chain(b.entries())
                .map(|(k, v)| (Rc::clone(k), v.clone()));
            Ok(Value::Map(Rc::new(Map::from_entries(entries))))
        }
        B::MapKeys => {
            let (m, _) = args.map()?;
            let keys = m.entries().map(|(k, _)| Value::Str(Rc::clone(k))).collect();
            list_value(keys, bounds, span)
        }
        B::MapValues => {
            let (m, _) = args.map()?;
            let values = m.entries().map(|(_, v)| v.clone()).collect();
            list_value(values, bounds, span)
        }
        B::StringLen => {
            let (s, _) = args.string()?;
            Ok(index(s.chars().count()))
        }
        B::StringSlice => {
            let (s, _) = args.string()?;
            let (start, _) = args.integer()?;
            let (end, _) = args.integer()?;
            match char_range(&s, start, end) {
                Some((from, to)) => Ok(Value::Str(s.get(from..to).unwrap_or("").into())),
                None => Err(at(
                    span,
                    Code::E2005,
                    format!(
                        "`string.slice` range {start}..{end} is out of range for a string of length {}",
                        s.chars().count()
                    ),
                )
                .with_note("indices count Unicode scalar values from zero; the range is half-open")),
            }
        }
        B::StringContains => {
            let (s, _) = args.string()?;
            let (needle, _) = args.string()?;
            Ok(Value::Bool(s.contains(&*needle)))
        }
        B::StringUpper => {
            let (s, _) = args.string()?;
            Ok(Value::Str(s.to_ascii_uppercase().into()))
        }
        B::StringLower => {
            let (s, _) = args.string()?;
            Ok(Value::Str(s.to_ascii_lowercase().into()))
        }
        B::StringJoin => {
            let (items, items_span) = args.list()?;
            let (separator, _) = args.string()?;
            let mut out = String::new();
            for (i, item) in items.iter().enumerate() {
                let Value::Str(text) = item else {
                    return Err(at(
                        items_span,
                        Code::E4005,
                        format!(
                            "`{name}` expects a list of strings for `items`, but item {i} is {}",
                            describe(item)
                        ),
                    ));
                };
                if i > 0 {
                    out.push_str(&separator);
                }
                out.push_str(text);
                if out.len() > bounds.string_bytes {
                    return Err(string_limit(bounds, span));
                }
            }
            Ok(Value::Str(out.into()))
        }
        B::MetaType => {
            let (x, _) = args.any()?;
            Ok(Value::Str(x.type_name().into()))
        }
    }
}

/// E8004 for a comparison that ran out of evaluation steps.
pub(crate) fn budget_error(span: Span) -> Diagnostic {
    at(
        span,
        Code::E8004,
        "evaluation exceeds the step limit while comparing values",
    )
    .with_note("raise it with the `evaluation-steps` limit")
}

/// Why `text` is not a valid CSS fragment (TYPES.md §7), if it is not:
/// unbalanced `()`/`[]`, any `{` or `}`, a `;` outside strings, a bad or
/// unterminated string or URL, an unterminated comment, or a lone `\`.
pub(crate) fn fragment_problem(text: &str) -> Option<&'static str> {
    let mut lexer = Lexer::new(text, 0, 1);
    let mut stack: Vec<K> = Vec::new();
    loop {
        let token = lexer.next(Mode::Css);
        let problem = match token.kind {
            K::Eof => {
                return (!stack.is_empty()).then_some("an opening bracket is never closed");
            }
            K::OpenBrace | K::CloseBrace => Some("`{` and `}` are not allowed"),
            K::Semicolon => Some("`;` is not allowed outside strings"),
            K::BadString | K::String { terminated: false } => Some("a string is not terminated"),
            K::BadUrl | K::Url { terminated: false } => Some("a `url(…)` is malformed"),
            K::Comment { terminated: false } => Some("a comment is not terminated"),
            K::Delim('\\') => Some("a lone `\\` is not a valid escape"),
            K::OpenParen | K::Function => {
                stack.push(K::CloseParen);
                None
            }
            K::OpenSquare => {
                stack.push(K::CloseSquare);
                None
            }
            K::CloseParen | K::CloseSquare => {
                if stack.pop() == Some(token.kind) {
                    None
                } else {
                    Some("a closing bracket does not match")
                }
            }
            _ => None,
        };
        if problem.is_some() {
            return problem;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_validation() {
        for ok in [
            "",
            "1px solid",
            "calc(100% - 8px)",
            "\"a;b{}\"",
            "url(a;b)",
            "[a] (b)",
            "/* ; */ x",
            "\\#",
        ] {
            assert_eq!(fragment_problem(ok), None, "{ok:?}");
        }
        for bad in [
            "a;b", "a{", "}", "(", "a)", "[)", "\"abc", "\"a\nb\"", "url(a b)", "/* x", "a\\",
            "#{x}",
        ] {
            assert!(fragment_problem(bad).is_some(), "{bad:?}");
        }
    }

    #[test]
    fn members() {
        assert_eq!(member("css", "space"), Member::Implemented(B::CssSpace));
        assert_eq!(member("math", "round"), Member::Implemented(B::MathRound));
        assert_eq!(member("color", "mix"), Member::Implemented(B::ColorMix));
        assert_eq!(member("css", "nope"), Member::Unknown);
        assert_eq!(member("nope", "space"), Member::Unknown);
        assert_eq!(member("", "range"), Member::Unknown);
        assert_eq!(function("range"), Some(B::Range));
        assert_eq!(function("space"), None);
        assert_eq!(prelude("css"), Some(Prelude::Namespace));
        assert_eq!(prelude("range"), Some(Prelude::Function));
        assert_eq!(prelude("gap"), None);
    }

    #[test]
    fn table_is_consistent() {
        for (i, (ns, name, builtin, params)) in TABLE.iter().enumerate() {
            assert!(ns.is_empty() || NAMESPACES.contains(ns), "{ns}");
            assert_eq!(entry(*builtin).1, *name);
            assert!(
                TABLE
                    .iter()
                    .skip(i + 1)
                    .all(|(n2, m2, b2, _)| (n2, m2) != (ns, name) && b2 != builtin),
                "{ns}.{name} listed twice"
            );
            let mut seen_optional = false;
            for p in *params {
                let optional = p.ends_with('?');
                assert!(
                    !seen_optional || optional,
                    "required after optional in {name}"
                );
                seen_optional |= optional;
            }
        }
    }

    #[test]
    fn rounding_uses_quantized_values() {
        let r = |v: f64, b| round(&Number::new(v, Unit::None), b).value;
        assert_eq!(r(2.5, B::MathRound), 3.0);
        assert_eq!(r(-2.5, B::MathRound), -2.0);
        assert_eq!(r(2.499_999_9, B::MathRound), 3.0);
        assert_eq!(r(2.4, B::MathRound), 2.0);
        assert_eq!(r(-0.5, B::MathFloor), -1.0);
        assert_eq!(r(-0.5, B::MathCeil), 0.0);
        assert_eq!(r(3.0, B::MathCeil), 3.0);
        assert_eq!(r(1e12, B::MathRound), 1e12);
        assert_eq!(r(-1e12, B::MathFloor), -1e12);
    }

    #[test]
    fn character_ranges() {
        assert_eq!(char_range("héllo", 1, 3), Some((1, 4)));
        assert_eq!(char_range("héllo", 0, 0), Some((0, 0)));
        assert_eq!(char_range("héllo", 5, 5), Some((6, 6)));
        assert_eq!(char_range("héllo", 0, 5), Some((0, 6)));
        assert_eq!(char_range("héllo", 0, 6), None);
        assert_eq!(char_range("héllo", 3, 2), None);
        assert_eq!(char_range("héllo", -1, 2), None);
    }
}
