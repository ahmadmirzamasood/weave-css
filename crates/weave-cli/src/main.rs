//! The `weave` command-line interface.

#![forbid(unsafe_code)]

mod args;
mod fs_resolver;
mod output;
mod pipeline;
mod report;
mod watch;

use std::io::{self, Read, Write};
use std::process::ExitCode;

use args::{Command, Input, Options, UsageError};
use pipeline::Session;
use weave_compiler::diagnostic::escape_control;
use weave_compiler::{LANGUAGE_VERSION, Limit, VERSION};

/// Exit status for a build that reported errors or could not write output.
const EXIT_FAILED: u8 = 1;
/// Exit status for an invalid command line.
const EXIT_USAGE: u8 = 2;

/// Stack size of the thread that runs commands. The compiler uses explicit
/// stacks, so the largest nesting and call-depth limits (4096) need far less
/// than this; it leaves a wide margin over the 1 MiB Windows main thread.
const STACK_BYTES: usize = 64 * 1024 * 1024;

fn help() -> String {
    let limits: Vec<String> = Limit::ALL
        .iter()
        .map(|limit| format!("{}={}", limit.name(), limit.default_value()))
        .collect();
    format!(
        "\
weave {VERSION} - compiler for the Weave stylesheet language (pre-alpha)

Usage:
  weave build <input.weave> [options]   Compile and write CSS (stdout by default)
  weave check <input.weave> [options]   Compile without writing CSS
  weave watch <input.weave> -o <file> [options]
                                        Build, then rebuild when inputs change
  weave help                            Show this help
  weave version                         Show version information

The input may be `-` to read the entry module from stdin (`build` and
`check`); its `load` paths are then relative to the module root.

Options:
  -o, --output <file|->      Write CSS to <file> (replaced atomically) or `-`
                             for stdout (default for `build`)
      --root <dir>           Module root: no module may be loaded from outside
                             it, symbolic links included. Default: the input
                             file's directory (stdin: the current directory)
      --source-map[=file|inline]
                             Write a source map: `file` (default) writes
                             <output>.map next to the CSS; `inline` embeds it
                             as a data URL. Source paths are relative to the
                             CSS file (stdout: the current directory)
      --source-map-content   Embed the source text in the map (off by default)
      --diagnostics <human|json>
                             Diagnostic format on stderr (default human); json
                             writes one object per build on one line
      --deps <file|->        Write the modules of the build, one path per line
                             relative to the current directory
      --limit <name>=<n>     Override a resource limit (repeatable)
      --poll-interval <ms>   How often `watch` checks inputs (default {poll},
                             10 to 60000)
  -h, --help                 Show this help
  -V, --version              Show version information

Limits and defaults:
  {limits}

Status: this build compiles the plain-CSS subset of Weave (`rule` blocks,
declarations, at-rules, `opaque` blocks, comments) with bindings,
expressions, interpolation, functions, `when`/`each`, recipes, nesting,
modules (`load`, `export`) and the built-ins of docs/BUILTINS.md. The
`color` built-ins fail with error E5001 (not implemented yet).

Outputs are written only after a successful build, through a temporary file
renamed over the target; a failed build leaves earlier output untouched.
Outputs may not overwrite an input module or each other, and may not have
the .weave extension.

Exit status: 0 success, 1 compilation failed or output not written,
2 invalid command line.
",
        poll = args::DEFAULT_POLL_MS,
        limits = limits.join("\n  "),
    )
}

fn version() -> String {
    format!(
        "weave {VERSION}\nlanguage specification: v{LANGUAGE_VERSION}\n\
         compilation: plain-CSS subset, values, functions, recipes, nesting and modules\n\
         tools: build, check, watch (polling), source maps (ECMA-426), JSON diagnostics (phase 08)\n"
    )
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let worker = std::thread::Builder::new()
        .name("weave".to_string())
        .stack_size(STACK_BYTES)
        .spawn(move || run_command(args));
    match worker.map(|handle| handle.join()) {
        Ok(Ok(code)) => ExitCode::from(code),
        _ => {
            eprintln!("error: internal failure");
            ExitCode::from(EXIT_FAILED)
        }
    }
}

fn run_command(args: Vec<std::ffi::OsString>) -> u8 {
    match args::parse(args) {
        Ok(Command::Help) => print_stdout(&help()),
        Ok(Command::Version) => print_stdout(&version()),
        Ok(Command::Build(options)) => build(options, "build"),
        Ok(Command::Check(options)) => build(options, "check"),
        Ok(Command::Watch(options)) => {
            let mut session = Session::new(options, "watch", None);
            match watch::run(&mut session) {
                Ok(()) => 0,
                Err(_) => EXIT_FAILED,
            }
        }
        Err(UsageError(message)) => {
            eprintln!(
                "error: {}\n\nRun `weave --help` for usage.",
                escape_control(&message)
            );
            EXIT_USAGE
        }
    }
}

fn print_stdout(text: &str) -> u8 {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => 0,
        Err(_) => EXIT_FAILED,
    }
}

fn build(options: Options, command: &'static str) -> u8 {
    let stdin = match options.input {
        Input::Stdin => {
            // Read one byte past the file limit so the compiler reports E8001
            // without this reading an unbounded stream.
            let max = options.limits.get(Limit::FileBytes).saturating_add(1);
            let mut bytes = Vec::new();
            if let Err(err) = io::stdin().lock().take(max).read_to_end(&mut bytes) {
                eprintln!("error: cannot read stdin: {err}");
                return EXIT_FAILED;
            }
            Some(bytes)
        }
        Input::File(_) => None,
    };
    let mut session = Session::new(options, command, stdin);
    let outcome = session.run();
    let _ = pipeline::write_stderr(&outcome.report);
    if outcome.success { 0 } else { EXIT_FAILED }
}
