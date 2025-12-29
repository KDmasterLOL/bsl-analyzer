//! Standard library extensions for bsl-analyzer.
//!
//! This crate contains utility functions and extensions used across
//! the bsl-analyzer codebase.

pub use itertools::Itertools;

/// Hash a value once using the specified hasher.
///
/// This is a convenience function for quickly computing a hash of a value
/// using a specific hasher type. Commonly used with `FxHasher` for fast hashing.
///
/// # Example
///
/// ```ignore
/// use stdx::hash_once;
/// use rustc_hash::FxHasher;
///
/// let hash = hash_once::<FxHasher>(&"hello");
/// ```
pub fn hash_once<Hasher: std::hash::Hasher + Default>(thing: impl std::hash::Hash) -> u64 {
    std::hash::BuildHasher::hash_one(&std::hash::BuildHasherDefault::<Hasher>::default(), thing)
}

/// Extension trait for `Option` with additional helper methods.
pub trait OptionExt<T> {
    fn and_if(self, condition: bool) -> Self;
}

impl<T> OptionExt<T> for Option<T> {
    fn and_if(self, condition: bool) -> Self {
        if condition {
            self
        } else {
            None
        }
    }
}

/// A helper macro similar to `format!` but returns `String` directly.
#[macro_export]
macro_rules! format_to {
    ($buf:expr) => ();
    ($buf:expr, $lit:literal $($arg:tt)*) => {
        {
            use std::fmt::Write as _;
            let _ = write!($buf, $lit $($arg)*);
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_option_and_if() {
        assert_eq!(Some(42).and_if(true), Some(42));
        assert_eq!(Some(42).and_if(false), None);
        assert_eq!(None::<i32>.and_if(true), None);
    }
}
