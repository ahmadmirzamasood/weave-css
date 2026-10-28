//! Deterministic benchmark workloads.
//!
//! Every workload is a set of module templates produced by a pure function
//! of [`GENERATOR_VERSION`] and the constants below: no randomness, clock or
//! environment. A template may contain [`EDIT`] markers; [`Workload::render`]
//! replaces them with an edit counter, `0` for the canonical corpus, so an
//! incremental-edit benchmark can produce a distinct revision of a module
//! for every sample.
//!
//! Each workload also states what a correct build must produce (the number
//! of CSS blocks and some exact lines), so that a faster build cannot win by
//! omitting work.

use std::fmt::Write as _;

/// Bump whenever a generator's output changes; recorded in every report.
pub const GENERATOR_VERSION: u32 = 1;

/// Replaced by the edit counter when a module is rendered.
pub const EDIT: &str = "@@EDIT@@";

#[derive(Debug)]
pub struct Workload {
    pub name: &'static str,
    pub description: &'static str,
    /// (root-relative module path, template), entry first.
    pub files: Vec<(String, String)>,
    /// Incremental edits: (label, module path whose markers change).
    pub edits: Vec<(&'static str, String)>,
    /// Lines of the output that open a block (`… {`).
    pub blocks: usize,
    /// Exact lines that the output must contain.
    pub lines: Vec<String>,
}

impl Workload {
    pub fn entry(&self) -> &str {
        self.files.first().map_or("", |(path, _)| path.as_str())
    }

    /// The text of `path` with every marker replaced by `counter`.
    pub fn render(&self, template: &str, counter: u64) -> String {
        template.replace(EDIT, &counter.to_string())
    }

    pub fn input_bytes(&self) -> usize {
        self.files
            .iter()
            .map(|(_, template)| self.render(template, 0).len())
            .sum()
    }

    /// Checks a build's CSS against the workload's expectations.
    pub fn check(&self, css: &str) -> Result<(), String> {
        let blocks = css.lines().filter(|line| line.ends_with('{')).count();
        if blocks != self.blocks {
            return Err(format!(
                "{}: expected {} blocks, found {blocks}",
                self.name, self.blocks
            ));
        }
        for expected in &self.lines {
            if !css.lines().any(|line| line == expected) {
                return Err(format!("{}: missing line `{expected}`", self.name));
            }
        }
        Ok(())
    }
}

pub fn all() -> Vec<Workload> {
    vec![
        small_component(),
        large_plain(),
        large_mixed(),
        module_graph_wide(),
        module_graph_deep(),
        selector_expansion(),
        function_heavy(),
    ]
}

fn file(path: &str, text: String) -> (String, String) {
    (path.to_string(), text)
}

fn small_component() -> Workload {
    let tokens = format!(
        "export bind gap = 8px;
export bind seed = {EDIT};
export bind palette = {{primary: #2563eb, surface: #ffffff, text: #111827}};

export fn space(n: number) -> length {{
    yield gap * n;
}}
"
    );
    let main = format!(
        "load \"./tokens.weave\" as tokens;

recipe panel(pad: length = 16px) {{
    padding: #{{pad}};
    background: #{{tokens.palette.surface}};
}}

rule .card {{
    apply panel(tokens.space(2));
    color: #{{tokens.palette.text}};
    order: #{{tokens.seed}};
    display: grid;
    rule &:hover {{ opacity: 0.8; }}
    rule .card-title {{ font-weight: 600; }}
}}

each i, value in [4px, 8px, 16px] {{
    rule .gap-#{{i}} {{ gap: #{{value}}; }}
}}

@media (width >= 768px) {{
    rule .card {{ grid-template-columns: 1fr 2fr; }}
}}

rule .edit {{ order: {EDIT}; }}
"
    );
    Workload {
        name: "small-component",
        description: "one component module and one token module (typical single-file edit loop)",
        files: vec![file("main.weave", main), file("tokens.weave", tokens)],
        edits: vec![
            ("entry", "main.weave".to_string()),
            ("shared", "tokens.weave".to_string()),
        ],
        blocks: 9,
        lines: vec![
            "  padding: 16px;".to_string(),
            "  color: #111827;".to_string(),
            ".card:hover {".to_string(),
            "  gap: 16px;".to_string(),
        ],
    }
}

const LARGE_PLAIN_RULES: usize = 20_000;

fn large_plain() -> Workload {
    let mut text = String::new();
    for i in 0..LARGE_PLAIN_RULES {
        if i % 500 == 0 {
            let _ = writeln!(text, "/* Generated plain CSS, section {} */", i / 500);
        }
        let _ = write!(
            text,
            "rule .c-{i} > .item:hover {{
    color: #{:06x};
    margin: {}px {}px;
    padding: 0 {}rem;
    background: rgb({} {} {} / 50%);
}}
",
            (i * 2_654_435_761) % 0x0100_0000,
            i % 16,
            i % 7,
            i % 5,
            i % 256,
            (i * 7) % 256,
            (i * 13) % 256,
        );
    }
    let _ = writeln!(text, "rule .edit {{ order: {EDIT}; }}");
    Workload {
        name: "large-plain",
        description: "one large generated stylesheet of plain CSS rules (tokenizer, parser and emitter)",
        files: vec![file("main.weave", text)],
        edits: vec![("entry", "main.weave".to_string())],
        blocks: LARGE_PLAIN_RULES + 1,
        lines: vec![
            ".c-19999 > .item:hover {".to_string(),
            "  background: rgb(31 217 147 / 50%);".to_string(),
        ],
    }
}

const LARGE_MIXED_RULES: usize = 5_000;

fn large_mixed() -> Workload {
    let mut text = String::from(
        "bind unit = 4px;
bind accent = #3b82f6;

",
    );
    for i in 0..LARGE_MIXED_RULES {
        let _ = write!(
            text,
            "rule .m-{i} {{
    margin: #{{unit * {}}} #{{unit * {}}};
    width: calc(100% - #{{unit * {}}});
    border-color: #{{accent}};
}}
",
            i % 8,
            i % 3,
            i % 11,
        );
    }
    let _ = writeln!(text, "rule .edit {{ order: {EDIT}; }}");
    Workload {
        name: "large-mixed",
        description: "one large stylesheet whose rules interpolate bindings and arithmetic",
        files: vec![file("main.weave", text)],
        edits: vec![("entry", "main.weave".to_string())],
        blocks: LARGE_MIXED_RULES + 1,
        lines: vec![
            ".m-4999 {".to_string(),
            "  margin: 28px 4px;".to_string(),
            "  width: calc(100% - 20px);".to_string(),
        ],
    }
}

const WIDE_COMPONENTS: usize = 200;
const WIDE_UTILS: usize = 10;
const WIDE_RULES_PER_COMPONENT: usize = 8;
const WIDE_EDITED: usize = 17;

fn module_graph_wide() -> Workload {
    let tokens = format!(
        "export bind seed = {EDIT};
export bind unit = 4px;
export bind radius = 6px;
export bind ink = #111827;

export fn space(n: number) -> length {{
    yield unit * n;
}}
"
    );
    let mut files = Vec::new();
    let mut main = String::new();
    for c in 0..WIDE_COMPONENTS {
        let _ = writeln!(main, "load \"./components/c{c:03}.weave\" as c{c:03};");
    }
    main.push_str("\nrule .app { display: grid; }\n");
    files.push(file("main.weave", main));
    files.push(file("tokens.weave", tokens));
    for u in 0..WIDE_UTILS {
        let text = format!(
            "load \"../tokens.weave\" as t;

export recipe box(pad: length) {{
    padding: #{{pad}};
    border-radius: #{{t.radius}};
}}

export fn tone(n: number) -> number {{
    yield n * {};
}}

rule .u{u} {{ display: block; }}
",
            u + 1
        );
        files.push(file(&format!("utils/u{u}.weave"), text));
    }
    for c in 0..WIDE_COMPONENTS {
        let a = c % WIDE_UTILS;
        let b = (c + 3) % WIDE_UTILS;
        let mut text = format!(
            "load \"../tokens.weave\" as t;
load \"../utils/u{a}.weave\" as ua;
load \"../utils/u{b}.weave\" as ub;

rule .c{c}-root {{
    apply ua.box(t.space(2));
    order: #{{t.seed}};
}}
"
        );
        for r in 1..WIDE_RULES_PER_COMPONENT {
            let _ = write!(
                text,
                "rule .c{c}-part-{r} {{
    margin: #{{t.space({r})}};
    z-index: #{{ub.tone({r})}};
    color: #{{t.ink}};
}}
"
            );
        }
        if c == WIDE_EDITED {
            let _ = writeln!(text, "rule .edit {{ order: {EDIT}; }}");
        }
        files.push(file(&format!("components/c{c:03}.weave"), text));
    }
    Workload {
        name: "module-graph-wide",
        description: "an entry loading 200 component modules that share 10 utility modules and one token module",
        files,
        edits: vec![
            ("leaf", format!("components/c{WIDE_EDITED:03}.weave")),
            ("shared", "tokens.weave".to_string()),
        ],
        blocks: WIDE_UTILS + WIDE_COMPONENTS * WIDE_RULES_PER_COMPONENT + 2,
        lines: vec![
            ".c199-part-7 {".to_string(),
            "  z-index: 7;".to_string(),
            "  border-radius: 6px;".to_string(),
        ],
    }
}

const DEEP_MODULES: usize = 256;

fn module_graph_deep() -> Workload {
    let mut files = Vec::new();
    for m in 0..DEEP_MODULES {
        let mut text = String::new();
        if m + 1 < DEEP_MODULES {
            let _ = write!(
                text,
                "load \"./m{:03}.weave\" as next;

export bind depth = next.depth + 1;
",
                m + 1
            );
        } else {
            let _ = writeln!(text, "export bind depth = {EDIT};");
        }
        let _ = writeln!(text, "\nrule .m{m} {{ order: #{{depth}}; }}");
        if m == 0 {
            let _ = writeln!(text, "rule .edit {{ order: {EDIT}; }}");
        }
        files.push(file(&format!("m{m:03}.weave"), text));
    }
    Workload {
        name: "module-graph-deep",
        description: "a chain of 256 modules, each loading the next and deriving a value from it",
        files,
        edits: vec![
            ("entry", "m000.weave".to_string()),
            ("deepest", format!("m{:03}.weave", DEEP_MODULES - 1)),
        ],
        blocks: DEEP_MODULES + 1,
        lines: vec![".m0 {".to_string(), "  order: 255;".to_string()],
    }
}

const SELECTOR_BLOCKS: usize = 300;

fn selector_expansion() -> Workload {
    let mut text = String::new();
    for i in 0..SELECTOR_BLOCKS {
        let _ = write!(
            text,
            "rule .a{i}, .b{i}, .c{i} {{
    color: red;
    rule .x, .y {{
        margin: 0;
        rule &:hover, &:focus-visible {{
            opacity: 0.5;
            rule > .z {{ padding: 1px; }}
        }}
    }}
    @media (width >= 40rem) {{
        display: block;
        rule .w {{ gap: 1px; }}
    }}
}}
"
        );
    }
    let _ = writeln!(text, "rule .edit {{ order: {EDIT}; }}");
    Workload {
        name: "selector-expansion",
        description: "300 nested rules with selector lists four levels deep and nested media queries",
        files: vec![file("main.weave", text)],
        edits: vec![("entry", "main.weave".to_string())],
        blocks: 7 * SELECTOR_BLOCKS + 1,
        lines: vec![
            ":is(:is(:is(.a299, .b299, .c299) .x, :is(.a299, .b299, .c299) .y):hover, :is(:is(.a299, .b299, .c299) .x, :is(.a299, .b299, .c299) .y):focus-visible) > .z {"
                .to_string(),
        ],
    }
}

const FUNCTION_ITERATIONS: usize = 1_200;

fn function_heavy() -> Workload {
    let text = format!(
        "bind unit = 4px;
bind scale = {{xs: 0.75, sm: 0.875, md: 1, lg: 1.25, xl: 1.5}};
bind names = [\"xs\", \"sm\", \"md\", \"lg\", \"xl\"];

fn sum-to(n: number) -> number {{
    when n <= 0 {{
        yield 0;
    }}
    yield n + sum-to(n - 1);
}}

fn ratio(i: number) -> number {{
    yield map.get(scale, names[i % 5], 1);
}}

recipe sized(i: number) {{
    font-size: #{{1rem * ratio(i)}};
    line-height: #{{math.round(ratio(i) * 150) / 100}};
}}

each i in range(0, {FUNCTION_ITERATIONS}) {{
    rule .f-#{{i}} {{
        apply sized(i);
        order: #{{sum-to(i % 16)}};
        width: #{{math.clamp(4px, unit * (i % 13), 40px)}};
    }}
}}

rule .edit {{ order: {EDIT}; }}
"
    );
    Workload {
        name: "function-heavy",
        description: "1200 loop iterations calling recursive functions, map/list built-ins and a recipe",
        files: vec![file("main.weave", text)],
        edits: vec![("entry", "main.weave".to_string())],
        blocks: FUNCTION_ITERATIONS + 1,
        lines: vec![
            ".f-1199 {".to_string(),
            "  order: 120;".to_string(),
            "  font-size: 1.5rem;".to_string(),
        ],
    }
}
