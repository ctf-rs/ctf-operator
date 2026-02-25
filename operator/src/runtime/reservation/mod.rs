pub mod port_claim;
pub mod reservation_set;

use std::collections::HashSet;

pub(crate) fn is_u16_str(s: &str) -> bool {
    !s.is_empty() && s.parse::<u16>().is_ok()
}

pub(crate) fn has_duplicate_chars(s: &str) -> bool {
    let mut seen = HashSet::new();
    for c in s.chars() {
        if !seen.insert(c) {
            return true;
        }
    }
    false
}

pub(crate) fn has_duplicate_by_key<'a, T, F>(items: &'a [T], mut key: F) -> bool
where
    F: FnMut(&'a T) -> &'a str,
{
    let mut seen = HashSet::<&str>::new();
    for it in items {
        if !seen.insert(key(it)) {
            return true;
        }
    }
    false
}
