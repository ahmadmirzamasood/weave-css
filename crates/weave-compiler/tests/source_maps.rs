//! Source maps: decoded mappings point at the expected original positions
//! (ECMA-426, UTF-16 columns). The decoder below is written independently of
//! the compiler's encoder.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use weave_compiler::{
    CompileOptions, CompileOutput, JsonOptions, MemoryResolver, ModuleId, SourceMap, compile,
};

/// One decoded segment: generated line and column, source index, original
/// line and column. All 0-based; columns in UTF-16 code units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Segment {
    line: u32,
    column: u32,
    source: u32,
    orig_line: u32,
    orig_column: u32,
}

fn base64_value(c: u8) -> i64 {
    match c {
        b'A'..=b'Z' => i64::from(c - b'A'),
        b'a'..=b'z' => i64::from(c - b'a') + 26,
        b'0'..=b'9' => i64::from(c - b'0') + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("invalid Base64 digit {c}"),
    }
}

/// Decodes the `mappings` field of an ECMA-426 source map.
fn decode(mappings: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let (mut source, mut orig_line, mut orig_column) = (0i64, 0i64, 0i64);
    for (line, text) in mappings.split(';').enumerate() {
        let mut column = 0i64;
        for segment in text.split(',').filter(|s| !s.is_empty()) {
            let mut fields = Vec::new();
            let (mut value, mut shift) = (0i64, 0u32);
            for &c in segment.as_bytes() {
                let digit = base64_value(c);
                value += (digit & 0x1f) << shift;
                if digit & 0x20 != 0 {
                    shift += 5;
                } else {
                    let magnitude = value >> 1;
                    fields.push(if value & 1 == 1 {
                        -magnitude
                    } else {
                        magnitude
                    });
                    value = 0;
                    shift = 0;
                }
            }
            assert_eq!(fields.len(), 4, "segment {segment:?}");
            column += fields[0];
            source += fields[1];
            orig_line += fields[2];
            orig_column += fields[3];
            out.push(Segment {
                line: u32::try_from(line).unwrap(),
                column: u32::try_from(column).unwrap(),
                source: u32::try_from(source).unwrap(),
                orig_line: u32::try_from(orig_line).unwrap(),
                orig_column: u32::try_from(orig_column).unwrap(),
            });
        }
    }
    out
}

/// The 0-based line and UTF-16 column of the first occurrence of `needle`,
/// counting LF line breaks only.
fn position_of(text: &str, needle: &str) -> (u32, u32) {
    let offset = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not in source"));
    let before = &text[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = before.matches('\n').count();
    let column = before[line_start..].encode_utf16().count();
    (u32::try_from(line).unwrap(), u32::try_from(column).unwrap())
}

/// The 0-based line and UTF-16 column of `needle` in generated CSS.
fn generated(css: &str, needle: &str) -> (u32, u32) {
    position_of(css, needle)
}

fn build(files: &[(&str, &str)]) -> CompileOutput {
    let mut resolver = MemoryResolver::new();
    for (name, text) in files {
        resolver.insert(ModuleId::new(*name).unwrap(), text.as_bytes());
    }
    let mut options = CompileOptions::default();
    options.source_map = true;
    let entry = ModuleId::new(files[0].0).unwrap();
    match compile(&entry, &mut resolver, &options).outcome {
        Ok(output) => output,
        Err(diags) => panic!("compilation failed: {diags:?}"),
    }
}

fn map(output: &CompileOutput) -> &SourceMap {
    output.source_map.as_ref().expect("source map requested")
}

/// Asserts that the mapping at the generated position of `gen_needle`
/// points at `orig_needle` in source `source`.
fn assert_maps(
    output: &CompileOutput,
    files: &[(&str, &str)],
    gen_needle: &str,
    source: u32,
    orig_needle: &str,
) {
    let segments = decode(&map(output).encoded_mappings());
    let (line, column) = generated(&output.css, gen_needle);
    let segment = segments
        .iter()
        .find(|s| s.line == line && s.column == column)
        .unwrap_or_else(|| {
            panic!("no mapping at {line}:{column} ({gen_needle:?}) in {segments:?}")
        });
    let (orig_line, orig_column) = position_of(files[source as usize].1, orig_needle);
    assert_eq!(
        (segment.source, segment.orig_line, segment.orig_column),
        (source, orig_line, orig_column),
        "{gen_needle:?} should map to {orig_needle:?}"
    );
}

#[test]
fn every_written_node_is_mapped_in_order() {
    let files = [("main.weave", "rule a {\n  color: red;\n  b: c;\n}\n")];
    let output = build(&files);
    let segments = decode(&map(&output).encoded_mappings());
    assert_eq!(
        segments,
        [
            Segment {
                line: 0,
                column: 0,
                source: 0,
                orig_line: 0,
                orig_column: 0
            },
            Segment {
                line: 1,
                column: 2,
                source: 0,
                orig_line: 1,
                orig_column: 2
            },
            Segment {
                line: 2,
                column: 2,
                source: 0,
                orig_line: 2,
                orig_column: 2
            },
        ]
    );
    assert_eq!(map(&output).encoded_mappings(), "AAAA;EACE;EACA");
}

#[test]
fn unicode_columns_are_utf16_code_units() {
    // U+1F600 is two UTF-16 units and four UTF-8 bytes; é is one unit and
    // two bytes. The CRLF line break counts as one line.
    let text =
        "/* é😀 */ rule .a { color: red; }\r\n/* 😀😀 */ rule .é😀 { content: \"→\"; b: c; }\n";
    let files = [("main.weave", text)];
    let output = build(&files);
    let lf = text.replace("\r\n", "\n");
    let files_lf = [("main.weave", lf.as_str())];
    assert_maps(&output, &files_lf, ".a {", 0, "rule .a");
    assert_maps(&output, &files_lf, ".é😀 {", 0, "rule .é😀");
    assert_maps(&output, &files_lf, "content:", 0, "content:");
    assert_maps(&output, &files_lf, "b: c", 0, "b: c");
    // One value checked by hand rather than through `position_of`.
    let segments = decode(&map(&output).encoded_mappings());
    let rule = segments
        .iter()
        .find(|s| s.orig_line == 1 && s.line > 2)
        .unwrap();
    // "/* 😀😀 */ " is 3 + 4 + 4 = 11 UTF-16 units (bytes: 3 + 8 + 4 = 15).
    assert_eq!(rule.orig_column, 11);
}

#[test]
fn byte_order_mark_is_not_a_column() {
    let text = "\u{feff}rule a { b: c; }";
    let output = build(&[("main.weave", text)]);
    let segments = decode(&map(&output).encoded_mappings());
    assert_eq!((segments[0].orig_line, segments[0].orig_column), (0, 0));
    assert_eq!((segments[1].orig_line, segments[1].orig_column), (0, 9));
    // The embedded content omits the byte-order mark, so columns agree with it.
    assert_eq!(map(&output).contents()[0], "rule a { b: c; }");
}

#[test]
fn generated_selectors_map_to_the_rule_that_wrote_them() {
    let text = "\
bind sizes = [4px, 8px];
each i, size in sizes {
    rule .gap-#{i} { gap: #{size}; }
}
rule .card {
    color: red;
    rule &:hover { opacity: 0.5; }
    rule .icon { width: 1px; }
    margin: 0;
}
";
    let files = [("main.weave", text)];
    let output = build(&files);
    // Both loop iterations map to the same `rule` item.
    assert_maps(&output, &files, ".gap-0 {", 0, "rule .gap-#{i}");
    assert_maps(&output, &files, ".gap-1 {", 0, "rule .gap-#{i}");
    assert_maps(&output, &files, "gap: 8px", 0, "gap: #{size}");
    // Lowered nested rules map to their own `rule` items, segments to the
    // parent rule, declarations to themselves.
    assert_maps(&output, &files, ".card:hover {", 0, "rule &:hover");
    assert_maps(&output, &files, ".card .icon {", 0, "rule .icon");
    assert_maps(&output, &files, "margin: 0", 0, "margin: 0");
    let css = &output.css;
    let second_segment = css.rfind(".card {").unwrap();
    let (line, column) = generated(&css[second_segment..], ".card {");
    let base = u32::try_from(css[..second_segment].matches('\n').count()).unwrap();
    let segments = decode(&map(&output).encoded_mappings());
    let segment = segments
        .iter()
        .find(|s| s.line == base + line && s.column == column)
        .unwrap();
    assert_eq!(
        (segment.orig_line, segment.orig_column),
        position_of(text, "rule .card")
    );
}

#[test]
fn function_results_map_to_the_calling_declaration() {
    let text = "\
fn double(x: number) -> number {
    yield x * 2;
}
rule .a {
    width: #{double(3px)};
}
";
    let files = [("main.weave", text)];
    let output = build(&files);
    assert!(output.css.contains("width: 6px;"));
    assert_maps(&output, &files, "width: 6px", 0, "width: #{double");
}

#[test]
fn recipe_expansion_maps_to_definitions_and_keeps_call_sites() {
    let text = "\
recipe inner {
    a: b;
    slot;
}
recipe outer {
    apply inner { c: d; }
}
rule .x {
    apply outer;
    e: f;
}
";
    let files = [("main.weave", text)];
    let output = build(&files);
    assert_maps(&output, &files, "a: b", 0, "a: b");
    assert_maps(&output, &files, "c: d", 0, "c: d");
    assert_maps(&output, &files, "e: f", 0, "e: f");
    let source_map = map(&output);
    let pos = |needle: &str| {
        let (line, column) = position_of(text, needle);
        weave_compiler::OriginalPosition {
            source: 0,
            position: weave_compiler::Position { line, column },
        }
    };
    let chains: Vec<_> = source_map
        .mappings()
        .iter()
        .map(|m| m.expansion.clone())
        .collect();
    // `.x {`, `a: b`, `c: d`, `e: f`.
    assert_eq!(chains.len(), 4);
    assert_eq!(chains[0], []);
    assert_eq!(chains[1], [pos("apply inner"), pos("apply outer")]);
    assert_eq!(chains[2], [pos("apply inner"), pos("apply outer")]);
    assert_eq!(chains[3], []);
}

#[test]
fn imported_modules_have_their_own_sources() {
    let files = [
        (
            "main.weave",
            "load \"./ui/button.weave\" as button;\nrule .main { apply button.base; }\n",
        ),
        (
            "ui/button.weave",
            "export recipe base { padding: 1px; }\nrule .button { color: red; }\n",
        ),
    ];
    let output = build(&files);
    let source_map = map(&output);
    assert_eq!(source_map.sources(), ["main.weave", "ui/button.weave"]);
    assert_maps(&output, &files, ".button {", 1, "rule .button");
    assert_maps(&output, &files, "color: red", 1, "color: red");
    assert_maps(&output, &files, ".main {", 0, "rule .main");
    // The recipe body is written in the loaded module.
    assert_maps(&output, &files, "padding: 1px", 1, "padding: 1px");
    let padding = source_map
        .mappings()
        .iter()
        .find(|m| m.original.source == 1 && m.original.position.line == 0)
        .unwrap();
    let (line, column) = position_of(files[0].1, "apply button.base");
    assert_eq!(padding.expansion.len(), 1);
    assert_eq!(padding.expansion[0].source, 0);
    assert_eq!(
        (
            padding.expansion[0].position.line,
            padding.expansion[0].position.column
        ),
        (line, column)
    );
}

#[test]
fn json_defaults_exclude_content_and_use_module_names() {
    let files = [("main.weave", "rule a { b: c; }\n")];
    let output = build(&files);
    let json = map(&output).to_json(&JsonOptions::default());
    assert_eq!(
        json,
        "{\"version\":3,\"sources\":[\"main.weave\"],\"names\":[],\"mappings\":\"AAAA;EAAS\"}"
    );
    let paths = ["../src/main.weave".to_string()];
    let json = map(&output).to_json(&JsonOptions {
        file: Some("out.css"),
        sources: Some(&paths),
        sources_content: true,
    });
    assert_eq!(
        json,
        "{\"version\":3,\"file\":\"out.css\",\"sources\":[\"../src/main.weave\"],\
         \"sourcesContent\":[\"rule a { b: c; }\\n\"],\"names\":[],\"mappings\":\"AAAA;EAAS\"}"
    );
}

#[test]
fn no_map_unless_requested_and_css_is_unchanged() {
    let text = "rule a { b: c; }\n";
    let mut resolver = MemoryResolver::new();
    let entry = ModuleId::new("main.weave").unwrap();
    resolver.insert(entry.clone(), text.as_bytes());
    let plain = compile(&entry, &mut resolver, &CompileOptions::default())
        .outcome
        .unwrap();
    assert!(plain.source_map.is_none());
    assert_eq!(plain.css, build(&[("main.weave", text)]).css);
}

#[test]
fn spec_fixtures_produce_maps_that_point_inside_their_sources() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/spec");
    let mut checked = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let case = entry.unwrap().path();
        if !case.join("expected.css").is_file() {
            continue;
        }
        let mut resolver = MemoryResolver::new();
        let mut texts = Vec::new();
        for file in std::fs::read_dir(&case).unwrap() {
            let path = file.unwrap().path();
            if path.extension().is_some_and(|e| e == "weave") {
                let name = path.file_name().unwrap().to_str().unwrap().to_string();
                let bytes = std::fs::read(&path).unwrap();
                texts.push((name.clone(), bytes.clone()));
                resolver.insert(ModuleId::new(name).unwrap(), bytes);
            }
        }
        let mut options = CompileOptions::default();
        options.source_map = true;
        let entry = ModuleId::new("main.weave").unwrap();
        let output = compile(&entry, &mut resolver, &options).outcome.unwrap();
        let source_map = output.source_map.unwrap();
        let css_lines: Vec<&str> = output.css.split('\n').collect();
        for segment in decode(&source_map.encoded_mappings()) {
            // Every generated position is the start of written text.
            let line = css_lines[segment.line as usize];
            let prefix: Vec<u16> = line.encode_utf16().collect();
            assert!(segment.column as usize <= prefix.len());
            assert!(
                prefix[..segment.column as usize]
                    .iter()
                    .all(|&u| u == u16::from(b' '))
            );
            // Every original position is inside its source.
            let name = &source_map.sources()[segment.source as usize];
            let content = &source_map.contents()[segment.source as usize];
            let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
            let source_line = normalized.split('\n').nth(segment.orig_line as usize);
            let source_line = source_line.unwrap_or_else(|| panic!("{name}: line out of range"));
            assert!(segment.orig_column as usize <= source_line.encode_utf16().count());
        }
        assert!(texts.iter().any(|(n, _)| n == "main.weave"));
        checked += 1;
    }
    assert!(checked >= 18, "{checked}");
}
