//! Semantic checks of nesting lowering (CSS-COMPATIBILITY.md §4).
//!
//! - Specificity: for a matrix of parent lists and nested selectors, one and
//!   two levels deep, every lowered complex selector must have the
//!   specificity that CSS Nesting gives it, computed independently: the
//!   native meaning replaces every `&` (explicit or implied) with
//!   `:is(<parent list>)`, and the specificity is computed with an
//!   independent Selectors Level 4 calculator over `cssparser` tokens.
//!   Lowering must also keep one output selector per nested selector, in
//!   order (no Cartesian expansion).
//! - Cascade order of interleaved declarations, contexts and limits.
//!
//! Matching is checked against a real browser by scripts/browser_check.py.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod independent;

use cssparser::{Parser, Token};
use weave_compiler::{Code, CompileOptions, Limit, MemoryResolver, ModuleId, compile};

fn build(source: &str, options: &CompileOptions) -> Result<String, Vec<Code>> {
    let mut resolver = MemoryResolver::new();
    let entry = ModuleId::new("main.weave").unwrap();
    resolver.insert(entry.clone(), source.as_bytes());
    match compile(&entry, &mut resolver, options).outcome {
        Ok(output) => Ok(output.css),
        Err(diags) => Err(diags.iter().map(|d| d.code()).collect()),
    }
}

fn css(source: &str) -> String {
    build(source, &CompileOptions::default()).unwrap_or_else(|codes| panic!("{source}\n{codes:?}"))
}

fn codes(source: &str) -> Vec<Code> {
    build(source, &CompileOptions::default())
        .err()
        .unwrap_or_default()
}

// ----- an independent specificity calculator -----------------------------------

type Spec = (u32, u32, u32);

fn add(a: Spec, b: Spec) -> Spec {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

/// Specificities of the complex selectors of a selector list.
fn list_specificity(text: &str) -> Vec<Spec> {
    let mut parser = Parser::new(text);
    list(&mut parser)
}

fn list(parser: &mut Parser<'_>) -> Vec<Spec> {
    let mut specs = vec![(0, 0, 0)];
    let mut after_colon = 0;
    while let Ok(token) = parser.next_including_whitespace() {
        let token = token.clone();
        let colons = after_colon;
        after_colon = 0;
        let last = specs.last_mut().unwrap();
        match token {
            Token::Comma => specs.push((0, 0, 0)),
            Token::IDHash(_) | Token::Hash(_) => *last = add(*last, (1, 0, 0)),
            Token::Delim('.') => {
                parser.next().unwrap();
                *last = add(*last, (0, 1, 0));
            }
            Token::SquareBracketBlock => *last = add(*last, (0, 1, 0)),
            Token::Colon => after_colon = colons + 1,
            Token::Ident(name) if colons == 2 => {
                let _ = name;
                *last = add(*last, (0, 0, 1));
            }
            Token::Ident(name) if colons == 1 => {
                let legacy = ["before", "after", "first-line", "first-letter"]
                    .contains(&&*name.to_ascii_lowercase());
                *last = add(*last, if legacy { (0, 0, 1) } else { (0, 1, 0) });
            }
            Token::Ident(_) => *last = add(*last, (0, 0, 1)),
            Token::Function(name) if colons > 0 => {
                let name = name.to_ascii_lowercase();
                let inner = parser
                    .parse_nested_block(|p| Ok::<_, cssparser::ParseError<()>>(arguments(p, &name)))
                    .unwrap();
                *last = add(*last, inner);
            }
            Token::Delim('&') => panic!("`&` in lowered output: {token:?}"),
            _ => {}
        }
    }
    specs
}

fn max(specs: &[Spec]) -> Spec {
    specs.iter().copied().max().unwrap_or((0, 0, 0))
}

/// The specificity a functional pseudo-class adds (Selectors Level 4 §17).
fn arguments(parser: &mut Parser<'_>, name: &str) -> Spec {
    match name {
        "is" | "matches" | "not" | "has" => max(&list(parser)),
        "where" => {
            let _ = list(parser);
            (0, 0, 0)
        }
        "nth-child" | "nth-last-child" => {
            // An+B, then optionally `of S`.
            while let Ok(token) = parser.next() {
                if matches!(token, Token::Ident(word) if word.eq_ignore_ascii_case("of")) {
                    return add((0, 1, 0), max(&list(parser)));
                }
            }
            (0, 1, 0)
        }
        _ => {
            while parser.next().is_ok() {}
            (0, 1, 0)
        }
    }
}

/// The native meaning of a nested selector list under `parent`: every `&`,
/// and the implied leading `&` of a relative selector, is `:is(parent)`.
fn native(parent: &str, child: &str) -> String {
    let is = format!(":is({parent})");
    split_list(child)
        .into_iter()
        .map(|c| {
            let relative = !c.contains('&') || c.starts_with(['>', '+', '~']);
            let replaced = c.replace('&', &is);
            if relative {
                format!("{is} {replaced}")
            } else {
                replaced
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Splits a selector list at its top-level commas.
fn split_list(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in text.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts
}

/// The selector of the only rule in `out`.
fn only_selector(out: &str) -> String {
    let (_, problems) = independent::parse(out);
    assert!(problems.is_empty(), "{out}\n{problems:?}");
    let mut rules = out.lines().filter(|l| l.ends_with(" {"));
    let selector = rules
        .next()
        .expect("a rule")
        .trim_end_matches(" {")
        .to_string();
    assert!(rules.next().is_none(), "{out}");
    selector
}

const PARENTS: [&str; 12] = [
    ".a",
    "#i",
    "div",
    "div.a",
    ".a .b",
    "#i > p",
    ".a, #i",
    "div, .a .b",
    "*",
    "a:hover",
    "ul li:first-child, #i",
    ":where(.w), .a",
];

const CHILDREN: [&str; 18] = [
    "&:hover",
    ".t",
    "> .t",
    "+ .t",
    "~ p",
    ".x &",
    "&.on",
    "& + &",
    ":not(&)",
    ":is(&, .z)",
    ":where(&)",
    "p&",
    ".x&",
    "& &",
    ":has(> &)",
    "#y &.k, .t",
    "> &.k",
    ":nth-child(2n + 1 of &)",
];

#[test]
fn specificity_matches_native_nesting_one_level() {
    let mut checked = 0;
    for parent in PARENTS {
        for child in CHILDREN {
            let out = css(&format!("rule {parent} {{ rule {child} {{ x: y; }} }}"));
            let lowered = only_selector(&out);
            let reference = native(parent, child);
            let got = list_specificity(&lowered);
            let want = list_specificity(&reference);
            assert_eq!(
                got, want,
                "{parent} / {child}: `{lowered}` vs `{reference}`"
            );
            assert_eq!(got.len(), split_list(child).len(), "{lowered}");
            checked += got.len();
        }
    }
    assert!(checked >= PARENTS.len() * CHILDREN.len());
}

#[test]
fn specificity_matches_native_nesting_two_levels() {
    let middles = [".m", "&:hover, .n", "> #k", ".x &", ":not(&) .o"];
    for parent in PARENTS {
        for middle in middles {
            for child in CHILDREN {
                let out = css(&format!(
                    "rule {parent} {{ rule {middle} {{ rule {child} {{ x: y; }} }} }}"
                ));
                let lowered = only_selector(&out);
                let reference = native(&native(parent, middle), child);
                assert_eq!(
                    list_specificity(&lowered),
                    list_specificity(&reference),
                    "{parent} / {middle} / {child}: `{lowered}` vs `{reference}`"
                );
            }
        }
    }
}

#[test]
fn calculator_sanity() {
    assert_eq!(list_specificity("#a .b c"), [(1, 1, 1)]);
    assert_eq!(list_specificity(":is(#a, .b) .c"), [(1, 1, 0)]);
    assert_eq!(list_specificity(":where(#a) p::before"), [(0, 0, 2)]);
    assert_eq!(list_specificity("a:not(.b, #c)"), [(1, 0, 1)]);
    assert_eq!(
        list_specificity("li:nth-child(2n of .x), *"),
        [(0, 2, 1), (0, 0, 0)]
    );
    // Cartesian expansion of `#main, .side` would give these instead of
    // (1,1,0) twice.
    assert_eq!(
        list_specificity("#main .item, .side .item"),
        [(1, 1, 0), (0, 2, 0)]
    );
}

#[test]
fn mixed_specificity_list_is_not_expanded() {
    let out = css("rule #main, .side { rule .item { color: red; } }");
    assert_eq!(out, ":is(#main, .side) .item {\n  color: red;\n}\n");
    assert_eq!(list_specificity(&only_selector(&out)), [(1, 1, 0)]);
}

// ----- cascade order ---------------------------------------------------------------

#[test]
fn interleaved_declarations_keep_source_order() {
    let out = css("rule .a {\n\
         a: 1;\n\
         rule .b { b: 2; }\n\
         c: 3; d: 4;\n\
         @media print { e: 5; rule .f { g: 6; } h: 7; }\n\
         i: 8;\n\
         apply r;\n\
         j: 9;\n\
         }\n\
         recipe r { k: 10; rule & + & { l: 11; } }");
    // Every declaration value in output order.
    let order: Vec<&str> = out
        .lines()
        .filter_map(|l| l.trim().strip_suffix(';'))
        .map(|d| d.split(": ").nth(1).unwrap())
        .collect();
    assert_eq!(
        order,
        ["1", "2", "3", "4", "5", "6", "7", "8", "10", "11", "9"]
    );
    // Segments around each nested rule and at-rule.
    let headers: Vec<&str> = out.lines().filter(|l| l.ends_with('{')).collect();
    assert_eq!(
        headers,
        [
            ".a {",
            ".a .b {",
            ".a {",
            "@media print {",
            "  .a {",
            "  .a .f {",
            "  .a {",
            ".a {",
            ".a + .a {",
            ".a {",
        ]
    );
}

#[test]
fn duplicate_declarations_and_segments_are_preserved() {
    assert_eq!(
        css("rule .a { color: red; color: blue; rule & { color: green; } color: red; }"),
        ".a {\n  color: red;\n  color: blue;\n}\n\n.a {\n  color: green;\n}\n\n.a {\n  color: red;\n}\n"
    );
}

// ----- contexts ------------------------------------------------------------------

#[test]
fn nested_errors() {
    let cases: [(&str, Code); 9] = [
        ("rule .a::before { rule &:hover { x: y; } }", Code::E6004),
        ("rule .a:after { rule .b { x: y; } }", Code::E6004),
        ("rule .a { rule &div { x: y; } }", Code::E6007),
        ("rule .a { rule &-x { x: y; } }", Code::E6007),
        ("rule .a { rule .b, , .c { x: y; } }", Code::E6007),
        ("rule .a { rule .b > { x: y; } }", Code::E6007),
        ("rule .a { @scope (.b) { } }", Code::E6006),
        (
            "recipe r { @scope (.b) { rule .c { x: y; } } } rule .a { apply r; }",
            Code::E6006,
        ),
        (
            "recipe r { @keyframes k { to { x: y; } } } rule .a { apply r; }",
            Code::E6005,
        ),
    ];
    for (source, code) in cases {
        assert_eq!(codes(source), [code], "{source}");
    }
    // Declarations and group at-rules under a pseudo-element are fine.
    assert_eq!(
        css("rule .a::before { content: \"\"; @media print { content: none; } }"),
        ".a::before {\n  content: \"\";\n}\n\n@media print {\n  .a::before {\n    content: none;\n  }\n}\n"
    );
}

#[test]
fn recipe_at_rules_are_checked_where_expanded() {
    let source = "recipe fonts {\n\
         @font-face { font-family: X; }\n\
         @layer a, b;\n\
         @property --p { syntax: \"*\"; inherits: false; }\n\
         @page { margin: 0; @top-left { content: \"x\"; } }\n\
         @font-feature-values F { @swash { s: 1; } }\n\
         }\n\
         apply fonts;";
    assert_eq!(
        css(source),
        "@font-face {\n  font-family: X;\n}\n\n@layer a, b;\n\n@property --p {\n  syntax: \"*\";\n  inherits: false;\n}\n\n\
         @page {\n  margin: 0;\n  @top-left {\n    content: \"x\";\n  }\n}\n\n@font-feature-values F { @swash { s: 1; } }\n"
    );
    for at in [
        "@font-face { a: b; }",
        "@layer a;",
        "@property --p { a: b; }",
        "@page { a: b; }",
        "@font-feature-values F { }",
    ] {
        assert_eq!(
            codes(&format!("recipe r {{ {at} }} rule .x {{ apply r; }}")),
            [Code::E6005],
            "{at}"
        );
    }
    // A group at-rule from a recipe declares nothing at stylesheet level.
    assert_eq!(
        codes("recipe r { @media print { a: b; } } apply r;"),
        [Code::E6001]
    );
    assert_eq!(
        css("recipe r { @media print { rule .x { a: b; } } } apply r;"),
        "@media print {\n  .x {\n    a: b;\n  }\n}\n"
    );
}

#[test]
fn opaque_and_unknown_syntax_inside_rules_is_unchanged() {
    let out = css("rule .a {\n\
         @future-thing  (x  :  y) { a  :  b ;  }\n\
         opaque { *zoom:1;  _h : 2 }\n\
         rule .b { --x: { a: b }; }\n\
         }");
    assert_eq!(
        out,
        ".a {\n  @future-thing  (x  :  y) { a  :  b ;  }\n  *zoom:1;  _h : 2\n}\n\n.a .b {\n  --x: { a: b };\n}\n"
    );
}

#[test]
fn empty_nested_blocks() {
    assert_eq!(
        css("rule .a { @media print { } @supports (x: y) { rule .b { } } }"),
        ""
    );
    assert_eq!(css("rule .a { @layer l { } }"), "@layer l {\n}\n");
    assert_eq!(css("rule .a { rule .b { } x: y; }"), ".a {\n  x: y;\n}\n");
}

#[test]
fn interpolated_nested_selectors_are_structural() {
    let source = "bind sel = css.fragment(\">   .child\");\n\
         bind cls = \"on\";\n\
         rule .p, .q { rule #{sel} { a: b; } rule &.#{cls} { c: d; } }";
    assert_eq!(
        css(source),
        ":is(.p, .q) > .child {\n  a: b;\n}\n\n:is(.p, .q).on {\n  c: d;\n}\n"
    );
    // Interpolation cannot smuggle `&` into a top-level rule or break the
    // structure of a nested one.
    assert_eq!(
        codes("bind s = css.fragment(\"& .x\"); rule #{s} { a: b; }"),
        [Code::E6003]
    );
    assert_eq!(
        codes("bind s = css.fragment(\"div\"); rule .a { rule &#{s} { a: b; } }"),
        [Code::E6007]
    );
}

// ----- limits -----------------------------------------------------------------------

fn with_limit(limit: Limit, value: u64) -> CompileOptions {
    let mut options = CompileOptions::default();
    options.limits.set(limit, value).unwrap();
    options
}

#[test]
fn selector_expansion_is_bounded() {
    // Each level doubles the copies of a two-selector parent list:
    // `:is(L) :is(L)` contains 1 + 2|L| selectors.
    let mut source = String::from("rule .a, .b { ");
    let depth = 18;
    for _ in 0..depth {
        source.push_str("rule & & { ");
    }
    source.push_str("x: y;");
    source.push_str(&" }".repeat(depth + 1));
    assert_eq!(
        build(&source, &CompileOptions::default()).err(),
        Some(vec![Code::E8005])
    );
    // Two levels: `:is(.a, .b) :is(.a, .b)` (5 selectors); then that
    // complex parent directly at the start (adding its 4 inner selectors)
    // and as `:is()` after a combinator (adding all 5): 1 + 4 + 5 = 10.
    let small = "rule .a, .b { rule & & { rule & & { x: y; } } }";
    let out = build(small, &with_limit(Limit::EmittedSelectors, 5 + 10)).unwrap();
    assert_eq!(
        out,
        ":is(.a, .b) :is(.a, .b) :is(:is(.a, .b) :is(.a, .b)) {\n  x: y;\n}\n"
    );
    assert_eq!(
        build(small, &with_limit(Limit::EmittedSelectors, 5 + 9)).err(),
        Some(vec![Code::E8005])
    );
    // Top-level selectors do not count.
    let flat = ".a, .b, .c { x: y; }\n";
    assert!(
        build(
            &format!("rule {flat}"),
            &with_limit(Limit::EmittedSelectors, 1)
        )
        .is_ok()
    );
}

#[test]
fn selector_text_growth_is_bounded_by_the_output_limit() {
    // A single compound parent substituted directly into one compound
    // doubles the text per level without adding selectors; the output
    // limit stops it before the text is built.
    let mut source = String::from("rule .abcdefgh { ");
    let depth = 40;
    for _ in 0..depth {
        source.push_str("rule && { ");
    }
    source.push_str("x: y;");
    source.push_str(&" }".repeat(depth + 1));
    assert_eq!(
        build(&source, &CompileOptions::default()).err(),
        Some(vec![Code::E8006])
    );
}
