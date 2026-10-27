//! Command-line parsing.

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    /// Compile and write CSS to stdout.
    Build {
        input: PathBuf,
    },
    /// Compile without writing output.
    Check {
        input: PathBuf,
    },
}

/// An invalid command line; the message is shown with the usage summary.
#[derive(Debug, PartialEq, Eq)]
pub struct UsageError(pub String);

/// Parses the arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, UsageError> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Err(UsageError("missing command".to_string()));
    };
    let rest: Vec<OsString> = args.collect();
    let command = match first.to_str() {
        Some("-h" | "--help" | "help") => return no_more(Command::Help, &rest),
        Some("-V" | "--version" | "version") => return no_more(Command::Version, &rest),
        Some("build") => "build",
        Some("check") => "check",
        _ => {
            return Err(UsageError(format!(
                "unknown command `{}`",
                first.to_string_lossy()
            )));
        }
    };

    let mut input: Option<PathBuf> = None;
    let mut options_done = false;
    for arg in rest {
        let flag = if options_done { None } else { arg.to_str() };
        match flag {
            Some("-h" | "--help") => return Ok(Command::Help),
            Some("--") => options_done = true,
            Some(other) if other.starts_with('-') && other != "-" => {
                return Err(UsageError(format!(
                    "unknown option `{other}` for `{command}`"
                )));
            }
            _ if input.is_some() => {
                return Err(UsageError(format!(
                    "`{command}` takes exactly one input file"
                )));
            }
            _ => input = Some(PathBuf::from(arg)),
        }
    }
    let Some(input) = input else {
        return Err(UsageError(format!("`{command}` requires an input file")));
    };
    Ok(match command {
        "build" => Command::Build { input },
        _ => Command::Check { input },
    })
}

fn no_more(command: Command, rest: &[OsString]) -> Result<Command, UsageError> {
    match rest.first() {
        None => Ok(command),
        Some(extra) => Err(UsageError(format!(
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
    fn build_and_check_take_one_input() {
        assert_eq!(
            parse_strs(&["build", "a.weave"]),
            Ok(Command::Build {
                input: PathBuf::from("a.weave")
            })
        );
        assert_eq!(
            parse_strs(&["check", "--", "-odd.weave"]),
            Ok(Command::Check {
                input: PathBuf::from("-odd.weave")
            })
        );
    }

    #[test]
    fn rejects_invalid_command_lines() {
        for args in [
            &[][..],
            &["compile", "a.weave"],
            &["--verbose"],
            &["build"],
            &["check", "a.weave", "b.weave"],
            &["build", "-o", "out.css", "a.weave"],
            &["version", "extra"],
        ] {
            assert!(parse_strs(args).is_err(), "{args:?}");
        }
    }
}
