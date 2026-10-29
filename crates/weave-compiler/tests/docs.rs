//! Documentation examples are compiled: every `weave` block of the public
//! documents that is directly followed by a `css` block must compile to
//! exactly that CSS, and one followed by a `text` block must fail with a
//! diagnostic whose first line is that text.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::PathBuf;

use weave_compiler::{CompileOptions, MemoryResolver, ModuleId, compile};

struct Block {
    info: String,
    text: String,
    line: usize,
    /// Only whitespace between the previous block's closing fence and this
    /// block's opening fence.
    adjacent: bool,
}

fn blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current: Option<Block> = None;
    let mut gap_blank = false;
    for (number, line) in markdown.replace("\r\n", "\n").lines().enumerate() {
        match (&mut current, line.strip_prefix("```")) {
            (None, Some(info)) => {
                current = Some(Block {
                    info: info.trim().to_string(),
                    text: String::new(),
                    line: number + 1,
                    adjacent: gap_blank,
                });
            }
            (Some(_), Some(rest)) if rest.trim().is_empty() => {
                blocks.extend(current.take());
                gap_blank = true;
            }
            (Some(block), _) => {
                block.text.push_str(line);
                block.text.push('\n');
            }
            (None, None) => {
                if !line.trim().is_empty() {
                    gap_blank = false;
                }
            }
        }
    }
    blocks
}

fn compile_one(source: &str) -> Result<String, String> {
    let entry = ModuleId::new("main.weave").unwrap();
    let mut files = MemoryResolver::new();
    files.insert(entry.clone(), source);
    let compilation = compile(&entry, &mut files, &CompileOptions::default());
    match &compilation.outcome {
        Ok(output) => Ok(output.css.clone()),
        Err(diagnostics) => Err(diagnostics
            .iter()
            .map(|d| d.render(&compilation.sources))
            .collect::<Vec<_>>()
            .join("\n")),
    }
}

fn check_document(relative: &str) -> usize {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    let markdown = fs::read_to_string(&path).unwrap();
    let blocks = blocks(&markdown);
    let mut checked = 0;
    for pair in blocks.windows(2) {
        let (source, expected) = (&pair[0], &pair[1]);
        if source.info != "weave" || !expected.adjacent {
            continue;
        }
        let at = format!("{relative}:{}", source.line);
        match expected.info.as_str() {
            "css" => {
                let css = compile_one(&source.text)
                    .unwrap_or_else(|errors| panic!("{at} does not compile:\n{errors}"));
                assert_eq!(css, expected.text, "{at}");
                checked += 1;
            }
            "text" => {
                let errors =
                    compile_one(&source.text).expect_err(&format!("{at} should fail but compiled"));
                let first = errors.lines().next().unwrap_or_default();
                assert_eq!(first, expected.text.trim_end(), "{at}");
                checked += 1;
            }
            _ => {}
        }
    }
    checked
}

#[test]
fn tutorial_examples_compile_to_the_documented_output() {
    assert!(check_document("docs/TUTORIAL.md") >= 12);
}

#[test]
fn readme_examples_compile_to_the_documented_output() {
    assert!(check_document("README.md") >= 1);
}
