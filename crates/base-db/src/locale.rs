//! Output locale for user-facing strings.
//!
//! BSL is bilingual (Russian / English) at the language level. The analyzer
//! emits diagnostics, hover labels, and completion details on the user-facing
//! side; for primitive type names and generic labels we want to render them in
//! the user's locale ("Число" rather than "Number" when the IDE is Russian).
//!
//! `Locale` is a small value-type that flows from [Driver layer](LSP /
//! TOML config) down to the presentation adapters (diagnostics emitters,
//! hover/completion renderers). The domain layer (`Ty`, `hir-ty`) treats it
//! as opaque — it only consults the variant when producing display strings.
//!
//! Default is [`Locale::Ru`] because BSL is a Russian-first language: the
//! BSL community writes code in Russian by overwhelming default, and a Russian
//! 1C project loaded in an IDE without any locale signal should display
//! Russian type names.

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
/// The caller is expected to log a warning and fall back to a default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownLocale(pub String);

impl std::fmt::Display for UnknownLocale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown locale: {:?}", self.0)
    }
}

impl std::error::Error for UnknownLocale {}

impl Locale {
    /// Parse a locale label from project config (`bsl-analyzer.toml`'s
    /// `[output] display_language` field).
    ///
    /// Accepts both bare codes (`"ru"`, `"en"`) and a few common aliases.
    /// Unknown values return an [`UnknownLocale`] so the caller can warn
    /// and pick an explicit fallback (typically [`Locale::default()`]).
    pub fn from_config_str(s: &str) -> Result<Self, UnknownLocale> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ru" | "russian" | "ru-ru" | "ru_ru" => Ok(Locale::Ru),
            "en" | "english" | "en-us" | "en_us" | "en-gb" | "en_gb" => Ok(Locale::En),
            _ => Err(UnknownLocale(s.to_string())),
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
}
