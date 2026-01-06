//! SDBL type system.
//!
//! Maps to 1C platform types for query fields.

use bsl_metadata::MdoType;

/// SDBL type.
///
/// Represents types in SDBL query expressions and fields.
/// Corresponds to 1C platform types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum SdblType {
    /// Boolean (Булево).
    ///
    /// Used for: ПометкаУдаления, Проведен, etc.
    Boolean,

    /// String (Строка).
    ///
    /// Used for: Код, Наименование, Комментарий, etc.
    String,

    /// Number (Число).
    ///
    /// Used for: Количество, Сумма, etc.
    Number {
        /// Total digits (including decimal).
        precision: Option<u8>,
        /// Digits after decimal point.
        scale: Option<u8>,
    },

    /// Date (Дата).
    ///
    /// Date without time component.
    Date,

    /// DateTime (ДатаВремя).
    ///
    /// Date with time component.
    DateTime,

    /// Reference to metadata object (Ссылка).
    ///
    /// Used for: Ссылка, Владелец, Родитель, etc.
    Ref(MdoRef),

    /// Value table (ТаблицаЗначений).
    ///
    /// Result of subquery or temporary table.
    ValueTable,

    /// NULL value.
    Null,

    /// Aggregate function result.
    ///
    /// Wraps the inner type (e.g., SUM(Number) -> Aggregate(Number)).
    Aggregate(Box<SdblType>),

    /// Unknown type (inference failed or not attempted).
    #[default]
    Unknown,

    /// Type error (conflicting types or invalid operation).
    Error,
}

impl SdblType {
    /// Create a Number type with no precision/scale info.
    pub fn number() -> Self {
        Self::Number { precision: None, scale: None }
    }

    /// Create a Number type with specified precision and scale.
    pub fn number_with_precision(precision: u8, scale: u8) -> Self {
        Self::Number { precision: Some(precision), scale: Some(scale) }
    }

    /// Create a reference type to a metadata object.
    pub fn reference(mdo_type: MdoType, name: impl Into<String>) -> Self {
        Self::Ref(MdoRef { mdo_type, name: name.into() })
    }

    /// Check if type is unknown or error.
    pub fn is_unknown_or_error(&self) -> bool {
        matches!(self, Self::Unknown | Self::Error)
    }

    /// Check if type is numeric (Number or Aggregate(Number)).
    pub fn is_numeric(&self) -> bool {
        match self {
            Self::Number { .. } => true,
            Self::Aggregate(inner) => inner.is_numeric(),
            _ => false,
        }
    }

    /// Check if type is a reference.
    pub fn is_ref(&self) -> bool {
        matches!(self, Self::Ref(_))
    }

    /// Get the inner type for aggregate, or self.
    pub fn unwrap_aggregate(&self) -> &Self {
        match self {
            Self::Aggregate(inner) => inner.unwrap_aggregate(),
            _ => self,
        }
    }

    /// Check if two types are compatible for comparison.
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        use SdblType::*;

        match (self.unwrap_aggregate(), other.unwrap_aggregate()) {
            // Unknown/Error is compatible with anything
            (Unknown, _) | (_, Unknown) => true,
            (Error, _) | (_, Error) => true,

            // Same types are compatible
            (Boolean, Boolean) => true,
            (String, String) => true,
            (Number { .. }, Number { .. }) => true,
            (Date, Date) | (DateTime, DateTime) | (Date, DateTime) | (DateTime, Date) => true,
            (Null, _) | (_, Null) => true,

            // References are compatible if they point to same MDO type
            (Ref(a), Ref(b)) => a.mdo_type == b.mdo_type,

            // ValueTable comparison
            (ValueTable, ValueTable) => true,

            _ => false,
        }
    }
}

impl std::fmt::Display for SdblType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boolean => write!(f, "Булево"),
            Self::String => write!(f, "Строка"),
            Self::Number { precision, scale } => {
                write!(f, "Число")?;
                if let (Some(p), Some(s)) = (precision, scale) {
                    write!(f, "({}, {})", p, s)?;
                }
                Ok(())
            }
            Self::Date => write!(f, "Дата"),
            Self::DateTime => write!(f, "ДатаВремя"),
            Self::Ref(mdo_ref) => write!(f, "{}", mdo_ref),
            Self::ValueTable => write!(f, "ТаблицаЗначений"),
            Self::Null => write!(f, "NULL"),
            Self::Aggregate(inner) => write!(f, "Агрегат({})", inner),
            Self::Unknown => write!(f, "Неизвестно"),
            Self::Error => write!(f, "Ошибка"),
        }
    }
}

/// Reference to a metadata object type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MdoRef {
    /// Type of metadata object (Catalog, Document, etc.).
    pub mdo_type: MdoType,
    /// Name of the specific object.
    pub name: String,
}

impl MdoRef {
    /// Create a new metadata reference.
    pub fn new(mdo_type: MdoType, name: impl Into<String>) -> Self {
        Self { mdo_type, name: name.into() }
    }

    /// Get full name (e.g., "Справочник.Валюты").
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.mdo_type.russian_name(), self.name)
    }
}

impl std::fmt::Display for MdoRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.mdo_type.russian_name(), self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdbl_type_display() {
        assert_eq!(SdblType::Boolean.to_string(), "Булево");
        assert_eq!(SdblType::String.to_string(), "Строка");
        assert_eq!(SdblType::number().to_string(), "Число");
        assert_eq!(SdblType::number_with_precision(10, 2).to_string(), "Число(10, 2)");
        assert_eq!(SdblType::Date.to_string(), "Дата");
        assert_eq!(SdblType::Unknown.to_string(), "Неизвестно");
    }

    #[test]
    fn test_mdo_ref() {
        let mdo_ref = MdoRef::new(MdoType::Catalog, "Валюты");
        assert_eq!(mdo_ref.full_name(), "Справочник.Валюты");
        assert_eq!(mdo_ref.to_string(), "Справочник.Валюты");
    }

    #[test]
    fn test_type_compatibility() {
        assert!(SdblType::Boolean.is_compatible_with(&SdblType::Boolean));
        assert!(SdblType::number().is_compatible_with(&SdblType::number_with_precision(10, 2)));
        assert!(SdblType::Date.is_compatible_with(&SdblType::DateTime));
        assert!(SdblType::Unknown.is_compatible_with(&SdblType::String));
        assert!(
            SdblType::Null.is_compatible_with(&SdblType::Number { precision: None, scale: None })
        );

        assert!(!SdblType::Boolean.is_compatible_with(&SdblType::String));
        assert!(!SdblType::number().is_compatible_with(&SdblType::String));
    }

    #[test]
    fn test_is_numeric() {
        assert!(SdblType::number().is_numeric());
        assert!(SdblType::number_with_precision(10, 2).is_numeric());
        assert!(SdblType::Aggregate(Box::new(SdblType::number())).is_numeric());

        assert!(!SdblType::String.is_numeric());
        assert!(!SdblType::Boolean.is_numeric());
    }
}
