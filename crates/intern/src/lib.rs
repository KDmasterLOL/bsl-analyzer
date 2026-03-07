//! String interning for bsl-analyzer.
//!
//! This crate provides efficient string interning to reduce memory usage
//! and speed up string comparisons.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dashmap::DashMap;
use once_cell::sync::Lazy;
use rustc_hash::FxBuildHasher;

type Interner = DashMap<Arc<str>, (), FxBuildHasher>;

static INTERNER: Lazy<Interner> = Lazy::new(|| DashMap::with_hasher(FxBuildHasher));

/// An interned string.
#[derive(Clone, Eq)]
pub struct Symbol(Arc<str>);

impl Symbol {
    pub fn intern(s: &str) -> Self {
        if let Some(entry) = INTERNER.get(s) {
            return Symbol(Arc::clone(entry.key()));
        }

        let arc: Arc<str> = Arc::from(s);
        INTERNER.insert(Arc::clone(&arc), ());
        Symbol(arc)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for Symbol {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Hash for Symbol {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl std::fmt::Debug for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Symbol({:?})", self.as_str())
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern() {
        let s1 = Symbol::intern("hello");
        let s2 = Symbol::intern("hello");
        let s3 = Symbol::intern("world");

        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
        assert_eq!(s1.as_str(), "hello");
    }
}
