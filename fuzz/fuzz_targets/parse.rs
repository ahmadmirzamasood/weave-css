//! Source registration and parsing: valid diagnostic spans, escaped rendering.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    weave_compiler::fuzzing::parse(data, 128);
});
