//! Numbers with units (TYPES.md §2, §5).
//!
//! A number is a binary64 value with an optional unit. Every literal and
//! every result must be finite with magnitude at most 1e12 (E3002). The
//! quantized value `q(x) = roundTiesEven(x × 1e6)` drives serialization,
//! equality, ordering and integer tests. Only IEEE-exact operations are
//! used, so results are identical on every platform.

use std::cmp::Ordering;
use std::fmt::Write as _;

use crate::diagnostic::Code;

/// The largest permitted magnitude (TYPES.md §2.1).
pub(crate) const MAX_MAGNITUDE: f64 = 1e12;

const SCALE: f64 = 1e6;
const SCALE_INT: u64 = 1_000_000;

/// Unit categories (TYPES.md §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    AbsoluteLength,
    Angle,
    Time,
    Frequency,
    Resolution,
    RelativeLength,
    Percentage,
    Flex,
}

impl Category {
    fn convertible(self) -> bool {
        matches!(
            self,
            Category::AbsoluteLength
                | Category::Angle
                | Category::Time
                | Category::Frequency
                | Category::Resolution
        )
    }
}

/// A known unit: canonical spelling, category and the conversion factor
/// `n / d` relative to the category's canonical unit (TYPES.md §2.3).
#[derive(Debug)]
struct UnitInfo {
    name: &'static str,
    category: Category,
    n: f64,
    d: f64,
}

const fn unit(name: &'static str, category: Category, n: f64, d: f64) -> UnitInfo {
    UnitInfo {
        name,
        category,
        n,
        d,
    }
}

const fn relative(name: &'static str) -> UnitInfo {
    unit(name, Category::RelativeLength, 1.0, 1.0)
}

use Category as C;

static UNITS: &[UnitInfo] = &[
    unit("px", C::AbsoluteLength, 1.0, 1.0),
    unit("cm", C::AbsoluteLength, 4800.0, 127.0),
    unit("mm", C::AbsoluteLength, 480.0, 127.0),
    unit("Q", C::AbsoluteLength, 120.0, 127.0),
    unit("in", C::AbsoluteLength, 96.0, 1.0),
    unit("pt", C::AbsoluteLength, 4.0, 3.0),
    unit("pc", C::AbsoluteLength, 16.0, 1.0),
    unit("deg", C::Angle, 1.0, 1.0),
    unit("grad", C::Angle, 9.0, 10.0),
    unit("rad", C::Angle, 57.295_779_513_082_32, 1.0),
    unit("turn", C::Angle, 360.0, 1.0),
    unit("ms", C::Time, 1.0, 1.0),
    unit("s", C::Time, 1000.0, 1.0),
    unit("Hz", C::Frequency, 1.0, 1.0),
    unit("kHz", C::Frequency, 1000.0, 1.0),
    unit("dppx", C::Resolution, 1.0, 1.0),
    unit("x", C::Resolution, 1.0, 1.0),
    unit("dpi", C::Resolution, 1.0, 96.0),
    unit("dpcm", C::Resolution, 127.0, 4800.0),
    unit("%", C::Percentage, 1.0, 1.0),
    unit("fr", C::Flex, 1.0, 1.0),
    relative("em"),
    relative("rem"),
    relative("ex"),
    relative("rex"),
    relative("cap"),
    relative("rcap"),
    relative("ch"),
    relative("rch"),
    relative("ic"),
    relative("ric"),
    relative("lh"),
    relative("rlh"),
    relative("vw"),
    relative("vh"),
    relative("vi"),
    relative("vb"),
    relative("vmin"),
    relative("vmax"),
    relative("svw"),
    relative("svh"),
    relative("svi"),
    relative("svb"),
    relative("svmin"),
    relative("svmax"),
    relative("lvw"),
    relative("lvh"),
    relative("lvi"),
    relative("lvb"),
    relative("lvmin"),
    relative("lvmax"),
    relative("dvw"),
    relative("dvh"),
    relative("dvi"),
    relative("dvb"),
    relative("dvmin"),
    relative("dvmax"),
    relative("cqw"),
    relative("cqh"),
    relative("cqi"),
    relative("cqb"),
    relative("cqmin"),
    relative("cqmax"),
];

/// A unit. Units compare ASCII case-insensitively; known units are stored
/// by table index and unknown ones in ASCII lowercase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    None,
    Known(usize),
    Other(Box<str>),
}

impl Unit {
    /// The unit spelled `text` (`[A-Za-z]+` or `%`).
    pub fn parse(text: &str) -> Unit {
        if text.is_empty() {
            return Unit::None;
        }
        match UNITS.iter().position(|u| u.name.eq_ignore_ascii_case(text)) {
            Some(index) => Unit::Known(index),
            None => Unit::Other(text.to_ascii_lowercase().into()),
        }
    }

    fn info(&self) -> Option<&'static UnitInfo> {
        match self {
            Unit::Known(index) => UNITS.get(*index),
            _ => None,
        }
    }

    /// The canonical spelling; empty for unitless numbers.
    pub fn name(&self) -> &str {
        match self {
            Unit::None => "",
            Unit::Known(_) => self.info().map_or("", |u| u.name),
            Unit::Other(name) => name,
        }
    }

    /// The unit as shown in messages.
    pub fn describe(&self) -> String {
        match self {
            Unit::None => "unitless".to_string(),
            _ => format!("`{}`", self.name()),
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Unit::None)
    }
}

/// A failed numeric operation: the code and message of its diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NumError {
    pub code: Code,
    pub message: String,
}

fn error(code: Code, message: impl Into<String>) -> NumError {
    NumError {
        code,
        message: message.into(),
    }
}

/// A finite number with an optional unit.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Number {
    pub value: f64,
    pub unit: Unit,
}

/// Checks the magnitude bound (E3002).
pub(crate) fn checked(value: f64) -> Result<f64, NumError> {
    if value.is_finite() && value.abs() <= MAX_MAGNITUDE {
        Ok(value)
    } else {
        Err(error(
            Code::E3002,
            "numeric overflow: results must be finite with magnitude at most 1e12",
        ))
    }
}

/// `q(x)`: exact because `|x| ≤ 1e12` keeps `|x × 1e6| ≤ 1e18`.
fn quantized(value: f64) -> i64 {
    (value * SCALE).round_ties_even() as i64
}

/// Whether two units can be combined: both unitless, the same unit, or
/// units of one convertible category.
fn compatible(a: &Unit, b: &Unit) -> bool {
    if a == b {
        return true;
    }
    match (a.info(), b.info()) {
        (Some(x), Some(y)) => x.category == y.category && x.category.convertible(),
        _ => false,
    }
}

/// Converts `value` from unit `from` to unit `to` (TYPES.md §2.3). Callers
/// ensure the units are compatible.
fn convert(value: f64, from: &Unit, to: &Unit) -> f64 {
    if from == to {
        return value;
    }
    match (from.info(), to.info()) {
        (Some(a), Some(b)) => (value * (a.n * b.d)) / (a.d * b.n),
        _ => value,
    }
}

impl Number {
    pub fn new(value: f64, unit: Unit) -> Self {
        Number { value, unit }
    }

    /// A literal's value; E3002 beyond the magnitude bound.
    pub fn literal(digits: &str, unit: Unit) -> Result<Self, NumError> {
        let value = digits
            .parse::<f64>()
            .map_err(|_| error(Code::E3002, "invalid number"))?;
        Ok(Number::new(checked(value)?, unit))
    }

    fn result(value: f64, unit: Unit) -> Result<Self, NumError> {
        Ok(Number::new(checked(value)?, unit))
    }

    /// The right operand converted to this number's unit, if compatible.
    fn converted(&self, right: &Number) -> Option<f64> {
        compatible(&self.unit, &right.unit).then(|| convert(right.value, &right.unit, &self.unit))
    }

    fn incompatible(&self, op: &str, right: &Number) -> NumError {
        error(
            Code::E3001,
            format!(
                "incompatible units: cannot {op} {} and {}",
                self.unit.describe(),
                right.unit.describe()
            ),
        )
    }

    pub fn add(&self, right: &Number) -> Result<Number, NumError> {
        let r = self
            .converted(right)
            .ok_or_else(|| self.incompatible("add", right))?;
        Number::result(self.value + r, self.unit.clone())
    }

    pub fn sub(&self, right: &Number) -> Result<Number, NumError> {
        let r = self
            .converted(right)
            .ok_or_else(|| self.incompatible("subtract", right))?;
        Number::result(self.value - r, self.unit.clone())
    }

    pub fn rem(&self, right: &Number) -> Result<Number, NumError> {
        let r = self
            .converted(right)
            .ok_or_else(|| self.incompatible("take the remainder of", right))?;
        if r == 0.0 {
            return Err(error(Code::E3003, "remainder by zero"));
        }
        Number::result(self.value % r, self.unit.clone())
    }

    pub fn mul(&self, right: &Number) -> Result<Number, NumError> {
        let unit = match (self.unit.is_none(), right.unit.is_none()) {
            (true, true) => Unit::None,
            (false, true) => self.unit.clone(),
            (true, false) => right.unit.clone(),
            (false, false) => {
                return Err(error(
                    Code::E3004,
                    format!(
                        "cannot multiply {} by {}: Weave has no compound units",
                        self.unit.describe(),
                        right.unit.describe()
                    ),
                ));
            }
        };
        Number::result(self.value * right.value, unit)
    }

    pub fn div(&self, right: &Number) -> Result<Number, NumError> {
        let (divisor, unit) = match (self.unit.is_none(), right.unit.is_none()) {
            (_, true) => (right.value, self.unit.clone()),
            (true, false) => {
                return Err(error(
                    Code::E3004,
                    format!(
                        "cannot divide a unitless number by {}: Weave has no inverse units",
                        right.unit.describe()
                    ),
                ));
            }
            (false, false) => match self.converted(right) {
                Some(r) => (r, Unit::None),
                None => {
                    return Err(error(
                        Code::E3004,
                        format!(
                            "cannot divide {} by {}: the units are not convertible",
                            self.unit.describe(),
                            right.unit.describe()
                        ),
                    ));
                }
            },
        };
        if divisor == 0.0 {
            return Err(error(Code::E3003, "division by zero"));
        }
        Number::result(self.value / divisor, unit)
    }

    pub fn neg(&self) -> Number {
        Number::new(-self.value, self.unit.clone())
    }

    /// Ordering of compatible numbers by quantized value (E3001 otherwise).
    pub fn compare(&self, right: &Number) -> Result<Ordering, NumError> {
        let r = self
            .converted(right)
            .ok_or_else(|| self.incompatible("compare", right))?;
        Ok(quantized(self.value).cmp(&quantized(r)))
    }

    /// `==`: false across incompatible units, never an error.
    pub fn equals(&self, right: &Number) -> bool {
        self.converted(right)
            .is_some_and(|r| quantized(self.value) == quantized(r))
    }

    /// The value as an integer, if it is one (`q mod 10^6 = 0`).
    pub fn as_integer(&self) -> Option<i64> {
        let q = quantized(self.value);
        (q % SCALE_INT as i64 == 0).then_some(q / SCALE_INT as i64)
    }

    /// Value-context serialization (TYPES.md §5).
    pub fn serialize(&self) -> String {
        let q = quantized(self.value);
        let mut out = String::new();
        if q == 0 {
            out.push('0');
        } else {
            if q < 0 {
                out.push('-');
            }
            let magnitude = q.unsigned_abs();
            let _ = write!(out, "{}", magnitude / SCALE_INT);
            let fraction = magnitude % SCALE_INT;
            if fraction != 0 {
                let digits = format!("{fraction:06}");
                out.push('.');
                out.push_str(digits.trim_end_matches('0'));
            }
        }
        out.push_str(self.unit.name());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(value: f64, unit: &str) -> Number {
        Number::new(value, Unit::parse(unit))
    }

    #[test]
    fn unit_table_is_consistent() {
        let mut names: Vec<String> = UNITS.iter().map(|u| u.name.to_ascii_lowercase()).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate unit names");
        assert_eq!(UNITS.len(), 63);
        for info in UNITS {
            assert!(info.n > 0.0 && info.d > 0.0);
        }
    }

    #[test]
    fn units_are_case_insensitive_and_canonical() {
        assert_eq!(n(1.0, "PX").serialize(), "1px");
        assert_eq!(n(1.0, "q").serialize(), "1Q");
        assert_eq!(n(1.0, "KHZ").serialize(), "1kHz");
        assert_eq!(n(1.0, "Foo").serialize(), "1foo");
        assert_eq!(Unit::parse("FOO"), Unit::parse("foo"));
        assert_eq!(n(2.0, "%").serialize(), "2%");
    }

    #[test]
    fn serialization() {
        let cases = [
            (16.0, "px", "16px"),
            (1.0 / 3.0, "", "0.333333"),
            (-2.5, "rem", "-2.5rem"),
            (-0.0, "px", "0px"),
            (1_000_000.0, "", "1000000"),
            (1e12, "", "1000000000000"),
            (-1e12, "", "-1000000000000"),
            (0.5, "", "0.5"),
            (0.000_000_4, "", "0"),
            (-0.000_000_4, "", "0"),
            (0.000_000_5, "", "0"),
            (0.000_001_5, "", "0.000002"),
            (0.000_002_5, "", "0.000002"),
            (2.0 / 3.0 * 3.0, "", "2"),
            (123.456_789_9, "", "123.45679"),
        ];
        for (value, unit, expected) in cases {
            assert_eq!(n(value, unit).serialize(), expected, "{value}");
        }
    }

    #[test]
    fn conversions_follow_the_specified_order() {
        let a = n(1.0, "in").add(&n(1.0, "cm")).unwrap();
        assert_eq!(a.serialize(), "1.393701in");
        let b = n(1.0, "cm").add(&n(1.0, "in")).unwrap();
        assert_eq!(b.serialize(), "3.54cm");
        assert_eq!(
            n(1.0, "turn").div(&n(1.0, "deg")).unwrap().serialize(),
            "360"
        );
        assert_eq!(
            n(100.0, "ms").add(&n(1.0, "s")).unwrap().serialize(),
            "1100ms"
        );
        assert_eq!(
            n(1.0, "rad").sub(&n(0.0, "deg")).unwrap().serialize(),
            "1rad"
        );
        assert_eq!(
            n(180.0, "deg").add(&n(0.0, "rad")).unwrap().serialize(),
            "180deg"
        );
        assert_eq!(
            n(1.0, "dppx").add(&n(96.0, "dpi")).unwrap().serialize(),
            "2dppx"
        );
        assert_eq!(n(1.0, "x").add(&n(1.0, "dppx")).unwrap().serialize(), "2x");
        assert!(n(1.0, "in").equals(&n(96.0, "px")));
        assert!(n(1.0, "kHz").equals(&n(1000.0, "Hz")));
    }

    #[test]
    fn incompatible_units_and_unit_rules() {
        let codes = |r: Result<Number, NumError>| r.map(|n| n.serialize()).map_err(|e| e.code);
        assert_eq!(codes(n(1.0, "px").add(&n(1.0, "em"))), Err(Code::E3001));
        assert_eq!(codes(n(1.0, "px").add(&n(1.0, ""))), Err(Code::E3001));
        assert_eq!(codes(n(1.0, "").sub(&n(1.0, "px"))), Err(Code::E3001));
        assert_eq!(codes(n(100.0, "%").sub(&n(4.0, "px"))), Err(Code::E3001));
        assert_eq!(codes(n(1.0, "em").add(&n(1.0, "EM"))), Ok("2em".into()));
        assert_eq!(codes(n(1.0, "foo").add(&n(1.0, "bar"))), Err(Code::E3001));
        assert_eq!(codes(n(10.0, "px").mul(&n(2.0, ""))), Ok("20px".into()));
        assert_eq!(codes(n(2.0, "").mul(&n(1.5, "rem"))), Ok("3rem".into()));
        assert_eq!(codes(n(10.0, "px").mul(&n(2.0, "px"))), Err(Code::E3004));
        assert_eq!(codes(n(1.0, "").div(&n(2.0, "px"))), Err(Code::E3004));
        assert_eq!(codes(n(1.0, "px").div(&n(2.0, "em"))), Err(Code::E3004));
        assert_eq!(codes(n(10.0, "px").div(&n(4.0, ""))), Ok("2.5px".into()));
        assert_eq!(codes(n(7.0, "px").rem(&n(3.0, "px"))), Ok("1px".into()));
        assert_eq!(codes(n(-7.0, "").rem(&n(3.0, ""))), Ok("-1".into()));
        assert_eq!(codes(n(7.0, "").rem(&n(-3.0, ""))), Ok("1".into()));
        assert!(!n(0.0, "").equals(&n(0.0, "px")));
        assert!(n(1.0, "px").compare(&n(1.0, "em")).is_err());
        assert_eq!(n(-1.0, "rem").neg().serialize(), "1rem");
    }

    #[test]
    fn division_and_remainder_by_zero() {
        for zero in [0.0, -0.0] {
            assert_eq!(n(1.0, "").div(&n(zero, "")).unwrap_err().code, Code::E3003);
            assert_eq!(
                n(1.0, "px").div(&n(zero, "px")).unwrap_err().code,
                Code::E3003
            );
            assert_eq!(n(1.0, "").rem(&n(zero, "")).unwrap_err().code, Code::E3003);
        }
        // A tiny divisor is not zero: the result overflows instead.
        assert_eq!(n(1.0, "").div(&n(1e-13, "")).unwrap_err().code, Code::E3002);
    }

    #[test]
    fn magnitude_bound_is_inclusive() {
        assert!(Number::literal("1000000000000", Unit::None).is_ok());
        assert_eq!(
            Number::literal("1000000000000.5", Unit::None)
                .unwrap_err()
                .code,
            Code::E3002
        );
        assert_eq!(
            Number::literal(&"9".repeat(400), Unit::None)
                .unwrap_err()
                .code,
            Code::E3002
        );
        let max = n(1e12, "");
        assert!(max.add(&n(0.0, "")).is_ok());
        assert_eq!(max.add(&n(1.0, "")).unwrap_err().code, Code::E3002);
        assert_eq!(max.mul(&n(10.0, "")).unwrap_err().code, Code::E3002);
        assert_eq!(max.neg().serialize(), "-1000000000000");
    }

    #[test]
    fn quantized_equality_ordering_and_integers() {
        assert!(n(0.1 + 0.2, "").equals(&n(0.3, "")));
        assert_eq!(
            n(0.1 + 0.2, "").compare(&n(0.3, "")).unwrap(),
            Ordering::Equal
        );
        assert_eq!(n(1.0, "cm").compare(&n(1.0, "in")).unwrap(), Ordering::Less);
        assert_eq!(n(3.0, "").as_integer(), Some(3));
        assert_eq!(n(-2.0, "").as_integer(), Some(-2));
        assert_eq!(n(2.5, "").as_integer(), None);
        assert_eq!(n(2.000_000_1, "").as_integer(), Some(2));
        assert!(n(-0.0, "").equals(&n(0.0, "")));
    }
}
