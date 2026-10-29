//! The `color` built-ins (TYPES.md §9, BUILTINS.md): exhaustive and
//! boundary checks against integer oracles written independently of the
//! implementation, plus round trips through the accessors.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use std::fmt::Write as _;

use weave_compiler::{CompileOptions, MemoryResolver, ModuleId, compile};

/// Compiles `body` as the declarations of one rule and returns the
/// declaration values in order.
fn values(body: &str) -> Vec<String> {
    let source = format!("rule a {{\n{body}\n}}\n");
    let entry = ModuleId::new("main.weave").unwrap();
    let mut files = MemoryResolver::new();
    files.insert(entry.clone(), source.clone());
    let compilation = compile(&entry, &mut files, &CompileOptions::default());
    match &compilation.outcome {
        Ok(output) => output
            .css
            .lines()
            .filter_map(|l| l.trim().strip_suffix(';'))
            .filter_map(|l| l.split_once(": ").map(|(_, v)| v.to_string()))
            .collect(),
        Err(diags) => panic!(
            "{}",
            diags
                .iter()
                .map(|d| d.render(&compilation.sources))
                .collect::<String>()
        ),
    }
}

fn hex(r: u32, g: u32, b: u32, a: u32) -> String {
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// `round(n / d)` with ties toward +infinity, for non-negative `n`.
fn half_up(n: u32, d: u32) -> u32 {
    (2 * n + d) / (2 * d)
}

#[test]
fn unitless_channels_are_identity_for_every_value() {
    let mut body = String::new();
    for v in 0..=255 {
        writeln!(body, "p: #{{color.rgb({v}, {}, {}, 1)}};", 255 - v, v / 2).unwrap();
    }
    let got = values(&body);
    assert_eq!(got.len(), 256);
    for (v, text) in (0..=255u32).zip(got) {
        assert_eq!(text, hex(v, 255 - v, v / 2, 255), "{v}");
    }
}

#[test]
fn percent_channels_match_an_integer_oracle() {
    let mut body = String::new();
    for p in 0..=100 {
        writeln!(body, "p: #{{color.rgb({p}%, {p}%, {p}%, {p}%)}};").unwrap();
    }
    for (p, text) in (0..=100u32).zip(values(&body)) {
        let c = half_up(p * 255, 100);
        assert_eq!(text, hex(c, c, c, c), "{p}%");
    }
}

#[test]
fn out_of_range_channels_clamp() {
    assert_eq!(
        values("p: #{color.rgb(-1, 256, 1000, 2)};\nq: #{color.rgb(-5%, 101%, 0, -1)};"),
        ["#00ffff", "#00ff0000"]
    );
}

#[test]
fn fractions_round_ties_toward_positive_infinity() {
    assert_eq!(
        values("p: #{color.rgb(0.5, 1.5, 2.49, 0)};\nq: #{color.rgb(0.4999999, 254.5, 127.5, 1)};"),
        ["#01020200", "#01ff80"]
    );
}

#[test]
fn alpha_round_trips_through_the_accessor_for_every_value() {
    let mut body = String::new();
    for a in 0..=255 {
        writeln!(
            body,
            "p: #{{color.rgb(1, 2, 3, color.alpha(color.rgb(1, 2, 3, {a} / 255)))}};"
        )
        .unwrap();
    }
    for (a, text) in (0..=255u32).zip(values(&body)) {
        assert_eq!(text, hex(1, 2, 3, a), "{a}");
    }
}

#[test]
fn accessors_return_the_channels() {
    let mut body = String::new();
    for v in (0..=255).step_by(5) {
        writeln!(
            body,
            "p: #{{color.red(color.rgb({v}, 0, 0))}} #{{color.green(color.rgb(0, {v}, 0))}} #{{color.blue(color.rgb(0, 0, {v}))}};"
        )
        .unwrap();
    }
    for (v, text) in (0..=255u32).step_by(5).zip(values(&body)) {
        assert_eq!(text, format!("{v} {v} {v}"));
    }
}

#[test]
fn mix_matches_an_integer_oracle_on_a_grid() {
    let colors = [
        (0u32, 0u32, 0u32, 255u32),
        (255, 255, 255, 255),
        (255, 0, 0, 128),
        (18, 52, 86, 255),
        (171, 205, 239, 7),
        (37, 99, 235, 200),
    ];
    let weights = [0u32, 1, 25, 33, 50, 66, 75, 99, 100];
    let mut body = String::new();
    let mut expected = Vec::new();
    for a in colors {
        for b in colors {
            for w in weights {
                writeln!(
                    body,
                    "p: #{{color.mix({}, {}, {w}%)}};",
                    hex(a.0, a.1, a.2, a.3),
                    hex(b.0, b.1, b.2, b.3)
                )
                .unwrap();
                let m = |x: u32, y: u32| half_up(x * w + y * (100 - w), 100);
                expected.push(hex(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2), m(a.3, b.3)));
            }
        }
    }
    assert_eq!(values(&body), expected);
}

#[test]
fn mix_endpoints_and_identity() {
    assert_eq!(
        values(
            "p: #{color.mix(#123456, #abcdef, 1)};\nq: #{color.mix(#123456, #abcdef, 0%)};\nr: #{color.mix(#123456, #123456, 37%)};\ns: #{color.mix(#000, #fff, weight: 0.5)};"
        ),
        ["#123456", "#abcdef", "#123456", "#808080"]
    );
}

#[test]
fn results_are_ordinary_colors() {
    assert_eq!(
        values("p: #{color.rgb(1, 2, 3) == #010203};\nq: #{meta.type(color.mix(#000, #fff))};"),
        ["true", "\"color\""]
    );
}
