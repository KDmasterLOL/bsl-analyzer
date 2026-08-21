//! Символы препроцессора: написания, допустимые в условии `#Если` / `#If`.
//!
//! ## Provenance
//!
//! Список выведен из раздела 4.8.1.2 «Инструкции препроцессора» руководства
//! разработчика 1С:Предприятие 8.3.27
//! (<https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000116>), где он приведён
//! двуязычной таблицей целиком. Аттестация —
//! `docs/legal/bsl-clean-room-slice-b2.md`.
//!
//! Продукция «Символ препроцессора» того же раздела включает ещё `Область` и
//! `КонецОбласти`. Здесь их нет намеренно: те же слова стоят в таблице
//! инструкций, `#Если Область Тогда` смысла не имеет, а признание их
//! известными погасило бы диагностику на настоящей опечатке.

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
];

pub fn is_known_symbol(symbol: &str) -> bool {
    let symbol_upper = symbol.to_uppercase();
    KNOWN_SYMBOLS.contains(&symbol_upper.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Тринадцать двуязычных пар раздела 4.8.1.2 руководства разработчика.
    ///
    /// Живут в тесте, а не в проверяемом коде: список, сверяемый сам с собой,
    /// сверкой не является.
    const SECTION_4_8_1_2: &[(&str, &str)] = &[
        ("Сервер", "Server"),
        ("НаСервере", "AtServer"),
        ("Клиент", "Client"),
        ("НаКлиенте", "AtClient"),
        ("ТонкийКлиент", "ThinClient"),
        ("МобильныйКлиент", "MobileClient"),
        ("ВебКлиент", "WebClient"),
        ("ВнешнееСоединение", "ExternalConnection"),
        ("ТолстыйКлиентУправляемоеПриложение", "ThickClientManagedApplication"),
        ("ТолстыйКлиентОбычноеПриложение", "ThickClientOrdinaryApplication"),
        ("МобильноеПриложениеКлиент", "MobileAppClient"),
        ("МобильноеПриложениеСервер", "MobileAppServer"),
        ("МобильныйАвтономныйСервер", "MobileStandaloneServer"),
    ];

    /// Список известных написаний равен таблице источника — в обе стороны.
    ///
    /// Проверка «каждое написание источника известно» одна пропустила бы
    /// лишнее: ровно так три написания без источника и прожили в списке.
    /// Поэтому рядом стоит счёт.
    #[test]
    fn the_known_set_equals_the_source_table() {
        for (ru, en) in SECTION_4_8_1_2 {
            assert!(is_known_symbol(ru), "{ru}: написание источника не признано известным");
            assert!(is_known_symbol(en), "{en}: написание источника не признано известным");
        }
        assert_eq!(
            KNOWN_SYMBOLS.len(),
            SECTION_4_8_1_2.len() * 2,
            "в списке есть написания сверх таблицы 4.8.1.2"
        );
    }

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
