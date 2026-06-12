use smol_str::SmolStr;
use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(SmolStr);

impl Name {
    pub fn new(text: &str) -> Self {
        Name(text.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn eq_ignore_case(&self, other: &Name) -> bool {
        stdx::case::eq_ignore_case(self.as_str(), other.as_str())
    }

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
        let name1 = Name::new("Тест");
        let name2 = Name::new("ТЕСТ");

        assert_ne!(name1, name2);
        assert!(name1.eq_ignore_case(&name2));
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
