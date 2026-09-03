//! Compiled patterns shared across bodies.
//!
//! A configurable check derives its pattern from the configuration, so the
//! set of distinct patterns in a run is tiny while the number of bodies is
//! not: compiling per body would spend more on regex construction than on
//! the check itself (measured at +27 % user CPU over a cold ERP run).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;
use regex::Regex;

static CACHE: Lazy<Mutex<HashMap<String, Option<Arc<Regex>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// The compiled `pattern`, built once per process; `None` when it does not
/// compile — remembered too, so an invalid configured pattern is not
/// re-parsed for every body either.
pub fn cached_regex(pattern: &str) -> Option<Arc<Regex>> {
    let mut cache = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(hit) = cache.get(pattern) {
        return hit.clone();
    }
    let compiled = Regex::new(pattern).ok().map(Arc::new);
    cache.insert(pattern.to_owned(), compiled.clone());
    compiled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_pattern_is_one_regex() {
        let a = cached_regex("(?i)^(abc)$").expect("valid");
        let b = cached_regex("(?i)^(abc)$").expect("valid");
        assert!(Arc::ptr_eq(&a, &b));
        assert!(cached_regex("(unclosed").is_none());
    }
}
