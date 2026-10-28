//! Modules (LANGUAGE.md §14, CSS-COMPATIBILITY.md §5): resolution,
//! namespaces and exports, `load` placement, native at-rule ordering
//! across modules, cycles, traversal and emission order, the dependency
//! graph and the parsed-module cache.
//!
//! Every build here goes through [`MemoryResolver`] or a counting wrapper
//! around it, so none of these tests touches the filesystem. Filesystem
//! resolution (root confinement, symbolic links) is tested in the CLI
//! crate.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use std::collections::BTreeMap;

use weave_compiler::{
    Code, Compilation, CompileOptions, Diagnostic, Limit, LoadedModule, MemoryResolver,
    ModuleCache, ModuleId, ModuleResolver, ResolveError, Severity, compile, compile_with_cache,
};

fn id(path: &str) -> ModuleId {
    ModuleId::new(path).unwrap()
}

fn resolver(files: &[(&str, &str)]) -> MemoryResolver {
    let mut resolver = MemoryResolver::new();
    for (path, text) in files {
        resolver.insert(id(path), text.as_bytes());
    }
    resolver
}

/// Builds `main.weave` from `files`.
fn build_with(files: &[(&str, &str)], options: &CompileOptions) -> Compilation {
    compile(&id("main.weave"), &mut resolver(files), options)
}

fn build(files: &[(&str, &str)]) -> Compilation {
    build_with(files, &CompileOptions::default())
}

fn css(files: &[(&str, &str)]) -> String {
    let result = build(files);
    match result.outcome {
        Ok(output) => output.css,
        Err(diags) => panic!("{}", render(&result.sources, &diags)),
    }
}

fn render(sources: &weave_compiler::SourceManager, diags: &[Diagnostic]) -> String {
    diags.iter().map(|d| d.render(sources)).collect()
}

/// `(code, "file:line:column")` of every diagnostic of a failed build.
fn errors(files: &[(&str, &str)]) -> Vec<(Code, String)> {
    errors_of(&build(files))
}

fn errors_of(result: &Compilation) -> Vec<(Code, String)> {
    match &result.outcome {
        Ok(output) => panic!("expected errors, got CSS:\n{}", output.css),
        Err(diags) => diags
            .iter()
            .map(|d| {
                let at = d
                    .primary()
                    .and_then(|span| result.sources.location(span))
                    .map_or_else(String::new, |(file, lc)| format!("{file}:{lc}"));
                (d.code(), at)
            })
            .collect(),
    }
}

fn first_error(files: &[(&str, &str)]) -> Diagnostic {
    match build(files).outcome {
        Ok(output) => panic!("expected an error, got:\n{}", output.css),
        Err(diags) => diags.into_iter().next().unwrap(),
    }
}

fn e(code: Code, at: &str) -> (Code, String) {
    (code, at.to_string())
}

/// Wraps a resolver and counts every `resolve` and `load` call.
#[derive(Debug, Default)]
struct Counting {
    inner: MemoryResolver,
    loads: BTreeMap<String, u32>,
    resolves: u32,
}

impl ModuleResolver for Counting {
    fn resolve(&mut self, importer: &ModuleId, specifier: &str) -> Result<ModuleId, ResolveError> {
        self.resolves += 1;
        self.inner.resolve(importer, specifier)
    }

    fn load(&mut self, id: &ModuleId, max_bytes: u64) -> Result<LoadedModule, ResolveError> {
        *self.loads.entry(id.to_string()).or_default() += 1;
        self.inner.load(id, max_bytes)
    }
}

// ----- resolution ----------------------------------------------------------------

#[test]
fn paths_resolve_relative_to_the_importer() {
    let out = css(&[
        (
            "main.weave",
            "load \"./ui/button.weave\" as button;\nrule .m { a: #{button.size}; }",
        ),
        (
            "ui/button.weave",
            "load \"../tokens/size.weave\" as sizes;\nexport bind size = sizes.base * 2;",
        ),
        ("tokens/size.weave", "export bind base = 3px;"),
    ]);
    assert_eq!(out, ".m {\n  a: 6px;\n}\n");
}

#[test]
fn identity_is_the_normalized_path() {
    // Two spellings of one module, from two importers: one module,
    // evaluated and emitted once.
    let mut counting = Counting {
        inner: resolver(&[
            (
                "main.weave",
                "load \"./x.weave\" as x;\nload \"./sub/../x.weave\" as again;\nload \"./sub/s.weave\" as s;\nrule .m { a: #{x.v + again.v + s.v}; }",
            ),
            ("x.weave", "export bind v = 1;\nrule .x { b: c; }"),
            (
                "sub/s.weave",
                "load \"../x.weave\" as x;\nexport bind v = x.v;",
            ),
        ]),
        ..Counting::default()
    };
    let result = compile(&id("main.weave"), &mut counting, &CompileOptions::default());
    let output = result.outcome.unwrap();
    assert_eq!(output.css, ".x {\n  b: c;\n}\n\n.m {\n  a: 3;\n}\n");
    assert_eq!(counting.loads.get("x.weave"), Some(&1));
    assert_eq!(counting.resolves, 4);
    assert_eq!(
        output.dependencies,
        [id("main.weave"), id("x.weave"), id("sub/s.weave")]
    );
}

#[test]
fn invalid_missing_and_escaping_paths_are_reported_at_the_path() {
    for path in [
        "x.weave",
        "/x.weave",
        "https://example.com/x.weave",
        "./x.css",
        ".\\\\x.weave",
        "./x",
    ] {
        let source = format!("load \"{path}\" as x;");
        assert_eq!(
            errors(&[("main.weave", &source)]),
            [e(Code::E7004, "main.weave:1:6")],
            "{path}"
        );
    }
    assert_eq!(
        errors(&[("main.weave", "\n  load \"./nope.weave\" as x;")]),
        [e(Code::E7001, "main.weave:2:8")]
    );
    assert_eq!(
        errors(&[("main.weave", "load \"../up.weave\" as x;")]),
        [e(Code::E7006, "main.weave:1:6")]
    );
    // Inside a dependency, the importer's span is used.
    assert_eq!(
        errors(&[
            ("main.weave", "load \"./a/a.weave\" as a;"),
            ("a/a.weave", "load \"../../b.weave\" as b;"),
        ]),
        [e(Code::E7006, "a/a.weave:1:6")]
    );
    let diag = first_error(&[("main.weave", "load \"./nope.weave\" as x;")]);
    assert!(diag.message().contains("nope.weave"), "{}", diag.message());
}

#[test]
fn load_paths_are_not_interpolated() {
    assert_eq!(
        errors(&[("main.weave", "bind n = \"x\";\nload \"./#{n}.weave\" as x;")]),
        [
            e(Code::E7005, "main.weave:2:1"),
            e(Code::E7001, "main.weave:2:6")
        ]
    );
    let out = css(&[
        (
            "main.weave",
            "load \"./#{n}.weave\" as x;\nrule a { b: #{x.v}; }",
        ),
        ("#{n}.weave", "export bind v = 1;"),
    ]);
    assert_eq!(out, "a {\n  b: 1;\n}\n");
}

#[test]
fn the_virtual_resolver_needs_no_filesystem() {
    // A module path that exists on no disk; everything is in memory.
    let out = css(&[
        (
            "main.weave",
            "load \"./virtual/only.weave\" as v;\nrule a { b: #{v.x}; }",
        ),
        ("virtual/only.weave", "export bind x = \"in memory\";"),
    ]);
    assert_eq!(out, "a {\n  b: \"in memory\";\n}\n");
}

// ----- namespaces and exports ------------------------------------------------------

#[test]
fn exported_bindings_functions_and_recipes() {
    let out = css(&[
        (
            "main.weave",
            "load \"./lib.weave\" as lib;\nrule .m {\n  apply lib.box(2) { color: #{lib.palette.fg}; }\n  width: #{lib.twice(lib.gap)};\n}",
        ),
        (
            "lib.weave",
            "export bind gap = 4px;\nexport bind palette = {fg: #123456};\nbind private-unit = 1px;\nfn helper(x) { yield x + private-unit; }\nexport fn twice(x) { yield helper(x) * 2 - 2 * private-unit; }\nexport recipe box(n: number) { padding: #{gap * n}; slot; }",
        ),
    ]);
    assert_eq!(
        out,
        ".m {\n  padding: 8px;\n  color: #123456;\n  width: 8px;\n}\n"
    );
}

#[test]
fn private_and_missing_members_are_e7003() {
    let lib =
        "bind secret = 1;\nfn hidden() { yield 1; }\nrecipe inner { a: b; }\nexport bind open = 2;";
    for (use_, at) in [
        ("rule a { b: #{lib.secret}; }", "main.weave:2:19"),
        ("rule a { b: #{lib.hidden()}; }", "main.weave:2:15"),
        ("rule a { apply lib.inner; }", "main.weave:2:16"),
        ("rule a { b: #{lib.nothing}; }", "main.weave:2:19"),
    ] {
        let main = format!("load \"./lib.weave\" as lib;\n{use_}");
        let files = [("main.weave", main.as_str()), ("lib.weave", lib)];
        assert_eq!(errors(&files), [e(Code::E7003, at)], "{use_}");
        assert!(first_error(&files).message().contains("lib.weave"));
    }
    // A private member gets a hint about `export`.
    let main = "load \"./lib.weave\" as lib;\nrule a { b: #{lib.secret}; }";
    let diag = first_error(&[("main.weave", main), ("lib.weave", lib)]);
    assert!(diag.notes().iter().any(|n| n.contains("export")));
}

#[test]
fn namespaces_are_not_values_and_members_keep_their_kind() {
    let lib = "export bind v = 1;\nexport fn f() { yield 1; }\nexport recipe r { a: b; }";
    for (use_, code) in [
        ("rule a { b: #{lib}; }", Code::E1004),
        ("bind x = lib;", Code::E1004),
        ("rule a { b: #{lib.f}; }", Code::E1004),
        ("rule a { b: #{lib.v()}; }", Code::E1004),
        ("rule a { b: #{lib.r()}; }", Code::E1004),
        ("rule a { apply lib.f; }", Code::E1004),
        ("rule a { apply lib.v; }", Code::E1004),
        ("rule a { apply lib; }", Code::E1004),
        ("rule a { b: #{lib()}; }", Code::E1004),
        ("rule a { b: #{nope.v}; }", Code::E1002),
    ] {
        let main = format!("load \"./lib.weave\" as lib;\n{use_}");
        let codes: Vec<Code> = errors(&[("main.weave", &main), ("lib.weave", lib)])
            .into_iter()
            .map(|(c, _)| c)
            .collect();
        assert_eq!(codes, [code], "{use_}");
    }
}

#[test]
fn member_chains_continue_into_maps() {
    let out = css(&[
        (
            "main.weave",
            "load \"./t.weave\" as t;\nrule a { b: #{t.colors.brand.main}; c: #{t.colors[\"brand\"].alt}; }",
        ),
        (
            "t.weave",
            "export bind colors = {brand: {main: #ff0000, alt: #00ff00}};",
        ),
    ]);
    assert_eq!(out, "a {\n  b: #ff0000;\n  c: #00ff00;\n}\n");
}

#[test]
fn imported_bindings_are_read_only_names() {
    let lib = ("lib.weave", "export bind v = 1px;");
    // A namespace name cannot be rebound in module scope...
    assert_eq!(
        errors(&[
            ("main.weave", "load \"./lib.weave\" as lib;\nbind lib = 2;"),
            lib
        ]),
        [e(Code::E1001, "main.weave:2:6")]
    );
    assert_eq!(
        errors(&[
            (
                "main.weave",
                "load \"./lib.weave\" as lib;\nfn lib() { yield 1; }"
            ),
            lib
        ]),
        [e(Code::E1001, "main.weave:2:4")]
    );
    // ...nor loaded twice under one name.
    assert_eq!(
        errors(&[
            (
                "main.weave",
                "load \"./lib.weave\" as lib;\nload \"./other.weave\" as lib;"
            ),
            lib,
            ("other.weave", ""),
        ]),
        [e(Code::E1001, "main.weave:2:25")]
    );
    // Shadowing in a block is allowed and leaves the namespace intact.
    let out = css(&[
        (
            "main.weave",
            "load \"./lib.weave\" as lib;\nrule a { bind lib = 5; x: #{lib}; }\nrule b { y: #{lib.v}; }",
        ),
        lib,
    ]);
    assert_eq!(out, "a {\n  x: 5;\n}\n\nb {\n  y: 1px;\n}\n");
}

#[test]
fn one_module_under_two_names_is_evaluated_once() {
    let result = build(&[
        (
            "main.weave",
            "load \"./lib.weave\" as a;\nload \"./lib.weave\" as b;\nrule m { x: #{a.v == b.v}; }",
        ),
        (
            "lib.weave",
            "warn \"evaluated\";\nexport bind v = 1;\nrule .lib { y: z; }",
        ),
    ]);
    let output = result.outcome.unwrap();
    assert_eq!(output.css, ".lib {\n  y: z;\n}\n\nm {\n  x: true;\n}\n");
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(output.warnings[0].severity(), Severity::Warning);
}

#[test]
fn loaded_modules_cannot_see_their_importer() {
    assert_eq!(
        errors(&[
            (
                "main.weave",
                "load \"./lib.weave\" as lib;\nbind secret = 1;"
            ),
            ("lib.weave", "rule a { b: #{secret}; }"),
        ]),
        [e(Code::E1002, "lib.weave:1:15")]
    );
}

#[test]
fn definitions_run_in_their_own_module() {
    // `x` means different things in each module; functions, recipes and
    // defaults always use their definition module, content blocks the
    // caller's scope.
    let out = css(&[
        (
            "main.weave",
            "load \"./lib.weave\" as lib;\nbind x = 100;\nrule a {\n  f: #{lib.get()};\n  apply lib.show { mine: #{x}; }\n}",
        ),
        (
            "lib.weave",
            "bind x = 1;\nexport fn get(d = x) { yield d + x; }\nexport recipe show(v = x) { theirs: #{v}; slot; }",
        ),
    ]);
    assert_eq!(out, "a {\n  f: 2;\n  theirs: 1;\n  mine: 100;\n}\n");
}

#[test]
fn a_namespace_may_shadow_a_prelude_namespace() {
    let out = css(&[
        (
            "main.weave",
            "load \"./m.weave\" as math;\nrule a { b: #{math.round(1.4)}; }",
        ),
        ("m.weave", "export fn round(x) { yield \"mine\"; }"),
    ]);
    assert_eq!(out, "a {\n  b: \"mine\";\n}\n");
}

#[test]
fn export_marks_only_top_level_definitions() {
    assert_eq!(
        errors(&[("main.weave", "rule a { export bind x = 1; }")]),
        [e(Code::E0104, "main.weave:1:10")]
    );
    assert_eq!(
        errors(&[("main.weave", "export load \"./a.weave\" as a;")]),
        [e(Code::E0101, "main.weave:1:1")]
    );
    // An exported definition is still an ordinary local definition.
    assert_eq!(
        css(&[(
            "main.weave",
            "export bind w = 2px;\nexport fn f() { yield w; }\nrule a { b: #{f()}; }"
        )]),
        "a {\n  b: 2px;\n}\n"
    );
}

// ----- `load` placement ------------------------------------------------------------

#[test]
fn load_must_come_first() {
    assert_eq!(
        errors(&[
            ("main.weave", "rule a { b: c; }\nload \"./x.weave\" as x;"),
            ("x.weave", "")
        ]),
        [e(Code::E7005, "main.weave:2:1")]
    );
    for before in [
        "bind y = 1;",
        "fn f() { yield 1; }",
        "@layer a;",
        "when true { }",
    ] {
        let main = format!("{before}\nload \"./x.weave\" as x;");
        let codes: Vec<Code> = errors(&[("main.weave", &main), ("x.weave", "")])
            .into_iter()
            .map(|(c, _)| c)
            .collect();
        assert_eq!(codes, [Code::E7005], "{before}");
    }
    // Comments (kept or not) and an initial `@charset` may precede it.
    let out = css(&[
        (
            "main.weave",
            "@charset \"UTF-8\";\n/*! banner */\n/* note */\nload \"./x.weave\" as x;\nrule a { b: #{x.v}; }",
        ),
        ("x.weave", "export bind v = 1;"),
    ]);
    assert_eq!(
        out,
        "@charset \"UTF-8\";\n\n/*! banner */\n\na {\n  b: 1;\n}\n"
    );
    // `load` is a top-level statement.
    assert_eq!(
        errors(&[("main.weave", "rule a { load \"./x.weave\" as x; }")]),
        [e(Code::E0104, "main.weave:1:10")]
    );
}

// ----- native at-rule ordering -------------------------------------------------------

#[test]
fn charset_belongs_to_the_entry_and_is_emitted_first() {
    let out = css(&[
        (
            "main.weave",
            "@charset \"UTF-8\";\nload \"./d.weave\" as d;\nrule .m { a: b; }",
        ),
        ("d.weave", "rule .d { a: b; }"),
    ]);
    assert_eq!(
        out,
        "@charset \"UTF-8\";\n\n.d {\n  a: b;\n}\n\n.m {\n  a: b;\n}\n"
    );
    assert_eq!(
        errors(&[
            ("main.weave", "load \"./d.weave\" as d;"),
            ("d.weave", "@charset \"UTF-8\";\nrule .d { a: b; }"),
        ]),
        [e(Code::E6008, "d.weave:1:1")]
    );
}

#[test]
fn import_after_css_from_another_module_is_e7010() {
    // The fixture case: the entry's @import follows a dependency's rule.
    let result = build(&[
        (
            "main.weave",
            "load \"./dep.weave\" as dep;\n@import url(\"t.css\");",
        ),
        ("dep.weave", "rule .dep { a: b; }"),
    ]);
    assert_eq!(errors_of(&result), [e(Code::E7010, "main.weave:2:1")]);
    let Err(diags) = &result.outcome else {
        unreachable!()
    };
    let label = &diags[0].labels()[0];
    let (file, lc) = result.sources.location(label.span).unwrap();
    assert_eq!((file, lc.line, lc.column), ("dep.weave", 1, 1));
    assert!(label.message.contains("dep.weave"));

    // Between two dependencies, and for @namespace.
    assert_eq!(
        errors(&[
            (
                "main.weave",
                "load \"./a.weave\" as a;\nload \"./b.weave\" as b;"
            ),
            ("a.weave", "@media print { rule x { y: z; } }"),
            ("b.weave", "@namespace svg url(http://www.w3.org/2000/svg);"),
        ]),
        [e(Code::E7010, "b.weave:1:1")]
    );
    // CSS generated by a recipe in another module counts too.
    assert_eq!(
        errors(&[
            (
                "main.weave",
                "load \"./a.weave\" as a;\nload \"./b.weave\" as b;"
            ),
            ("a.weave", "recipe r { rule .gen { y: z; } }\napply r;"),
            ("b.weave", "@import \"x.css\";"),
        ]),
        [e(Code::E7010, "b.weave:1:1")]
    );
    // An @namespace in an earlier module blocks a later @import.
    assert_eq!(
        errors(&[
            ("main.weave", "load \"./a.weave\" as a;\n@import \"x.css\";"),
            ("a.weave", "@namespace url(x);"),
        ]),
        [e(Code::E7010, "main.weave:2:1")]
    );
}

#[test]
fn native_statements_that_stay_valid_are_kept_in_place() {
    // Dependencies that emit only @import, @layer statements, comments or
    // nothing do not block a later @import; nothing is reordered.
    let out = css(&[
        (
            "main.weave",
            "load \"./a.weave\" as a;\nload \"./b.weave\" as b;\n@import \"main.css\";\n@namespace url(x);\nrule .m { c: d; }",
        ),
        (
            "a.weave",
            "/*! a */\n@import \"a.css\";\n@layer base, theme;\nexport bind v = 1;",
        ),
        ("b.weave", "fn f() { yield 1; }"),
    ]);
    assert_eq!(
        out,
        "/*! a */\n\n@import \"a.css\";\n\n@layer base, theme;\n\n@import \"main.css\";\n\n@namespace url(x);\n\n.m {\n  c: d;\n}\n"
    );
    // A dependency's @import is fine when it is loaded first.
    let out = css(&[
        (
            "main.weave",
            "load \"./fonts.weave\" as f;\nrule .m { c: d; }",
        ),
        ("fonts.weave", "@import url(\"fonts.css\");"),
    ]);
    assert_eq!(out, "@import url(\"fonts.css\");\n\n.m {\n  c: d;\n}\n");
}

#[test]
fn opaque_and_unknown_at_rules_block_imports() {
    for dep in [
        "opaque { .x { y: z } }",
        "@font-feature-values Font { @swash { a: 1; } }",
        "@unknown-thing;",
    ] {
        let codes: Vec<Code> = errors(&[
            ("main.weave", "load \"./d.weave\" as d;\n@import \"x.css\";"),
            ("d.weave", dep),
        ])
        .into_iter()
        .map(|(c, _)| c)
        .collect();
        assert_eq!(codes, [Code::E7010], "{dep}");
    }
    // An opaque block that is exactly one comment is just a comment.
    assert_eq!(
        css(&[
            ("main.weave", "load \"./d.weave\" as d;\n@import \"x.css\";"),
            ("d.weave", "opaque { /* only a comment */ }"),
        ]),
        "/* only a comment */\n\n@import \"x.css\";\n"
    );
}

// ----- cycles ----------------------------------------------------------------------------

#[test]
fn cycles_report_the_full_chain_at_the_closing_load() {
    let result = build(&[
        ("main.weave", "load \"./a.weave\" as a;"),
        ("a.weave", "load \"./b.weave\" as b;"),
        ("b.weave", "/* x */\n  load \"./a.weave\" as a;"),
    ]);
    assert_eq!(errors_of(&result), [e(Code::E7002, "b.weave:2:3")]);
    let Err(diags) = &result.outcome else {
        unreachable!()
    };
    assert!(
        diags[0]
            .message()
            .contains("main.weave -> a.weave -> b.weave -> a.weave"),
        "{}",
        diags[0].message()
    );
    // Self-loads and cycles through the entry.
    assert_eq!(
        errors(&[("main.weave", "load \"./main.weave\" as me;")]),
        [e(Code::E7002, "main.weave:1:1")]
    );
    let diag = first_error(&[
        ("main.weave", "load \"./a.weave\" as a;"),
        ("a.weave", "load \"./main.weave\" as m;"),
    ]);
    assert!(
        diag.message()
            .contains("main.weave -> a.weave -> main.weave")
    );
}

#[test]
fn a_cycle_does_not_hide_independent_errors() {
    let codes: Vec<Code> = errors(&[
        (
            "main.weave",
            "load \"./a.weave\" as a;\nload \"./missing.weave\" as m;",
        ),
        (
            "a.weave",
            "load \"./main.weave\" as m;\nrule x { y: #{1px + 1em}; }",
        ),
    ])
    .into_iter()
    .map(|(c, _)| c)
    .collect();
    // Evaluation does not start after loading failed.
    assert_eq!(codes, [Code::E7002, Code::E7001]);
}

#[test]
fn parse_errors_in_several_modules_are_all_reported() {
    assert_eq!(
        errors(&[
            (
                "main.weave",
                "load \"./a.weave\" as a;\nload \"./b.weave\" as b;\n.x { }"
            ),
            ("a.weave", "bind = 1;"),
            ("b.weave", "rule y { z: #{1 +}; }"),
        ])
        .into_iter()
        .map(|(_, at)| at.split(':').next().unwrap().to_string())
        .collect::<Vec<_>>(),
        ["main.weave", "a.weave", "b.weave"]
    );
}

// ----- traversal and emission order -----------------------------------------------------

/// The diamond of CSS-COMPATIBILITY.md §5.1.
fn diamond(main: &str) -> Vec<(&'static str, String)> {
    vec![
        ("main.weave", main.to_string()),
        (
            "a.weave",
            "load \"./c.weave\" as c;\nrule .a { x: y; }".to_string(),
        ),
        (
            "b.weave",
            "load \"./c.weave\" as c;\nrule .b { x: y; }".to_string(),
        ),
        ("c.weave", "rule .c { x: y; }".to_string()),
    ]
}

fn names(css: &str) -> Vec<&str> {
    css.lines().filter_map(|l| l.strip_suffix(" {")).collect()
}

#[test]
fn diamond_dependencies_emit_once_in_post_order() {
    let files = diamond(
        "load \"./a.weave\" as a;\nload \"./b.weave\" as b;\nload \"./c.weave\" as c;\nrule .main { x: y; }",
    );
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let mut counting = Counting {
        inner: resolver(&borrowed),
        ..Counting::default()
    };
    let result = compile(&id("main.weave"), &mut counting, &CompileOptions::default());
    let output = result.outcome.unwrap();
    assert_eq!(names(&output.css), [".c", ".a", ".b", ".main"]);
    assert!(
        counting.loads.values().all(|&n| n == 1),
        "{:?}",
        counting.loads
    );
    assert_eq!(
        result.graph.emission_order(),
        [
            &id("c.weave"),
            &id("a.weave"),
            &id("b.weave"),
            &id("main.weave")
        ]
    );
    // Repeated builds are identical.
    let again = compile(
        &id("main.weave"),
        &mut resolver(&borrowed),
        &CompileOptions::default(),
    );
    assert_eq!(again.outcome.unwrap().css, output.css);
}

#[test]
fn emission_order_follows_load_order() {
    let files = diamond("load \"./b.weave\" as b;\nload \"./a.weave\" as a;\nrule .main { x: y; }");
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    assert_eq!(names(&css(&borrowed)), [".c", ".b", ".a", ".main"]);
    // Loading c first moves it, and only it.
    let files = diamond(
        "load \"./c.weave\" as c;\nload \"./b.weave\" as b;\nload \"./a.weave\" as a;\nrule .main { x: y; }",
    );
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    assert_eq!(names(&css(&borrowed)), [".c", ".b", ".a", ".main"]);
}

#[test]
fn deep_chains_use_no_recursion() {
    let depth = 3000;
    let mut files: Vec<(String, String)> = (0..depth)
        .map(|i| {
            let next = if i + 1 < depth {
                format!(
                    "load \"./m{}.weave\" as next;\nexport bind v = next.v + 1;",
                    i + 1
                )
            } else {
                "export bind v = 0;".to_string()
            };
            (format!("m{i}.weave"), next)
        })
        .collect();
    files.push((
        "main.weave".to_string(),
        "load \"./m0.weave\" as m;\nrule a { b: #{m.v}; }".to_string(),
    ));
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    let mut resolver = resolver(&borrowed);
    let entry = id("main.weave");
    // A small thread stack: traversal and evaluation must not recurse per
    // module.
    let out = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            compile(&entry, &mut resolver, &CompileOptions::default())
                .outcome
                .map(|o| o.css)
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(out.unwrap(), format!("a {{\n  b: {};\n}}\n", depth - 1));
}

#[test]
fn module_and_input_limits_apply_across_modules() {
    let files = [
        (
            "main.weave",
            "load \"./a.weave\" as a;\nload \"./b.weave\" as b;",
        ),
        (
            "a.weave",
            "/* a dependency longer than the entry module itself */",
        ),
        (
            "b.weave",
            "/* b dependency longer than the entry module itself */",
        ),
    ];
    let options = |limit, value| {
        let mut options = CompileOptions::default();
        options.limits.set(limit, value).unwrap();
        options
    };
    assert_eq!(
        errors_of(&build_with(&files, &options(Limit::Modules, 2))),
        [e(Code::E8008, "main.weave:2:6")]
    );
    assert!(
        build_with(&files, &options(Limit::Modules, 3))
            .outcome
            .is_ok()
    );
    let total = files.iter().map(|(_, t)| t.len() as u64).sum::<u64>();
    assert!(
        build_with(&files, &options(Limit::TotalInputBytes, total))
            .outcome
            .is_ok()
    );
    assert_eq!(
        errors_of(&build_with(
            &files,
            &options(Limit::TotalInputBytes, total - 1)
        )),
        [e(Code::E8001, "main.weave:2:6")]
    );
    assert_eq!(
        errors_of(&build_with(&files, &options(Limit::FileBytes, 50))),
        [
            e(Code::E8001, "main.weave:1:6"),
            e(Code::E8001, "main.weave:2:6")
        ]
    );
}

#[test]
fn evaluation_limits_are_shared_by_all_modules() {
    // Output from every module counts toward one output limit.
    let files = [
        ("main.weave", "load \"./a.weave\" as a;\nrule .m { x: y; }"),
        ("a.weave", "rule .a { x: y; }"),
    ];
    let full = css(&files).len() as u64;
    let mut options = CompileOptions::default();
    options.limits.set(Limit::OutputBytes, full).unwrap();
    assert!(build_with(&files, &options).outcome.is_ok());
    options.limits.set(Limit::OutputBytes, full - 1).unwrap();
    let codes: Vec<Code> = errors_of(&build_with(&files, &options))
        .into_iter()
        .map(|(c, _)| c)
        .collect();
    assert_eq!(codes, [Code::E8006]);
}

// ----- dependency graph ---------------------------------------------------------------------

#[test]
fn the_dependency_graph_records_edges() {
    let files =
        diamond("load \"./a.weave\" as a;\nload \"./b.weave\" as b;\nload \"./a.weave\" as a2;");
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let graph = build(&borrowed).graph;
    let (main, a, b, c) = (
        id("main.weave"),
        id("a.weave"),
        id("b.weave"),
        id("c.weave"),
    );
    assert_eq!(
        graph.modules(),
        [main.clone(), a.clone(), c.clone(), b.clone()]
    );
    assert_eq!(graph.dependencies(&main), [&a, &b]);
    assert_eq!(graph.dependencies(&a), [&c]);
    assert!(graph.dependencies(&c).is_empty());
    assert_eq!(graph.dependents(&c), [&a, &b]);
    assert_eq!(graph.affected_by(&c), [&main, &a, &c, &b]);
    assert_eq!(graph.affected_by(&b), [&main, &b]);
    assert_eq!(graph.affected_by(&main), [&main]);
    assert!(graph.affected_by(&id("other.weave")).is_empty());
    assert!(graph.contains(&c) && !graph.contains(&id("other.weave")));
}

#[test]
fn a_failed_build_keeps_the_discovered_graph() {
    let result = build(&[
        (
            "main.weave",
            "load \"./a.weave\" as a;\nload \"./gone.weave\" as g;",
        ),
        ("a.weave", "rule x { y: #{1px + 1em}; }"),
    ]);
    assert!(result.outcome.is_err());
    assert_eq!(result.graph.modules(), [id("main.weave"), id("a.weave")]);
}

// ----- parsed-module cache ------------------------------------------------------------------

const MAIN: &str =
    "load \"./a.weave\" as a;\nload \"./b.weave\" as b;\nrule .m { v: #{a.v + b.v}; }";

#[test]
fn unchanged_modules_reuse_their_parse() {
    let mut cache = ModuleCache::new();
    let mut files = resolver(&[
        ("main.weave", MAIN),
        ("a.weave", "export bind v = 1;"),
        ("b.weave", "export bind v = 2;"),
    ]);
    let options = CompileOptions::default();
    let first = compile_with_cache(&id("main.weave"), &mut files, &options, &mut cache);
    assert_eq!(first.outcome.unwrap().css, ".m {\n  v: 3;\n}\n");
    assert_eq!((cache.stats().hits, cache.stats().misses), (0, 3));
    assert_eq!(cache.len(), 3);

    let second = compile_with_cache(&id("main.weave"), &mut files, &options, &mut cache);
    assert_eq!(second.outcome.unwrap().css, ".m {\n  v: 3;\n}\n");
    assert_eq!((cache.stats().hits, cache.stats().misses), (3, 3));
}

#[test]
fn a_changed_dependency_is_never_served_stale() {
    let mut cache = ModuleCache::new();
    let mut files = resolver(&[
        ("main.weave", MAIN),
        ("a.weave", "export bind v = 1;"),
        ("b.weave", "export bind v = 2;"),
    ]);
    let options = CompileOptions::default();
    let entry = id("main.weave");
    let _ = compile_with_cache(&entry, &mut files, &options, &mut cache);

    // Same length, different contents, no invalidation call: the cache
    // must notice by itself.
    files.insert(id("b.weave"), "export bind v = 7;");
    let rebuilt = compile_with_cache(&entry, &mut files, &options, &mut cache);
    assert_eq!(rebuilt.outcome.unwrap().css, ".m {\n  v: 8;\n}\n");
    assert_eq!((cache.stats().hits, cache.stats().misses), (2, 4));

    // A dependency that gains a load of its own changes file positions;
    // everything still compiles from current sources.
    files.insert(
        id("a.weave"),
        "load \"./c.weave\" as c;\nexport bind v = c.v;",
    );
    files.insert(id("c.weave"), "export bind v = 10;");
    let rebuilt = compile_with_cache(&entry, &mut files, &options, &mut cache);
    assert_eq!(rebuilt.outcome.unwrap().css, ".m {\n  v: 17;\n}\n");
    assert_eq!(
        rebuilt.graph.affected_by(&id("c.weave")),
        [&entry, &id("a.weave"), &id("c.weave")]
    );

    // A module that becomes invalid is reported, not served from cache.
    files.insert(id("b.weave"), "export bind v = ;");
    let broken = compile_with_cache(&entry, &mut files, &options, &mut cache);
    assert!(broken.outcome.is_err());
    assert!(!cache.contains(&id("b.weave")));
}

#[test]
fn explicit_invalidation_hooks() {
    let mut cache = ModuleCache::new();
    let mut files = resolver(&[
        ("main.weave", MAIN),
        ("a.weave", "export bind v = 1;"),
        ("b.weave", "export bind v = 2;"),
    ]);
    let options = CompileOptions::default();
    let entry = id("main.weave");
    let _ = compile_with_cache(&entry, &mut files, &options, &mut cache);
    assert!(cache.invalidate(&id("a.weave")));
    assert!(!cache.invalidate(&id("a.weave")));
    let _ = compile_with_cache(&entry, &mut files, &options, &mut cache);
    assert_eq!((cache.stats().hits, cache.stats().misses), (2, 4));
    cache.clear();
    assert!(cache.is_empty());
    let _ = compile_with_cache(&entry, &mut files, &options, &mut cache);
    assert_eq!((cache.stats().hits, cache.stats().misses), (2, 7));

    // A cached module is not reused in a different role: as an entry it
    // may start with `@charset`, as a dependency it may not.
    let mut cache = ModuleCache::new();
    let mut files = resolver(&[
        ("main.weave", "load \"./lib.weave\" as l;"),
        ("lib.weave", "@charset \"UTF-8\";"),
    ]);
    assert!(
        compile_with_cache(&id("lib.weave"), &mut files, &options, &mut cache)
            .outcome
            .is_ok()
    );
    let as_dependency = compile_with_cache(&entry, &mut files, &options, &mut cache);
    assert_eq!(errors_of(&as_dependency), [e(Code::E6008, "lib.weave:1:1")]);
}

#[test]
fn a_changed_nesting_limit_reparses() {
    let mut cache = ModuleCache::new();
    let mut files = resolver(&[("main.weave", "rule a { rule b { rule c { d: e; } } }")]);
    let entry = id("main.weave");
    assert!(
        compile_with_cache(&entry, &mut files, &CompileOptions::default(), &mut cache)
            .outcome
            .is_ok()
    );
    let mut options = CompileOptions::default();
    options.limits.set(Limit::NestingDepth, 2).unwrap();
    let result = compile_with_cache(&entry, &mut files, &options, &mut cache);
    let codes: Vec<Code> = errors_of(&result).into_iter().map(|(c, _)| c).collect();
    assert_eq!(codes, [Code::E8002]);
}
