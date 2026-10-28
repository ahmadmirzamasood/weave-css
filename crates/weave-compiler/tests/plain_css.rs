//! Behavior of the plain-CSS subset through the public API: semantic checks
//! with the independent parser, line breaks, positions, resource limits and
//! robustness on generated input.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

mod independent;

use independent::{Body, Node};
use weave_compiler::{Code, Compilation, CompileOptions, Limit, MemoryResolver, ModuleId, compile};

fn compile_with(source: &[u8], options: &CompileOptions) -> Compilation {
    let mut resolver = MemoryResolver::new();
    let entry = ModuleId::new("main.weave").unwrap();
    resolver.insert(entry.clone(), source);
    compile(&entry, &mut resolver, options)
}

fn css(source: &str) -> String {
    let result = compile_with(source.as_bytes(), &CompileOptions::default());
    match result.outcome {
        Ok(output) => output.css,
        Err(diags) => panic!(
            "{}",
            diags
                .iter()
                .map(|d| d.render(&result.sources))
                .collect::<String>()
        ),
    }
}

fn codes(source: &[u8], options: &CompileOptions) -> Vec<Code> {
    match compile_with(source, options).outcome {
        Ok(_) => Vec::new(),
        Err(diags) => diags.iter().map(|d| d.code()).collect(),
    }
}

fn limits(limit: Limit, value: u64) -> CompileOptions {
    let mut options = CompileOptions::default();
    options.limits.set(limit, value).unwrap();
    options
}

// ----- the independent parser is a real check -----------------------------

#[test]
fn independent_parser_rejects_invalid_css() {
    for bad in [
        "a { color: \"x\n; }",
        "a { b: url(x y); }",
        "a { b: c); }",
        "a { color: red { } }",
        "a { *zoom: 1; }",
        "color: red;",
        "@font-face { a { } }",
    ] {
        let (_, problems) = independent::parse(bad);
        assert!(!problems.is_empty(), "accepted {bad:?}");
    }
    let (_, problems) = independent::parse("a { --x: { y }; b: c !important }");
    assert!(problems.is_empty(), "{problems:?}");
}

// ----- semantics checked through the independent parser -------------------

#[test]
fn declarations_survive_in_order_with_importance() {
    let out = css(r#"
rule .button, a[href^="https://"]:not(.plain) {
    color: red;
    color: rgb(0 0 0 / 50%);
    content: "a; b { c } /* not a comment */";
    margin: 0 /* keep */ auto !IMPORTANT;
    --empty:;
    --block: { a: b; c: [d] };
    font: 12px/1.5 system-ui
}
"#);
    let nodes = independent::validate(&out);
    let [Node::Rule { prelude, body }] = nodes.as_slice() else {
        panic!("one rule expected: {nodes:#?}");
    };
    assert_eq!(prelude, r#".button, a[href^="https://"]:not(.plain)"#);
    let decls: Vec<_> = body.iter().flat_map(Node::declarations).collect();
    assert_eq!(
        decls,
        [
            ("color", "red", false),
            ("color", "rgb(0 0 0 / 50%)", false),
            ("content", r#""a; b { c } /* not a comment */""#, false),
            ("margin", "0 /* keep */ auto", true),
            ("--empty", "", false),
            ("--block", "{ a: b; c: [d] }", false),
            ("font", "12px/1.5 system-ui", false),
        ]
    );
}

#[test]
fn css_values_are_never_evaluated() {
    let out = css(
        "rule a { width: calc(1px + 2px); height: 10px/2; z-index: 1+1; margin: -0px; color: RED; }",
    );
    let nodes = independent::validate(&out);
    let values: Vec<_> = nodes[0]
        .declarations()
        .into_iter()
        .map(|(_, v, _)| v.to_string())
        .collect();
    assert_eq!(values, ["calc(1px + 2px)", "10px/2", "1+1", "-0px", "RED"]);
}

#[test]
fn at_rule_bodies_have_their_css_grammar() {
    let out = css(r#"
@media print { rule .a { color: red; } }
@font-face { font-family: X; }
@keyframes k { from { opacity: 0; } to { opacity: 1; } }
@page :first { margin: 1cm; @top-left { content: "x"; } }
@layer a, b;
"#);
    let nodes = independent::validate(&out);
    let names: Vec<_> = nodes
        .iter()
        .map(|n| match n {
            Node::AtRule { name, body, .. } => (name.as_str(), matches!(body, Body::Nodes(_))),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(
        names,
        [
            ("media", true),
            ("font-face", true),
            ("keyframes", true),
            ("page", true),
            ("layer", false)
        ]
    );
    let Node::AtRule {
        body: Body::Nodes(frames),
        ..
    } = &nodes[2]
    else {
        unreachable!()
    };
    assert!(
        matches!(&frames[..], [Node::Rule { prelude: a, .. }, Node::Rule { prelude: b, .. }] if a == "from" && b == "to")
    );
}

// ----- line breaks and positions --------------------------------------------

#[test]
fn every_line_break_form_is_written_as_lf() {
    let source = "rule .a {\r\n    grid-template-areas:\r\n        \"a\"\r        \"b\";\x0c    --c: x\r\ny;\r\n}\r\nopaque {\r\n  .x {}\r\n  .y {}\r\n}\r\n@unknown {\r\n}\r\n";
    let out = css(source);
    assert!(!out.contains('\r') && !out.contains('\x0c'), "{out:?}");
    assert_eq!(
        out,
        ".a {\n  grid-template-areas: \"a\"\n        \"b\";\n  --c: x\ny;\n}\n\n.x {}\n  .y {}\n\n@unknown {\n}\n"
    );
}

#[test]
fn crlf_positions_and_unicode_columns() {
    let result = compile_with(
        "rule .é {\r\n  a: \"😀\r\n}\r\n".as_bytes(),
        &CompileOptions::default(),
    );
    let Err(diags) = &result.outcome else {
        panic!("expected an error");
    };
    let (file, lc) = result
        .sources
        .location(diags[0].primary().unwrap())
        .unwrap();
    assert_eq!(
        (diags[0].code(), file, lc.line, lc.column),
        (Code::E0002, "main.weave", 2, 6)
    );
}

#[test]
fn output_is_deterministic_and_has_no_bom() {
    let source = "\u{feff}rule a { color: red; }";
    assert_eq!(css(source), "a {\n  color: red;\n}\n");
    assert_eq!(css(source), css(source));
}

// ----- resource limits -------------------------------------------------------

#[test]
fn bracket_depth_limit_is_exact() {
    // One block (the rule body) plus N brackets.
    let source = |n: usize| format!("rule a {{ b: {}x{}; }}", "(".repeat(n), ")".repeat(n));
    let options = limits(Limit::NestingDepth, 8);
    assert_eq!(codes(source(7).as_bytes(), &options), []);
    assert_eq!(codes(source(8).as_bytes(), &options), [Code::E8002]);
}

#[test]
fn block_depth_limit_is_exact() {
    let source = |n: usize| {
        format!(
            "{}rule a {{ b: c; }}{}",
            "@media x {".repeat(n),
            "}".repeat(n)
        )
    };
    let options = limits(Limit::NestingDepth, 8);
    assert_eq!(codes(source(7).as_bytes(), &options), []);
    assert_eq!(codes(source(8).as_bytes(), &options), [Code::E8002]);
}

#[test]
fn verbatim_and_interpolation_depth_are_bounded() {
    let options = limits(Limit::NestingDepth, 16);
    let deep = format!("opaque {{ {} }}", "[".repeat(100));
    assert_eq!(codes(deep.as_bytes(), &options), [Code::E8002]);
    let deep = format!("@unknown {}", "(".repeat(100));
    assert_eq!(codes(deep.as_bytes(), &options), [Code::E8002]);
    let mut interp = String::from("x");
    for _ in 0..40 {
        interp = format!("#{{\"{interp}\"}}");
    }
    let source = format!("rule a {{ b: {interp}; }}");
    assert!(codes(source.as_bytes(), &options).contains(&Code::E8002));
}

#[test]
fn deepest_allowed_nesting_does_not_overflow_the_stack() {
    // The largest configurable depth, on a thread with the default 2 MiB
    // test-thread stack.
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let options = limits(Limit::NestingDepth, 4096);
            let n = 4095;
            let source = format!(
                "{}rule a {{ b: c; }}{}",
                "@media x {".repeat(n),
                "}".repeat(n)
            );
            let ok = codes(source.as_bytes(), &options);
            let source = format!("{}{}", "@media x {".repeat(5000), "}".repeat(5000));
            let too_deep = codes(source.as_bytes(), &options);
            (ok, too_deep)
        })
        .unwrap();
    let (ok, too_deep) = handle.join().unwrap();
    assert_eq!(ok, []);
    assert_eq!(too_deep, [Code::E8002]);
}

#[test]
fn input_and_output_size_limits() {
    let source = b"rule a { color: red; }";
    let options = limits(Limit::FileBytes, source.len() as u64);
    assert_eq!(codes(source, &options), []);
    let options = limits(Limit::FileBytes, source.len() as u64 - 1);
    assert_eq!(codes(source, &options), [Code::E8001]);

    let big = "rule a { b: c; }\n".repeat(1000);
    let out_len = css(&big).len() as u64;
    assert_eq!(
        codes(big.as_bytes(), &limits(Limit::OutputBytes, out_len)),
        []
    );
    assert_eq!(
        codes(big.as_bytes(), &limits(Limit::OutputBytes, out_len - 1)),
        [Code::E8006]
    );
}

#[test]
fn diagnostic_limit_stops_parsing() {
    let source = "x;".repeat(10_000);
    let options = limits(Limit::Diagnostics, 5);
    assert_eq!(codes(source.as_bytes(), &options), [Code::E0101; 5]);
}

// ----- unsupported constructs ------------------------------------------------

#[test]
fn skipped_statements_report_once_but_keep_end_of_input_errors() {
    let options = CompileOptions::default();
    // One E5001 for the statement, none for interpolation inside it.
    assert_eq!(codes(br##"warn "#{y}" + #{z};"##, &options), [Code::E5001]);
    assert_eq!(
        codes(
            b"when #{a} { rule .x { b: #{c}; } } otherwise { }",
            &options
        ),
        [Code::E5001]
    );
    // A keyword lexed as a function token still starts its statement.
    assert_eq!(
        codes(b"when(a) { } rule a { b: c; }", &options),
        [Code::E5001]
    );
    // An unterminated comment inside a skipped statement is still reported.
    assert_eq!(
        codes(b"fail 1 /* never closed", &options),
        [Code::E5001, Code::E0003]
    );
}

#[test]
fn nothing_unsupported_is_silently_dropped() {
    // Every compile-time construct either compiles or is reported.
    for source in [
        "rule a { apply r; }",
        "rule a { rule b { c: d; } }",
        "rule a { @media print { b: c; } }",
        "rule a { b: #{math.round(1)}; }",
        "bind r = range(0, 2);",
        "each i in [1] { }",
    ] {
        assert!(
            codes(source.as_bytes(), &CompileOptions::default()).contains(&Code::E5001),
            "{source}"
        );
    }
}

// ----- robustness -------------------------------------------------------------

/// A small deterministic generator (xorshift), so failures are reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

const PIECES: [&str; 40] = [
    "rule",
    " ",
    "\n",
    "\r\n",
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    ";",
    ":",
    ",",
    "\"",
    "'",
    "\\",
    "/*",
    "*/",
    "/*!",
    "#{",
    "\\#{",
    "url(",
    "@media",
    "@font-face",
    "@keyframes",
    "@x",
    "opaque",
    "bind",
    "when",
    "&",
    ".a",
    "color",
    "--c",
    "!important",
    "é",
    "😀",
    "\x0c",
    "%",
    "from",
    "@import",
];

#[test]
fn generated_inputs_terminate_with_valid_spans() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let options = limits(Limit::NestingDepth, 32);
    for _ in 0..3000 {
        let len = (rng.next() % 40) as usize;
        let source: String = (0..len)
            .map(|_| PIECES[(rng.next() % PIECES.len() as u64) as usize])
            .collect();
        let first = compile_with(source.as_bytes(), &options);
        let second = compile_with(source.as_bytes(), &options);
        match (&first.outcome, &second.outcome) {
            (Ok(a), Ok(b)) => assert_eq!(a.css, b.css, "{source:?}"),
            (Err(a), Err(b)) => {
                assert_eq!(a, b, "{source:?}");
                for diag in a {
                    if let Some(span) = diag.primary() {
                        let file = first.sources.get(span.file).unwrap();
                        assert!(file.slice(span).is_some(), "{source:?}: {diag:?}");
                    }
                }
            }
            _ => panic!("nondeterministic outcome for {source:?}"),
        }
    }
}
