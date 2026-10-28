//! Diagnostic reports: human-readable text or one JSON object per build.
//!
//! JSON format (version 1), written as a single line to stderr:
//!
//! ```json
//! {"version":1,"command":"build","success":false,"output":"out.css",
//!  "dependencies":["main.weave"],
//!  "diagnostics":[{"severity":"error","code":"E1002","message":"...",
//!    "location":{"file":"main.weave","line":2,"column":5,"end_line":2,"end_column":9},
//!    "labels":[{"message":"...","location":{...}}],"notes":["..."]}]}
//! ```
//!
//! Lines and columns are 1-based; columns count Unicode scalar values, as in
//! the human format. `location` is `null` for problems without a source
//! position (for example an unreadable input or an output that cannot be
//! written), and `code` is `null` for problems that are not compiler
//! diagnostics.

use std::fmt::Write as _;

use weave_compiler::diagnostic::escape_control;
use weave_compiler::source_map::json_string;
use weave_compiler::{Diagnostic, SourceManager, Span};

/// A problem found outside the compiler: the input, the command line's
/// files or writing outputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub code: Option<&'static str>,
    pub message: String,
}

impl Problem {
    pub fn new(code: Option<&'static str>, message: impl Into<String>) -> Self {
        Problem {
            code,
            message: message.into(),
        }
    }

    pub fn render(&self) -> String {
        let message = escape_control(&self.message);
        match self.code {
            Some(code) => format!("error[{code}]: {message}\n"),
            None => format!("error: {message}\n"),
        }
    }
}

/// Everything one build reports.
#[derive(Debug)]
pub struct Report<'a> {
    pub command: &'static str,
    pub success: bool,
    /// The CSS output as given on the command line (`-` for stdout).
    pub output: Option<String>,
    pub dependencies: Vec<String>,
    pub sources: Option<&'a SourceManager>,
    pub diagnostics: &'a [Diagnostic],
    pub problems: &'a [Problem],
}

impl Report<'_> {
    /// Diagnostics then problems, as rendered text.
    pub fn human(&self) -> String {
        let mut out = String::new();
        if let Some(sources) = self.sources {
            for diagnostic in self.diagnostics {
                out.push_str(&diagnostic.render(sources));
            }
        }
        for problem in self.problems {
            out.push_str(&problem.render());
        }
        out
    }

    /// The JSON object, on one line, with a trailing LF.
    pub fn json(&self) -> String {
        let mut out = String::from("{\"version\":1,\"command\":");
        json_string(self.command, &mut out);
        let _ = write!(out, ",\"success\":{}", self.success);
        out.push_str(",\"output\":");
        match &self.output {
            Some(output) => json_string(output, &mut out),
            None => out.push_str("null"),
        }
        out.push_str(",\"dependencies\":[");
        for (i, dependency) in self.dependencies.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            json_string(dependency, &mut out);
        }
        out.push_str("],\"diagnostics\":[");
        let mut first = true;
        for diagnostic in self.diagnostics {
            if !first {
                out.push(',');
            }
            first = false;
            self.diagnostic(diagnostic, &mut out);
        }
        for problem in self.problems {
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str("{\"severity\":\"error\",\"code\":");
            match problem.code {
                Some(code) => json_string(code, &mut out),
                None => out.push_str("null"),
            }
            out.push_str(",\"message\":");
            json_string(&problem.message, &mut out);
            out.push_str(",\"location\":null,\"labels\":[],\"notes\":[]}");
        }
        out.push_str("]}\n");
        out
    }

    fn diagnostic(&self, diagnostic: &Diagnostic, out: &mut String) {
        out.push_str("{\"severity\":");
        json_string(diagnostic.severity().as_str(), out);
        out.push_str(",\"code\":");
        json_string(diagnostic.code().as_str(), out);
        out.push_str(",\"message\":");
        json_string(diagnostic.message(), out);
        out.push_str(",\"location\":");
        self.location(diagnostic.primary(), out);
        out.push_str(",\"labels\":[");
        for (i, label) in diagnostic.labels().iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"message\":");
            json_string(&label.message, out);
            out.push_str(",\"location\":");
            self.location(Some(label.span), out);
            out.push('}');
        }
        out.push_str("],\"notes\":[");
        for (i, note) in diagnostic.notes().iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            json_string(note, out);
        }
        out.push_str("]}");
    }

    fn location(&self, span: Option<Span>, out: &mut String) {
        let found = span.and_then(|span| {
            let file = self.sources?.get(span.file)?;
            Some((
                file.name(),
                file.line_col(span.start)?,
                file.line_col(span.end)?,
            ))
        });
        match found {
            Some((name, start, end)) => {
                out.push_str("{\"file\":");
                json_string(name, out);
                let _ = write!(
                    out,
                    ",\"line\":{},\"column\":{},\"end_line\":{},\"end_column\":{}}}",
                    start.line, start.column, end.line, end.column
                );
            }
            None => out.push_str("null"),
        }
    }
}
