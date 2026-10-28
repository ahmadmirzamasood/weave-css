//! The prelude (LANGUAGE.md §4.3, TYPES.md §9).
//!
//! This build implements the explicit CSS sequence constructors of the
//! `css` namespace: `css.space`, `css.comma`, `css.slash`, `css.ident` and
//! `css.fragment`. The other specified built-ins are recognized and
//! reported as E5001 (not implemented by this build).

use std::rc::Rc;

use crate::diagnostic::{Code, Diagnostic};
use crate::expr::Call;
use crate::lexer::{Lexer, Mode, TokenKind as K};
use crate::source::Span;
use crate::value::{Value, escape_ident};

/// Built-in namespaces and their specified members (TYPES.md §9).
const NAMESPACES: [(&str, &[&str]); 7] = [
    ("css", &["space", "comma", "slash", "ident", "fragment"]),
    (
        "math",
        &[
            "round", "floor", "ceil", "abs", "min", "max", "clamp", "unit", "value", "convert",
        ],
    ),
    ("list", &["len", "append", "concat", "slice", "contains"]),
    (
        "map",
        &[
            "len", "has", "get", "set", "remove", "merge", "keys", "values",
        ],
    ),
    (
        "string",
        &["len", "slice", "contains", "upper", "lower", "join"],
    ),
    ("color", &["rgb", "mix", "red", "green", "blue", "alpha"]),
    ("meta", &["type"]),
];

/// What a prelude name denotes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prelude {
    Namespace,
    /// `range`.
    Function,
}

pub(crate) fn prelude(name: &str) -> Option<Prelude> {
    if name == "range" {
        Some(Prelude::Function)
    } else if NAMESPACES.iter().any(|(ns, _)| *ns == name) {
        Some(Prelude::Namespace)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Space,
    Comma,
    Slash,
    Ident,
    Fragment,
}

impl Builtin {
    fn params(self) -> &'static [&'static str] {
        match self {
            Builtin::Space | Builtin::Comma => &["items"],
            Builtin::Slash => &["left", "right"],
            Builtin::Ident | Builtin::Fragment => &["text"],
        }
    }

    fn name(self) -> &'static str {
        match self {
            Builtin::Space => "css.space",
            Builtin::Comma => "css.comma",
            Builtin::Slash => "css.slash",
            Builtin::Ident => "css.ident",
            Builtin::Fragment => "css.fragment",
        }
    }
}

/// A namespace member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Member {
    Implemented(Builtin),
    /// Specified but not implemented by this build (E5001).
    Specified,
    Unknown,
}

pub(crate) fn member(namespace: &str, name: &str) -> Member {
    let builtin = match (namespace, name) {
        ("css", "space") => Some(Builtin::Space),
        ("css", "comma") => Some(Builtin::Comma),
        ("css", "slash") => Some(Builtin::Slash),
        ("css", "ident") => Some(Builtin::Ident),
        ("css", "fragment") => Some(Builtin::Fragment),
        _ => None,
    };
    if let Some(builtin) = builtin {
        return Member::Implemented(builtin);
    }
    let specified = NAMESPACES
        .iter()
        .any(|(ns, members)| *ns == namespace && members.contains(&name));
    if specified {
        Member::Specified
    } else {
        Member::Unknown
    }
}

/// Size bounds for built-in results.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds {
    pub string_bytes: usize,
}

fn at(span: Span, code: Code, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, message).with_primary(span)
}

/// Binds arguments to parameters (LANGUAGE.md §5.4). Every parameter of
/// the implemented built-ins is required.
fn bind_args(
    builtin: Builtin,
    call: &Call,
    args: Vec<Value>,
) -> Result<Vec<(Value, Span)>, Diagnostic> {
    let params = builtin.params();
    let mut slots: Vec<Option<(Value, Span)>> = vec![None; params.len()];
    let mut position = 0;
    for (arg, value) in call.args.iter().zip(args) {
        let index = match &arg.name {
            None => {
                if position >= params.len() {
                    return Err(at(
                        arg.span,
                        Code::E4004,
                        format!(
                            "too many arguments: `{}` takes {} argument{}",
                            builtin.name(),
                            params.len(),
                            if params.len() == 1 { "" } else { "s" }
                        ),
                    ));
                }
                position += 1;
                position - 1
            }
            Some(name) => match params.iter().position(|p| *p == &**name) {
                Some(index) => index,
                None => {
                    return Err(at(
                        arg.span,
                        Code::E4002,
                        format!("`{}` has no parameter named `{name}`", builtin.name()),
                    )
                    .with_note(format!("parameters: {}", params.join(", "))));
                }
            },
        };
        match slots.get_mut(index) {
            Some(slot @ None) => *slot = Some((value, arg.span)),
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
    let mut bound = Vec::with_capacity(params.len());
    for (slot, param) in slots.into_iter().zip(params) {
        match slot {
            Some(value) => bound.push(value),
            None => {
                return Err(at(
                    call.callee.span,
                    Code::E4001,
                    format!("missing argument `{param}` for `{}`", builtin.name()),
                ));
            }
        }
    }
    Ok(bound)
}

fn type_error(
    builtin: Builtin,
    param: &str,
    expected: &str,
    got: &Value,
    span: Span,
) -> Diagnostic {
    at(
        span,
        Code::E4005,
        format!(
            "`{}` expects a {expected} for `{param}`, got a {}",
            builtin.name(),
            got.type_name()
        ),
    )
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

fn bounded(text: String, bounds: Bounds, span: Span) -> Result<Value, Diagnostic> {
    if text.len() > bounds.string_bytes {
        return Err(at(
            span,
            Code::E8007,
            format!(
                "the result exceeds the string limit of {} bytes",
                bounds.string_bytes
            ),
        )
        .with_note("raise it with the `string-bytes` limit"));
    }
    Ok(Value::Css(text.into()))
}

/// Calls an implemented built-in. Failures are reported at the callee
/// unless they concern one argument.
pub(crate) fn call(
    builtin: Builtin,
    call: &Call,
    args: Vec<Value>,
    bounds: Bounds,
) -> Result<Value, Diagnostic> {
    let span = call.callee.span;
    let mut args = bind_args(builtin, call, args)?.into_iter();
    let mut next = || {
        args.next()
            .ok_or_else(|| at(span, Code::E4001, "missing argument"))
    };
    match builtin {
        Builtin::Space | Builtin::Comma => {
            let (items, arg_span) = next()?;
            let Value::List(items) = &items else {
                return Err(type_error(builtin, "items", "list", &items, arg_span));
            };
            let separator = if builtin == Builtin::Space { " " } else { ", " };
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
                    return bounded(out, bounds, span);
                }
            }
            bounded(out, bounds, span)
        }
        Builtin::Slash => {
            let (left, _) = next()?;
            let (right, _) = next()?;
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
            bounded(out, bounds, span)
        }
        Builtin::Ident => {
            let (text, arg_span) = next()?;
            let Value::Str(text) = &text else {
                return Err(type_error(builtin, "text", "string", &text, arg_span));
            };
            if text.is_empty() {
                return Err(at(
                    span,
                    Code::E2006,
                    "invalid CSS fragment: `css.ident` needs a non-empty string",
                ));
            }
            let mut out = String::new();
            escape_ident(text, "", &mut out);
            bounded(out, bounds, span)
        }
        Builtin::Fragment => {
            let (text, arg_span) = next()?;
            let Value::Str(text) = &text else {
                return Err(type_error(builtin, "text", "string", &text, arg_span));
            };
            if text.len() > bounds.string_bytes {
                return bounded(text.to_string(), bounds, span);
            }
            if let Some(problem) = fragment_problem(text) {
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
    }
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
        assert_eq!(member("css", "space"), Member::Implemented(Builtin::Space));
        assert_eq!(member("math", "round"), Member::Specified);
        assert_eq!(member("css", "nope"), Member::Unknown);
        assert_eq!(member("nope", "space"), Member::Unknown);
        assert_eq!(prelude("css"), Some(Prelude::Namespace));
        assert_eq!(prelude("range"), Some(Prelude::Function));
        assert_eq!(prelude("gap"), None);
    }
}
