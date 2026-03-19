use rand::RngExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::{ValidationError, ValidationResult};

#[derive(
    Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema, garde::Validate,
)]
pub struct Flag {
    #[garde(pattern(r"^[a-zA-Z0-9]+$"))]
    #[garde(length(min = 3, max = 6))]
    pub prefix: String,
    #[serde(default)]
    #[garde(skip)]
    pub format: FlagFormat,
    #[garde(length(min = 1, max = 127))]
    pub body: String,
    #[serde(default)]
    #[garde(dive)]
    pub random: Option<FlagRandomRules>,
    #[serde(default)]
    #[garde(dive)]
    pub options: FlagMatchOptions,
    /// Scoring configuration for dynamic points
    #[serde(default)]
    #[garde(dive)]
    pub scoring: FlagScoring,
}

#[derive(Debug, Default, Clone, Copy, JsonSchema, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FlagFormat {
    #[default]
    Curly, // prefix{body}
    Round,      // prefix(body)
    Square,     // prefix[body]
    Angle,      // prefix<body>
    Underscore, // prefix_body
}

#[derive(
    Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate,
)]
pub struct FlagRandomRules {
    #[serde(default = "default_random_len")]
    #[garde(range(min = 1, max = 63))]
    pub len: u8,
    #[serde(default = "default_random_upper")]
    #[garde(skip)]
    pub upper: bool,
    #[serde(default = "default_random_lower")]
    #[garde(skip)]
    pub lower: bool,
    #[serde(default = "default_random_digits")]
    #[garde(skip)]
    pub digits: bool,
    #[serde(default = "default_random_special_chars")]
    #[garde(ascii)]
    #[garde(custom(validate_unique_chars))]
    pub special: String,
}

#[derive(
    Debug, Default, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate,
)]
pub struct FlagMatchOptions {
    #[serde(rename = "ignoreCase", default)]
    #[garde(skip)]
    pub ignore_case: bool,
    #[serde(rename = "ignoreRandom", default)]
    #[garde(skip)]
    pub ignore_random: bool,
}

/// Controls dynamic scoring for a flag based on solve count.
#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq, Eq, garde::Validate)]
pub struct FlagScoring {
    /// Base points when no one has solved (default: 500)
    #[serde(rename = "basePoints", default = "default_base_points")]
    #[garde(range(min = 1, max = 10000))]
    pub base_points: u32,
    /// Minimum points floor (default: 100)
    #[serde(rename = "minPoints", default = "default_min_points")]
    #[garde(range(min = 0, max = 10000))]
    pub min_points: u32,
    /// Decay factor - higher = slower decay (default: 15)
    #[serde(default = "default_decay")]
    #[garde(range(min = 1, max = 100))]
    pub decay: u32,
}

impl Default for FlagScoring {
    fn default() -> Self {
        Self {
            base_points: default_base_points(),
            min_points: default_min_points(),
            decay: default_decay(),
        }
    }
}

impl FlagScoring {
    /// Calculate points for this flag given the number of solves.
    /// Uses formula: `max(min_points`, `base_points` / (1 + (solves / decay)))
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "points are always positive and within u32 range"
    )]
    pub fn calculate_points(&self, solves: u32) -> u32 {
        if solves == 0 {
            return self.base_points;
        }
        let points =
            f64::from(self.base_points) / (1.0 + (f64::from(solves) / f64::from(self.decay)));
        points.round().max(f64::from(self.min_points)) as u32
    }
}

const fn default_base_points() -> u32 {
    500
}

const fn default_min_points() -> u32 {
    50
}

const fn default_decay() -> u32 {
    6
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "garde custom validator signature requirement"
)]
fn validate_unique_chars(value: &str, _: &()) -> garde::Result {
    let mut seen = HashSet::new();
    for c in value.chars() {
        if !seen.insert(c) {
            return Err(garde::Error::new(
                "Flag random special must contain unique characters",
            ));
        }
    }
    Ok(())
}

const fn default_random_len() -> u8 {
    8
}

const fn default_random_upper() -> bool {
    true
}

const fn default_random_lower() -> bool {
    true
}

const fn default_random_digits() -> bool {
    true
}

fn default_random_special_chars() -> String {
    String::new()
}

impl Flag {
    pub fn validate_strict(&self) -> ValidationResult<()> {
        garde::Validate::validate(self).map_err(ValidationError::from_garde)
    }

    #[must_use]
    pub fn verify(&self, flag: &str) -> bool {
        let prefix = &self.prefix;
        let body = &self.body;
        let (start, end) = match self.format {
            FlagFormat::Curly => (format!("{prefix}{{"), "}"),
            FlagFormat::Round => (format!("{prefix}("), ")"),
            FlagFormat::Square => (format!("{prefix}["), "]"),
            FlagFormat::Angle => (format!("{prefix}<"), ">"),
            FlagFormat::Underscore => (format!("{prefix}_"), ""),
        };

        if !flag.starts_with(&start) || !flag.ends_with(end) {
            return false;
        }

        if flag.len() < start.len() + body.len() + end.len() {
            return false;
        }

        let content_start = start.len();
        let content_end = flag.len() - end.len();
        let content = &flag[content_start..content_end];

        if content.len() < body.len() {
            return false;
        }

        let (flag_body, flag_random) = content.split_at(body.len());
        let body_matches = if self.options.ignore_case {
            flag_body.eq_ignore_ascii_case(body)
        } else {
            flag_body == body
        };

        if !body_matches {
            return false;
        }

        if let Some(random) = &self.random {
            if !self.options.ignore_random {
                if flag_random.len() != random.len as usize {
                    return false;
                }
                if !random.verify(flag_random, &self.options) {
                    return false;
                }
            }
        } else if !flag_random.is_empty() {
            return false;
        }

        true
    }

    #[must_use]
    pub fn generate(&self) -> String {
        let prefix = &self.prefix;
        let mut body = self.body.clone();
        if let Some(random) = &self.random {
            body.push_str(&random.generate());
        }
        match self.format {
            FlagFormat::Curly => format!("{prefix}{{{body}}}"),
            FlagFormat::Round => format!("{prefix}({body})"),
            FlagFormat::Square => format!("{prefix}[{body}]"),
            FlagFormat::Angle => format!("{prefix}<{body}>"),
            FlagFormat::Underscore => format!("{prefix}_{body}"),
        }
    }
}

impl FlagRandomRules {
    #[must_use]
    pub fn generate(&self) -> String {
        let mut body = String::new();
        let alphabet = self.alphabet();
        let mut rng = rand::rng();
        let value: String = (0..self.len)
            .map(|_| {
                let idx = rng.random_range(0..alphabet.len());
                alphabet.as_bytes()[idx] as char
            })
            .collect();
        debug_assert_eq!(value.len(), self.len as usize);
        body.push_str(&value);
        body
    }

    #[must_use]
    pub fn verify(&self, text: &str, options: &FlagMatchOptions) -> bool {
        let alphabet = if options.ignore_case {
            self.alphabet().to_lowercase()
        } else {
            self.alphabet()
        };

        let alphabet = alphabet.chars().collect::<HashSet<char>>();
        for c in text.chars() {
            if options.ignore_case && alphabet.contains(&c.to_ascii_lowercase()) {
                continue;
            }
            if alphabet.contains(&c) {
                continue;
            }

            return false;
        }
        true
    }

    #[must_use]
    pub fn alphabet(&self) -> String {
        let mut alphabet = String::new();
        if self.digits {
            alphabet.push_str("0123456789");
        }
        if self.upper {
            alphabet.push_str("ABCDEFGHIJKLMNOPQRSTUVWXYZ");
        }
        if self.lower {
            alphabet.push_str("abcdefghijklmnopqrstuvwxyz");
        }
        if !self.special.is_empty() {
            // Discard duplicated chars
            let mut set: HashSet<char> = alphabet.chars().collect();
            for c in self.special.chars() {
                if set.insert(c) {
                    alphabet.push(c);
                }
            }
        }
        alphabet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_flag() -> Flag {
        Flag {
            prefix: "SCC".to_string(),
            format: FlagFormat::Curly,
            body: "body".to_string(),
            random: None,
            options: FlagMatchOptions::default(),
            scoring: FlagScoring::default(),
        }
    }

    #[test]
    fn validate_strict_accepts_valid_flag() {
        let flag = make_flag();
        flag.validate_strict().unwrap();
    }

    #[test]
    fn validate_strict_rejects_invalid_prefix_lengths() {
        let mut short = make_flag();
        short.prefix = "AB".to_string();
        assert!(short.validate_strict().is_err());

        let mut long = make_flag();
        long.prefix = "ABCDEFG".to_string();
        assert!(long.validate_strict().is_err());
    }

    #[test]
    fn validate_strict_rejects_non_alphanumeric_prefix() {
        let mut flag = make_flag();
        flag.prefix = "SC-1".to_string();
        assert!(flag.validate_strict().is_err());
    }

    #[test]
    fn validate_strict_rejects_invalid_body_lengths() {
        let mut empty = make_flag();
        empty.body = String::new();
        assert!(empty.validate_strict().is_err());

        let mut too_long = make_flag();
        too_long.body = "a".repeat(128);
        assert!(too_long.validate_strict().is_err());
    }

    #[test]
    fn validate_strict_rejects_invalid_random_bounds() {
        let mut zero_len = make_flag();
        zero_len.random = Some(FlagRandomRules {
            len: 0,
            upper: true,
            lower: true,
            digits: true,
            special: String::new(),
        });
        assert!(zero_len.validate_strict().is_err());

        let mut too_large = make_flag();
        too_large.random = Some(FlagRandomRules {
            len: 64,
            upper: true,
            lower: true,
            digits: true,
            special: String::new(),
        });
        assert!(too_large.validate_strict().is_err());
    }

    #[test]
    fn validate_strict_rejects_duplicate_or_non_ascii_special_chars() {
        let mut duplicate_special = make_flag();
        duplicate_special.random = Some(FlagRandomRules {
            len: 8,
            upper: false,
            lower: false,
            digits: true,
            special: "!!".to_string(),
        });
        assert!(duplicate_special.validate_strict().is_err());

        let mut non_ascii_special = make_flag();
        non_ascii_special.random = Some(FlagRandomRules {
            len: 8,
            upper: false,
            lower: false,
            digits: true,
            special: "é".to_string(),
        });
        assert!(non_ascii_special.validate_strict().is_err());
    }

    #[test]
    fn generate_includes_random_suffix_when_configured() {
        let mut flag = make_flag();
        flag.random = Some(FlagRandomRules {
            len: 6,
            upper: false,
            lower: false,
            digits: true,
            special: String::new(),
        });

        let generated = flag.generate();
        assert!(generated.starts_with("SCC{body"));
        assert!(generated.ends_with('}'));
        assert_eq!(generated.len(), "SCC{".len() + "body".len() + 6 + "}".len());
        assert!(flag.verify(&generated));
    }

    #[test]
    fn verify_rejects_missing_or_extra_random_content() {
        let mut flag = make_flag();
        flag.random = Some(FlagRandomRules {
            len: 4,
            upper: false,
            lower: false,
            digits: true,
            special: String::new(),
        });

        assert!(!flag.verify("SCC{body}"));
        assert!(!flag.verify("SCC{body12345}"));
    }

    #[test]
    fn generate_uses_correct_delimiters_for_all_formats() {
        let mut flag = make_flag();

        flag.format = FlagFormat::Curly;
        assert_eq!(flag.generate(), "SCC{body}");

        flag.format = FlagFormat::Round;
        assert_eq!(flag.generate(), "SCC(body)");

        flag.format = FlagFormat::Square;
        assert_eq!(flag.generate(), "SCC[body]");

        flag.format = FlagFormat::Angle;
        assert_eq!(flag.generate(), "SCC<body>");

        flag.format = FlagFormat::Underscore;
        assert_eq!(flag.generate(), "SCC_body");
    }

    #[test]
    fn verify_accepts_generated_output_for_all_formats() {
        let formats = [
            FlagFormat::Curly,
            FlagFormat::Round,
            FlagFormat::Square,
            FlagFormat::Angle,
            FlagFormat::Underscore,
        ];

        for format in formats {
            let mut flag = make_flag();
            flag.format = format;
            let generated = flag.generate();
            assert!(flag.verify(&generated));
        }
    }

    #[test]
    fn verify_rejects_prefix_or_delimiter_mismatch() {
        let flag = make_flag();
        assert!(!flag.verify("XYZ{body}"));
        assert!(!flag.verify("SCC(body)"));
    }

    #[test]
    fn verify_respects_ignore_case_option() {
        let mut case_sensitive = make_flag();
        case_sensitive.body = "AbCd".to_string();
        case_sensitive.options.ignore_case = false;
        assert!(!case_sensitive.verify("SCC{abcd}"));

        let mut ignore_case = case_sensitive;
        ignore_case.options.ignore_case = true;
        assert!(ignore_case.verify("SCC{abcd}"));
        assert!(ignore_case.verify("SCC{ABCD}"));
    }

    #[test]
    fn random_alphabet_includes_enabled_sets_only() {
        let rules = FlagRandomRules {
            len: 8,
            upper: false,
            lower: false,
            digits: true,
            special: "!@".to_string(),
        };

        let alphabet = rules.alphabet();
        assert!(alphabet.contains('0'));
        assert!(alphabet.contains('9'));
        assert!(alphabet.contains('!'));
        assert!(alphabet.contains('@'));
        assert!(!alphabet.contains('A'));
        assert!(!alphabet.contains('a'));
    }

    #[test]
    fn random_alphabet_deduplicates_special_chars_against_base_sets() {
        let rules = FlagRandomRules {
            len: 8,
            upper: false,
            lower: false,
            digits: true,
            special: "019!".to_string(),
        };

        let alphabet = rules.alphabet();
        let count_0 = alphabet.chars().filter(|c| *c == '0').count();
        let count_1 = alphabet.chars().filter(|c| *c == '1').count();
        let count_9 = alphabet.chars().filter(|c| *c == '9').count();
        assert_eq!(count_0, 1);
        assert_eq!(count_1, 1);
        assert_eq!(count_9, 1);
        assert!(alphabet.contains('!'));
    }

    #[test]
    fn random_generate_matches_len_and_allowed_charset() {
        let rules = FlagRandomRules {
            len: 32,
            upper: true,
            lower: false,
            digits: true,
            special: "!".to_string(),
        };
        let generated = rules.generate();
        assert_eq!(generated.len(), 32);
        assert!(rules.verify(&generated, &FlagMatchOptions::default()));
    }

    #[test]
    fn random_verify_respects_ignore_case() {
        let rules = FlagRandomRules {
            len: 4,
            upper: true,
            lower: false,
            digits: false,
            special: String::new(),
        };

        let strict = FlagMatchOptions {
            ignore_case: false,
            ignore_random: false,
        };
        let insensitive = FlagMatchOptions {
            ignore_case: true,
            ignore_random: false,
        };

        assert!(!rules.verify("abcd", &strict));
        assert!(rules.verify("abcd", &insensitive));
    }

    #[test]
    fn verify_requires_exact_random_length() {
        let mut flag = make_flag();
        flag.random = Some(FlagRandomRules {
            len: 3,
            upper: false,
            lower: false,
            digits: true,
            special: String::new(),
        });

        assert!(flag.verify("SCC{body123}"));
        assert!(!flag.verify("SCC{body12}"));
        assert!(!flag.verify("SCC{body1234}"));
    }

    #[test]
    fn verify_rejects_extra_content_when_random_is_none() {
        let flag = make_flag();
        assert!(!flag.verify("SCC{body123}"));
    }

    #[test]
    fn verify_accepts_any_random_suffix_when_ignore_random_enabled() {
        let mut flag = make_flag();
        flag.random = Some(FlagRandomRules {
            len: 8,
            upper: true,
            lower: true,
            digits: true,
            special: String::new(),
        });
        flag.options.ignore_random = true;

        assert!(flag.verify("SCC{body}"));
        assert!(flag.verify("SCC{bodyabc}"));
        assert!(flag.verify("SCC{body__any-random__}"));
    }
}
