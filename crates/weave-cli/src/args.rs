//! Command-line parsing.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use weave_compiler::{Limit, ResourceLimits};

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    /// Compile and write CSS to stdout or a file.
    Build(Options),
    /// Compile without writing CSS.
    Check(Options),
    /// Compile, then recompile whenever an input changes.
    Watch(Options),
}

/// Where an output goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Stdout,
    File(PathBuf),
}

/// The input stylesheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Stdin,
    File(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceMapMode {
    /// A `.map` file next to the CSS file, linked by a comment.
    File,
    /// A `data:` URL in the CSS.
    Inline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiagnosticFormat {
    #[default]
    Human,
    /// One JSON object per build, on one line of stderr.
    Json,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub input: Input,
    /// `None` for `check`.
    pub output: Option<Target>,
    pub root: Option<PathBuf>,
    pub source_map: Option<SourceMapMode>,
    pub source_map_content: bool,
    pub diagnostics: DiagnosticFormat,
    pub deps: Option<Target>,
    pub limits: ResourceLimits,
    pub poll_interval: Duration,
}

/// The default watch polling interval.
pub const DEFAULT_POLL_MS: u64 = 200;
const POLL_RANGE_MS: std::ops::RangeInclusive<u64> = 10..=60_000;

/// An invalid command line; the message is shown with the usage summary.
#[derive(Debug, PartialEq, Eq)]
pub struct UsageError(pub String);

fn usage(message: impl Into<String>) -> UsageError {
    UsageError(message.into())
}

/// Parses the arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, UsageError> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Err(usage("missing command"));
    };
    let rest: Vec<OsString> = args.collect();
    let command = match first.to_str() {
        Some("-h" | "--help" | "help") => return no_more(Command::Help, &rest),
        Some("-V" | "--version" | "version") => return no_more(Command::Version, &rest),
        Some(name @ ("build" | "check" | "watch")) => name,
        _ => {
            return Err(usage(format!(
                "unknown command `{}`",
                first.to_string_lossy()
            )));
        }
    };

    let mut input: Option<Input> = None;
    let mut output: Option<Target> = None;
    let mut root: Option<PathBuf> = None;
    let mut source_map: Option<SourceMapMode> = None;
    let mut source_map_content = false;
    let mut diagnostics: Option<DiagnosticFormat> = None;
    let mut deps: Option<Target> = None;
    let mut limits = ResourceLimits::default();
    let mut limits_seen = Vec::new();
    let mut poll: Option<u64> = None;

    let mut options_done = false;
    let mut rest = rest.into_iter();
    while let Some(arg) = rest.next() {
        let flag = if options_done { None } else { arg.to_str() };
        let (name, inline_value) = match flag {
            Some(text) if text.starts_with("--") && text.len() > 2 => match text.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (text, None),
            },
            Some(text) => (text, None),
            None => ("", None),
        };
        // Takes the option's value: `--name=value` or the next argument.
        let mut value = |name: &str| -> Result<OsString, UsageError> {
            if let Some(v) = &inline_value {
                return Ok(OsString::from(v));
            }
            rest.next()
                .ok_or_else(|| usage(format!("`{name}` requires a value")))
        };
        match name {
            "-h" | "--help" => return Ok(Command::Help),
            "--" if inline_value.is_none() => options_done = true,
            "-o" | "--output" => {
                once(output.is_some(), name)?;
                output = Some(target(value(name)?));
            }
            "--root" => {
                once(root.is_some(), name)?;
                root = Some(PathBuf::from(value(name)?));
            }
            "--source-map" => {
                once(source_map.is_some(), name)?;
                source_map = Some(match inline_value.as_deref() {
                    None | Some("file") => SourceMapMode::File,
                    Some("inline") => SourceMapMode::Inline,
                    Some(other) => {
                        return Err(usage(format!(
                            "`--source-map` takes `file` or `inline`, not `{other}`"
                        )));
                    }
                });
            }
            "--source-map-content" => {
                no_value(name, inline_value.as_deref())?;
                source_map_content = true;
            }
            "--diagnostics" => {
                once(diagnostics.is_some(), name)?;
                diagnostics = Some(match value(name)?.to_str() {
                    Some("human") => DiagnosticFormat::Human,
                    Some("json") => DiagnosticFormat::Json,
                    _ => return Err(usage("`--diagnostics` takes `human` or `json`")),
                });
            }
            "--deps" => {
                once(deps.is_some(), name)?;
                deps = Some(target(value(name)?));
            }
            "--limit" => {
                let text = value(name)?;
                let text = text.to_str().unwrap_or("");
                let (limit_name, number) = text
                    .split_once('=')
                    .ok_or_else(|| usage("`--limit` takes `<name>=<value>`"))?;
                let limit = Limit::from_name(limit_name)
                    .ok_or_else(|| usage(format!("unknown limit `{limit_name}`")))?;
                if limits_seen.contains(&limit) {
                    return Err(usage(format!("limit `{limit}` given more than once")));
                }
                limits_seen.push(limit);
                let number: u64 = number.parse().map_err(|_| {
                    usage(format!(
                        "limit `{limit}` needs a whole number, not `{number}`"
                    ))
                })?;
                limits
                    .set(limit, number)
                    .map_err(|err| usage(err.to_string()))?;
            }
            "--poll-interval" => {
                once(poll.is_some(), name)?;
                let text = value(name)?;
                let ms = text
                    .to_str()
                    .and_then(|t| t.parse::<u64>().ok())
                    .filter(|ms| POLL_RANGE_MS.contains(ms))
                    .ok_or_else(|| {
                        usage(format!(
                            "`--poll-interval` takes milliseconds between {} and {}",
                            POLL_RANGE_MS.start(),
                            POLL_RANGE_MS.end()
                        ))
                    })?;
                poll = Some(ms);
            }
            other if !other.is_empty() && other.starts_with('-') && other != "-" => {
                return Err(usage(format!("unknown option `{other}` for `{command}`")));
            }
            _ if input.is_some() => {
                return Err(usage(format!("`{command}` takes exactly one input file")));
            }
            _ => {
                input = Some(if arg == "-" {
                    Input::Stdin
                } else {
                    Input::File(PathBuf::from(arg))
                });
            }
        }
    }
    let Some(input) = input else {
        return Err(usage(format!("`{command}` requires an input file")));
    };

    match command {
        "check" => {
            for (present, name) in [
                (output.is_some(), "--output"),
                (source_map.is_some(), "--source-map"),
                (source_map_content, "--source-map-content"),
                (poll.is_some(), "--poll-interval"),
            ] {
                if present {
                    return Err(usage(format!(
                        "`check` writes no CSS; `{name}` is not allowed"
                    )));
                }
            }
        }
        "watch" => {
            if input == Input::Stdin {
                return Err(usage("`watch` needs an input file, not stdin"));
            }
            if !matches!(output, Some(Target::File(_))) {
                return Err(usage("`watch` requires `--output <file>`"));
            }
            if deps == Some(Target::Stdout) {
                return Err(usage("`watch` cannot write dependencies to stdout"));
            }
        }
        _ => {
            if poll.is_some() {
                return Err(usage("`--poll-interval` applies only to `watch`"));
            }
        }
    }
    let output = match command {
        "check" => None,
        _ => Some(output.unwrap_or(Target::Stdout)),
    };
    if source_map_content && source_map.is_none() {
        return Err(usage("`--source-map-content` requires `--source-map`"));
    }
    if source_map == Some(SourceMapMode::File) && output == Some(Target::Stdout) {
        return Err(usage(
            "a source map file needs `--output <file>`; use `--source-map=inline` with stdout",
        ));
    }
    if deps == Some(Target::Stdout) && output == Some(Target::Stdout) {
        return Err(usage(
            "CSS and dependencies cannot both go to stdout; use `--output <file>` or `--deps <file>`",
        ));
    }
    for target in [&output, &deps].into_iter().flatten() {
        if let Target::File(path) = target {
            let extension = path.extension().map(|ext| ext.to_string_lossy());
            if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("weave")) {
                return Err(usage(format!(
                    "refusing to write output to `{}`: `.weave` files are inputs",
                    path.display()
                )));
            }
        }
    }
    if let (Some(Target::File(css)), Some(Target::File(list))) = (&output, &deps) {
        let map = map_path(css);
        if list == css || (source_map == Some(SourceMapMode::File) && *list == map) {
            return Err(usage(format!(
                "`--deps` and `--output` name the same file `{}`",
                list.display()
            )));
        }
    }

    let options = Options {
        input,
        output,
        root,
        source_map,
        source_map_content,
        diagnostics: diagnostics.unwrap_or_default(),
        deps,
        limits,
        poll_interval: Duration::from_millis(poll.unwrap_or(DEFAULT_POLL_MS)),
    };
    Ok(match command {
        "build" => Command::Build(options),
        "check" => Command::Check(options),
        _ => Command::Watch(options),
    })
}

/// The source map path for a CSS output: `<output>.map`.
pub fn map_path(css: &std::path::Path) -> PathBuf {
    let mut name = css.as_os_str().to_owned();
    name.push(".map");
    PathBuf::from(name)
}

fn target(value: OsString) -> Target {
    if value == "-" {
        Target::Stdout
    } else {
        Target::File(PathBuf::from(value))
    }
}

fn once(seen: bool, name: &str) -> Result<(), UsageError> {
    if seen {
        Err(usage(format!("`{name}` given more than once")))
    } else {
        Ok(())
    }
}

fn no_value(name: &str, value: Option<&str>) -> Result<(), UsageError> {
    match value {
        Some(_) => Err(usage(format!("`{name}` takes no value"))),
        None => Ok(()),
    }
}

fn no_more(command: Command, rest: &[OsString]) -> Result<Command, UsageError> {
    match rest.first() {
        None => Ok(command),
        Some(extra) => Err(usage(format!(
            "unexpected argument `{}`",
            extra.to_string_lossy()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Command, UsageError> {
        parse(args.iter().map(OsString::from))
    }

    fn options(args: &[&str]) -> Options {
        match parse_strs(args) {
            Ok(Command::Build(o) | Command::Check(o) | Command::Watch(o)) => o,
            other => panic!("{args:?}: {other:?}"),
        }
    }

    #[test]
    fn help_and_version_forms() {
        for args in [&["-h"][..], &["--help"], &["help"], &["build", "--help"]] {
            assert_eq!(parse_strs(args), Ok(Command::Help), "{args:?}");
        }
        for args in [&["-V"][..], &["--version"], &["version"]] {
            assert_eq!(parse_strs(args), Ok(Command::Version), "{args:?}");
        }
    }

    #[test]
    fn defaults() {
        let o = options(&["build", "a.weave"]);
        assert_eq!(o.input, Input::File(PathBuf::from("a.weave")));
        assert_eq!(o.output, Some(Target::Stdout));
        assert_eq!(o.source_map, None);
        assert_eq!(o.diagnostics, DiagnosticFormat::Human);
        assert_eq!(o.limits, ResourceLimits::default());
        assert_eq!(o.poll_interval, Duration::from_millis(DEFAULT_POLL_MS));
        assert_eq!(options(&["check", "--", "-odd.weave"]).output, None);
        assert_eq!(
            options(&["check", "--", "-odd.weave"]).input,
            Input::File(PathBuf::from("-odd.weave"))
        );
        assert_eq!(options(&["build", "-"]).input, Input::Stdin);
    }

    #[test]
    fn options_with_values() {
        let o = options(&[
            "build",
            "--output=out.css",
            "--root",
            "src",
            "--source-map",
            "--source-map-content",
            "--diagnostics",
            "json",
            "--deps",
            "deps.txt",
            "--limit",
            "call-depth=64",
            "--limit=output-bytes=10",
            "src/a.weave",
        ]);
        assert_eq!(o.output, Some(Target::File(PathBuf::from("out.css"))));
        assert_eq!(o.root, Some(PathBuf::from("src")));
        assert_eq!(o.source_map, Some(SourceMapMode::File));
        assert!(o.source_map_content);
        assert_eq!(o.diagnostics, DiagnosticFormat::Json);
        assert_eq!(o.deps, Some(Target::File(PathBuf::from("deps.txt"))));
        assert_eq!(o.limits.get(Limit::CallDepth), 64);
        assert_eq!(o.limits.get(Limit::OutputBytes), 10);
        let o = options(&["build", "-o", "-", "--source-map=inline", "a.weave"]);
        assert_eq!(o.source_map, Some(SourceMapMode::Inline));
        let o = options(&["watch", "a.weave", "-o", "x.css", "--poll-interval", "50"]);
        assert_eq!(o.poll_interval, Duration::from_millis(50));
        let o = options(&["check", "a.weave", "--deps", "-"]);
        assert_eq!(o.deps, Some(Target::Stdout));
    }

    #[test]
    fn rejects_invalid_command_lines() {
        for args in [
            &[][..],
            &["compile", "a.weave"],
            &["--verbose"],
            &["build"],
            &["check", "a.weave", "b.weave"],
            &["version", "extra"],
            &["build", "-o"],
            &["build", "-o", "a.css", "-o", "b.css", "a.weave"],
            &["check", "-o", "out.css", "a.weave"],
            &["check", "--source-map=inline", "a.weave"],
            &["build", "--source-map", "a.weave"],
            &["build", "--source-map=both", "-o", "x.css", "a.weave"],
            &["build", "--source-map-content", "-o", "x.css", "a.weave"],
            &["build", "--source-map-content=yes", "a.weave"],
            &["build", "--diagnostics", "xml", "a.weave"],
            &["build", "--deps", "-", "a.weave"],
            &["build", "-o", "x.css", "--deps", "x.css", "a.weave"],
            &[
                "build",
                "-o",
                "x.css",
                "--source-map",
                "--deps",
                "x.css.map",
                "a.weave",
            ],
            &["build", "-o", "lib.weave", "a.weave"],
            &["build", "-o", "lib.WEAVE", "a.weave"],
            &["build", "--limit", "call-depth", "a.weave"],
            &["build", "--limit", "depth=3", "a.weave"],
            &["build", "--limit", "call-depth=0", "a.weave"],
            &["build", "--limit", "call-depth=-1", "a.weave"],
            &["build", "--limit", "call-depth=4097", "a.weave"],
            &[
                "build",
                "--limit",
                "call-depth=8",
                "--limit",
                "call-depth=9",
                "a.weave",
            ],
            &["build", "--poll-interval", "100", "a.weave"],
            &["watch", "a.weave"],
            &["watch", "-o", "-", "a.weave"],
            &["watch", "-o", "x.css", "-"],
            &["watch", "-o", "x.css", "--deps", "-", "a.weave"],
            &["watch", "-o", "x.css", "--poll-interval", "5", "a.weave"],
            &["watch", "-o", "x.css", "--poll-interval", "ms", "a.weave"],
        ] {
            assert!(parse_strs(args).is_err(), "{args:?}");
        }
    }
}
