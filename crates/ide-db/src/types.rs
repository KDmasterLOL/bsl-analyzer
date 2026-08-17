#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Procedure,
    Function,
    Variable,
    Region,
}

impl SymbolKind {
    /// The canonical wire spelling, shared by every consumer that publishes a symbol kind.
    ///
    /// BSL declares the same method as `Процедура` or `Procedure`, in any case, so a kind
    /// recovered from the declaration text would differ per file for the same construct.
    /// The kind here comes from the parsed item, and this is the one name it is served
    /// under; an adapter inventing its own spelling is how two surfaces start disagreeing
    /// about the same file.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Procedure => "procedure",
            Self::Function => "function",
            Self::Variable => "variable",
            Self::Region => "region",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SymbolKind;

    /// Every kind names itself, and no two share a spelling. The match above has no `_`
    /// arm, so a new kind fails the build here rather than shipping as an empty string.
    #[test]
    fn every_kind_has_its_own_canonical_spelling() {
        let all =
            [SymbolKind::Procedure, SymbolKind::Function, SymbolKind::Variable, SymbolKind::Region];

        let spellings: Vec<&str> = all.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(spellings, ["procedure", "function", "variable", "region"]);

        let unique: std::collections::BTreeSet<&str> = spellings.iter().copied().collect();
        assert_eq!(unique.len(), all.len(), "two kinds share a spelling: {spellings:?}");
    }
}
