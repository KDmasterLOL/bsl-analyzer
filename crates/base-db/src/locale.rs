//! Output locale for user-facing strings.
//!
//! BSL is bilingual (Russian / English) at the language level. The analyzer
//! uses `Locale` when rendering user-facing type names and generic labels in
//! diagnostics, hover, and completion output.
//!
//! The default is [`Locale::Ru`] because BSL projects and platform APIs are
//! Russian-first. Project config and LSP locale signals may override it before
//! values reach presentation code.

/// User-facing output locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Locale {
    /// Russian — `Число`, `Строка`, `Тип`.
    #[default]
    Ru,
    /// English — `Number`, `String`, `Type`.
    En,
}

/// Returned when a config string could not be mapped to a known locale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownLocale(pub String);

impl std::fmt::Display for UnknownLocale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown locale: {:?}", self.0)
    }
}

impl std::error::Error for UnknownLocale {}

impl Locale {
    /// Parse `[output] display_language` from project config.
    ///
    /// Accepts both bare codes (`"ru"`, `"en"`) and a few common aliases.
    /// Unknown values return an [`UnknownLocale`] so the caller can warn
    /// and pick an explicit fallback (typically [`Locale::default()`]).
    pub fn from_config_str(s: &str) -> Result<Self, UnknownLocale> {
        let trimmed = s.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "ru" | "russian" | "ru-ru" | "ru_ru" => Ok(Locale::Ru),
            "en" | "english" | "en-us" | "en_us" | "en-gb" | "en_gb" => Ok(Locale::En),
            _ => Err(UnknownLocale(trimmed.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_ru() {
        assert_eq!(Locale::default(), Locale::Ru);
    }

    #[test]
    fn from_config_str_recognises_common_codes() {
        assert_eq!(Locale::from_config_str("ru"), Ok(Locale::Ru));
        assert_eq!(Locale::from_config_str("RU"), Ok(Locale::Ru));
        assert_eq!(Locale::from_config_str("russian"), Ok(Locale::Ru));
        assert_eq!(Locale::from_config_str("ru-RU"), Ok(Locale::Ru));
        assert_eq!(Locale::from_config_str("en"), Ok(Locale::En));
        assert_eq!(Locale::from_config_str("English"), Ok(Locale::En));
        assert_eq!(Locale::from_config_str("en-US"), Ok(Locale::En));
        assert_eq!(Locale::from_config_str(" en "), Ok(Locale::En));
    }

    #[test]
    fn from_config_str_rejects_unknown() {
        let err = Locale::from_config_str("fr").unwrap_err();
        assert_eq!(err.0, "fr");
        assert!(Locale::from_config_str("").is_err());
        assert!(Locale::from_config_str("xx-YY").is_err());
    }

    #[test]
    fn from_config_str_trims_unknown_value() {
        let err = Locale::from_config_str(" fr ").unwrap_err();
        assert_eq!(err.0, "fr");
    }
}
