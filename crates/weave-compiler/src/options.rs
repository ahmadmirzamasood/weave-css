//! Compiler options and resource limits (LANGUAGE.md §17, SECURITY.md).

use std::fmt;

/// One configurable resource limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Limit {
    /// Bytes per source file (E8001).
    FileBytes,
    /// Bytes of all source files together (E8001).
    TotalInputBytes,
    /// Modules per build (E8008).
    Modules,
    /// Combined nesting depth of blocks, expressions and brackets (E8002).
    NestingDepth,
    /// Depth of function calls and recipe applications (E8003).
    CallDepth,
    /// Evaluation steps per build (E8004).
    EvaluationSteps,
    /// Emitted selectors after nesting expansion (E8005).
    EmittedSelectors,
    /// Bytes of emitted CSS (E8006).
    OutputBytes,
    /// Elements per list or map (E8007).
    CollectionElements,
    /// Bytes per string or CSS fragment (E8007).
    StringBytes,
    /// Diagnostics reported per build.
    Diagnostics,
}

impl Limit {
    pub const ALL: [Limit; 11] = [
        Limit::FileBytes,
        Limit::TotalInputBytes,
        Limit::Modules,
        Limit::NestingDepth,
        Limit::CallDepth,
        Limit::EvaluationSteps,
        Limit::EmittedSelectors,
        Limit::OutputBytes,
        Limit::CollectionElements,
        Limit::StringBytes,
        Limit::Diagnostics,
    ];

    /// A stable kebab-case name, suitable for command-line options.
    pub fn name(self) -> &'static str {
        match self {
            Limit::FileBytes => "file-bytes",
            Limit::TotalInputBytes => "total-input-bytes",
            Limit::Modules => "modules",
            Limit::NestingDepth => "nesting-depth",
            Limit::CallDepth => "call-depth",
            Limit::EvaluationSteps => "evaluation-steps",
            Limit::EmittedSelectors => "emitted-selectors",
            Limit::OutputBytes => "output-bytes",
            Limit::CollectionElements => "collection-elements",
            Limit::StringBytes => "string-bytes",
            Limit::Diagnostics => "diagnostics",
        }
    }

    pub fn from_name(name: &str) -> Option<Limit> {
        Limit::ALL.into_iter().find(|limit| limit.name() == name)
    }

    /// The default from LANGUAGE.md §17.
    pub fn default_value(self) -> u64 {
        const MIB: u64 = 1024 * 1024;
        match self {
            Limit::FileBytes => 16 * MIB,
            Limit::TotalInputBytes => 128 * MIB,
            Limit::Modules => 4096,
            Limit::NestingDepth => 128,
            Limit::CallDepth => 128,
            Limit::EvaluationSteps => 1_000_000,
            Limit::EmittedSelectors => 100_000,
            Limit::OutputBytes => 64 * MIB,
            Limit::CollectionElements => 65_536,
            Limit::StringBytes => MIB,
            Limit::Diagnostics => 100,
        }
    }

    /// The largest accepted value. Byte limits are capped by 32-bit span
    /// offsets; depth limits are capped well below what would exhaust a
    /// thread stack in a recursive implementation.
    pub fn max_value(self) -> u64 {
        match self {
            Limit::FileBytes | Limit::TotalInputBytes | Limit::OutputBytes | Limit::StringBytes => {
                u64::from(u32::MAX)
            }
            Limit::NestingDepth | Limit::CallDepth => 4096,
            Limit::Modules | Limit::CollectionElements | Limit::Diagnostics => u64::from(u32::MAX),
            Limit::EvaluationSteps | Limit::EmittedSelectors => u64::MAX,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Limit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A rejected limit override.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitError {
    pub limit: Limit,
    pub value: u64,
}

impl fmt::Display for LimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "limit `{}` must be between 1 and {} (got {})",
            self.limit,
            self.limit.max_value(),
            self.value
        )
    }
}

impl std::error::Error for LimitError {}

/// Every resource limit, each a concrete finite value.
///
/// There is no "unlimited" setting: an override replaces one limit with
/// another finite value and is validated when set, so a `ResourceLimits`
/// value is always valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    values: [u64; Limit::ALL.len()],
}

impl Default for ResourceLimits {
    fn default() -> Self {
        ResourceLimits {
            values: Limit::ALL.map(Limit::default_value),
        }
    }
}

impl ResourceLimits {
    pub fn get(&self, limit: Limit) -> u64 {
        self.values
            .get(limit.index())
            .copied()
            .unwrap_or_else(|| limit.default_value())
    }

    /// Overrides one limit. Zero and values above [`Limit::max_value`] are
    /// rejected.
    pub fn set(&mut self, limit: Limit, value: u64) -> Result<(), LimitError> {
        if value == 0 || value > limit.max_value() {
            return Err(LimitError { limit, value });
        }
        if let Some(slot) = self.values.get_mut(limit.index()) {
            *slot = value;
        }
        Ok(())
    }

    pub fn with(mut self, limit: Limit, value: u64) -> Result<Self, LimitError> {
        self.set(limit, value)?;
        Ok(self)
    }
}

/// The CSS output style. v0.1 defines one style (LANGUAGE.md §15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OutputStyle {
    #[default]
    Expanded,
}

/// Options for one compilation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompileOptions {
    pub limits: ResourceLimits,
    pub output_style: OutputStyle,
    /// Produce a source map ([`crate::SourceMap`]) with the CSS.
    pub source_map: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_specification() {
        let limits = ResourceLimits::default();
        let mib = 1024 * 1024;
        let expected = [
            (Limit::FileBytes, 16 * mib),
            (Limit::TotalInputBytes, 128 * mib),
            (Limit::Modules, 4096),
            (Limit::NestingDepth, 128),
            (Limit::CallDepth, 128),
            (Limit::EvaluationSteps, 1_000_000),
            (Limit::EmittedSelectors, 100_000),
            (Limit::OutputBytes, 64 * mib),
            (Limit::CollectionElements, 65_536),
            (Limit::StringBytes, mib),
            (Limit::Diagnostics, 100),
        ];
        assert_eq!(expected.len(), Limit::ALL.len());
        for (limit, value) in expected {
            assert_eq!(limits.get(limit), value, "{limit}");
        }
    }

    #[test]
    fn zero_never_means_unlimited() {
        for limit in Limit::ALL {
            let mut limits = ResourceLimits::default();
            assert_eq!(limits.set(limit, 0), Err(LimitError { limit, value: 0 }));
            assert_eq!(limits.get(limit), limit.default_value());
        }
    }

    #[test]
    fn values_above_the_maximum_are_rejected() {
        let mut limits = ResourceLimits::default();
        let too_big = u64::from(u32::MAX) + 1;
        assert!(limits.set(Limit::FileBytes, too_big).is_err());
        assert!(limits.set(Limit::CallDepth, 4097).is_err());
        assert!(limits.set(Limit::CallDepth, 4096).is_ok());
        assert!(limits.set(Limit::EvaluationSteps, u64::MAX).is_ok());
        assert_eq!(limits.get(Limit::CallDepth), 4096);
    }

    #[test]
    fn names_round_trip_and_are_unique() {
        for limit in Limit::ALL {
            assert_eq!(Limit::from_name(limit.name()), Some(limit));
            assert!(limit.default_value() <= limit.max_value());
        }
        assert_eq!(Limit::from_name("FileBytes"), None);
    }
}
