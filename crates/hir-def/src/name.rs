//! Name interning for efficient storage and comparison.
//!
//! BSL is case-insensitive, so names like "Процедура", "ПРОЦЕДУРА", and "процедура"
//! are all equivalent. This module provides a Name type that handles this correctly.

use smol_str::SmolStr;
use std::fmt;

/// Interned name for efficient storage and comparison.
///
/// Uses SmolStr which stores strings ≤22 bytes inline, longer strings in Arc.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Name(SmolStr);

impl Name {
    /// Create a new name from a string.
    pub fn new(text: &str) -> Self {
        Name(text.into())
    }

    /// Get the name as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// BSL-specific: case-insensitive comparison.
    ///
    /// In BSL, `Процедура`, `ПРОЦЕДУРА`, and `процедура` are all the same name.
    /// Also works with English: `Procedure`, `PROCEDURE`, and `procedure`.
    pub fn eq_ignore_case(&self, other: &Name) -> bool {
        // Use Unicode-aware lowercase comparison (works for both Cyrillic and Latin)
        self.0.to_lowercase() == other.0.to_lowercase()
    }

    /// Create a name for a missing identifier (error recovery).
    pub fn missing() -> Self {
        Name::new("<missing>")
    }
}

impl From<&str> for Name {
    fn from(s: &str) -> Self {
        Name::new(s)
    }
}

impl From<String> for Name {
    fn from(s: String) -> Self {
        Name(s.into())
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Name({})", self.0)
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_creation() {
        let name = Name::new("Процедура");
        assert_eq!(name.as_str(), "Процедура");
    }

    #[test]
    fn test_name_from_str() {
        let name: Name = "Функция".into();
        assert_eq!(name.as_str(), "Функция");
    }

    #[test]
    fn test_name_equality() {
        let name1 = Name::new("Тест");
        let name2 = Name::new("Тест");
        assert_eq!(name1, name2);
    }

    #[test]
    fn test_case_insensitive_comparison() {
        let name1 = Name::new("Процедура");
        let name2 = Name::new("ПРОЦЕДУРА");
        let name3 = Name::new("процедура");
        let name4 = Name::new("ПроЦеДурА");

        assert!(name1.eq_ignore_case(&name2));
        assert!(name1.eq_ignore_case(&name3));
        assert!(name1.eq_ignore_case(&name4));
        assert!(name2.eq_ignore_case(&name3));
    }

    #[test]
    fn test_case_sensitive_inequality() {
        // Regular equality should be case-sensitive
        let name1 = Name::new("Тест");
        let name2 = Name::new("ТЕСТ");

        assert_ne!(name1, name2); // Different case = not equal
        assert!(name1.eq_ignore_case(&name2)); // But case-insensitive equal
    }

    #[test]
    fn test_missing_name() {
        let name = Name::missing();
        assert_eq!(name.as_str(), "<missing>");
    }

    #[test]
    fn test_name_display() {
        let name = Name::new("ТестоваяФункция");
        assert_eq!(format!("{}", name), "ТестоваяФункция");
        assert_eq!(format!("{:?}", name), "Name(ТестоваяФункция)");
    }
}
