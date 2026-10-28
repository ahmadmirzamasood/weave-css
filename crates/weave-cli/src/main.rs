//! The `weave` command-line interface.

#![forbid(unsafe_code)]

mod args;
mod fs_resolver;

use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use args::{Command, UsageError};
use fs_resolver::FsResolver;
use weave_compiler::diagnostic::escape_control;
use weave_compiler::{CompileOptions, LANGUAGE_VERSION, VERSION, compile};

/// Exit status for a build that reported errors.
const EXIT_FAILED: u8 = 1;
/// Exit status for an invalid command line.
const EXIT_USAGE: u8 = 2;

fn help() -> String {
    format!(
        "\
weave {VERSION} - compiler for the Weave stylesheet language (pre-alpha)

Usage:
  weave build <input.weave>   Compile the input and write CSS to stdout
  weave check <input.weave>   Compile the input without writing output
  weave help                  Show this help
  weave version               Show version information

Options:
  -h, --help       Show this help
  -V, --version    Show version information

Status: this build compiles the plain-CSS subset of Weave (`rule` blocks,
declarations, at-rules, `opaque` blocks, comments) with bindings,
expressions, interpolation, functions, `when`/`each`, recipes, nesting,
modules (`load`, `export`) and the built-ins of docs/BUILTINS.md. The
`color` built-ins fail with error E5001 (not implemented yet).

Modules are resolved relative to the file that loads them. The directory of
the input file is the module root: no module, including through symbolic
links, may be loaded from outside it.

Exit status: 0 success, 1 compilation failed, 2 invalid command line.
"
    )
}

fn version() -> String {
    format!(
        "weave {VERSION}\nlanguage specification: v{LANGUAGE_VERSION}\ncompilation: plain-CSS subset, values, functions, recipes, nesting and modules (phase 07)\n"
    )
}

fn main() -> ExitCode {
    match args::parse(std::env::args_os().skip(1)) {
        Ok(Command::Help) => print_stdout(&help()),
        Ok(Command::Version) => print_stdout(&version()),
        Ok(Command::Build { input }) => run(&input, true),
        Ok(Command::Check { input }) => run(&input, false),
        Err(UsageError(message)) => {
            eprintln!(
                "error: {}\n\nRun `weave --help` for usage.",
                escape_control(&message)
            );
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn print_stdout(text: &str) -> ExitCode {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(EXIT_FAILED),
    }
}

fn run(input: &Path, write_css: bool) -> ExitCode {
    let shown = escape_control(&input.to_string_lossy());
    let (mut resolver, entry) = match FsResolver::for_entry(input) {
        Ok(found) => found,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            eprintln!("error[E7001]: input file `{shown}` not found");
            return ExitCode::from(EXIT_FAILED);
        }
        Err(err) => {
            eprintln!(
                "error: cannot use `{shown}` as input: {}",
                escape_control(&err.to_string())
            );
            return ExitCode::from(EXIT_FAILED);
        }
    };

    let compilation = compile(&entry, &mut resolver, &CompileOptions::default());
    match compilation.outcome {
        Ok(output) => {
            let mut stderr = io::stderr().lock();
            for warning in &output.warnings {
                let _ = stderr.write_all(warning.render(&compilation.sources).as_bytes());
            }
            if write_css {
                print_stdout(&output.css)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(diagnostics) => {
            let mut stderr = io::stderr().lock();
            for diagnostic in &diagnostics {
                let _ = stderr.write_all(diagnostic.render(&compilation.sources).as_bytes());
            }
            ExitCode::from(EXIT_FAILED)
        }
    }
}
