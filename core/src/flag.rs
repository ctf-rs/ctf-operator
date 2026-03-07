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
}

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

#[allow(dead_code)]
fn default_random_len() -> u8 {
    8
}

#[allow(dead_code)]
fn default_random_upper() -> bool {
    true
}

#[allow(dead_code)]
fn default_random_lower() -> bool {
    true
}

#[allow(dead_code)]
fn default_random_digits() -> bool {
    true
}

#[allow(dead_code)]
fn default_random_special_chars() -> String {
    "".into()
}

impl Flag {
    pub fn validate_strict(&self) -> ValidationResult<()> {
        garde::Validate::validate(self).map_err(ValidationError::from_garde)
    }

    pub fn verify(&self, flag: &str) -> bool {
        let prefix = &self.prefix;
        let body = &self.body;
        let (start, end) = match self.format {
            FlagFormat::Curly => (format!("{prefix}{{{body}"), "}"),
            FlagFormat::Round => (format!("{prefix}({body}"), ")"),
            FlagFormat::Square => (format!("{prefix}[{body}"), "]"),
            FlagFormat::Angle => (format!("{prefix}<{body}"), ">"),
            FlagFormat::Underscore => (format!("{prefix}_"), ""),
        };

        let flag_random_len = self.random.as_ref().map(|r| r.len as usize).unwrap_or(0);
        let flag_len = start.len() + body.len() + flag_random_len + end.len();

        if flag.len() != flag_len {
            return false;
        }

        if !flag.starts_with(&start) || !flag.ends_with(end) {
            return false;
        }

        let body_start = start.len();
        let body_end = body_start + body.len();
        let flag_body = &flag[body_start..body_end];
        if self.options.ignore_case && !flag_body.eq_ignore_ascii_case(&body) || flag_body != body {
            return false;
        }

        if let Some(random) = &self.random {
            debug_assert_eq!(flag_random_len, random.len as usize);
            let flag_end = body_end + flag_random_len;
            let flag_random = &flag[body_end..flag_end];
            if !random.verify(flag_random, &self.options) {
                return false;
            }
        }

        true
    }

    pub fn generate(&self) -> String {
        let prefix = &self.prefix;
        let body = self.body.to_owned();
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
            alphabet.push_str(&self.special);
        }
        alphabet
    }
}
