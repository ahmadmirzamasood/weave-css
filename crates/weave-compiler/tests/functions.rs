//! Functions, control flow, recipes and built-ins through the public API
//! (phase 05): limits at their exact boundaries, stack safety at the
//! largest call depth, expansion bombs that must fail gracefully,
//! warnings, the locations diagnostics point at, and the agreement between
//! docs/BUILTINS.md and the implemented prelude.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use std::time::{Duration, Instant};

use weave_compiler::{
    Code, Compilation, CompileOptions, Diagnostic, Limit, MemoryResolver, ModuleId, Severity,
    compile,
};

fn compile_with(source: &str, options: &CompileOptions) -> Compilation {
    let mut resolver = MemoryResolver::new();
    let entry = ModuleId::new("main.weave").unwrap();
    resolver.insert(entry.clone(), source.as_bytes());
    compile(&entry, &mut resolver, options)
}

fn css_with(source: &str, options: &CompileOptions) -> String {
    let result = compile_with(source, options);
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

fn css(source: &str) -> String {
    css_with(source, &CompileOptions::default())
}

fn diagnostics(source: &str, options: &CompileOptions) -> Vec<Diagnostic> {
    match compile_with(source, options).outcome {
        Ok(_) => Vec::new(),
        Err(diags) => diags,
    }
}

fn codes_with(source: &str, options: &CompileOptions) -> Vec<Code> {
    diagnostics(source, options)
        .iter()
        .filter(|d| d.is_error())
        .map(Diagnostic::code)
        .collect()
}

fn codes(source: &str) -> Vec<Code> {
    codes_with(source, &CompileOptions::default())
}

fn limits(pairs: &[(Limit, u64)]) -> CompileOptions {
    let mut options = CompileOptions::default();
    for &(limit, value) in pairs {
        options.limits.set(limit, value).unwrap();
    }
    options
}

/// Runs `f` on a thread with a deliberately small stack.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

/// Asserts that compiling finished within a generous time budget, so a
/// bomb that the limits failed to stop shows up as a failure, not a hang.
fn quickly<T>(f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = f();
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "took {:?}",
        start.elapsed()
    );
    result
}

// ----- functions -------------------------------------------------------------------

#[test]
fn arguments_bind_left_to_right_with_defaults() {
    let source = "fn f(a, b = a * 2, c = a + b) { yield css.space([a, b, c]); }";
    let call = |args: &str| {
        let out = css(&format!("{source} rule a {{ p: #{{f({args})}}; }}"));
        out.lines().nth(1).unwrap().trim().to_string()
    };
    assert_eq!(call("1"), "p: 1 2 3;");
    assert_eq!(call("1, 5"), "p: 1 5 6;");
    assert_eq!(call("c: 0, a: 2"), "p: 2 4 0;");
    // A default sees only the parameters before it.
    assert_eq!(
        codes("fn f(a = b, b = 1) { yield a; } bind x = f();"),
        [Code::E1003]
    );
}

#[test]
fn functions_only_see_their_module() {
    // A caller's local binding is invisible; the module binding is used.
    let source = "bind size = 1px;
        fn get() { yield size; }
        rule a { bind size = 2px; p: #{get()} #{size}; }";
    assert_eq!(css(source), "a {\n  p: 1px 2px;\n}\n");
    // Module bindings are read when the call runs.
    assert_eq!(
        codes("fn get() { yield later; } bind x = get(); bind later = 1;"),
        [Code::E1003]
    );
    // An unknown name inside a function is unresolved, not dynamically
    // scoped to the caller.
    assert_eq!(
        codes("fn get() { yield local; } rule a { bind local = 1; p: #{get()}; }"),
        [Code::E1002]
    );
}

#[test]
fn short_circuit_never_calls_the_skipped_function() {
    let source = "fn boom() -> bool { fail \"called\"; }
        rule a { p: #{false && boom()} #{true || boom()}; }";
    assert_eq!(css(source), "a {\n  p: false true;\n}\n");
    // Only the selected branch runs, so its errors cannot occur.
    let source = "fn pick(x) {
            when x == 0 { yield \"zero\"; }
            otherwise when 1 / x > 0 { yield \"positive\"; }
            otherwise { yield \"negative\"; }
        }
        rule a { p: #{pick(0)} #{pick(2)} #{pick(-2)}; }";
    assert_eq!(
        css(source),
        "a {\n  p: \"zero\" \"positive\" \"negative\";\n}\n"
    );
}

#[test]
fn functions_are_not_values() {
    assert_eq!(codes("fn f() { yield 1; } bind x = f;"), [Code::E1004]);
    assert_eq!(codes("recipe r { } bind x = r;"), [Code::E1004]);
    assert_eq!(codes("bind v = 1; bind x = v();"), [Code::E1004]);
}

#[test]
fn user_definitions_shadow_prelude_names() {
    let out = css("fn range(n) { yield \"mine\"; } rule a { p: #{range(3)}; }");
    assert_eq!(out, "a {\n  p: \"mine\";\n}\n");
}

// ----- limits -----------------------------------------------------------------------

#[test]
fn call_depth_limit_is_exact() {
    let source = |depth: u32| {
        format!(
            "fn down(n) {{ when n == 0 {{ yield 0; }} yield down(n - 1); }} bind x = down({});",
            depth - 1
        )
    };
    let options = limits(&[(Limit::CallDepth, 10)]);
    // `down(9)` makes 10 nested calls.
    assert_eq!(codes_with(&source(10), &options), []);
    assert_eq!(codes_with(&source(11), &options), [Code::E8003]);

    let recipes = |depth: u32| {
        format!(
            "recipe r(n) {{ when n > 0 {{ apply r(n - 1); }} }} rule a {{ apply r({}); }}",
            depth - 1
        )
    };
    assert_eq!(codes_with(&recipes(10), &options), []);
    assert_eq!(codes_with(&recipes(11), &options), [Code::E8003]);
}

#[test]
fn deepest_recursion_does_not_use_the_stack() {
    // The largest call-depth limit on a 512 KiB thread: calls are tasks on
    // an explicit stack, not Rust frames.
    let results = on_small_stack(|| {
        let options = limits(&[(Limit::CallDepth, 4096)]);
        let mut results = Vec::new();
        // Recursion through arguments, a template inside a string and a
        // loop body.
        let source = "fn down(n) {
                when n == 0 { yield \"\"; }
                each unused in [1] { bind s = \"#{down(n - 1)}\"; yield s; }
            }
            bind x = down(4095);";
        results.push(codes_with(source, &options));
        // Recursive recipes with content blocks passed down 4095 levels:
        // every content block keeps the scope chain of its caller.
        let source = "recipe r(n) {
                when n > 0 { apply r(n - 1) { slot; } } otherwise { slot; }
            }
            rule a { apply r(4095) { color: red; } }";
        results.push(codes_with(source, &options));
        // One more level is reported, not a crash.
        results.push(codes_with(
            "fn f(n) { yield f(n + 1); } bind x = f(0);",
            &options,
        ));
        results
    });
    assert_eq!(results, [vec![], vec![], vec![Code::E8003]]);
}

#[test]
fn step_limit_counts_statements_and_iterations() {
    // `each` over 1000 elements with an empty body is bounded by its
    // iterations, not by expression operations.
    let source = "each i in range(0, 1000) { }";
    assert_eq!(
        codes_with(source, &limits(&[(Limit::EvaluationSteps, 2000)])),
        []
    );
    assert_eq!(
        codes_with(source, &limits(&[(Limit::EvaluationSteps, 900)])),
        [Code::E8004]
    );
}

#[test]
fn expansion_bombs_fail_gracefully() {
    quickly(|| {
        // Exponential recipe expansion.
        let source = "recipe r(n) { when n > 0 { apply r(n - 1); apply r(n - 1); } color: red; }
            rule a { apply r(60); }";
        assert_eq!(codes(source), [Code::E8004]);
        // Exponential function recursion.
        let source = "fn f(n) { when n == 0 { yield 1; } yield f(n - 1) + f(n - 1); }
            bind x = f(80);";
        assert_eq!(codes(source), [Code::E8004]);
        // Nested loops multiplying their iterations.
        let source = "each a in range(0, 1000) { each b in range(0, 1000) { each c in range(0, 1000) { } } }";
        assert_eq!(codes(source), [Code::E8004]);
    });
}

#[test]
fn output_limit_stops_expansion_early() {
    quickly(|| {
        // Each rule adds a 64 KiB declaration: the output limit stops the
        // expansion as soon as the CSS produced passes it.
        let options = limits(&[
            (Limit::OutputBytes, 1024 * 1024),
            (Limit::EvaluationSteps, u64::MAX),
        ]);
        let source = "fn grow(s, n) { when n == 0 { yield s; } yield grow(s + s, n - 1); }
            bind chunk = grow(\"x\", 16);
            each i in range(0, 65536) { rule .r#{i} { content: #{chunk}; } }";
        assert_eq!(codes_with(source, &options), [Code::E8006]);
    });
}

#[test]
fn collections_and_strings_are_bounded_before_allocation() {
    quickly(|| {
        // A huge range is rejected before it is built.
        assert_eq!(codes("bind r = range(0, 1000000000000);"), [Code::E8007]);
        assert_eq!(
            codes("bind r = range(1000000000000, -1000000000000, -1);"),
            [Code::E8007]
        );
        // Doubling a string hits the string limit, not memory.
        let source = "fn grow(s) { yield grow(s + s); } bind x = grow(\"ab\");";
        assert_eq!(codes(source), [Code::E8007]);
        // Appending in a loop hits the collection limit.
        let options = limits(&[(Limit::CollectionElements, 100)]);
        let source = "fn fill(l) { yield fill(list.append(l, 0)); } bind x = fill([]);";
        assert_eq!(codes_with(source, &options), [Code::E8007]);
        assert_eq!(
            codes_with(
                "bind m = map.merge({}, {}); bind r = range(0, 101);",
                &options
            ),
            [Code::E8007]
        );
    });
}

#[test]
fn comparison_bombs_are_bounded() {
    // Distinct lists that share children make `==` exponential without a
    // budget: each level doubles the pairs to compare.
    let mut source = String::from("bind a0 = [1]; bind b0 = [1];");
    for i in 1..=60 {
        let p = i - 1;
        source.push_str(&format!(
            " bind a{i} = [a{p}, a{p}]; bind b{i} = [b{p}, b{p}];"
        ));
    }
    source.push_str(" bind same = a60 == b60; bind found = list.contains([a60], b60);");
    let diags = quickly(|| codes(&source));
    assert_eq!(diags, [Code::E8004]);
    // Shared collections compare without being visited.
    let shared = "bind a = range(0, 60000); bind l = [a, a, a]; bind same = l == [a, a, a];
        rule x { p: #{same}; }";
    assert_eq!(css(shared), "x {\n  p: true;\n}\n");
}

// ----- recipes ----------------------------------------------------------------------

#[test]
fn recipe_expansion_order_is_deterministic() {
    let source = "recipe trio(prefix: string) {
            each i in range(0, 3) { --v-#{\"#{prefix}-#{i}\"}: #{i}; }
            slot;
        }
        rule a {
            --first: 0;
            apply trio(\"x\") { apply trio(\"y\"); }
            --last: 1;
        }";
    let expected = "a {
  --first: 0;
  --v-x-0: 0;
  --v-x-1: 1;
  --v-x-2: 2;
  --v-y-0: 0;
  --v-y-1: 1;
  --v-y-2: 2;
  --last: 1;
}
";
    for _ in 0..3 {
        assert_eq!(css(source), expected);
    }
}

#[test]
fn content_blocks_use_the_callers_scope() {
    let source = "bind tone = \"module\";
        recipe wrap(tone: string = \"recipe\") { --inner: #{tone}; slot; }
        rule a {
            bind tone = \"caller\";
            apply wrap { --content: #{tone}; }
        }";
    assert_eq!(
        css(source),
        "a {\n  --inner: \"recipe\";\n  --content: \"caller\";\n}\n"
    );
    // The recipe body cannot see the caller's bindings.
    assert_eq!(
        codes("recipe r { --x: #{secret}; } rule a { bind secret = 1; apply r; }"),
        [Code::E1002]
    );
}

#[test]
fn slot_misuse_is_reported_once_per_site() {
    // A slot in a loop of 100 iterations is reported once.
    let source = "recipe many { each i in range(0, 100) { slot; } }
        rule a { apply many { color: red; } }";
    assert_eq!(codes(source), [Code::E4011]);
    // A failure inside the recipe does not add E4010 on top.
    let source = "recipe broken { bind x = 1 / 0; slot; }
        rule a { apply broken { color: red; } }";
    assert_eq!(codes(source), [Code::E3003]);
}

#[test]
fn recipes_emitting_rules_respect_import_order() {
    let source = "recipe make { rule .x { color: red; } }
        apply make;
        @import \"later.css\";";
    assert_eq!(codes(source), [Code::E7010]);
    // Declarations-only or empty expansions do not block `@import`.
    let source = "recipe nothing { }
        apply nothing;
        @import \"fine.css\";";
    assert_eq!(css(source), "@import \"fine.css\";\n");
}

// ----- fail and warn ----------------------------------------------------------------

#[test]
fn warnings_do_not_fail_the_build() {
    let options = limits(&[(Limit::Diagnostics, 5)]);
    let source = "each i in range(0, 20) { warn \"step #{i}\"; }
        rule a { color: red; }";
    let result = compile_with(source, &options);
    let output = result.outcome.expect("warnings must not fail the build");
    assert_eq!(output.css, "a {\n  color: red;\n}\n");
    assert_eq!(output.warnings.len(), 5);
    assert!(
        output
            .warnings
            .iter()
            .all(|w| w.severity() == Severity::Warning && w.code() == Code::W9001)
    );
    assert_eq!(output.warnings[0].message(), "step 0");
}

#[test]
fn fail_reports_the_message() {
    let diags = diagnostics(
        "fn check(n) { when n < 0 { fail \"negative: #{n}\"; } yield n; } bind x = check(-3);",
        &CompileOptions::default(),
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), Code::E9001);
    assert_eq!(diags[0].message(), "negative: -3");
}

// ----- locations ------------------------------------------------------------------

#[test]
fn diagnostics_point_at_definitions_and_call_sites() {
    let source = "fn half(x: length) -> length { yield x / 0; }\nbind h = half(4px);\n";
    let result = compile_with(source, &CompileOptions::default());
    let Err(diags) = &result.outcome else {
        panic!("expected an error")
    };
    let rendered = diags[0].render(&result.sources);
    // The division inside the function, then the call that led there.
    assert!(rendered.contains("main.weave:1:40"), "{rendered}");
    assert!(rendered.contains("main.weave:2:10"), "{rendered}");
    assert!(rendered.contains("in a call to `half`"), "{rendered}");

    // Type errors label the parameter declaration.
    let source = "recipe pad(size: length) { padding: #{size}; }\nrule a { apply pad(1); }\n";
    let result = compile_with(source, &CompileOptions::default());
    let Err(diags) = &result.outcome else {
        panic!("expected an error")
    };
    let rendered = diags[0].render(&result.sources);
    assert!(rendered.contains("main.weave:2:20"), "{rendered}");
    assert!(rendered.contains("main.weave:1:12"), "{rendered}");
    assert!(rendered.contains("parameter declared here"), "{rendered}");
}

// ----- built-ins ------------------------------------------------------------------

#[test]
fn builtin_errors() {
    for (source, code) in [
        ("bind x = range(0, 1.5);", Code::E2009),
        ("bind x = range(0px, 3);", Code::E2009),
        ("bind x = range(0, 3, 0);", Code::E3003),
        ("bind x = list.slice([1, 2], 1, 3);", Code::E2005),
        ("bind x = list.slice([1, 2], 2, 1);", Code::E2005),
        ("bind x = string.slice(\"ab\", -1, 1);", Code::E2005),
        ("bind x = string.join([\"a\", 1], \",\");", Code::E4005),
        ("bind x = math.min(1px, 1em);", Code::E3001),
        ("bind x = math.clamp(3, 2, 1);", Code::E2003),
        ("bind x = math.convert(1px, \"em\");", Code::E3005),
        ("bind x = math.convert(1, \"px\");", Code::E3005),
        ("bind x = math.convert(1px, \"furlong\");", Code::E3005),
        (
            "bind x = math.convert(1000000000000in, \"px\");",
            Code::E3002,
        ),
        ("bind x = map.get({}, 1, 0);", Code::E4005),
        ("bind x = list.len(\"abc\");", Code::E4005),
        ("bind x = color.rgb(1, 2, 3);", Code::E5001),
        ("bind x = math.round();", Code::E4001),
        ("bind x = meta.type(1, 2);", Code::E4004),
    ] {
        assert_eq!(codes(source), [code], "{source}");
    }
}

/// Every implemented built-in is documented in docs/BUILTINS.md, and every
/// built-in documented as implemented exists.
#[test]
fn builtins_reference_matches_the_prelude() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/BUILTINS.md"),
    )
    .expect("docs/BUILTINS.md");
    // Documented signatures are level-3 headings: ### `name(params)`.
    let documented: Vec<&str> = doc
        .lines()
        .filter_map(|l| l.strip_prefix("### `"))
        .filter_map(|l| l.split('(').next())
        .collect();
    assert!(documented.len() >= 36, "{documented:?}");
    for name in &documented {
        let probe = format!("bind x = {name}();");
        let found = codes(&probe);
        assert!(
            !found.contains(&Code::E7003) && !found.contains(&Code::E1002),
            "{name} is documented but not implemented: {found:?}"
        );
        assert!(
            !found.contains(&Code::E5001),
            "{name} is documented as implemented but reports E5001"
        );
    }
    for namespace in ["css", "math", "list", "map", "string", "meta"] {
        for member in [
            "space", "comma", "slash", "ident", "fragment", "round", "floor", "ceil", "abs", "min",
            "max", "clamp", "unit", "value", "convert", "len", "append", "concat", "slice",
            "contains", "has", "get", "set", "remove", "merge", "keys", "values", "upper", "lower",
            "join", "type",
        ] {
            let name = format!("{namespace}.{member}");
            if !codes(&format!("bind x = {name}();")).contains(&Code::E7003) {
                assert!(
                    documented.contains(&name.as_str()),
                    "{name} is not documented"
                );
            }
        }
    }
}
