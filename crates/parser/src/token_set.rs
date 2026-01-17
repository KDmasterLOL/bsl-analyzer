//! TokenSet for efficient token matching in parser.
//!
//! Inspired by rust-analyzer's TokenSet pattern.
//! Uses bitwise operations for O(1) membership checks.
//!
//! # Example
//!
//! ```
//! use lexer::TokenKind;
//! use parser::token_set::TokenSet;
//!
//! const RECOVERY_SET: TokenSet = TokenSet::new(&[
//!     TokenKind::Comma,
//!     TokenKind::RParen,
//!     TokenKind::Semicolon,
//! ]);
//!
//! assert!(RECOVERY_SET.contains(TokenKind::Comma));
//! assert!(!RECOVERY_SET.contains(TokenKind::Plus));
//! ```

use lexer::TokenKind;

/// A set of token kinds for efficient membership testing.
///
/// Uses up to 128 bits (u128) to represent a set of tokens.
/// Each bit corresponds to a token kind's discriminant value.
///
/// Created at compile-time via const fn for zero runtime overhead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenSet(u128);

impl TokenSet {
    /// Creates a new TokenSet from a slice of TokenKinds.
    ///
    /// This is a const fn, allowing creation of constant token sets at compile time.
    ///
    /// # Example
    ///
    /// ```
    /// use lexer::TokenKind;
    /// use parser::token_set::TokenSet;
    ///
    /// const PUNCTUATION: TokenSet = TokenSet::new(&[
    ///     TokenKind::Comma,
    ///     TokenKind::Semicolon,
    /// ]);
    /// ```
    pub const fn new(kinds: &[TokenKind]) -> Self {
        let mut bits: u128 = 0;
        let mut i = 0;
        while i < kinds.len() {
            let kind = kinds[i];
            let bit_index = kind as u8;

            // Safety: TokenKind discriminants must be < 128
            // This is enforced by the enum definition (we have ~100 variants max)
            bits |= 1 << bit_index;
            i += 1;
        }
        TokenSet(bits)
    }

    /// Creates an empty TokenSet.
    pub const fn empty() -> Self {
        TokenSet(0)
    }

    /// Checks if the set contains the given token kind.
    ///
    /// Time complexity: O(1)
    #[inline]
    pub const fn contains(&self, kind: TokenKind) -> bool {
        let bit_index = kind as u8;
        (self.0 & (1 << bit_index)) != 0
    }

    /// Creates a union of two token sets.
    #[inline]
    pub const fn union(self, other: TokenSet) -> TokenSet {
        TokenSet(self.0 | other.0)
    }

    /// Checks if the set is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_set() {
        let set = TokenSet::empty();
        assert!(set.is_empty());
        assert!(!set.contains(TokenKind::Comma));
        assert!(!set.contains(TokenKind::Plus));
    }

    #[test]
    fn test_single_token() {
        let set = TokenSet::new(&[TokenKind::Comma]);
        assert!(!set.is_empty());
        assert!(set.contains(TokenKind::Comma));
        assert!(!set.contains(TokenKind::Semicolon));
    }

    #[test]
    fn test_multiple_tokens() {
        let set = TokenSet::new(&[TokenKind::Comma, TokenKind::RParen, TokenKind::Semicolon]);

        assert!(set.contains(TokenKind::Comma));
        assert!(set.contains(TokenKind::RParen));
        assert!(set.contains(TokenKind::Semicolon));

        assert!(!set.contains(TokenKind::LParen));
        assert!(!set.contains(TokenKind::Plus));
    }

    #[test]
    fn test_union() {
        let set1 = TokenSet::new(&[TokenKind::Comma, TokenKind::Plus]);
        let set2 = TokenSet::new(&[TokenKind::Minus, TokenKind::Star]);
        let union = set1.union(set2);

        assert!(union.contains(TokenKind::Comma));
        assert!(union.contains(TokenKind::Plus));
        assert!(union.contains(TokenKind::Minus));
        assert!(union.contains(TokenKind::Star));
        assert!(!union.contains(TokenKind::Slash));
    }

    #[test]
    fn test_const_creation() {
        // Verify that TokenSet can be created as a const
        const RECOVERY_SET: TokenSet =
            TokenSet::new(&[TokenKind::Comma, TokenKind::RParen, TokenKind::Semicolon]);

        assert!(RECOVERY_SET.contains(TokenKind::Comma));
        assert!(RECOVERY_SET.contains(TokenKind::RParen));
        assert!(RECOVERY_SET.contains(TokenKind::Semicolon));
    }

    #[test]
    fn test_duplicate_tokens() {
        // Duplicates should be handled gracefully (idempotent)
        let set = TokenSet::new(&[TokenKind::Comma, TokenKind::Comma, TokenKind::Plus]);

        assert!(set.contains(TokenKind::Comma));
        assert!(set.contains(TokenKind::Plus));
    }

    #[test]
    fn test_arithmetic_operators() {
        const ARITHMETIC: TokenSet = TokenSet::new(&[
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Percent,
        ]);

        assert!(ARITHMETIC.contains(TokenKind::Plus));
        assert!(ARITHMETIC.contains(TokenKind::Minus));
        assert!(ARITHMETIC.contains(TokenKind::Star));
        assert!(ARITHMETIC.contains(TokenKind::Slash));
        assert!(ARITHMETIC.contains(TokenKind::Percent));

        assert!(!ARITHMETIC.contains(TokenKind::Eq));
        assert!(!ARITHMETIC.contains(TokenKind::Lt));
    }

    #[test]
    fn test_comparison_operators() {
        const COMPARISON: TokenSet = TokenSet::new(&[
            TokenKind::Eq,
            TokenKind::Neq,
            TokenKind::Lt,
            TokenKind::Le,
            TokenKind::Gt,
            TokenKind::Ge,
        ]);

        assert!(COMPARISON.contains(TokenKind::Eq));
        assert!(COMPARISON.contains(TokenKind::Neq));
        assert!(COMPARISON.contains(TokenKind::Lt));
        assert!(COMPARISON.contains(TokenKind::Le));
        assert!(COMPARISON.contains(TokenKind::Gt));
        assert!(COMPARISON.contains(TokenKind::Ge));

        assert!(!COMPARISON.contains(TokenKind::Plus));
    }

    #[test]
    fn test_punctuation() {
        const PUNCTUATION: TokenSet = TokenSet::new(&[
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::Comma,
            TokenKind::Semicolon,
            TokenKind::Dot,
        ]);

        assert!(PUNCTUATION.contains(TokenKind::LParen));
        assert!(PUNCTUATION.contains(TokenKind::RParen));
        assert!(PUNCTUATION.contains(TokenKind::Comma));
        assert!(PUNCTUATION.contains(TokenKind::Semicolon));
        assert!(PUNCTUATION.contains(TokenKind::Dot));
    }
}
