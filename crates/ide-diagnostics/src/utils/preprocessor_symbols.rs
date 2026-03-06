//! Known preprocessor symbols for BSL conditional compilation.
//!
//! These symbols are used in `#Если` / `#If` directives to control conditional compilation.
//! Unknown symbols should trigger the UnknownPreprocessorSymbol diagnostic.
//!

const KNOWN_SYMBOLS: &[&str] = &[
    "КЛИЕНТ",
    "CLIENT",
    "НАСЕРВЕРЕ",
    "ATSERVER",
    "СЕРВЕР",
    "SERVER",
    "НАКЛИЕНТЕ",
    "ATCLIENT",
    "ТОНКИЙКЛИЕНТ",
    "THINCLIENT",
    "ВЕБКЛИЕНТ",
    "WEBCLIENT",
    "ТОЛСТЫЙКЛИЕНТУПРАВЛЯЕМОЕПРИЛОЖЕНИЕ",
    "THICKCLIENTMANAGEDAPPLICATION",
    "ТОЛСТЫЙКЛИЕНТОБЫЧНОЕПРИЛОЖЕНИЕ",
    "THICKCLIENTORDINARYAPPLICATION",
    "ВНЕШНЕЕСОЕДИНЕНИЕ",
    "EXTERNALCONNECTION",
    "МОБИЛЬНЫЙКЛИЕНТ",
    "MOBILECLIENT",
    "МОБИЛЬНОЕПРИЛОЖЕНИЕКЛИЕНТ",
    "MOBILEAPPCLIENT",
    "МОБИЛЬНОЕПРИЛОЖЕНИЕСЕРВЕР",
    "MOBILEAPPSERVER",
    "МОБИЛЬНЫЙАВТОНОМНЫЙСЕРВЕР",
    "MOBILESTANDALONESERVER",
    "LINUX",
    "WINDOWS",
    "MACOS",
];

pub fn is_known_symbol(symbol: &str) -> bool {
    let symbol_upper = symbol.to_uppercase();
    KNOWN_SYMBOLS.contains(&symbol_upper.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_symbols_russian() {
        assert!(is_known_symbol("Клиент"));
        assert!(is_known_symbol("Сервер"));
        assert!(is_known_symbol("НаСервере"));
        assert!(is_known_symbol("НаКлиенте"));
        assert!(is_known_symbol("ТонкийКлиент"));
        assert!(is_known_symbol("ВебКлиент"));
        assert!(is_known_symbol("МобильныйАвтономныйСервер"));
    }

    #[test]
    fn test_known_symbols_english() {
        assert!(is_known_symbol("Client"));
        assert!(is_known_symbol("Server"));
        assert!(is_known_symbol("AtServer"));
        assert!(is_known_symbol("AtClient"));
        assert!(is_known_symbol("ThinClient"));
        assert!(is_known_symbol("WebClient"));
        assert!(is_known_symbol("MobileStandaloneServer"));
    }

    #[test]
    fn test_known_symbols_os() {
        assert!(is_known_symbol("Linux"));
        assert!(is_known_symbol("Windows"));
        assert!(is_known_symbol("MacOS"));
        assert!(is_known_symbol("LINUX"));
        assert!(is_known_symbol("WINDOWS"));
        assert!(is_known_symbol("MACOS"));
    }

    #[test]
    fn test_unknown_symbols() {
        assert!(!is_known_symbol("Нечто"));
        assert!(!is_known_symbol("_"));
        assert!(!is_known_symbol("Unknown"));
        assert!(!is_known_symbol("Test"));
    }

    #[test]
    fn test_case_insensitive() {
        assert!(is_known_symbol("клиент"));
        assert!(is_known_symbol("КЛИЕНТ"));
        assert!(is_known_symbol("client"));
        assert!(is_known_symbol("CLIENT"));
    }
}
