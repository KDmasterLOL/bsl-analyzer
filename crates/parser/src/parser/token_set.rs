use crate::parser::input::Sig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenSet(u128);

impl TokenSet {
    /// Тривию отказывать больше негде и нечем: набор строится из записей
    /// алфавита, а тривиального вида в алфавите не существует. Прежний
    /// рантайм-`assert` здесь проверял то, что теперь не выражается.
    ///
    /// Разряд бита берётся через [`Sig::kind`], видимый этому модулю как
    /// потомку `crate::parser`, — грамматике он недоступен.
    pub const fn new(kinds: &[Sig]) -> Self {
        let mut bits: u128 = 0;
        let mut i = 0;
        while i < kinds.len() {
            bits |= 1 << (kinds[i].kind() as u8);
            i += 1;
        }
        TokenSet(bits)
    }

    pub const fn empty() -> Self {
        TokenSet(0)
    }

    #[inline]
    pub const fn contains(&self, kind: Sig) -> bool {
        (self.0 & (1 << (kind.kind() as u8))) != 0
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
        assert!(!set.contains(T![Comma]));
        assert!(!set.contains(T![Plus]));
    }

    #[test]
    fn test_single_token() {
        let set = TokenSet::new(&[T![Comma]]);
        assert!(!set.is_empty());
        assert!(set.contains(T![Comma]));
        assert!(!set.contains(T![Semicolon]));
    }

    #[test]
    fn test_multiple_tokens() {
        let set = TokenSet::new(&[T![Comma], T![RParen], T![Semicolon]]);

        assert!(set.contains(T![Comma]));
        assert!(set.contains(T![RParen]));
        assert!(set.contains(T![Semicolon]));

        assert!(!set.contains(T![LParen]));
        assert!(!set.contains(T![Plus]));
    }

    #[test]
    fn test_union() {
        let set1 = TokenSet::new(&[T![Comma], T![Plus]]);
        let set2 = TokenSet::new(&[T![Minus], T![Star]]);
        let union = set1.union(set2);

        assert!(union.contains(T![Comma]));
        assert!(union.contains(T![Plus]));
        assert!(union.contains(T![Minus]));
        assert!(union.contains(T![Star]));
        assert!(!union.contains(T![Slash]));
    }

    #[test]
    fn test_const_creation() {
        const RECOVERY_SET: TokenSet = TokenSet::new(&[T![Comma], T![RParen], T![Semicolon]]);

        assert!(RECOVERY_SET.contains(T![Comma]));
        assert!(RECOVERY_SET.contains(T![RParen]));
        assert!(RECOVERY_SET.contains(T![Semicolon]));
    }

    // Прежний `#[should_panic]` на `TokenSet::new(&[TokenKind::Newline])`
    // здесь больше не выражается: аргумента такого вида не существует. То же
    // свойство сторожит перебор `Sig::ALL` в `input`, и там его видно
    // провалившимся.

    #[test]
    fn test_duplicate_tokens() {
        let set = TokenSet::new(&[T![Comma], T![Comma], T![Plus]]);

        assert!(set.contains(T![Comma]));
        assert!(set.contains(T![Plus]));
    }

    #[test]
    fn test_arithmetic_operators() {
        const ARITHMETIC: TokenSet =
            TokenSet::new(&[T![Plus], T![Minus], T![Star], T![Slash], T![Percent]]);

        assert!(ARITHMETIC.contains(T![Plus]));
        assert!(ARITHMETIC.contains(T![Minus]));
        assert!(ARITHMETIC.contains(T![Star]));
        assert!(ARITHMETIC.contains(T![Slash]));
        assert!(ARITHMETIC.contains(T![Percent]));

        assert!(!ARITHMETIC.contains(T![Eq]));
        assert!(!ARITHMETIC.contains(T![Lt]));
    }

    #[test]
    fn test_comparison_operators() {
        const COMPARISON: TokenSet =
            TokenSet::new(&[T![Eq], T![Neq], T![Lt], T![Le], T![Gt], T![Ge]]);

        assert!(COMPARISON.contains(T![Eq]));
        assert!(COMPARISON.contains(T![Neq]));
        assert!(COMPARISON.contains(T![Lt]));
        assert!(COMPARISON.contains(T![Le]));
        assert!(COMPARISON.contains(T![Gt]));
        assert!(COMPARISON.contains(T![Ge]));

        assert!(!COMPARISON.contains(T![Plus]));
    }

    #[test]
    fn test_punctuation() {
        const PUNCTUATION: TokenSet =
            TokenSet::new(&[T![LParen], T![RParen], T![Comma], T![Semicolon], T![Dot]]);

        assert!(PUNCTUATION.contains(T![LParen]));
        assert!(PUNCTUATION.contains(T![RParen]));
        assert!(PUNCTUATION.contains(T![Comma]));
        assert!(PUNCTUATION.contains(T![Semicolon]));
        assert!(PUNCTUATION.contains(T![Dot]));
    }
}
