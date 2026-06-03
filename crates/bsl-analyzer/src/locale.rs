use base_db::Locale;

pub fn parse_lsp_locale(s: &str) -> Locale {
    let primary = s.split(['-', '_']).next().unwrap_or("").trim();
    if primary.eq_ignore_ascii_case("ru") {
        Locale::Ru
    } else {
        Locale::En
    }
}

pub fn resolve_locale(project: Option<Locale>, lsp: Option<Locale>) -> Locale {
    project.or(lsp).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ru_variants() {
        assert_eq!(parse_lsp_locale("ru"), Locale::Ru);
        assert_eq!(parse_lsp_locale("ru-RU"), Locale::Ru);
        assert_eq!(parse_lsp_locale("ru_RU"), Locale::Ru);
        assert_eq!(parse_lsp_locale("RU"), Locale::Ru);
        assert_eq!(parse_lsp_locale("ru-BY"), Locale::Ru);
    }

    #[test]
    fn parses_en_variants() {
        assert_eq!(parse_lsp_locale("en"), Locale::En);
        assert_eq!(parse_lsp_locale("en-US"), Locale::En);
        assert_eq!(parse_lsp_locale("en-GB"), Locale::En);
    }

    #[test]
    fn unrecognised_locale_is_english() {
        assert_eq!(parse_lsp_locale("de-DE"), Locale::En);
        assert_eq!(parse_lsp_locale("fr"), Locale::En);
        assert_eq!(parse_lsp_locale(""), Locale::En);
    }

    #[test]
    fn resolve_priority() {
        assert_eq!(
            resolve_locale(Some(Locale::En), Some(Locale::Ru)),
            Locale::En,
            "TOML wins over LSP",
        );
        assert_eq!(
            resolve_locale(None, Some(Locale::En)),
            Locale::En,
            "LSP fills in when TOML absent",
        );
        assert_eq!(resolve_locale(None, None), Locale::default(), "default when neither is set",);
    }
}
