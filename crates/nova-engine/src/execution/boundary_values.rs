//! Built-in boundary-value generators for common "unhappy path" cases.
//!
//! Hand-writing a values file for every common edge case (empty string, a
//! very long string, negative/zero/huge numbers, unicode, a missing value)
//! is repetitive busywork a developer shouldn't have to redo per field, per
//! project. This module is a small, named library of those common cases —
//! pure value-producing logic, with no I/O and no coupling to requests,
//! sessions, or execution.
//!
//! This is a standalone building block: nothing in this crate consumes it
//! yet. It exists so a future sweep feature (marking one position in a
//! request and resending it once per value) can offer these generators as a
//! built-in value set alongside a project's own values file, without a
//! developer maintaining a wordlist for the common cases by hand.
//!
//! [`BoundaryValue::generate`] is the entry point: given a
//! [`BoundaryGenerator`], it produces the value that generator represents.
//! [`BoundaryGenerator::ALL`] lists every built-in generator, for a caller
//! that wants to offer "run every built-in boundary case" without hand
//! naming each one.

use std::fmt;

/// A named built-in boundary-value generator.
///
/// Each variant represents one common "unhappy path" input a developer
/// would otherwise have to hand-write into a values file: an empty string,
/// an excessively long one, a negative/zero/huge number, a string with
/// multi-byte/non-ASCII content, or the field being absent entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoundaryGenerator {
    /// An empty string (`""`).
    Empty,
    /// A long string — long enough to exercise a length limit, without
    /// being unreasonable to include in test output.
    VeryLong,
    /// A representative negative number.
    Negative,
    /// The literal zero.
    Zero,
    /// A representative very large number.
    Huge,
    /// A string containing representative multi-byte/non-ASCII content
    /// (emoji, combining characters, and right-to-left script).
    Unicode,
    /// The field is absent entirely, rather than present with any value —
    /// meaningfully different from [`BoundaryGenerator::Empty`], which is
    /// present but blank.
    Missing,
}

/// The length, in `char`s, of the string [`BoundaryGenerator::VeryLong`]
/// produces: long enough to be "clearly excessive" for an ordinary field,
/// without being absurd to carry around in test output.
pub const VERY_LONG_LENGTH: usize = 4096;

/// A representative negative number, as text (see [`BoundaryValue`] for why
/// generated numbers are strings).
pub const NEGATIVE_VALUE: &str = "-1";

/// A representative very large number, as text — comfortably past
/// [`i32::MAX`] and even [`u32::MAX`], to stress a naive fixed-width
/// integer parser.
pub const HUGE_VALUE: &str = "9223372036854775807";

/// A string with representative multi-byte/non-ASCII content: an emoji
/// (multi-byte, outside the Basic Multilingual Plane), a combining
/// diacritic (a visual character built from two Unicode scalar values),
/// and a right-to-left Arabic word — the kind of input a naive
/// ASCII-assuming implementation (fixed byte-length truncation, `[u8]`
/// slicing) would mishandle.
pub const UNICODE_VALUE: &str = "héllo 🚀 mañana \u{0301} السلام";

impl BoundaryGenerator {
    /// Every built-in boundary-value generator, in a stable order — for a
    /// caller that wants to run "all the built-in boundary cases" without
    /// naming each one by hand.
    pub const ALL: &'static [BoundaryGenerator] = &[
        BoundaryGenerator::Empty,
        BoundaryGenerator::VeryLong,
        BoundaryGenerator::Negative,
        BoundaryGenerator::Zero,
        BoundaryGenerator::Huge,
        BoundaryGenerator::Unicode,
        BoundaryGenerator::Missing,
    ];

    /// The generator's stable, lowercase name — used to reference a
    /// built-in generator by name (e.g. from a future sweep's value-set
    /// configuration) and matched by [`BoundaryGenerator::parse`].
    pub fn name(self) -> &'static str {
        match self {
            BoundaryGenerator::Empty => "empty",
            BoundaryGenerator::VeryLong => "very_long",
            BoundaryGenerator::Negative => "negative",
            BoundaryGenerator::Zero => "zero",
            BoundaryGenerator::Huge => "huge",
            BoundaryGenerator::Unicode => "unicode",
            BoundaryGenerator::Missing => "missing",
        }
    }

    /// Look up a built-in generator by its [`name`](Self::name), e.g. for
    /// parsing a value-set reference like `"empty"` out of a future sweep
    /// configuration. Returns `None` for a name that isn't one of the
    /// built-ins.
    pub fn parse(name: &str) -> Option<BoundaryGenerator> {
        BoundaryGenerator::ALL
            .iter()
            .copied()
            .find(|generator| generator.name() == name)
    }

    /// Produce the value this generator represents.
    pub fn generate(self) -> BoundaryValue {
        match self {
            BoundaryGenerator::Empty => BoundaryValue::Present(String::new()),
            BoundaryGenerator::VeryLong => BoundaryValue::Present("a".repeat(VERY_LONG_LENGTH)),
            BoundaryGenerator::Negative => BoundaryValue::Present(NEGATIVE_VALUE.to_string()),
            BoundaryGenerator::Zero => BoundaryValue::Present("0".to_string()),
            BoundaryGenerator::Huge => BoundaryValue::Present(HUGE_VALUE.to_string()),
            BoundaryGenerator::Unicode => BoundaryValue::Present(UNICODE_VALUE.to_string()),
            BoundaryGenerator::Missing => BoundaryValue::Missing,
        }
    }
}

impl fmt::Display for BoundaryGenerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The value a [`BoundaryGenerator`] produces.
///
/// Most generators produce a concrete string suitable for substitution
/// wherever a resolved request value already flows as text (headers, query
/// parameters, form/body fields — see
/// [`crate::request::resolve::substitute`]). [`BoundaryGenerator::Missing`]
/// is different in kind, not degree: it represents the field being absent
/// entirely, which a plain string (even `""`) can't distinguish from
/// "present but blank". A future sweep consuming this needs to tell the two
/// apart — "assert the API tolerates an empty value" and "assert the API
/// tolerates the field being left out" are different test cases — so this
/// is an enum rather than always handing back a `String`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryValue {
    /// The field is present, with this value.
    Present(String),
    /// The field is absent entirely.
    Missing,
}

impl BoundaryValue {
    /// The generated text, if this value is present; `None` for
    /// [`BoundaryValue::Missing`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            BoundaryValue::Present(value) => Some(value),
            BoundaryValue::Missing => None,
        }
    }

    /// Whether this value represents the field being left out entirely.
    pub fn is_missing(&self) -> bool {
        matches!(self, BoundaryValue::Missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_present_and_blank() {
        let value = BoundaryGenerator::Empty.generate();
        assert_eq!(value, BoundaryValue::Present(String::new()));
        assert_eq!(value.as_str(), Some(""));
        assert!(!value.is_missing());
    }

    #[test]
    fn very_long_has_the_documented_length() {
        let value = BoundaryGenerator::VeryLong.generate();
        let text = value.as_str().expect("very_long should be present");
        assert_eq!(text.chars().count(), VERY_LONG_LENGTH);
        assert_eq!(text.len(), VERY_LONG_LENGTH); // ASCII, so bytes == chars
    }

    #[test]
    fn negative_parses_as_a_negative_number() {
        let value = BoundaryGenerator::Negative.generate();
        let text = value.as_str().expect("negative should be present");
        let parsed: i64 = text.parse().expect("should parse as an integer");
        assert!(parsed < 0);
    }

    #[test]
    fn zero_is_literal_zero() {
        let value = BoundaryGenerator::Zero.generate();
        assert_eq!(value.as_str(), Some("0"));
    }

    #[test]
    fn huge_parses_as_a_very_large_number() {
        let value = BoundaryGenerator::Huge.generate();
        let text = value.as_str().expect("huge should be present");
        let parsed: i64 = text.parse().expect("should parse as an integer");
        assert!(parsed > i32::MAX as i64);
        assert!(parsed > u32::MAX as i64);
    }

    #[test]
    fn unicode_contains_multibyte_content() {
        let value = BoundaryGenerator::Unicode.generate();
        let text = value.as_str().expect("unicode should be present");
        // A real multi-byte string has more bytes than chars.
        assert!(text.len() > text.chars().count());
        // Contains an emoji (outside the Basic Multilingual Plane).
        assert!(text.chars().any(|c| (c as u32) > 0x1_0000));
        // Contains a right-to-left Arabic character.
        assert!(text.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)));
        // Contains a standalone combining diacritic.
        assert!(text.chars().any(|c| ('\u{0300}'..='\u{036F}').contains(&c)));
    }

    #[test]
    fn missing_has_no_string_value() {
        let value = BoundaryGenerator::Missing.generate();
        assert_eq!(value, BoundaryValue::Missing);
        assert_eq!(value.as_str(), None);
        assert!(value.is_missing());
    }

    #[test]
    fn name_round_trips_through_parse() {
        for generator in BoundaryGenerator::ALL {
            let name = generator.name();
            assert_eq!(BoundaryGenerator::parse(name), Some(*generator));
        }
    }

    #[test]
    fn parse_rejects_unknown_names() {
        assert_eq!(BoundaryGenerator::parse("not_a_generator"), None);
        assert_eq!(BoundaryGenerator::parse(""), None);
        assert_eq!(BoundaryGenerator::parse("Empty"), None); // case-sensitive
    }

    #[test]
    fn all_generators_have_distinct_names() {
        let mut names: Vec<&str> = BoundaryGenerator::ALL.iter().map(|g| g.name()).collect();
        let original_len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), original_len, "generator names must be unique");
    }

    #[test]
    fn display_matches_name() {
        for generator in BoundaryGenerator::ALL {
            assert_eq!(generator.to_string(), generator.name());
        }
    }
}
