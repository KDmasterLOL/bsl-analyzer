//! 1C:Enterprise platform types and methods database.
//!
//! This crate provides access to platform types (Строка, Число, Массив, etc.)
//! and their methods with documentation.
//!
//! # Build-time Data Extraction
//!
//! The crate automatically detects installed 1C:Enterprise platform during build
//! and extracts documentation from `shcntx_ru.hbk`. If platform is not found,
//! builds with minimal data (types and signatures only, no documentation).
//!
//! # Usage
//!
//! ```
//! use bsl_platform::PlatformDataInner;
//!
//! let data = PlatformDataInner::instance();
//!
//! // Get all platform types
//! for ty in data.all_types() {
//!     println!("{} / {}", ty.name, ty.english_name);
//! }
//!
//! // Get methods for a type
//! let methods = data.get_type_methods("Строка");
//! for method in methods {
//!     println!("Method: {}", method.name);
//! }
//! ```

mod db;
pub mod standard_mdo_attributes;
mod types;

// Include generated code from build.rs
#[allow(warnings)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));

    use super::types::*;
}

pub use standard_mdo_attributes::{
    standard_attributes_for, AttrValueKind, MdoTemplateKind, ObjectView, PresenceCondition,
    StandardAttrSpec, StandardKind,
};

// Re-export public API
pub use db::{
    find_prefixed_method, global_function_query, global_member_method_query, global_property_query,
    manager_methods_query, platform_constructors_query, platform_method_query,
    platform_property_query, platform_type_query, prefixed_method_query, type_methods_query,
    type_properties_query, MethodLookupInput, PlatformData, PlatformDataInner,
    PrefixedMethodLookupInput, TypeNameInput, GLOBAL_CONTEXT_OWNER,
};
pub use types::*;

/// Split a raw HBK type-string on either `,` or `;` separator, trimming each
/// segment and dropping empties.
///
/// 1С platform pages mix the two separators between alternatives:
/// `param_type = "Форма ; Элемент управления"`,
/// `return_type = "Null, Булево; Дата"`, and a stray trailing separator
/// (`"Метаданные, Массив ;"`) is common scraper noise. Treating both
/// characters uniformly and folding away empties is the single contract
/// every consumer (`hir-ty` lowering, hover, completion) shares — we keep
/// it here so each consumer cannot drift into its own ad-hoc split.
pub fn split_type_alternatives(raw: &str) -> Vec<&str> {
    raw.split([',', ';']).map(str::trim).filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_data_loads() {
        let data = PlatformData::instance();

        // Should not panic
        let types = data.all_types();
        let methods = data.all_methods();

        println!("Loaded {} types and {} methods", types.len(), methods.len());
    }

    #[test]
    fn split_type_alternatives_handles_comma_semicolon_and_trailing_garbage() {
        assert_eq!(split_type_alternatives("Число"), vec!["Число"]);
        assert_eq!(split_type_alternatives("Число, Строка"), vec!["Число", "Строка"]);
        assert_eq!(split_type_alternatives("Форма ; Элемент"), vec!["Форма", "Элемент"]);
        // 1С HBK scraper emits a stray trailing `;` after a `,`-list — both
        // separators are folded uniformly.
        assert_eq!(split_type_alternatives("Метаданные, Массив ;"), vec!["Метаданные", "Массив"],);
        // All-separator / all-whitespace inputs produce no segments after
        // empty-filter; callers see this and fall back to `Ty::Unknown`.
        assert!(split_type_alternatives(", ,").is_empty());
        assert!(split_type_alternatives("").is_empty());
    }

    #[test]
    fn test_bilingual_lookup() {
        let data = PlatformData::instance();

        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        // Should be able to look up by both Russian and English names
        for ty in data.all_types() {
            assert!(
                data.get_type(&ty.name).is_some(),
                "Failed to find type by Russian name: {}",
                ty.name
            );
            assert!(
                data.get_type(&ty.english_name).is_some(),
                "Failed to find type by English name: {}",
                ty.english_name
            );
        }
    }
}
