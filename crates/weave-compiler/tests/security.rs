//! Security boundaries through the public API (phase 10): every resource
//! limit fails with its diagnostic, inputs that used to take superlinear
//! time, malformed and hostile Unicode, and control characters in
//! diagnostics. Regressions found by fuzzing and probing live here; see
//! fuzz/README.md for the campaigns.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use weave_compiler::{Code, Compilation, CompileOptions, Limit, MemoryResolver, ModuleId, compile};

fn compile_with(source: &str, options: &CompileOptions) -> Compilation {
    compile_bytes(source.as_bytes(), options)
}

fn compile_bytes(source: &[u8], options: &CompileOptions) -> Compilation {
    let mut resolver = MemoryResolver::new();
    let entry = ModuleId::new("main.weave").unwrap();
    resolver.insert(entry.clone(), source);
    compile(&entry, &mut resolver, options)
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

/// Runs `build` and fails if it takes longer than `budget`. The budgets are
/// far above the expected time (debug builds included) and only catch
/// superlinear behaviour, which turns them into minutes.
fn within<T>(budget: Duration, what: &str, build: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = build();
    let elapsed = start.elapsed();
    assert!(
        elapsed < budget,
        "{what} took {elapsed:?} (budget {budget:?})"
    );
    result
}

fn map_literal(keys: usize) -> String {
    let mut source = String::from("bind x = {");
    for i in 0..keys {
        let _ = write!(source, "k{i}: 1, ");
    }
    source.push_str("};\nrule a { n: #{map.len(x)}; }\n");
    source
}

#[test]
fn map_literals_with_many_keys_take_linear_time() {
    // Found by probing (phase 10): duplicate-key detection scanned every
    // earlier key, so 65,536 keys (the default collection limit) took
    // about 20 s in a release build and ~174,000 keys over two minutes.
    let keys = 65_536;
    let result = within(Duration::from_secs(20), "65,536-key map", || {
        compile_with(&map_literal(keys), &CompileOptions::default())
    });
    let css = result.outcome.expect("the map is within the limits").css;
    assert_eq!(css, format!("a {{\n  n: {keys};\n}}\n"));

    // One key more is E8007, also without scanning.
    let over = within(Duration::from_secs(20), "65,537-key map", || {
        codes(&map_literal(keys + 1))
    });
    assert_eq!(over, [Code::E8007]);

    // Duplicates are still found, wherever they are.
    let mut late_duplicate = map_literal(10_000);
    late_duplicate = late_duplicate.replace("k9999: 1", "k0: 2");
    assert_eq!(codes(&late_duplicate), [Code::E2008]);
}

fn joined(count: usize, item: impl Fn(usize) -> String) -> String {
    (0..count).map(item).collect::<Vec<_>>().join(", ")
}

#[test]
fn long_parameter_and_argument_lists_take_linear_time() {
    // Found by probing (phase 10): duplicate parameter names, duplicate
    // named arguments, positional-after-named arguments and named-argument
    // binding were each checked by scanning, so 32,000 parameters took 4 s
    // in a release build and a 1 MiB input could take minutes.
    let n = 40_000;
    let budget = Duration::from_secs(20);
    let params = joined(n, |i| format!("p{i}"));
    let named = joined(n, |i| format!("p{i}: {i}"));
    let positional = joined(n, |i| i.to_string());

    let source = format!(
        "fn f({params}) {{ yield p{last}; }}\nrule a {{ x: #{{f({named})}}; y: #{{f({positional})}}; }}\n",
        last = n - 1
    );
    let result = within(budget, "40,000 parameters and arguments", || {
        compile_with(&source, &CompileOptions::default())
    });
    let css = result.outcome.expect("valid calls").css;
    assert_eq!(
        css,
        format!("a {{\n  x: {last};\n  y: {last};\n}}\n", last = n - 1)
    );

    let recipe = format!(
        "recipe r({}) {{ out: #{{p{last}}}; }}\nrule a {{ apply r({named}); }}\n",
        joined(n, |i| format!("p{i} = 0")),
        last = n - 1
    );
    let result = within(budget, "40,000 recipe parameters", || {
        compile_with(&recipe, &CompileOptions::default())
    });
    assert_eq!(
        result.outcome.expect("valid apply").css,
        format!("a {{\n  out: {};\n}}\n", n - 1)
    );

    // The diagnostics are unchanged, wherever the offending item is.
    let duplicate_param = format!("fn f({params}, p0) {{ yield 1; }}\n");
    assert_eq!(codes(&duplicate_param), [Code::E1001]);
    let duplicate_arg =
        format!("fn f(a) {{ yield 1; }}\nrule a {{ x: #{{f({named}, p7: 1)}}; }}\n");
    assert_eq!(
        within(budget, "duplicate named argument", || codes(&duplicate_arg)),
        [Code::E4003]
    );
    let positional_after = format!("fn f(a) {{ yield 1; }}\nrule a {{ x: #{{f({named}, 1)}}; }}\n");
    assert_eq!(
        within(budget, "positional after named", || codes(
            &positional_after
        )),
        [Code::E0113]
    );
    let unknown =
        format!("fn f({params}) {{ yield 1; }}\nrule a {{ x: #{{f({named}, q: 1)}}; }}\n");
    assert_eq!(
        within(budget, "unknown named argument", || codes(&unknown)),
        [Code::E4002]
    );
}

fn build(modules: &[(&str, &str)], options: &CompileOptions) -> Compilation {
    let mut resolver = MemoryResolver::new();
    for (name, text) in modules {
        resolver.insert(ModuleId::new(*name).unwrap(), text.as_bytes());
    }
    compile(
        &ModuleId::new(modules[0].0).unwrap(),
        &mut resolver,
        options,
    )
}

/// Module names and texts, entry first.
type Modules<'a> = &'a [(&'a str, &'a str)];

fn limited(limit: Limit, value: u64) -> CompileOptions {
    let mut options = CompileOptions::default();
    options.limits.set(limit, value).unwrap();
    options
}

/// Each limit, lowered, stops a build that needs more with its own code;
/// the same build passes with the default limits. Exact boundaries are
/// tested next to each feature (values.rs, functions.rs, nesting.rs,
/// modules.rs, plain_css.rs).
#[test]
fn every_resource_limit_fails_gracefully_with_its_code() {
    let lib = "export bind v = 1;\n";
    let cases: [(Limit, u64, Modules<'_>, Code); 10] = [
        (
            Limit::FileBytes,
            8,
            &[("main.weave", "rule a { b: c; }")],
            Code::E8001,
        ),
        (
            Limit::TotalInputBytes,
            40,
            &[
                (
                    "main.weave",
                    "load \"./l.weave\" as l;\nrule a { b: #{l.v}; }",
                ),
                ("l.weave", lib),
            ],
            Code::E8001,
        ),
        (
            Limit::Modules,
            1,
            &[
                (
                    "main.weave",
                    "load \"./l.weave\" as l;\nrule a { b: #{l.v}; }",
                ),
                ("l.weave", lib),
            ],
            Code::E8008,
        ),
        (
            Limit::NestingDepth,
            3,
            &[(
                "main.weave",
                "rule a { rule b { rule c { rule d { e: f; } } } }",
            )],
            Code::E8002,
        ),
        (
            Limit::CallDepth,
            5,
            &[(
                "main.weave",
                "fn f(n) { when n == 0 { yield 0; } yield f(n - 1); }\nrule a { b: #{f(10)}; }",
            )],
            Code::E8003,
        ),
        (
            Limit::EvaluationSteps,
            100,
            &[(
                "main.weave",
                "rule a { each i in range(0, 1000) { --x: #{i}; } }",
            )],
            Code::E8004,
        ),
        (
            Limit::EmittedSelectors,
            3,
            &[("main.weave", "rule a, b { rule c, d { e: f; } }")],
            Code::E8005,
        ),
        (
            Limit::OutputBytes,
            10,
            &[("main.weave", "rule a { b: c; }\nrule d { e: f; }")],
            Code::E8006,
        ),
        (
            Limit::CollectionElements,
            2,
            &[("main.weave", "bind x = [1, 2, 3];")],
            Code::E8007,
        ),
        (
            Limit::StringBytes,
            4,
            &[("main.weave", "bind x = \"abc\" + \"def\";")],
            Code::E8007,
        ),
    ];
    for (limit, value, modules, code) in cases {
        let ok = build(modules, &CompileOptions::default());
        assert!(ok.outcome.is_ok(), "{limit}: fails with the default limits");
        let failed = build(modules, &limited(limit, value));
        let codes: Vec<Code> = match &failed.outcome {
            Ok(_) => Vec::new(),
            Err(diags) => diags.iter().map(|d| d.code()).collect(),
        };
        assert_eq!(codes, [code], "{limit} = {value}");
    }

    // The diagnostic limit caps what is stored; the build still fails.
    let many = "rule a { b: #{u1}; c: #{u2}; d: #{u3}; e: #{u4}; f: #{u5}; }";
    let all = codes(many);
    assert_eq!(all.len(), 5);
    let capped = codes_with(many, &limited(Limit::Diagnostics, 2));
    assert_eq!(capped, [Code::E1002, Code::E1002]);
}

/// Every prefix of a source rich in multi-byte characters, CRLF and
/// escapes: cuts inside a character are E0001 at the cut, everything else
/// compiles or fails with diagnostics whose positions resolve, and no
/// prefix panics.
#[test]
fn every_prefix_of_a_unicode_source_is_handled() {
    let source = "\u{feff}/* é中😀 */\r\nbind s = \"x\u{202e}y\";\r\nrule .\u{e9}\\1F600 { --v: \"😀\"; c: #{s}; d: \\41 b; }\r\n";
    let bytes = source.as_bytes();
    for cut in 0..=bytes.len() {
        let prefix = &bytes[..cut];
        let result = compile_bytes(prefix, &CompileOptions::default());
        let diags = match &result.outcome {
            Ok(output) => &output.warnings,
            Err(diags) => diags,
        };
        for diag in diags {
            if let Some(span) = diag.primary() {
                assert!(
                    result.sources.location(span).is_some(),
                    "cut {cut}: {diag:?}"
                );
            }
            assert!(
                !diag.render(&result.sources).contains('\u{202e}'),
                "cut {cut}"
            );
        }
        if std::str::from_utf8(prefix).is_err() {
            let codes: Vec<Code> = diags.iter().map(|d| d.code()).collect();
            assert_eq!(codes, [Code::E0001], "cut {cut}");
        }
    }
}

#[test]
fn malformed_encodings_are_reported_at_their_offset() {
    let cases: [(&[u8], u32); 6] = [
        (b"\xff", 0),
        (b"rule a { b: \xc0\x80; }", 12),         // overlong NUL
        (b"rule a { b: \xed\xa0\x80; }", 12),     // UTF-16 surrogate
        (b"rule a { b: \xf4\x90\x80\x80; }", 12), // above U+10FFFF
        (b"rule a { b: \xe4\xb8; }", 12),         // truncated sequence
        (b"\xef\xbb\xbfrule a { b: \x80; }", 15), // after a BOM
    ];
    for (bytes, offset) in cases {
        let result = compile_bytes(bytes, &CompileOptions::default());
        let Err(diags) = &result.outcome else {
            panic!("{bytes:?} compiled");
        };
        assert_eq!(diags.len(), 1, "{bytes:?}");
        assert_eq!(diags[0].code(), Code::E0001);
        assert_eq!(
            diags[0].primary().map(|s| s.start),
            Some(offset),
            "{bytes:?}"
        );
    }
    let nul = compile_bytes(b"rule a { b: \0; }", &CompileOptions::default());
    let codes: Vec<Code> = nul.outcome.unwrap_err().iter().map(|d| d.code()).collect();
    assert_eq!(codes, [Code::E0006]);
    // A file holding only a byte-order mark is an empty stylesheet.
    let bom = compile_bytes(b"\xef\xbb\xbf", &CompileOptions::default());
    assert_eq!(bom.outcome.unwrap().css, "");
}

#[test]
fn rendered_diagnostics_cannot_forge_lines_or_reorder_text() {
    // User text reaches messages through `fail` (and names and strings);
    // each item is the body of a Weave string literal.
    let hostile = [
        "\u{1b}[2J",
        "\u{1b}]0;title\u{7}",
        "\u{9b}31m",
        "line\u{2028}forged",
        "para\u{2029}forged",
        "\u{202e}desrever",
        "\u{2066}isolate\u{2069}",
        "\u{7f}",
        // CSS escapes for CR and LF.
        "cr\\d lf\\a error[E0000]: forged",
    ];
    let forbidden = |c: char| {
        c != '\n'
            && (c.is_control()
                || matches!(c, '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
    };
    for text in hostile {
        let source = format!("fail \"{text}\";\n");
        let result = compile_with(&source, &CompileOptions::default());
        let diags = result.outcome.as_ref().unwrap_err();
        assert_eq!(diags[0].code(), Code::E9001, "{text:?}");
        let rendered: String = diags.iter().map(|d| d.render(&result.sources)).collect();
        assert!(
            !rendered.chars().any(forbidden),
            "{text:?} rendered as {rendered:?}"
        );
        // One message line and one location line: nothing forged.
        assert_eq!(rendered.lines().count(), 2, "{rendered:?}");
    }
}
