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
    String {
        /// Maximum string length.
        length: Option<u32>,
    },

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

    /// Any reference (ЛюбаяСсылка).
    ///
    /// Can hold reference to any metadata object.
    AnyRef,

    /// UUID (УникальныйИдентификатор).
    Uuid,

    /// Value storage (ХранилищеЗначения).
    ///
    /// Binary storage for arbitrary data.
    ValueStorage,

    /// DefinedType (ОпределяемыйТип).
    ///
    /// User-defined type that can hold multiple primitive/reference types.
    /// Contains the type name and optionally the resolved underlying type.
    DefinedType {
        /// Name of the defined type
        name: String,
        /// Resolved underlying type (if available from metadata)
        underlying_type: Option<Box<SdblType>>,
    },

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

    /// Composite type (union of multiple types).
    ///
    /// Field can hold values of different types.
    /// Example: Dimension can be one of several enum types.
    Composite {
        /// List of allowed types
        types: Vec<SdblType>,
    },

    /// Unknown type (inference failed or not attempted).
    #[default]
    Unknown,

    /// Type error (conflicting types or invalid operation).
    Error,
}

impl SdblType {
    /// Create a String type with no length info.
    pub fn string() -> Self {
        Self::String { length: None }
    }

    /// Create a String type with specified length.
    pub fn string_with_length(length: u32) -> Self {
        Self::String { length: Some(length) }
    }

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

    /// Convert from bsl-metadata AttributeType to SdblType.
    pub fn from_attribute_type(attr_type: &bsl_metadata::AttributeType) -> Self {
        use bsl_metadata::AttributeType;

        match attr_type {
            AttributeType::String { length } => Self::String { length: *length },
            AttributeType::Number { precision, scale } => {
                Self::Number { precision: Some(*precision), scale: Some(*scale) }
            }
            AttributeType::Boolean => Self::Boolean,
            AttributeType::Date => Self::Date,
            AttributeType::DateTime => Self::DateTime,
            AttributeType::Ref { mdo_type, name } => {
                Self::Ref(MdoRef { mdo_type: *mdo_type, name: name.clone() })
            }
            AttributeType::AnyRef => Self::AnyRef,
            AttributeType::Uuid => Self::Uuid,
            AttributeType::ValueStorage => Self::ValueStorage,
            AttributeType::DefinedType { name } => {
                Self::DefinedType { name: name.clone(), underlying_type: None }
            }
            AttributeType::Composite { types } => {
                // Convert all types in composite
                let sdbl_types: Vec<SdblType> =
                    types.iter().map(Self::from_attribute_type).collect();

                if sdbl_types.is_empty() {
                    Self::Unknown
                } else if sdbl_types.len() == 1 {
                    // Single type - unwrap it
                    sdbl_types.into_iter().next().unwrap()
                } else {
                    // Multiple types - create Composite
                    Self::Composite { types: sdbl_types }
                }
            }
            AttributeType::Unknown => Self::Unknown,
        }
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

            // DefinedType is compatible with anything
            (DefinedType { .. }, _) | (_, DefinedType { .. }) => true,

            // Same types are compatible
            (Boolean, Boolean) => true,
            (String { .. }, String { .. }) => true,
            (Number { .. }, Number { .. }) => true,
            (Date, Date) | (DateTime, DateTime) | (Date, DateTime) | (DateTime, Date) => true,
            (Null, _) | (_, Null) => true,

            // References are compatible if they point to same MDO type
            (Ref(a), Ref(b)) => a.mdo_type == b.mdo_type,

            // AnyRef is compatible with any Ref and vice versa
            (AnyRef, Ref(_)) | (Ref(_), AnyRef) => true,
            (AnyRef, AnyRef) => true,

            // Special types - only compatible with themselves
            (Uuid, Uuid) => true,
            (ValueStorage, ValueStorage) => true,
            (ValueTable, ValueTable) => true,

            // Composite types - compatible if any of the types is compatible
            (Composite { types }, other) => types.iter().any(|t| t.is_compatible_with(other)),
            (other, Composite { types }) => types.iter().any(|t| other.is_compatible_with(t)),

            _ => false,
        }
    }
}

impl std::fmt::Display for SdblType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boolean => write!(f, "Булево"),
            Self::String { length } => {
                write!(f, "Строка")?;
                if let Some(len) = length {
                    write!(f, "({})", len)?;
                }
                Ok(())
            }
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
            Self::AnyRef => write!(f, "ЛюбаяСсылка"),
            Self::Uuid => write!(f, "УникальныйИдентификатор"),
            Self::ValueStorage => write!(f, "ХранилищеЗначения"),
            Self::DefinedType { name, underlying_type } => {
                write!(f, "ОпределяемыйТип.{}", name)?;
                if let Some(ty) = underlying_type {
                    write!(f, " ({})", ty)?;
                }
                Ok(())
            }
            Self::ValueTable => write!(f, "ТаблицаЗначений"),
            Self::Null => write!(f, "NULL"),
            Self::Aggregate(inner) => write!(f, "Агрегат({})", inner),
            Self::Composite { types } => {
                // Display all types on separate lines at same level
                if types.is_empty() {
                    write!(f, "Составной тип (пусто)")
                } else if types.len() == 1 {
                    write!(f, "{}", types[0])
                } else {
                    // Multiple types - all on new lines at same level
                    for ty in types.iter() {
                        write!(f, "\n{}", ty)?;
                    }
                    Ok(())
                }
            }
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
        assert_eq!(SdblType::string().to_string(), "Строка");
        assert_eq!(SdblType::string_with_length(17).to_string(), "Строка(17)");
        assert_eq!(SdblType::number().to_string(), "Число");
        assert_eq!(SdblType::number_with_precision(10, 2).to_string(), "Число(10, 2)");
        assert_eq!(SdblType::Date.to_string(), "Дата");
        assert_eq!(SdblType::AnyRef.to_string(), "ЛюбаяСсылка");
        assert_eq!(SdblType::Uuid.to_string(), "УникальныйИдентификатор");
        assert_eq!(SdblType::ValueStorage.to_string(), "ХранилищеЗначения");

        // DefinedType without underlying type
        assert_eq!(
            SdblType::DefinedType {
                name: "ОтметкаВремени".to_string(), underlying_type: None
            }
            .to_string(),
            "ОпределяемыйТип.ОтметкаВремени"
        );

        // DefinedType with underlying type
        assert_eq!(
            SdblType::DefinedType {
                name: "ОтметкаВремени".to_string(),
                underlying_type: Some(Box::new(SdblType::string_with_length(17)))
            }
            .to_string(),
            "ОпределяемыйТип.ОтметкаВремени (Строка(17))"
        );

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
        assert!(SdblType::Unknown.is_compatible_with(&SdblType::string()));
        assert!(
            SdblType::Null.is_compatible_with(&SdblType::Number { precision: None, scale: None })
        );

        // DefinedType is compatible with anything
        assert!(SdblType::DefinedType { name: "Test".to_string(), underlying_type: None }
            .is_compatible_with(&SdblType::string()));
        assert!(SdblType::Boolean.is_compatible_with(&SdblType::DefinedType {
            name: "Test".to_string(),
            underlying_type: None
        }));

        assert!(!SdblType::Boolean.is_compatible_with(&SdblType::string()));
        assert!(!SdblType::number().is_compatible_with(&SdblType::string()));
    }

    #[test]
    fn test_is_numeric() {
        assert!(SdblType::number().is_numeric());
        assert!(SdblType::number_with_precision(10, 2).is_numeric());
        assert!(SdblType::Aggregate(Box::new(SdblType::number())).is_numeric());

        assert!(!SdblType::string().is_numeric());
        assert!(!SdblType::Boolean.is_numeric());
    }

    #[test]
    fn test_composite_type_display() {
        use bsl_metadata::MdoType;

        // Single type should unwrap
        let single = SdblType::Composite {
            types: vec![SdblType::reference(MdoType::Enum, "ВидДействия1")],
        };
        assert_eq!(single.to_string(), "Перечисление.ВидДействия1");

        // Multiple types should show all with separator
        let composite = SdblType::Composite {
            types: vec![
                SdblType::reference(MdoType::Enum, "ВидыУсловийОтбораИсходящихПисем"),
                SdblType::reference(MdoType::Enum, "ВидыДействийПриОбработкеИсходящихПисем"),
                SdblType::reference(MdoType::Enum, "ВидыУсловийОтбораВходящихПисем"),
                SdblType::reference(MdoType::Enum, "ВидыДействийПриОбработкеВходящихПисем"),
            ],
        };

        let display = composite.to_string();
        assert!(display.contains("ВидыУсловийОтбораИсходящихПисем"));
        assert!(display.contains("ВидыДействийПриОбработкеИсходящихПисем"));
        assert!(display.contains("ВидыУсловийОтбораВходящихПисем"));
        assert!(display.contains("ВидыДействийПриОбработкеВходящихПисем"));

        // Verify newlines are present (multiline format)
        assert!(display.contains('\n'));

        // Verify all 4 types are shown on separate lines (excluding empty lines)
        let lines: Vec<&str> = display.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 4, "Expected 4 non-empty lines in composite type display");
    }

    #[test]
    fn test_composite_type_compatibility() {
        use bsl_metadata::MdoType;

        let composite = SdblType::Composite {
            types: vec![
                SdblType::reference(MdoType::Enum, "Enum1"),
                SdblType::reference(MdoType::Enum, "Enum2"),
                SdblType::Boolean,
            ],
        };

        // Composite is compatible with any of its types
        assert!(composite.is_compatible_with(&SdblType::reference(MdoType::Enum, "Enum1")));
        assert!(composite.is_compatible_with(&SdblType::reference(MdoType::Enum, "Enum2")));
        assert!(composite.is_compatible_with(&SdblType::Boolean));

        // Not compatible with types not in the composite
        assert!(!composite.is_compatible_with(&SdblType::string()));
        assert!(!composite.is_compatible_with(&SdblType::number()));
    }
}
