//! Structural selectors and nesting lowering (LANGUAGE.md §11,
//! CSS-COMPATIBILITY.md §4).
//!
//! Selector text is first brought to its canonical form (the normalization
//! of LANGUAGE.md §15), then scanned token by token with an explicit stack
//! of bracket levels: the top-level selector list, pseudo-class (and
//! pseudo-element) argument lists, which are selector levels too, and
//! opaque groups such as attribute selectors. The scan records, for every
//! complex selector of the list, its compound structure and every `&`
//! occurrence with its position.
//!
//! Lowering replaces each `&` either by the single parent selector, when
//! that is provably equivalent (splice-safe, §4.1 step 5), or by
//! `:is(<parent list>)`, which is exactly how CSS Nesting defines `&`. It
//! never expands parent lists into their Cartesian product, so specificity
//! is preserved and the output grows linearly per level; the growth across
//! levels is bounded by the caller with the selector-expansion limit
//! (E8005) and the output limit (E8006).

use crate::css_text::{Seg, normalize_selector};
use crate::lexer::{Lexer, Mode, Token, TokenKind as K};

/// Why a selector cannot be lowered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Problem {
    /// The nested selector is not a valid selector (E6007).
    Invalid(&'static str),
    /// The parent list contains a pseudo-element (E6004).
    PseudoElement,
    /// The lowered selector list would be longer than the output limit.
    TooLong,
}

/// A lowered selector list that nested rules refer to as `&`.
#[derive(Debug)]
pub(crate) struct SelectorList {
    /// Canonical text: complex selectors joined with `, `.
    pub text: String,
    items: Vec<Item>,
    /// How many complex selectors the text contains, including those in
    /// copies of ancestor lists inserted for `&` (the selector-expansion
    /// measure).
    pub weight: u64,
}

#[derive(Debug)]
struct Item {
    start: usize,
    end: usize,
    /// A single compound selector (no combinator at the top level).
    compound: bool,
    /// A type or universal selector at the top level.
    typed: bool,
    pseudo_element: bool,
}

impl SelectorList {
    /// A selector list written at stylesheet level (no `&`). Each complex
    /// selector counts once.
    pub fn top_level(text: &str) -> SelectorList {
        let text = canonical(text);
        let scan = scan(&text);
        let weight = scan.len() as u64;
        Self::from_scan(text, &scan, weight)
    }

    fn from_scan(text: String, scan: &[Complex], weight: u64) -> SelectorList {
        let items = scan
            .iter()
            .map(|c| Item {
                start: c.start,
                end: c.end,
                compound: c.compound,
                typed: c.typed,
                pseudo_element: c.pseudo_element,
            })
            .collect();
        SelectorList {
            text,
            items,
            weight,
        }
    }

    fn item_text(&self, item: &Item) -> &str {
        self.text.get(item.start..item.end).unwrap_or("")
    }

    /// The text that replaces one `&` occurrence (§4.1 steps 4–5).
    fn replacement(&self, amp: &Amp) -> Replacement<'_> {
        if let [only] = self.items.as_slice() {
            let leads = !amp.nested && amp.leads;
            let spliced = only.compound && (amp.first_in_compound || !only.typed);
            if leads || spliced {
                return Replacement::Direct(self.item_text(only));
            }
        }
        Replacement::Is(&self.text)
    }

    /// Lowers a nested rule's selector list against this parent list.
    /// `max_bytes` bounds the length of the result.
    pub fn lower(&self, child: &str, max_bytes: usize) -> Result<SelectorList, Problem> {
        if self.items.iter().any(|i| i.pseudo_element) {
            return Err(Problem::PseudoElement);
        }
        let child = canonical(child);
        let complexes = scan(&child);
        let mut out = String::new();
        let mut weight: u64 = 0;
        for (index, complex) in complexes.iter().enumerate() {
            if let Some(problem) = complex.problem() {
                return Err(Problem::Invalid(problem));
            }
            if index > 0 {
                out.push_str(", ");
            }
            let mut cost: u64 = 1;
            // A selector without `&`, or one starting with a combinator, is
            // relative to an implied leading `&` (§4.1 step 1).
            if complex.amps.is_empty() || complex.leading_combinator {
                let leading = Amp {
                    at: complex.start,
                    nested: false,
                    leads: true,
                    first_in_compound: true,
                };
                let added = self.push_replacement(&leading, &mut out, max_bytes)?;
                out.push(' ');
                cost = cost.saturating_add(added);
            }
            let mut copied = complex.start;
            for amp in &complex.amps {
                out.push_str(child.get(copied..amp.at).unwrap_or(""));
                let added = self.push_replacement(amp, &mut out, max_bytes)?;
                copied = amp.at + 1;
                cost = cost.saturating_add(added);
            }
            out.push_str(child.get(copied..complex.end).unwrap_or(""));
            if out.len() > max_bytes {
                return Err(Problem::TooLong);
            }
            weight = weight.saturating_add(cost);
        }
        let scanned = scan(&out);
        Ok(Self::from_scan(out, &scanned, weight))
    }

    /// Writes the replacement of one `&` and returns how many complex
    /// selectors it adds: the whole parent list for `:is()`, and the
    /// parent's inner selectors for a direct substitution (the parent
    /// selector itself merges into the enclosing complex selector).
    fn push_replacement(
        &self,
        amp: &Amp,
        out: &mut String,
        max_bytes: usize,
    ) -> Result<u64, Problem> {
        let text = self.replacement(amp);
        let len = match text {
            Replacement::Direct(t) => t.len(),
            Replacement::Is(t) => t.len() + 5,
        };
        if out.len().saturating_add(len) > max_bytes {
            return Err(Problem::TooLong);
        }
        match text {
            Replacement::Direct(t) => {
                out.push_str(t);
                Ok(self.weight.saturating_sub(1))
            }
            Replacement::Is(t) => {
                out.push_str(":is(");
                out.push_str(t);
                out.push(')');
                Ok(self.weight)
            }
        }
    }
}

enum Replacement<'a> {
    Direct(&'a str),
    Is(&'a str),
}

/// One `&` occurrence.
#[derive(Clone, Copy, Debug)]
struct Amp {
    /// Byte offset of the `&`.
    at: usize,
    /// Inside a pseudo-class (or other functional) argument.
    nested: bool,
    /// The first simple selector of the first compound of its complex
    /// selector.
    leads: bool,
    first_in_compound: bool,
}

/// The scan of one top-level complex selector.
#[derive(Debug)]
struct Complex {
    start: usize,
    end: usize,
    compound: bool,
    typed: bool,
    pseudo_element: bool,
    leading_combinator: bool,
    amps: Vec<Amp>,
    /// Problems that make it invalid as a nested selector.
    empty: bool,
    trailing_combinator: bool,
    double_combinator: bool,
    misplaced_type: bool,
}

impl Complex {
    fn new(start: usize) -> Complex {
        Complex {
            start,
            end: start,
            compound: true,
            typed: false,
            pseudo_element: false,
            leading_combinator: false,
            amps: Vec::new(),
            empty: true,
            trailing_combinator: false,
            double_combinator: false,
            misplaced_type: false,
        }
    }

    fn problem(&self) -> Option<&'static str> {
        if self.empty {
            Some("a selector in the list is empty")
        } else if self.trailing_combinator {
            Some("a combinator must be followed by a selector")
        } else if self.double_combinator {
            Some("two combinators follow each other")
        } else if self.misplaced_type {
            Some(
                "a type or universal selector must be the first simple selector of its compound (`&` counts as one)",
            )
        } else {
            None
        }
    }
}

/// One bracket level of the scan.
#[derive(Debug)]
struct Level {
    closer: K,
    /// A selector level (pseudo-class arguments) rather than an opaque
    /// group (attribute selectors, other brackets).
    selector: bool,
    /// At the start of a compound selector.
    start: bool,
}

/// Brings selector text to its canonical form: the normalization the
/// parser applies to `rule` selectors, applied to the final text, so that
/// text inserted by interpolation is laid out like written text.
pub(crate) fn canonical(text: &str) -> String {
    let mut lexer = Lexer::new(text, 0, 1);
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next(Mode::Css);
        if token.kind == K::Eof {
            break;
        }
        tokens.push(token);
    }
    let mut out = String::new();
    for seg in normalize_selector(&tokens, text) {
        if let Seg::Text(t) = seg {
            out.push_str(&t);
        }
    }
    let trimmed = out.trim_end_matches(' ');
    if trimmed.len() < out.len() {
        out.truncate(trimmed.len());
    }
    out
}

fn closer(kind: K) -> K {
    match kind {
        K::OpenSquare => K::CloseSquare,
        K::OpenBrace => K::CloseBrace,
        _ => K::CloseParen,
    }
}

/// Legacy single-colon pseudo-elements (Selectors Level 4 §3.6.1).
fn legacy_pseudo_element(name: &str) -> bool {
    ["before", "after", "first-line", "first-letter"]
        .iter()
        .any(|n| n.eq_ignore_ascii_case(name))
}

/// Scans canonical selector-list text into its complex selectors.
fn scan(text: &str) -> Vec<Complex> {
    let mut lexer = Lexer::new(text, 0, 1);
    let mut complexes = Vec::new();
    let mut current = Complex::new(0);
    let mut levels: Vec<Level> = Vec::new();
    // Level-0 state.
    let mut start = true;
    let mut first_compound = true;
    let mut last_combinator = false;
    // Prefixes that make the next token part of the same simple selector.
    let mut colons = 0u8;
    let mut after_dot = false;
    let mut after_pipe = false;
    loop {
        let token: Token = lexer.next(Mode::Css);
        if token.kind == K::Eof {
            break;
        }
        let (at, end) = (token.start as usize, token.end as usize);
        let depth = levels.len();
        // Inside an opaque group only brackets matter.
        if let Some(level) = levels.last() {
            if !level.selector {
                match token.kind {
                    k if k == level.closer => {
                        levels.pop();
                    }
                    K::OpenSquare | K::OpenParen | K::Function | K::OpenBrace => {
                        levels.push(Level {
                            closer: closer(token.kind),
                            selector: false,
                            start: false,
                        });
                    }
                    _ => {}
                }
                continue;
            }
        }
        let (prefix_colons, prefix_dot, prefix_pipe) = (colons, after_dot, after_pipe);
        colons = 0;
        after_dot = false;
        after_pipe = false;
        if depth > 0 {
            // A selector level inside pseudo-class arguments.
            let Some(level) = levels.last_mut() else {
                break;
            };
            match token.kind {
                k if k == level.closer => {
                    levels.pop();
                }
                K::Whitespace | K::Comma | K::Delim('>' | '+' | '~') => level.start = true,
                K::Delim('&') => {
                    current.amps.push(Amp {
                        at,
                        nested: true,
                        leads: false,
                        first_in_compound: level.start,
                    });
                    level.start = false;
                }
                K::Ident | K::Delim('*') if !(prefix_dot || prefix_colons > 0 || prefix_pipe) => {
                    if !level.start {
                        current.misplaced_type = true;
                    }
                    level.start = false;
                }
                K::Colon => {
                    colons = prefix_colons.saturating_add(1);
                    level.start = false;
                }
                K::Delim('.') => {
                    after_dot = true;
                    level.start = false;
                }
                K::Delim('|') => {
                    after_pipe = true;
                    level.start = false;
                }
                K::Function | K::OpenSquare | K::OpenParen | K::OpenBrace => {
                    let selector = token.kind == K::Function && prefix_colons > 0;
                    level.start = false;
                    levels.push(Level {
                        closer: closer(token.kind),
                        selector,
                        start: true,
                    });
                }
                K::Comment { .. } => {}
                _ => level.start = false,
            }
            continue;
        }
        match token.kind {
            K::Comma => {
                current.end = at;
                if last_combinator {
                    current.trailing_combinator = true;
                }
                complexes.push(current);
                current = Complex::new(end);
                start = true;
                first_compound = true;
                last_combinator = false;
                continue;
            }
            K::Whitespace => {
                // Skip the space after `, `.
                if current.empty {
                    current.start = end;
                } else {
                    start = true;
                    first_compound = false;
                    current.compound = false;
                }
                continue;
            }
            K::Comment { .. } => continue,
            K::Delim('>' | '+' | '~') => {
                if current.empty {
                    current.leading_combinator = true;
                } else if last_combinator {
                    current.double_combinator = true;
                }
                current.empty = false;
                current.compound = false;
                start = true;
                first_compound = false;
                last_combinator = true;
                current.end = end;
                continue;
            }
            _ => {}
        }
        current.empty = false;
        current.end = end;
        last_combinator = false;
        match token.kind {
            K::Delim('&') => {
                current.amps.push(Amp {
                    at,
                    nested: false,
                    leads: first_compound && start,
                    first_in_compound: start,
                });
                start = false;
            }
            K::Ident | K::Delim('*') if !(prefix_dot || prefix_colons > 0 || prefix_pipe) => {
                if !start {
                    current.misplaced_type = true;
                }
                current.typed = true;
                start = false;
            }
            K::Ident if prefix_colons == 2 => current.pseudo_element = true,
            K::Ident if prefix_colons == 1 => {
                if legacy_pseudo_element(token.text(text)) {
                    current.pseudo_element = true;
                }
            }
            K::Colon => {
                colons = prefix_colons.saturating_add(1);
                start = false;
            }
            K::Delim('.') => {
                after_dot = true;
                start = false;
            }
            K::Delim('|') => {
                // `|a` (no namespace) is a type selector of its own.
                if start {
                    current.typed = true;
                }
                after_pipe = true;
                start = false;
            }
            K::Function | K::OpenSquare | K::OpenParen | K::OpenBrace => {
                let selector = token.kind == K::Function && prefix_colons > 0;
                if token.kind == K::Function && prefix_colons == 2 {
                    current.pseudo_element = true;
                }
                start = false;
                levels.push(Level {
                    closer: closer(token.kind),
                    selector,
                    start: true,
                });
            }
            _ => start = false,
        }
    }
    current.end = current.end.max(current.start);
    if last_combinator {
        current.trailing_combinator = true;
    }
    if !(current.empty && complexes.is_empty() && text.is_empty()) {
        complexes.push(current);
    }
    // Opening tokens extend `end` only to the opener; extend each complex
    // to the text before the next separator.
    let mut next_starts: Vec<usize> = complexes.iter().skip(1).map(|c| c.start).collect();
    next_starts.push(text.len() + 2);
    for (complex, next) in complexes.iter_mut().zip(next_starts) {
        let limit = next.saturating_sub(2).min(text.len());
        if !complex.empty {
            complex.end = limit.max(complex.start);
        }
    }
    complexes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(parent: &str, child: &str) -> Result<String, Problem> {
        SelectorList::top_level(parent)
            .lower(child, 1 << 20)
            .map(|l| l.text)
    }

    #[test]
    fn normative_examples() {
        // CSS-COMPATIBILITY.md §4.2.
        let cases = [
            (".card", "&:hover", ".card:hover"),
            (".card", ".title", ".card .title"),
            ("ul li", "> a", "ul li > a"),
            (".a .b", "&.on", ".a .b.on"),
            (".a .b", ".x &", ".x :is(.a .b)"),
            (".btn", ".group:hover &", ".group:hover .btn"),
            (".btn", ":not(&)", ":not(.btn)"),
            (".a", "& + &", ".a + .a"),
            ("span.x", "a&", "a:is(span.x)"),
            ("#main, .side", ".item", ":is(#main, .side) .item"),
            (".a, .b", "&:hover", ":is(.a, .b):hover"),
        ];
        for (parent, child, expected) in cases {
            assert_eq!(
                lower(parent, child).as_deref(),
                Ok(expected),
                "{parent} / {child}"
            );
        }
    }

    #[test]
    fn pseudo_class_arguments_and_lists() {
        assert_eq!(lower(".a .b", ":not(&)").as_deref(), Ok(":not(:is(.a .b))"));
        assert_eq!(lower("div", ":is(&.x, p)").as_deref(), Ok(":is(div.x, p)"));
        assert_eq!(lower("div", ":is(.x&)").as_deref(), Ok(":is(.x:is(div))"));
        assert_eq!(lower(".a", ":has(> &)").as_deref(), Ok(":has(> .a)"));
        assert_eq!(
            lower(".a", "&:nth-child(2n + 1 of .b)").as_deref(),
            Ok(".a:nth-child(2n + 1 of .b)")
        );
        assert_eq!(
            lower(".a, .b", "& > &, :where(&)").as_deref(),
            Ok(":is(.a, .b) > :is(.a, .b), :where(:is(.a, .b))")
        );
        assert_eq!(
            lower(".a", "[data-x=\"&\"]").as_deref(),
            Ok(".a [data-x=\"&\"]")
        );
        // A leading combinator is relative even when `&` appears later.
        assert_eq!(lower(".a", "> &.x").as_deref(), Ok(".a > .a.x"));
    }

    #[test]
    fn interpolated_layout_is_canonicalized() {
        assert_eq!(lower(".a", "  >   .b  ").as_deref(), Ok(".a > .b"));
        assert_eq!(
            lower(".a", "&:is( .x ,.y )").as_deref(),
            Ok(".a:is(.x, .y)")
        );
    }

    #[test]
    fn invalid_nested_selectors() {
        let invalid = [
            "&div",
            "&*",
            ".x, , .y",
            ".x >",
            "> > .x",
            "&-suffix",
            ":not(&div)",
        ];
        for child in invalid {
            assert!(
                matches!(lower(".a", child), Err(Problem::Invalid(_))),
                "{child}"
            );
        }
        assert_eq!(lower(".a::before", "&:hover"), Err(Problem::PseudoElement));
        assert_eq!(lower(".a:after", ".b"), Err(Problem::PseudoElement));
        assert_eq!(lower(".a::part(x)", ".b"), Err(Problem::PseudoElement));
        assert_eq!(lower(".a", "ns|b&").as_deref(), Ok("ns|b.a"));
    }

    #[test]
    fn namespaces_and_types() {
        assert_eq!(lower(".a", "svg|rect&").as_deref(), Ok("svg|rect.a"));
        assert_eq!(lower(".a", "& svg|rect").as_deref(), Ok(".a svg|rect"));
        assert_eq!(lower("*|a", "&.x").as_deref(), Ok("*|a.x"));
        assert_eq!(lower("*|a", ".x&").as_deref(), Ok(".x:is(*|a)"));
    }

    #[test]
    fn weight_counts_parent_copies() {
        // `:is(.a, .b) :is(.a, .b)`: one complex selector plus two copies
        // of a two-selector list.
        let top = SelectorList::top_level(".a, .b");
        assert_eq!(top.weight, 2);
        let one = top.lower("& &", 1 << 20).unwrap();
        assert_eq!(one.weight, 1 + 2 + 2);
        // A single parent: direct substitutions add only what the parent
        // already contains; `:is()` adds the whole list.
        let two = one.lower("&.y, .x", 1 << 20).unwrap();
        assert_eq!(two.weight, (1 + 4) + (1 + 4));
        let chain = SelectorList::top_level(".a")
            .lower("&:hover", 1 << 20)
            .unwrap();
        assert_eq!(chain.weight, 1);
        let is = SelectorList::top_level("span")
            .lower(".x&", 1 << 20)
            .unwrap();
        assert_eq!((is.text.as_str(), is.weight), (".x:is(span)", 2));
    }

    #[test]
    fn length_is_bounded() {
        let top = SelectorList::top_level(".abcdef");
        assert_eq!(top.lower("& & & &", 16).map(|_| ()), Err(Problem::TooLong));
        assert!(top.lower("&", 16).is_ok());
    }
}
