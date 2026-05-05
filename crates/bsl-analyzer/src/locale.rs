//! LSP locale handling for the BSL analyzer LSP server.
//!
//! Bridge between the LSP-protocol locale string (RFC 4646: `"ru-RU"`,
//! `"en-US"`, `"ru"`, …) and the analyzer-internal [`base_db::Locale`].
//! Lives in this crate (rather than in `base-db`) because the wire-format
//! belongs to the Drivers layer of the architecture — the domain core
//! should not know about LSP.

use base_db::Locale;

/// Parse an LSP `InitializeParams.locale` string (RFC 4646 / BCP 47).
///
/// Only the primary language subtag is consulted. Anything that maps to
/// Russian (`"ru"`, `"ru-RU"`, `"ru-BY"`, …) yields [`Locale::Ru`];
/// everything else collapses to [`Locale::En`]. We deliberately do NOT
/// fall back to [`Locale::default()`] for unrecognised tags — an English
/// IDE label like `"de-DE"` should produce English analyzer output, not
/// the Russian default reserved for "no signal at all".
pub fn parse_lsp_locale(s: &str) -> Locale {
    let primary = s.split(['-', '_']).next().unwrap_or("").trim();
    if primary.eq_ignore_ascii_case("ru") {
        Locale::Ru
    } else {
        Locale::En
    }
}

/// Resolve the effective output locale from project + LSP signals.
///
/// Priority (highest first):
/// 1. Explicit project setting (`bsl-analyzer.toml` `[output] display_language`).
/// 2. LSP `InitializeParams.locale` from the IDE handshake.
/// 3. [`Locale::default()`] (= `Ru`, since BSL is Russian-first).
///
/// The priority puts the project in charge: a team can pin their analyzer
/// output to a single language regardless of which IDE locale individual
/// developers use.
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
        // "de" is not Russian → English (NOT default Ru).
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
