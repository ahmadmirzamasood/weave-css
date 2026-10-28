//! Values, scopes and interpolation through the public API (phase 04):
//! scoping rules, CSS that must stay CSS, resource limits at their exact
//! boundaries, stack safety at the largest nesting limit, and generated
//! hostile strings that must never change the structure of the output
//! (checked with the independent CSS parser).

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

mod independent;

use std::fmt::Write as _;

use independent::Node;
use weave_compiler::{Code, Compilation, CompileOptions, Limit, MemoryResolver, ModuleId, compile};

fn compile_with(source: &str, options: &CompileOptions) -> Compilation {
    let mut resolver = MemoryResolver::new();
    let entry = ModuleId::new("main.weave").unwrap();
    resolver.insert(entry.clone(), source.as_bytes());
    compile(&entry, &mut resolver, options)
}

fn css(source: &str) -> String {
    let result = compile_with(source, &CompileOptions::default());
    match result.outcome {
        Ok(output) => output.css,
        Err(diags) => panic!(
            "{source}\n{}",
            diags
                .iter()
                .map(|d| d.render(&result.sources))
                .collect::<String>()
        ),
    }
}

fn codes_with(source: &str, options: &CompileOptions) -> Vec<Code> {
    match compile_with(source, options).outcome {
        Ok(_) => Vec::new(),
        Err(diags) => diags.iter().map(|d| d.code()).collect(),
    }
}

fn codes(source: &str) -> Vec<Code> {
    codes_with(source, &CompileOptions::default())
}

fn limits(limit: Limit, value: u64) -> CompileOptions {
    let mut options = CompileOptions::default();
    options.limits.set(limit, value).unwrap();
    options
}

/// The single declaration value of `rule a { p: <value>; }`.
fn value(expr_value: &str) -> String {
    let out = css(&format!("rule a {{ p: {expr_value}; }}"));
    out.strip_prefix("a {\n  p: ")
        .and_then(|rest| rest.strip_suffix(";\n}\n"))
        .unwrap_or_else(|| panic!("unexpected output {out:?}"))
        .to_string()
}

// ----- scopes ------------------------------------------------------------------

#[test]
fn blocks_scope_their_bindings() {
    // A binding inside a group at-rule is not visible after it.
    assert_eq!(
        codes("@media print { bind x = 1px; } rule a { w: #{x}; }"),
        [Code::E1002]
    );
    // Sibling rules do not see each other's bindings.
    assert_eq!(
        codes("rule a { bind x = 1; } rule b { w: #{x}; }"),
        [Code::E1002]
    );
    // Shadowing in an inner block leaves the outer binding unchanged.
    assert_eq!(
        css("bind x = 1px; @media print { bind x = 2px; rule a { w: #{x}; } } rule b { w: #{x}; }"),
        "@media print {\n  a {\n    w: 2px;\n  }\n}\n\nb {\n  w: 1px;\n}\n"
    );
    // The same name may be bound once in each scope.
    assert_eq!(
        codes("bind x = 1; rule a { bind x = 2; } rule b { bind x = 3; }"),
        []
    );
}

#[test]
fn bindings_are_not_hoisted() {
    assert_eq!(codes("rule a { w: #{x}; } bind x = 1;"), [Code::E1003]);
    // Self-reference inside the binding's own block is E1003 as well, even
    // with an outer binding of the same name.
    assert_eq!(
        codes("bind x = 1; rule a { bind x = x + 1; }"),
        [Code::E1003]
    );
    // A binding sees earlier bindings of its block.
    assert_eq!(
        css("rule a { bind a = 1px; bind b = a * 2; p: #{b}; }"),
        "a {\n  p: 2px;\n}\n"
    );
}

/// Scopes switch from a short list to a hash map as they grow (phase 09);
/// lookups, shadowing and the not-yet-bound rule must not change at the
/// switch, in either direction of the size threshold.
#[test]
fn scopes_of_every_size_resolve_names_the_same_way() {
    for count in [1usize, 7, 8, 9, 40] {
        let mut source = String::new();
        for i in 0..count {
            let _ = writeln!(source, "bind v{i} = {i}px;");
        }
        let last = count - 1;
        let _ = writeln!(
            source,
            "rule a {{ first: #{{v0}}; last: #{{v{last}}}; }}\nrule b {{ bind v0 = 99px; w: #{{v0}}; }}"
        );
        assert_eq!(
            css(&source),
            format!("a {{\n  first: 0px;\n  last: {last}px;\n}}\n\nb {{\n  w: 99px;\n}}\n"),
            "{count} bindings"
        );
        // A name used before its `bind`, with the scope at every size.
        let early = format!("rule c {{ w: #{{late}}; }}\n{source}bind late = 1;");
        assert_eq!(codes(&early), [Code::E1003], "{count} bindings");
    }
}

#[test]
fn a_failed_binding_is_reported_once() {
    // `y` and `z` depend on the failed `x` and add no diagnostics.
    assert_eq!(
        codes("bind x = 1px + 1em; bind y = x * 2; rule a { w: #{y}; h: #{x}; }"),
        [Code::E3001]
    );
    // Independent failures are all reported.
    assert_eq!(
        codes("bind x = 1 / 0; bind y = [][0]; bind z = nope;"),
        [Code::E3003, Code::E2005, Code::E1002]
    );
}

#[test]
fn prelude_names_can_be_shadowed() {
    assert_eq!(codes("rule a { p: #{css}; }"), [Code::E1004]);
    assert_eq!(
        css("bind css = 3px; rule a { w: #{css * 2}; }"),
        "a {\n  w: 6px;\n}\n"
    );
    // Once shadowed, the namespace is unreachable in that scope.
    assert_eq!(
        codes("bind css = 1; rule a { w: #{css.space([1])}; }"),
        [Code::E1004]
    );
}

// ----- CSS stays CSS -------------------------------------------------------------

#[test]
fn bare_identifiers_and_browser_functions_are_never_evaluated() {
    let source = "bind gap = 8px; bind primary = #fff;\n\
                  rule a {\n\
                  color: primary;\n\
                  margin: gap gap;\n\
                  width: calc(100% - var(--gap, gap));\n\
                  padding: env(safe-area-inset-left, 1px) max(gap, 1px) clamp(1px, 2vw, gap);\n\
                  --u: unknown-fn(gap, primary) attr(data-x type(<length>), gap);\n\
                  }";
    let out = css(source);
    for text in [
        "color: primary;",
        "margin: gap gap;",
        "width: calc(100% - var(--gap, gap));",
        "padding: env(safe-area-inset-left, 1px) max(gap, 1px) clamp(1px, 2vw, gap);",
        "--u: unknown-fn(gap, primary) attr(data-x type(<length>), gap);",
    ] {
        assert!(out.contains(text), "{text}\n{out}");
    }
    independent::validate(&out);
}

#[test]
fn interpolation_inside_browser_functions() {
    assert_eq!(value("calc(100% - #{2px * 4})"), "calc(100% - 8px)");
    assert_eq!(value("var(--x, #{\"a\"})"), "var(--x, \"a\")");
    assert_eq!(value("env(x, #{1 + 1})"), "env(x, 2)");
    assert_eq!(value("my-fn(#{`a b`})"), "my-fn(a b)");
}

// ----- serialization ---------------------------------------------------------------

#[test]
fn value_context_serialization() {
    assert_eq!(value("#{1 / 3}"), "0.333333");
    assert_eq!(value("#{-0.5em}"), "-0.5em");
    assert_eq!(value("#{2px * -0}"), "0px");
    assert_eq!(value("#{1in + 0px}"), "1in");
    assert_eq!(value("#{0px + 1in}"), "96px");
    assert_eq!(value("#{#ABCD}"), "#aabbccdd");
    assert_eq!(value("#{true}"), "true");
    assert_eq!(value("#{\"é \\\"q\\\"\"}"), "\"é \\\"q\\\"\"");
    assert_eq!(value("#{`  spaced   out  `}"), "spaced   out");
    assert_eq!(
        value("#{css.comma([1, \"a\", #fff, true, `b`])}"),
        "1, \"a\", #ffffff, true, b"
    );
}

#[test]
fn equality_never_errors_and_ordering_is_strict() {
    assert_eq!(value("#{1 == \"1\"}"), "false");
    assert_eq!(value("#{[1, [2]] != [1, [2]]}"), "false");
    assert_eq!(value("#{{a: 1} == {a: 1px}}"), "false");
    assert_eq!(value("#{`a` == \"a\"}"), "false");
    assert_eq!(value("#{\"b\" > \"a\" && \"é\" > \"z\"}"), "true");
    assert_eq!(codes("rule a { p: #{true < false}; }"), [Code::E2001]);
    assert_eq!(codes("rule a { p: #{[1] < [2]}; }"), [Code::E2001]);
}

// ----- limits ----------------------------------------------------------------------

#[test]
fn evaluation_step_limit_is_exact() {
    // The `bind` statement is one step and `1 + 1` three more.
    let source = |terms: usize| format!("bind x = {};", vec!["1"; terms].join(" + "));
    let options = limits(Limit::EvaluationSteps, 10);
    assert_eq!(codes_with(&source(5), &options), []);
    assert_eq!(codes_with(&source(6), &options), [Code::E8004]);
}

#[test]
fn collection_limit_is_exact() {
    let options = limits(Limit::CollectionElements, 3);
    assert_eq!(codes_with("bind x = [1, 2, 3];", &options), []);
    assert_eq!(
        codes_with("bind x = [1, 2, 3, 4];", &options),
        [Code::E8007]
    );
    assert_eq!(codes_with("bind x = {a: 1, b: 2, c: 3};", &options), []);
    assert_eq!(
        codes_with("bind x = {a: 1, b: 2, c: 3, d: 4};", &options),
        [Code::E8007]
    );
}

#[test]
fn string_limit_is_exact() {
    let options = limits(Limit::StringBytes, 4);
    assert_eq!(codes_with("bind x = \"ab\" + \"cd\";", &options), []);
    assert_eq!(
        codes_with("bind x = \"ab\" + \"cde\";", &options),
        [Code::E8007]
    );
    assert_eq!(codes_with("bind x = \"abcde\";", &options), [Code::E8007]);
    assert_eq!(codes_with("bind x = `abcde`;", &options), [Code::E8007]);
    assert_eq!(
        codes_with("bind x = \"a#{1234}\";", &options),
        [Code::E8007]
    );
    assert_eq!(codes_with("bind x = css.space([1, 2]);", &options), []);
    assert_eq!(
        codes_with("bind x = css.space([1, 2, 3]);", &options),
        [Code::E8007]
    );
    // Declaration values are bounded by the output limit, not this one.
    assert_eq!(codes_with("rule a { p: #{1234} #{5678}; }", &options), []);
}

#[test]
fn expression_brackets_share_the_nesting_limit() {
    // One block (the rule body) plus N brackets.
    let source = |n: usize| format!("rule a {{ p: #{{{}1{}}}; }}", "(".repeat(n), ")".repeat(n));
    let options = limits(Limit::NestingDepth, 8);
    assert_eq!(codes_with(&source(7), &options), []);
    assert_eq!(codes_with(&source(8), &options), [Code::E8002]);
    let list = |n: usize| format!("bind x = {}1{};", "[".repeat(n), "]".repeat(n));
    assert_eq!(codes_with(&list(8), &options), []);
    assert_eq!(codes_with(&list(9), &options), [Code::E8002]);
}

#[test]
fn deepest_expressions_do_not_overflow_the_stack() {
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let options = limits(Limit::NestingDepth, 4096);
            let mut results = Vec::new();
            // Lists nested 4095 deep are built, compared and dropped.
            let n = 4095;
            let list = format!("{}1{}", "[".repeat(n), "]".repeat(n));
            let source = format!("bind x = {list}; bind y = x == {list}; rule a {{ p: #{{y}}; }}");
            results.push(codes_with(&source, &options));
            // Parentheses inside an interpolation inside a rule (1 + 4095).
            let source = format!("rule a {{ p: #{{{}1{}}}; }}", "(".repeat(n), ")".repeat(n));
            results.push(codes_with(&source, &options));
            // Interpolations nested inside strings: 64 levels are allowed.
            let nested = |levels: usize| {
                let mut s = String::from("1");
                for _ in 0..levels {
                    s = format!("\"#{{{s}}}\"");
                }
                format!("rule a {{ p: #{{{s}}}; }}")
            };
            results.push(codes_with(&nested(63), &options));
            results.push(codes_with(&nested(64), &options));
            results
        })
        .unwrap();
    let results = handle.join().unwrap();
    assert_eq!(results[0], []);
    assert_eq!(results[1], []);
    assert_eq!(results[2], []);
    assert_eq!(results[3], [Code::E8002]);
}

// ----- injection -------------------------------------------------------------------

/// A Weave string literal for arbitrary text: everything but ASCII
/// alphanumerics is hex-escaped, so the literal itself is always valid.
fn weave_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            let _ = write!(out, "\\{:x} ", u32::from(c));
        }
    }
    out.push('"');
    out
}

/// The decoded value of a CSS string token.
fn css_string_value(text: &str) -> String {
    let mut parser = cssparser::Parser::new(text);
    let value = parser
        .expect_string()
        .unwrap_or_else(|e| panic!("not a string: {text:?}: {e:?}"))
        .to_string();
    assert!(parser.is_exhausted(), "more than a string: {text:?}");
    value
}

/// What CSS makes of the text: U+0000 becomes U+FFFD.
fn css_text(text: &str) -> String {
    text.replace('\0', "\u{fffd}")
}

const HOSTILE: [&str; 26] = [
    "",
    "red; } body { display: none",
    "*/ /* ",
    "\") ; x: y; z: \"",
    "'; x: y",
    "\\",
    "\\;",
    "a\nb\r\nc\x0cd",
    "\0",
    "</style><script>alert(1)</script>",
    "url(x) ) ( [",
    "#{x}",
    "\\#{x}",
    "-",
    "--",
    "1",
    "-1",
    "é😀\u{202e}",
    "\u{7f}\u{1}\u{1f}",
    " ",
    "!important",
    "@media x {",
    "<!-- -->",
    "/",
    "*",
    "\u{fffd}",
];

/// Inserts `text` through every interpolation context and checks with the
/// independent parser that the structure is exactly the template's and
/// that every value round-trips.
fn check_injection(text: &str) {
    let s = weave_string(text);
    let source = format!(
        "bind s = {s};\n\
         rule .a-#{{s}} {{\n\
         --p-#{{s}}: #{{s}};\n\
         content: \"#{{s}}\";\n\
         quotes: '#{{s}}';\n\
         background: url(#{{s}});\n\
         background: url(\"#{{s}}\");\n\
         }}\n\
         rule [data-x=\"#{{s}}\"] {{ color: red; }}\n\
         @media (min-width: 1px) {{ rule .m {{ content: \"x#{{s}}\"; }} }}\n"
    );
    let out = css(&source);
    let (nodes, problems) = independent::parse(&out);
    assert!(problems.is_empty(), "{text:?}: {problems:?}\n{out}");
    assert_eq!(
        nodes.len(),
        3,
        "{text:?}: top-level structure changed\n{out}"
    );
    let Node::Rule { prelude, body } = &nodes[0] else {
        panic!("{text:?}: expected a rule\n{out}");
    };
    assert!(prelude.starts_with(".a-"), "{prelude}");
    let declarations: Vec<(&str, &str, bool)> = body.iter().flat_map(Node::declarations).collect();
    assert_eq!(
        declarations.len(),
        5,
        "{text:?}: declarations changed\n{out}"
    );
    let expected = css_text(text);
    let (name, raw, important) = declarations[0];
    assert_eq!(name, format!("--p-{expected}"), "{text:?}\n{out}");
    assert!(!important);
    assert_eq!(css_string_value(raw), expected, "{text:?}");
    for &(name, raw, important) in &declarations[1..3] {
        assert!(!important, "{name}");
        assert_eq!(css_string_value(raw), expected, "{text:?} {name}");
    }
    let (_, url, _) = declarations[3];
    assert!(url.starts_with("url(") && url.ends_with(')'), "{url}");
    let (_, quoted_url, _) = declarations[4];
    let inner = quoted_url
        .strip_prefix("url(")
        .and_then(|u| u.strip_suffix(')'))
        .unwrap_or_else(|| panic!("{quoted_url}"));
    assert_eq!(css_string_value(inner), expected, "{text:?}");
    let Node::Rule { body, .. } = &nodes[1] else {
        panic!("{text:?}: expected the attribute rule\n{out}");
    };
    assert_eq!(body.len(), 1, "{text:?}\n{out}");
    let Node::AtRule { .. } = &nodes[2] else {
        panic!("{text:?}: expected @media\n{out}");
    };
}

#[test]
fn hostile_strings_cannot_change_structure() {
    for text in HOSTILE {
        check_injection(text);
    }
}

#[test]
fn generated_strings_cannot_change_structure() {
    let alphabet: Vec<char> = "a-_1 ;:{}()[]\"'\\/*!@#<>,.\n\r\t\0\u{7f}é😀%&|`$"
        .chars()
        .collect();
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..300 {
        let len = (next() % 12) as usize;
        let text: String = (0..len)
            .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
            .collect();
        check_injection(&text);
    }
}

#[test]
fn identifier_fragments_keep_at_rule_preludes_intact() {
    for text in HOSTILE.iter().filter(|t| !t.is_empty()) {
        let source = format!(
            "@container #{{css.ident({})}} (width > 1px) {{ rule .p {{ color: red; }} }}",
            weave_string(text)
        );
        let out = css(&source);
        let nodes = independent::validate(&out);
        assert_eq!(nodes.len(), 1, "{text:?}\n{out}");
    }
}

#[test]
fn fragments_reject_structure_but_allow_composition() {
    assert_eq!(codes("bind f = css.fragment(\"a; b\");"), [Code::E2006]);
    assert_eq!(codes("bind f = css.fragment(\"a } b {\");"), [Code::E2006]);
    assert_eq!(codes("bind f = css.fragment(\"/* open\");"), [Code::E2006]);
    assert_eq!(codes("bind f = css.fragment(\"a\\\\\");"), [Code::E2006]);
    // A fragment may add commas and combinators to a selector.
    assert_eq!(
        css("bind more = `.b > .c`; rule .a, #{more} { x: y; }"),
        ".a, .b > .c {\n  x: y;\n}\n"
    );
    // A fragment may add tokens to a value, including `!important`.
    assert_eq!(
        css("bind f = css.fragment(\"red !important\"); rule a { color: #{f}; }"),
        "a {\n  color: red !important;\n}\n"
    );
}

#[test]
fn output_is_deterministic() {
    let source = "bind m = {z: 1, a: 2, m: 3};\n\
                  rule a { p: #{css.comma([m.z, m.a, m[\"m\"]])}; q: #{1 / 7}; }";
    let first = css(source);
    for _ in 0..5 {
        assert_eq!(css(source), first);
    }
    assert_eq!(first, "a {\n  p: 1, 2, 3;\n  q: 0.142857;\n}\n");
}

#[test]
fn diagnostics_point_into_crlf_and_unicode_sources() {
    let source = "bind é = 1;\r\nrule a {\r\n  p: #{é + \"x\"};\r\n}";
    let result = compile_with(source, &CompileOptions::default());
    let Err(diags) = &result.outcome else {
        panic!("expected an error");
    };
    assert_eq!(diags.len(), 1);
    let (_, position) = result
        .sources
        .location(diags[0].primary().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (3, 10));
}
