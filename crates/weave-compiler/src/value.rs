//! Values (TYPES.md §1, §3–§7) and their serialization into CSS text
//! (LANGUAGE.md §7).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use crate::number::Number;

/// An 8-bit sRGB color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Parses the 3, 4, 6 or 8 hex digits of a color literal.
    pub fn from_hex(digits: &str) -> Option<Color> {
        let nibbles: Vec<u8> = digits
            .chars()
            .map(|c| c.to_digit(16).and_then(|d| u8::try_from(d).ok()))
            .collect::<Option<_>>()?;
        let channel = |i: usize, wide: bool| -> Option<u8> {
            if wide {
                Some(nibbles.get(2 * i)? * 16 + nibbles.get(2 * i + 1)?)
            } else {
                let n = *nibbles.get(i)?;
                Some(n * 16 + n)
            }
        };
        let (wide, alpha) = match nibbles.len() {
            3 => (false, false),
            4 => (false, true),
            6 => (true, false),
            8 => (true, true),
            _ => return None,
        };
        Some(Color {
            r: channel(0, wide)?,
            g: channel(1, wide)?,
            b: channel(2, wide)?,
            a: if alpha { channel(3, wide)? } else { 255 },
        })
    }

    /// `#rrggbb`, or `#rrggbbaa` when not opaque; lowercase, never short.
    pub fn serialize(self) -> String {
        let mut out = format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b);
        if self.a != 255 {
            let _ = write!(out, "{:02x}", self.a);
        }
        out
    }
}

/// An insertion-ordered map with string keys.
#[derive(Debug, Default)]
pub(crate) struct Map {
    entries: Vec<(Rc<str>, Value)>,
    index: HashMap<Rc<str>, usize>,
}

impl Map {
    /// Builds a map from entries with distinct keys (callers check
    /// duplicates); a repeated key keeps its first position and last value.
    pub fn from_entries(entries: impl IntoIterator<Item = (Rc<str>, Value)>) -> Map {
        let mut map = Map::default();
        for (key, value) in entries {
            match map.index.get(&key) {
                Some(&at) => {
                    if let Some(slot) = map.entries.get_mut(at) {
                        slot.1 = value;
                    }
                }
                None => {
                    map.index.insert(Rc::clone(&key), map.entries.len());
                    map.entries.push((key, value));
                }
            }
        }
        map
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        let at = *self.index.get(key)?;
        self.entries.get(at).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> impl Iterator<Item = (&Rc<str>, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

/// A Weave value. Collections are reference counted, so copying a value is
/// cheap; all values are immutable.
#[derive(Clone, Debug)]
pub(crate) enum Value {
    Number(Number),
    Str(Rc<str>),
    Color(Color),
    Bool(bool),
    List(Rc<Vec<Value>>),
    Map(Rc<Map>),
    /// A validated CSS fragment (TYPES.md §7).
    Css(Rc<str>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "number",
            Value::Str(_) => "string",
            Value::Color(_) => "color",
            Value::Bool(_) => "bool",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Css(_) => "css",
        }
    }

    /// Value-context serialization (TYPES.md §5); `None` for collections.
    pub fn css_text(&self) -> Option<String> {
        Some(match self {
            Value::Str(text) => quote_string(text),
            Value::List(_) | Value::Map(_) => return None,
            _ => self.plain_text()?,
        })
    }

    /// The *text* of a value used by the string, url, selector and name
    /// contexts: string contents unquoted, otherwise the value-context
    /// serialization. `None` for collections.
    pub fn plain_text(&self) -> Option<String> {
        Some(match self {
            Value::Number(n) => n.serialize(),
            Value::Str(text) | Value::Css(text) => text.to_string(),
            Value::Color(c) => c.serialize(),
            Value::Bool(b) => b.to_string(),
            Value::List(_) | Value::Map(_) => return None,
        })
    }
}

/// `==` (TYPES.md §1, §6): never an error; values of different types are
/// unequal; maps ignore order. Iterative, so deeply nested lists cannot
/// exhaust the stack.
pub(crate) fn equal(a: &Value, b: &Value) -> bool {
    let mut pending = vec![(a, b)];
    while let Some(pair) = pending.pop() {
        let same = match pair {
            (Value::Number(x), Value::Number(y)) => x.equals(y),
            (Value::Str(x), Value::Str(y)) | (Value::Css(x), Value::Css(y)) => x == y,
            (Value::Color(x), Value::Color(y)) => x == y,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::List(x), Value::List(y)) => {
                if x.len() != y.len() {
                    return false;
                }
                pending.extend(x.iter().zip(y.iter()));
                true
            }
            (Value::Map(x), Value::Map(y)) => {
                if x.len() != y.len() {
                    return false;
                }
                for (key, value) in x.entries() {
                    match y.get(key) {
                        Some(other) => pending.push((value, other)),
                        None => return false,
                    }
                }
                true
            }
            _ => false,
        };
        if !same {
            return false;
        }
    }
    true
}

impl Drop for Value {
    /// Drops nested collections iteratively: a list nested thousands of
    /// levels deep would otherwise recurse once per level.
    fn drop(&mut self) {
        let mut pending: Vec<Value> = Vec::new();
        take_children(self, &mut pending);
        while let Some(mut value) = pending.pop() {
            take_children(&mut value, &mut pending);
        }
    }
}

/// Moves the elements of a uniquely owned collection into `out`.
fn take_children(value: &mut Value, out: &mut Vec<Value>) {
    match value {
        Value::List(list) => {
            if let Some(items) = Rc::get_mut(list) {
                out.append(items);
            }
        }
        Value::Map(map) => {
            if let Some(map) = Rc::get_mut(map) {
                map.index.clear();
                out.extend(map.entries.drain(..).map(|(_, v)| v));
            }
        }
        _ => {}
    }
}

/// Appends `\` + lowercase hex + one space.
fn hex_escape(c: char, out: &mut String) {
    let _ = write!(out, "\\{:x} ", u32::from(c));
}

fn is_control(c: char) -> bool {
    matches!(c, '\u{1}'..='\u{1f}' | '\u{7f}')
}

/// Escapes text as the contents of a CSS string delimited by `quote`
/// (TYPES.md §5): U+0000 → U+FFFD, controls hex-escaped, the quote and `\`
/// backslash-escaped, everything else literal.
pub(crate) fn escape_string_contents(text: &str, quote: char, out: &mut String) {
    for c in text.chars() {
        match c {
            '\0' => out.push('\u{fffd}'),
            c if is_control(c) => hex_escape(c, out),
            '\\' => out.push_str("\\\\"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
}

/// A double-quoted CSS string.
pub(crate) fn quote_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    escape_string_contents(text, '"', &mut out);
    out.push('"');
    out
}

/// Escapes text for an unquoted `url(…)` token (LANGUAGE.md §7.1).
pub(crate) fn escape_url(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '\0' => out.push('\u{fffd}'),
            '\u{1}'..='\u{20}' | '\u{7f}' => hex_escape(c, out),
            '"' | '\'' | '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
}

/// A CSS name code point.
pub(crate) fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || !c.is_ascii()
}

/// The trailing run of name code points of `out`: the part of an
/// identifier that inserted text would continue.
pub(crate) fn trailing_name(out: &str) -> &str {
    let start = out
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_name_char(c))
        .last()
        .map_or(out.len(), |(i, _)| i);
    out.get(start..).unwrap_or("")
}

/// Escapes text inserted in an identifier position (LANGUAGE.md §7.2),
/// after `prefix`, the identifier text already written (see
/// [`trailing_name`]). With an empty prefix the text starts an identifier: a
/// leading digit, or a digit after a leading `-`, is hex-escaped and a lone
/// `-` is written `\-`. After a prefix that is exactly `-`, a leading digit
/// is hex-escaped. Otherwise name code points are literal.
pub(crate) fn escape_ident(text: &str, prefix: &str, out: &mut String) {
    let starts = prefix.is_empty();
    if starts && text == "-" {
        out.push_str("\\-");
        return;
    }
    let leading_dash = starts && text.starts_with('-');
    for (i, c) in text.chars().enumerate() {
        let digit_needs_escape = c.is_ascii_digit()
            && ((i == 0 && (starts || prefix == "-")) || (i == 1 && leading_dash));
        match c {
            '\0' => out.push('\u{fffd}'),
            c if is_control(c) => hex_escape(c, out),
            c if digit_needs_escape => hex_escape(c, out),
            c if is_name_char(c) => out.push(c),
            c => {
                out.push('\\');
                out.push(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::number::Unit;

    fn ident(text: &str, prefix: &str) -> String {
        let mut out = String::new();
        escape_ident(text, prefix, &mut out);
        out
    }

    #[test]
    fn colors_parse_and_serialize() {
        let hex = |s: &str| Color::from_hex(s).map(Color::serialize);
        assert_eq!(hex("2563EB").as_deref(), Some("#2563eb"));
        assert_eq!(hex("fff").as_deref(), Some("#ffffff"));
        assert_eq!(hex("0003").as_deref(), Some("#00000033"));
        assert_eq!(hex("64748b80").as_deref(), Some("#64748b80"));
        assert_eq!(hex("abcdef"), Some("#abcdef".into()));
        assert_eq!(hex("abcde"), None);
        assert_eq!(hex("ggg"), None);
    }

    #[test]
    fn string_escaping() {
        assert_eq!(quote_string("Say \"hi\"\\"), r#""Say \"hi\"\\""#);
        assert_eq!(quote_string("a\tb"), r#""a\9 b""#);
        assert_eq!(quote_string("x\0\n\u{7f}é"), "\"x\u{fffd}\\a \\7f é\"");
        assert_eq!(quote_string("it's"), "\"it's\"");
        let mut out = String::new();
        escape_string_contents("it's \"q\"", '\'', &mut out);
        assert_eq!(out, r#"it\'s "q""#);
    }

    #[test]
    fn url_escaping() {
        let mut out = String::new();
        escape_url("img/a b(1).png\"'\\\u{1}\0", &mut out);
        assert_eq!(out, "img/a\\20 b\\(1\\).png\\\"\\'\\\\\\1 \u{fffd}");
    }

    #[test]
    fn identifier_escaping_by_position() {
        assert_eq!(ident("0.5", "w-"), r"0\.5");
        assert_eq!(ident("10", ""), r"\31 0");
        assert_eq!(ident("1up", ""), r"\31 up");
        assert_eq!(ident("-", ""), r"\-");
        assert_eq!(ident("-", "a"), "-");
        assert_eq!(ident("-1", ""), r"-\31 ");
        assert_eq!(ident("--x", ""), "--x");
        assert_eq!(ident("1", "-"), r"\31 ");
        assert_eq!(ident("1", "--"), "1");
        assert_eq!(
            ident("red; } body { display: none", "sel-"),
            r"red\;\ \}\ body\ \{\ display\:\ none"
        );
        assert_eq!(ident("x: y; z", "--prop-"), r"x\:\ y\;\ z");
        assert_eq!(ident("日本", "naïve-"), "日本");
        assert_eq!(ident("a\0b\u{1}", "x"), "a\u{fffd}b\\1 ");
        assert_eq!(ident("#abc", ""), r"\#abc");
    }

    #[test]
    fn trailing_names() {
        assert_eq!(trailing_name(".a-b"), "a-b");
        assert_eq!(trailing_name(".é"), "é");
        assert_eq!(trailing_name("a "), "");
        assert_eq!(trailing_name(""), "");
        assert_eq!(trailing_name("--prop-"), "--prop-");
    }

    #[test]
    fn equality_rules() {
        let num = |v: f64, u: &str| Value::Number(Number::new(v, Unit::parse(u)));
        let list = |items: Vec<Value>| Value::List(Rc::new(items));
        let map = |items: Vec<(&str, Value)>| {
            Value::Map(Rc::new(Map::from_entries(
                items.into_iter().map(|(k, v)| (Rc::from(k), v)),
            )))
        };
        assert!(equal(&num(1.0, "in"), &num(96.0, "px")));
        assert!(!equal(&num(0.0, ""), &num(0.0, "px")));
        assert!(!equal(&Value::Str("a".into()), &Value::Css("a".into())));
        assert!(equal(
            &list(vec![num(1.0, ""), Value::Bool(true)]),
            &list(vec![num(1.0, ""), Value::Bool(true)])
        ));
        assert!(!equal(&list(vec![num(1.0, "")]), &list(vec![])));
        assert!(equal(
            &map(vec![("a", num(1.0, "")), ("b", num(2.0, ""))]),
            &map(vec![("b", num(2.0, "")), ("a", num(1.0, ""))])
        ));
        assert!(!equal(
            &map(vec![("a", num(1.0, ""))]),
            &map(vec![("b", num(1.0, ""))])
        ));
    }

    #[test]
    fn deeply_nested_values_compare_and_drop_without_recursion() {
        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut a = Value::Bool(true);
                let mut b = Value::Bool(true);
                for _ in 0..100_000 {
                    a = Value::List(Rc::new(vec![a]));
                    b = Value::List(Rc::new(vec![b]));
                }
                equal(&a, &b)
            })
            .unwrap();
        assert!(handle.join().unwrap());
    }
}
