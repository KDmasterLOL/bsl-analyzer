//! Canonical names for the standard 1C module-structure regions.
//!
//! BSL standard regions exist in bilingual RU/EN form (e.g. `Public` ≡
//! `ПрограммныйИнтерфейс`). For diagnostics like `DuplicateRegion` that
//! group regions by identity, both spellings must collapse onto a single
//! canonical name.
//!
//! The alias table is universal across [`bsl_metadata::ModuleType`] —
//! verified by recon of the previous in-handler table in
//! `ide-diagnostics/src/handlers/duplicate_region.rs`.

/// Returns the canonical name for a known standard-region alias.
///
/// `name` is matched case-insensitively against the BSL bilingual alias
/// set. Returns [`None`] for non-standard region names. Callers that
/// need an owned fall-through to `name` should use
/// `canonical_alias(name).map(str::to_string).unwrap_or_else(|| name.to_string())`
/// — `.unwrap_or(name)` does not compile because `name: &str` is not
/// `&'static str`.
pub fn canonical_alias(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    Some(match lower.as_str() {
        "public" | "программныйинтерфейс" => "Public",
        "internal" | "служебныйпрограммныйинтерфейс" => "Internal",
        "private" | "служебныепроцедурыифункции" => "Private",
        "eventhandlers" | "обработчикисобытий" => "EventHandlers",
        "formeventhandlers" | "обработчикисобытийформы" => {
            "FormEventHandlers"
        }
        "formheaderitemseventhandlers" | "обработчикисобытийэлементовшапкиформы" => {
            "FormHeaderItemsEventHandlers"
        }
        "formcommandseventhandlers" | "обработчикикомандформы" => {
            "FormCommandsEventHandlers"
        }
        "variables" | "описаниепеременных" => "Variables",
        "initialize" | "инициализация" => "Initialize",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::canonical_alias;

    #[test]
    fn en_aliases_canonicalize() {
        assert_eq!(canonical_alias("Public"), Some("Public"));
        assert_eq!(canonical_alias("Internal"), Some("Internal"));
        assert_eq!(canonical_alias("Private"), Some("Private"));
        assert_eq!(canonical_alias("EventHandlers"), Some("EventHandlers"));
        assert_eq!(canonical_alias("FormEventHandlers"), Some("FormEventHandlers"));
        assert_eq!(
            canonical_alias("FormHeaderItemsEventHandlers"),
            Some("FormHeaderItemsEventHandlers")
        );
        assert_eq!(canonical_alias("FormCommandsEventHandlers"), Some("FormCommandsEventHandlers"));
        assert_eq!(canonical_alias("Variables"), Some("Variables"));
        assert_eq!(canonical_alias("Initialize"), Some("Initialize"));
    }

    #[test]
    fn ru_aliases_canonicalize_to_en() {
        assert_eq!(canonical_alias("ПрограммныйИнтерфейс"), Some("Public"));
        assert_eq!(canonical_alias("СлужебныйПрограммныйИнтерфейс"), Some("Internal"));
        assert_eq!(canonical_alias("СлужебныеПроцедурыИФункции"), Some("Private"));
        assert_eq!(canonical_alias("ОбработчикиСобытий"), Some("EventHandlers"));
        assert_eq!(canonical_alias("ОбработчикиСобытийФормы"), Some("FormEventHandlers"));
        assert_eq!(
            canonical_alias("ОбработчикиСобытийЭлементовШапкиФормы"),
            Some("FormHeaderItemsEventHandlers")
        );
        assert_eq!(canonical_alias("ОбработчикиКомандФормы"), Some("FormCommandsEventHandlers"));
        assert_eq!(canonical_alias("ОписаниеПеременных"), Some("Variables"));
        assert_eq!(canonical_alias("Инициализация"), Some("Initialize"));
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert_eq!(canonical_alias("PUBLIC"), Some("Public"));
        assert_eq!(canonical_alias("public"), Some("Public"));
        assert_eq!(canonical_alias("PuBlIc"), Some("Public"));
        assert_eq!(canonical_alias("программныйинтерфейс"), Some("Public"));
        assert_eq!(canonical_alias("ПРОГРАММНЫЙИНТЕРФЕЙС"), Some("Public"));
    }

    #[test]
    fn non_standard_names_return_none() {
        assert_eq!(canonical_alias("МояКастомнаяОбласть"), None);
        assert_eq!(canonical_alias("MyCustomRegion"), None);
        assert_eq!(canonical_alias(""), None);
    }

    #[test]
    fn each_canonical_form_round_trips() {
        for canonical in [
            "Public",
            "Internal",
            "Private",
            "EventHandlers",
            "FormEventHandlers",
            "FormHeaderItemsEventHandlers",
            "FormCommandsEventHandlers",
            "Variables",
            "Initialize",
        ] {
            assert_eq!(
                canonical_alias(canonical),
                Some(canonical),
                "canonical form {canonical} must map to itself"
            );
        }
    }
}
