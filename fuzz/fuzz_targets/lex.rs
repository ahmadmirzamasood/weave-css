//! Tokenizer: contiguous, non-empty, boundary-aligned tokens that end in Eof.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    weave_compiler::fuzzing::lex(&text, 128);
});
