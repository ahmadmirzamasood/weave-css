//! Phase 08 command-line tools: pipes, output files, collisions, the module
//! root, JSON diagnostics, dependency lists, limits and source maps.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn weave_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_weavec"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("failed to run weave")
}

fn weave_stdin(dir: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_weavec"))
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run weave");
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("UTF-8 output")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("tools-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn stdin_to_stdout_pipe() {
    let dir = scratch("pipe");
    fs::write(dir.join("t.weave"), "export bind g = 3px;\n").unwrap();
    let out = weave_stdin(
        &dir,
        &["build", "-"],
        b"load \"./t.weave\" as t;\nrule a { gap: #{t.g}; }\n",
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "a {\n  gap: 3px;\n}\n");
    assert!(out.stderr.is_empty());

    // `-o -` is stdout too; errors name the stdin module.
    let out = weave_stdin(&dir, &["check", "-"], b"rule a { b: #{x}; }");
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(
        text(&out.stderr).contains("--> <stdin>.weave:1:15"),
        "{}",
        text(&out.stderr)
    );

    // Loads from stdin cannot leave the root.
    let out = weave_stdin(&dir, &["check", "-"], b"load \"../t.weave\" as t;");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).starts_with("error[E7006]"),
        "{}",
        text(&out.stderr)
    );

    // An explicit root moves the base of stdin loads.
    fs::create_dir_all(dir.join("lib")).unwrap();
    fs::write(dir.join("lib/u.weave"), "rule .u { a: b; }\n").unwrap();
    let out = weave_stdin(
        &dir,
        &["build", "--root", "lib", "-"],
        b"load \"./u.weave\" as u;",
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), ".u {\n  a: b;\n}\n");
}

#[test]
fn exit_statuses() {
    let dir = scratch("exit");
    fs::write(dir.join("ok.weave"), "rule a { b: c; }").unwrap();
    fs::write(dir.join("bad.weave"), "rule a { b: #{c}; }").unwrap();
    let code = |args: &[&str]| weave_in(&dir, args).status.code();
    assert_eq!(code(&["build", "ok.weave"]), Some(0));
    assert_eq!(code(&["check", "ok.weave"]), Some(0));
    assert_eq!(code(&["build", "bad.weave"]), Some(1));
    assert_eq!(code(&["check", "missing.weave"]), Some(1));
    assert_eq!(
        code(&["check", "ok.weave", "--limit", "output-bytes=3"]),
        Some(1)
    );
    assert_eq!(
        code(&["build", "ok.weave", "-o", "missing-dir/out.css"]),
        Some(1)
    );
    for args in [
        &["build", "ok.weave", "--bogus"][..],
        &["watch", "ok.weave"],
        &["check", "ok.weave", "-o", "x.css"],
        &["build", "ok.weave", "-o", "ok2.weave"],
        &["build", "ok.weave", "--limit", "steps=1"],
    ] {
        let out = weave_in(&dir, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(text(&out.stderr).contains("weavec --help"));
    }
    assert!(!dir.join("ok2.weave").exists());
}

#[test]
fn output_files_are_replaced_atomically_and_kept_on_failure() {
    let dir = scratch("atomic");
    fs::write(dir.join("main.weave"), "rule a { b: c; }").unwrap();
    let out = weave_in(&dir, &["build", "main.weave", "-o", "out.css"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(
        fs::read_to_string(dir.join("out.css")).unwrap(),
        "a {\n  b: c;\n}\n"
    );

    // A failing build leaves the earlier output untouched.
    fs::write(dir.join("main.weave"), "rule a { b: #{oops}; }").unwrap();
    let out = weave_in(&dir, &["build", "main.weave", "-o", "out.css"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        fs::read_to_string(dir.join("out.css")).unwrap(),
        "a {\n  b: c;\n}\n"
    );

    // A successful build replaces it; no temporary file is left behind.
    fs::write(dir.join("main.weave"), "rule a { b: d; }").unwrap();
    let out = weave_in(&dir, &["build", "main.weave", "-o", "out.css"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(dir.join("out.css")).unwrap(),
        "a {\n  b: d;\n}\n"
    );
    assert_eq!(names(&dir), ["main.weave", "out.css"]);

    // Writing into a missing directory fails cleanly.
    let out = weave_in(&dir, &["build", "main.weave", "-o", "nope/out.css"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).starts_with("error: cannot write `nope/out.css`"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(names(&dir), ["main.weave", "out.css"]);
}

#[test]
fn input_output_collisions_are_refused() {
    let dir = scratch("collide");
    fs::write(dir.join("main.weave"), "rule a { b: c; }").unwrap();
    // The same file spelled two ways is caught after resolving paths.
    let out = weave_in(
        &dir,
        &[
            "build",
            "main.weave",
            "-o",
            "out.css",
            "--deps",
            "./out.css",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("are the same file"),
        "{}",
        text(&out.stderr)
    );
    assert!(!dir.join("out.css").exists());
    let out = weave_in(
        &dir,
        &[
            "build",
            "main.weave",
            "-o",
            "sub/../out.css",
            "--source-map",
            "--deps",
            "out.css.map",
        ],
    );
    assert_ne!(out.status.code(), Some(0));
    assert!(!dir.join("out.css").exists());
    // `.weave` outputs are refused before compiling, in any letter case.
    let out = weave_in(&dir, &["build", "main.weave", "-o", "MAIN.WEAVE"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(dir.join("main.weave")).unwrap(),
        "rule a { b: c; }"
    );
}

#[cfg(unix)]
#[test]
fn output_symlinked_to_an_input_is_refused() {
    let dir = scratch("collide-link");
    fs::write(
        dir.join("main.weave"),
        "load \"./lib.weave\" as l;\nrule a { b: c; }",
    )
    .unwrap();
    fs::write(dir.join("lib.weave"), "rule l { m: n; }").unwrap();
    std::os::unix::fs::symlink(dir.join("lib.weave"), dir.join("out.css")).unwrap();
    let out = weave_in(&dir, &["build", "main.weave", "-o", "out.css"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("is the input module `lib.weave`"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.join("lib.weave")).unwrap(),
        "rule l { m: n; }"
    );
}

#[test]
fn root_option_confines_and_extends_loading() {
    let dir = scratch("root");
    fs::create_dir_all(dir.join("src/pages")).unwrap();
    fs::write(dir.join("src/tokens.weave"), "export bind g = 1px;\n").unwrap();
    fs::write(
        dir.join("src/pages/home.weave"),
        "load \"../tokens.weave\" as t;\nrule .home { gap: #{t.g}; }\n",
    )
    .unwrap();
    // Default root: the input's directory, so `..` leaves it.
    let out = weave_in(&dir, &["check", "src/pages/home.weave"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).starts_with("error[E7006]"));
    // With the project root, the same load is inside.
    let out = weave_in(&dir, &["build", "--root", "src", "src/pages/home.weave"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), ".home {\n  gap: 1px;\n}\n");
    // Module names in diagnostics are relative to the root.
    fs::write(dir.join("src/pages/home.weave"), "rule a { b: #{x}; }").unwrap();
    let out = weave_in(&dir, &["check", "--root", "src", "src/pages/home.weave"]);
    assert!(
        text(&out.stderr).contains("--> pages/home.weave:1:15"),
        "{}",
        text(&out.stderr)
    );
    // An input outside the root, or a missing root, is an error.
    let out = weave_in(&dir, &["check", "--root", "src/pages", "src/tokens.weave"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("outside the module root"),
        "{}",
        text(&out.stderr)
    );
    let out = weave_in(&dir, &["check", "--root", "nowhere", "src/tokens.weave"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("as the module root"));
}

#[test]
fn json_diagnostics_are_one_line_per_build() {
    let dir = scratch("json");
    fs::write(
        dir.join("main.weave"),
        "bind é = 1;\nrule a {\n  b: #{nope};\n}\nwarn \"careful \\\"x\\\"\";\n",
    )
    .unwrap();
    let out = weave_in(&dir, &["check", "main.weave", "--diagnostics", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert_eq!(err.matches('\n').count(), 1, "{err}");
    assert!(
        err.starts_with("{\"version\":1,\"command\":\"check\",\"success\":false,\"output\":null,"),
        "{err}"
    );
    assert!(err.contains("\"dependencies\":[\"main.weave\"]"), "{err}");
    assert!(
        err.contains("\"code\":\"E1002\",\"message\":\"unresolved name `nope`\",\"location\":{\"file\":\"main.weave\",\"line\":3,\"column\":8,\"end_line\":3,\"end_column\":12}"),
        "{err}"
    );

    fs::write(
        dir.join("main.weave"),
        "warn \"careful \\\"x\\\"\";\nrule a { b: c; }",
    )
    .unwrap();
    let out = weave_in(
        &dir,
        &["build", "main.weave", "--diagnostics=json", "-o", "o.css"],
    );
    assert_eq!(out.status.code(), Some(0));
    let err = text(&out.stderr);
    assert!(
        err.contains("\"success\":true,\"output\":\"o.css\""),
        "{err}"
    );
    assert!(
        err.contains(
            "\"severity\":\"warning\",\"code\":\"W9001\",\"message\":\"careful \\\"x\\\"\""
        ),
        "{err}"
    );

    // Problems outside the compiler have no code or location.
    let out = weave_in(
        &dir,
        &[
            "build",
            "main.weave",
            "--diagnostics",
            "json",
            "-o",
            "no/o.css",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("\"code\":null,\"message\":\"cannot write `no/o.css`"));
    let out = weave_in(&dir, &["check", "gone.weave", "--diagnostics", "json"]);
    assert!(text(&out.stderr).contains(
        "\"code\":\"E7001\",\"message\":\"input file `gone.weave` not found\",\"location\":null"
    ));
}

#[test]
fn dependency_lists() {
    let dir = scratch("deps");
    fs::create_dir_all(dir.join("ui")).unwrap();
    fs::write(
        dir.join("main.weave"),
        "load \"./ui/b.weave\" as b;\nload \"./t.weave\" as t;\n",
    )
    .unwrap();
    fs::write(dir.join("ui/b.weave"), "load \"../t.weave\" as t;\n").unwrap();
    fs::write(dir.join("t.weave"), "").unwrap();
    let out = weave_in(&dir, &["check", "main.weave", "--deps", "-"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "main.weave\nui/b.weave\nt.weave\n");
    let out = weave_in(
        &dir,
        &["build", "main.weave", "-o", "out.css", "--deps", "out.d"],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(dir.join("out.d")).unwrap(),
        "main.weave\nui/b.weave\nt.weave\n"
    );
    // Paths are relative to the current directory.
    let out = weave_in(&dir.join("ui"), &["check", "../main.weave", "--deps", "-"]);
    assert_eq!(text(&out.stdout), "../main.weave\nb.weave\n../t.weave\n");
}

#[test]
fn deep_nesting_runs_with_raised_limits() {
    let dir = scratch("deep");
    let depth = 4000;
    let source = format!(
        "bind x = {}1{};\nrule a {{ b: #{{x}}; }}\n",
        "(".repeat(depth),
        ")".repeat(depth)
    );
    fs::write(dir.join("main.weave"), &source).unwrap();
    let out = weave_in(&dir, &["check", "main.weave"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).starts_with("error[E8002]"));
    let out = weave_in(
        &dir,
        &["build", "main.weave", "--limit", "nesting-depth=4096"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "a {\n  b: 1;\n}\n");
}

// Source maps --------------------------------------------------------------

fn base64_decode(text: &str) -> Vec<u8> {
    let value = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("bad base64"),
        }
    };
    let mut out = Vec::new();
    for chunk in text.trim_end_matches('=').as_bytes().chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= value(c) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    out
}

/// Decodes `mappings` into (generated line, column, source, line, column).
fn decode(mappings: &str) -> Vec<[i64; 5]> {
    let mut out = Vec::new();
    let mut state = [0i64; 4];
    for (line, text) in mappings.split(';').enumerate() {
        state[0] = 0;
        for segment in text.split(',').filter(|s| !s.is_empty()) {
            let (mut fields, mut value, mut shift) = (Vec::new(), 0i64, 0);
            for c in segment.bytes() {
                let digit = i64::from(
                    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
                        .iter()
                        .position(|&d| d == c)
                        .unwrap() as u8,
                );
                value |= (digit & 31) << shift;
                if digit & 32 == 0 {
                    fields.push(if value & 1 == 1 {
                        -(value >> 1)
                    } else {
                        value >> 1
                    });
                    (value, shift) = (0, 0);
                } else {
                    shift += 5;
                }
            }
            for (slot, delta) in state.iter_mut().zip(&fields) {
                *slot += delta;
            }
            out.push([line as i64, state[0], state[1], state[2], state[3]]);
        }
    }
    out
}

/// The string value of a top-level JSON field of a source map that this
/// test controls (no escapes in the values).
fn field<'a>(json: &'a str, name: &str) -> &'a str {
    let key = format!("\"{name}\":");
    let start = json
        .find(&key)
        .unwrap_or_else(|| panic!("{name} missing: {json}"))
        + key.len();
    let rest = &json[start..];
    let end = if rest.starts_with('[') {
        rest.find(']').unwrap() + 1
    } else {
        rest[1..].find('"').unwrap() + 2
    };
    &rest[..end]
}

#[test]
fn source_map_files_use_relative_paths_and_no_content_by_default() {
    let dir = scratch("map-file");
    fs::create_dir_all(dir.join("src/ui")).unwrap();
    fs::create_dir_all(dir.join("dist/css")).unwrap();
    // Line 2 starts with characters outside the BMP, so UTF-16 columns
    // differ from scalar and byte columns.
    fs::write(
        dir.join("src/main.weave"),
        "load \"./ui/button.weave\" as b;\n/* 😀é */ rule .é { apply b.base; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/ui/button.weave"),
        "export recipe base {\n  pad: 1px;\n}\n",
    )
    .unwrap();
    let out = weave_in(
        &dir,
        &[
            "build",
            "src/main.weave",
            "-o",
            "dist/css/site.css",
            "--source-map",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let css = fs::read_to_string(dir.join("dist/css/site.css")).unwrap();
    assert_eq!(
        css,
        ".é {\n  pad: 1px;\n}\n/*# sourceMappingURL=site.css.map */\n"
    );
    let json = fs::read_to_string(dir.join("dist/css/site.css.map")).unwrap();
    assert!(
        json.starts_with("{\"version\":3,\"file\":\"site.css\","),
        "{json}"
    );
    assert_eq!(
        field(&json, "sources"),
        "[\"../../src/main.weave\",\"../../src/ui/button.weave\"]"
    );
    assert!(!json.contains("sourcesContent"), "{json}");
    assert!(
        !json.contains(dir.to_str().unwrap()),
        "absolute path leaked: {json}"
    );
    let mappings = field(&json, "mappings").trim_matches('"').to_string();
    // `.é {` → main.weave line 1, column after "/* 😀é */ " = 3 + 2 + 1 + 4.
    // `pad: 1px` → button.weave line 1, column 2.
    assert_eq!(decode(&mappings), [[0, 0, 0, 1, 10], [1, 2, 1, 1, 2]]);
    assert_eq!(names(&dir.join("dist/css")), ["site.css", "site.css.map"]);

    // Opting in embeds the sources.
    let out = weave_in(
        &dir,
        &[
            "build",
            "src/main.weave",
            "-o",
            "dist/css/site.css",
            "--source-map",
            "--source-map-content",
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    let json = fs::read_to_string(dir.join("dist/css/site.css.map")).unwrap();
    assert!(
        json.contains("\"sourcesContent\":[\"load \\\"./ui/button.weave\\\" as b;\\n/* 😀é */"),
        "{json}"
    );
}

#[test]
fn inline_source_maps_on_stdout() {
    let dir = scratch("map-inline");
    fs::create_dir_all(dir.join("styles")).unwrap();
    fs::write(dir.join("styles/a.weave"), "rule a { b: c; }\n").unwrap();
    let out = weave_in(&dir, &["build", "styles/a.weave", "--source-map=inline"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let css = text(&out.stdout);
    let prefix =
        "a {\n  b: c;\n}\n/*# sourceMappingURL=data:application/json;charset=utf-8;base64,";
    assert!(css.starts_with(prefix), "{css}");
    let encoded = css[prefix.len()..].trim_end_matches(" */\n");
    let json = String::from_utf8(base64_decode(encoded)).unwrap();
    // Without an output file, sources are relative to the current directory.
    assert_eq!(
        json,
        "{\"version\":3,\"sources\":[\"styles/a.weave\"],\"names\":[],\"mappings\":\"AAAA;EAAS\"}"
    );
}

// Documented examples ------------------------------------------------------

/// Every example under `examples/` builds through the CLI to its
/// `expected.css`.
#[test]
fn examples_compile_to_their_expected_output() {
    let examples = repo_root().join("examples");
    let mut count = 0;
    for entry in fs::read_dir(&examples).unwrap() {
        let case = entry.unwrap().path();
        if !case.join("main.weave").is_file() {
            continue;
        }
        count += 1;
        let out = weave_in(&case, &["build", "main.weave"]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}: {}",
            case.display(),
            text(&out.stderr)
        );
        let expected = fs::read_to_string(case.join("expected.css")).unwrap();
        assert_eq!(text(&out.stdout), expected, "{}", case.display());
    }
    assert!(count >= 3, "{count}");
}

/// The complete example of LANGUAGE.md §20 is `examples/complete`, and its
/// documented output is what the compiler produces.
#[test]
fn language_reference_complete_example_compiles() {
    let spec = fs::read_to_string(repo_root().join("docs/LANGUAGE.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let section = &spec[spec.find("## 20. Complete example").unwrap()..];
    let block = |fence: &str| {
        let start = section.find(fence).unwrap() + fence.len();
        section[start..start + section[start..].find("```").unwrap()].to_string()
    };
    let source = block("```weave\n");
    let css = block("```css\n");
    let example = repo_root().join("examples/complete");
    assert_eq!(
        fs::read_to_string(example.join("main.weave"))
            .unwrap()
            .replace("\r\n", "\n"),
        source
    );
    let out = weave_in(&example, &["build", "main.weave"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), css);
}
