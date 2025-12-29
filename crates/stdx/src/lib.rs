//! Standard library extensions for bsl-analyzer.
//!
//! This crate contains utility functions and extensions used across
//! the bsl-analyzer codebase.

pub use itertools::Itertools;

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
