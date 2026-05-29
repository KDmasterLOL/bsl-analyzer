#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Procedure,
    Function,
    Variable,
    Region,
}
