use lexer::TokenKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenSet(u128);

impl TokenSet {
    /// Trivia is refused here rather than where a set is consulted, and this
    /// is the whole of what keeps it out: the bits are private, `empty` is
    /// empty and `union` is clean by induction, so a set can hold trivia only
    /// if it entered through here. Every set in the grammar is a `const`, so
    /// the refusal is an error at compile time.
    pub const fn new(kinds: &[TokenKind]) -> Self {
        let mut bits: u128 = 0;
        let mut i = 0;
        while i < kinds.len() {
            let kind = kinds[i];
            assert!(
                !matches!(
                    kind,
                    TokenKind::Whitespace
                        | TokenKind::Comment
                        | TokenKind::Newline
                        | TokenKind::Bom
                ),
                "тривия в наборе токенов: о переводе строки грамматике сообщает предикат"
            );
            let bit_index = kind as u8;

            bits |= 1 << bit_index;
            i += 1;
        }
        TokenSet(bits)
    }

    pub const fn empty() -> Self {
        TokenSet(0)
    }

    #[inline]
    pub const fn contains(&self, kind: TokenKind) -> bool {
        let bit_index = kind as u8;
        (self.0 & (1 << bit_index)) != 0
    }

    #[inline]
    pub const fn union(self, other: TokenSet) -> TokenSet {
        TokenSet(self.0 | other.0)
    }

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
        const RECOVERY_SET: TokenSet =
            TokenSet::new(&[TokenKind::Comma, TokenKind::RParen, TokenKind::Semicolon]);

        assert!(RECOVERY_SET.contains(TokenKind::Comma));
        assert!(RECOVERY_SET.contains(TokenKind::RParen));
        assert!(RECOVERY_SET.contains(TokenKind::Semicolon));
    }

    /// Вызов в рантайме, а не `const`: у `const`-ветки того же запрета
    /// срабатывание выражается отказом сборки, и тестом его не выразить.
    #[test]
    #[should_panic(expected = "тривия в наборе токенов")]
    fn trivia_has_no_place_in_a_token_set() {
        let _ = TokenSet::new(&[TokenKind::Newline]);
    }

    #[test]
    fn test_duplicate_tokens() {
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
