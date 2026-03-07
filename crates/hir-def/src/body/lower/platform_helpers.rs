//! Platform method lookup helpers.
//!
//! Uses bsl-platform for bilingual, case-insensitive method name resolution.
//! Replaces hardcoded RU/EN pairs with O(1) hash lookup.

use bsl_platform::PlatformDataInner;

/// Check if method name matches a platform global function by English name.
///
/// Uses O(1) hash lookup instead of O(N) string matching.
/// Handles bilingual matching automatically (RU/EN).
pub fn is_global_function(name: &str, english_name: &str) -> bool {
    let platform = PlatformDataInner::instance();
    platform
        .get_global_function(name)
        .is_some_and(|f| f.english_name.eq_ignore_ascii_case(english_name))
}

/// Check if method name matches any of the listed global functions.
///
/// Uses O(1) hash lookup per function.
/// Handles bilingual matching automatically (RU/EN).
pub fn is_any_global_function(name: &str, english_names: &[&str]) -> bool {
    let platform = PlatformDataInner::instance();
    platform
        .get_global_function(name)
        .is_some_and(|f| english_names.iter().any(|en| f.english_name.eq_ignore_ascii_case(en)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_global_function_bilingual() {
        // Russian name
        assert!(is_global_function("НачатьТранзакцию", "BeginTransaction"));
        // English name
        assert!(is_global_function("BeginTransaction", "BeginTransaction"));
        // Case-insensitive
        assert!(is_global_function("НАЧАТЬТРАНЗАКЦИЮ", "BeginTransaction"));
        assert!(is_global_function("begintransaction", "BeginTransaction"));
        // Non-matching
        assert!(!is_global_function("SomeOtherMethod", "BeginTransaction"));
        assert!(!is_global_function("НачатьТранзакцию", "CommitTransaction"));
    }

    #[test]
    fn test_is_any_global_function() {
        // Matches first
        assert!(is_any_global_function(
            "УстановитьБезопасныйРежим",
            &["SetSafeMode", "SetSafeModeDisabled"]
        ));
        // Matches second
        assert!(is_any_global_function(
            "УстановитьОтключениеБезопасногоРежима",
            &["SetSafeMode", "SetSafeModeDisabled"]
        ));
        // Non-matching
        assert!(!is_any_global_function(
            "SomeOtherMethod",
            &["SetSafeMode", "SetSafeModeDisabled"]
        ));
    }
}
