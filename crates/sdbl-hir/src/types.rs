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

    /// Any object of specific type.
    ///
    /// E.g., any Catalog, any Document, any BusinessProcess.
    /// Used in SDBL when field can hold any object of a specific type.
    AnyObjectRef {
        /// Type of metadata object
        mdo_type: MdoType,
    },

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

    /// Tabular section reference (ссылка на табличную часть).
    ///
    /// Represents a reference to a tabular section of a metadata object.
    /// When resolved through nested field access, provides access to tabular section attributes.
    ///
    /// Example: `Документ.Заказ.Товары` where Товары is a tabular section
    TabularSectionRef {
        /// Parent MDO type (Document, Catalog, etc.)
        parent_mdo_type: bsl_metadata::MdoType,
        /// Parent MDO name
        parent_mdo_name: String,
        /// Tabular section name
        ts_name: String,
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
            AttributeType::AnyObjectRef { mdo_type } => Self::AnyObjectRef { mdo_type: *mdo_type },
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
        matches!(self, Self::Ref(_) | Self::AnyRef | Self::AnyObjectRef { .. })
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

            // AnyObjectRef is compatible with:
            // 1. AnyRef (bidirectional)
            (AnyObjectRef { .. }, AnyRef) | (AnyRef, AnyObjectRef { .. }) => true,
            // 2. Ref of the same MDO type
            (AnyObjectRef { mdo_type: a }, Ref(b)) | (Ref(b), AnyObjectRef { mdo_type: a }) => {
                *a == b.mdo_type
            }
            // 3. Another AnyObjectRef of the same MDO type
            (AnyObjectRef { mdo_type: a }, AnyObjectRef { mdo_type: b }) => a == b,

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

impl SdblType {
    /// Locale-aware human-readable rendering of this query-language type.
    ///
    /// Mirrors [`hir_def::ty::Ty::display_name`] for the BSL side: lets a
    /// future SDBL-diagnostic frame substitute Russian or English type
    /// names per `[output] display_language` without re-parsing the
    /// `Display`-formatted output. Today's [`std::fmt::Display`] impl on
    /// `SdblType` keeps producing Russian (the historic single-locale
    /// behaviour), so the only callers that benefit from this method are
    /// new ones that want to render `expected: <T>` / `actual: <T>` in
    /// the client locale.
    ///
    /// Parametric forms (`String(17)`, `Number(10, 2)`,
    /// `Catalog.Товары`, `ОпределяемыйТип.X`) keep their parenthesised /
    /// dotted suffixes verbatim so the IDE still shows the precision,
    /// length, or referenced name regardless of locale.
    pub fn display_name(&self, locale: base_db::Locale) -> String {
        use base_db::Locale;
        match self {
            Self::Boolean => match locale {
                Locale::Ru => "Булево".into(),
                Locale::En => "Boolean".into(),
            },
            Self::String { length } => {
                let head = match locale {
                    Locale::Ru => "Строка",
                    Locale::En => "String",
                };
                match length {
                    Some(len) => format!("{head}({len})"),
                    None => head.into(),
                }
            }
            Self::Number { precision, scale } => {
                let head = match locale {
                    Locale::Ru => "Число",
                    Locale::En => "Number",
                };
                match (precision, scale) {
                    (Some(p), Some(s)) => format!("{head}({p}, {s})"),
                    _ => head.into(),
                }
            }
            Self::Date => match locale {
                Locale::Ru => "Дата".into(),
                Locale::En => "Date".into(),
            },
            Self::DateTime => match locale {
                Locale::Ru => "ДатаВремя".into(),
                Locale::En => "DateTime".into(),
            },
            Self::Ref(mdo_ref) => {
                // MdoRef is rendered as `<MdoLabel>.<Name>`. The MDO label
                // switches per locale, but the source-declared `name` is
                // surfaced verbatim so the IDE still pinpoints the
                // referenced object even when its identifier is Russian.
                let label = match locale {
                    Locale::Ru => mdo_ref.mdo_type.russian_name(),
                    Locale::En => mdo_ref.mdo_type.english_name(),
                };
                format!("{}.{}", label, mdo_ref.name)
            }
            Self::AnyRef => match locale {
                Locale::Ru => "ЛюбаяСсылка".into(),
                Locale::En => "AnyRef".into(),
            },
            Self::AnyObjectRef { mdo_type } => match locale {
                Locale::Ru => mdo_type.russian_name().into(),
                Locale::En => mdo_type.english_name().into(),
            },
            Self::Uuid => match locale {
                Locale::Ru => "УникальныйИдентификатор".into(),
                Locale::En => "Uuid".into(),
            },
            Self::ValueStorage => match locale {
                Locale::Ru => "ХранилищеЗначения".into(),
                Locale::En => "ValueStorage".into(),
            },
            Self::DefinedType { name, .. } => match locale {
                Locale::Ru => format!("ОпределяемыйТип.{name}"),
                Locale::En => format!("DefinedType.{name}"),
            },
            Self::ValueTable => match locale {
                Locale::Ru => "ТаблицаЗначений".into(),
                Locale::En => "ValueTable".into(),
            },
            Self::Null => "NULL".into(),
            Self::Aggregate(inner) => {
                let head = match locale {
                    Locale::Ru => "Агрегат",
                    Locale::En => "Aggregate",
                };
                format!("{head}({})", inner.display_name(locale))
            }
            Self::Composite { types } => {
                if types.is_empty() {
                    match locale {
                        Locale::Ru => "Составной тип (пусто)".into(),
                        Locale::En => "Composite type (empty)".into(),
                    }
                } else if types.len() == 1 {
                    types[0].display_name(locale)
                } else {
                    match locale {
                        Locale::Ru => "Составной тип:".into(),
                        Locale::En => "Composite type:".into(),
                    }
                }
            }
            Self::TabularSectionRef { parent_mdo_type, parent_mdo_name, ts_name } => {
                let parent_label = match locale {
                    Locale::Ru => parent_mdo_type.russian_name(),
                    Locale::En => parent_mdo_type.english_name(),
                };
                format!("{parent_label}.{parent_mdo_name}.{ts_name}")
            }
            Self::Unknown => match locale {
                Locale::Ru => "Неизвестно".into(),
                Locale::En => "Unknown".into(),
            },
            Self::Error => match locale {
                Locale::Ru => "Ошибка".into(),
                Locale::En => "Error".into(),
            },
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
            Self::AnyObjectRef { mdo_type } => {
                write!(f, "{}", mdo_type.russian_name())
            }
            Self::Uuid => write!(f, "УникальныйИдентификатор"),
            Self::ValueStorage => write!(f, "ХранилищеЗначения"),
            Self::DefinedType { name, .. } => {
                write!(f, "ОпределяемыйТип.{}", name)
            }
            Self::ValueTable => write!(f, "ТаблицаЗначений"),
            Self::Null => write!(f, "NULL"),
            Self::Aggregate(inner) => write!(f, "Агрегат({})", inner),
            Self::Composite { types } => {
                // Display brief description for composite types
                if types.is_empty() {
                    write!(f, "Составной тип (пусто)")
                } else if types.len() == 1 {
                    // Single type - just show it
                    write!(f, "{}", types[0])
                } else {
                    // Multiple types - show brief label
                    write!(f, "Составной тип:")
                }
            }
            Self::TabularSectionRef { parent_mdo_type, parent_mdo_name, ts_name } => {
                write!(f, "{}.{}.{}", parent_mdo_type.russian_name(), parent_mdo_name, ts_name)
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
        assert_eq!(SdblType::AnyObjectRef { mdo_type: MdoType::Catalog }.to_string(), "Справочник");
        assert_eq!(SdblType::AnyObjectRef { mdo_type: MdoType::Document }.to_string(), "Документ");
        assert_eq!(
            SdblType::AnyObjectRef { mdo_type: MdoType::BusinessProcess }.to_string(),
            "БизнесПроцесс"
        );
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

        // DefinedType with underlying type (underlying not shown in Display)
        assert_eq!(
            SdblType::DefinedType {
                name: "ОтметкаВремени".to_string(),
                underlying_type: Some(Box::new(SdblType::string_with_length(17)))
            }
            .to_string(),
            "ОпределяемыйТип.ОтметкаВремени"
        );

        assert_eq!(SdblType::Unknown.to_string(), "Неизвестно");
    }

    #[test]
    fn display_name_localizes_primitive_and_parametric_forms() {
        use base_db::Locale;

        // Primitive: same length suffix in either locale.
        assert_eq!(SdblType::Boolean.display_name(Locale::Ru), "Булево");
        assert_eq!(SdblType::Boolean.display_name(Locale::En), "Boolean");
        assert_eq!(SdblType::string_with_length(17).display_name(Locale::Ru), "Строка(17)");
        assert_eq!(SdblType::string_with_length(17).display_name(Locale::En), "String(17)");
        assert_eq!(SdblType::number_with_precision(10, 2).display_name(Locale::Ru), "Число(10, 2)");
        assert_eq!(
            SdblType::number_with_precision(10, 2).display_name(Locale::En),
            "Number(10, 2)"
        );

        // MDO ref: label switches per locale, source-declared `name` stays
        // verbatim so the IDE still pinpoints the referenced object.
        let mdo_ref = SdblType::reference(MdoType::Catalog, "Валюты");
        assert_eq!(mdo_ref.display_name(Locale::Ru), "Справочник.Валюты");
        assert_eq!(mdo_ref.display_name(Locale::En), "Catalog.Валюты");

        // Special types.
        assert_eq!(SdblType::AnyRef.display_name(Locale::Ru), "ЛюбаяСсылка");
        assert_eq!(SdblType::AnyRef.display_name(Locale::En), "AnyRef");
        assert_eq!(SdblType::Uuid.display_name(Locale::En), "Uuid");
        assert_eq!(
            SdblType::DefinedType {
                name: "ОтметкаВремени".into(), underlying_type: None
            }
            .display_name(Locale::En),
            "DefinedType.ОтметкаВремени"
        );

        // Display impl is unchanged: still Russian regardless of new API.
        assert_eq!(SdblType::Boolean.to_string(), "Булево");
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
    fn test_any_object_ref_compatibility() {
        use bsl_metadata::MdoType;

        let catalog_ref = SdblType::AnyObjectRef { mdo_type: MdoType::Catalog };
        let document_ref = SdblType::AnyObjectRef { mdo_type: MdoType::Document };
        let bp_ref = SdblType::AnyObjectRef { mdo_type: MdoType::BusinessProcess };

        // AnyObjectRef compatible with AnyRef
        assert!(catalog_ref.is_compatible_with(&SdblType::AnyRef));
        assert!(SdblType::AnyRef.is_compatible_with(&catalog_ref));

        // AnyObjectRef compatible with Ref of same type
        let catalog_item = SdblType::reference(MdoType::Catalog, "Валюты");
        assert!(catalog_ref.is_compatible_with(&catalog_item));
        assert!(catalog_item.is_compatible_with(&catalog_ref));

        // AnyObjectRef not compatible with Ref of different type
        let document_item = SdblType::reference(MdoType::Document, "ПКО");
        assert!(!catalog_ref.is_compatible_with(&document_item));
        assert!(!document_item.is_compatible_with(&catalog_ref));

        // AnyObjectRef compatible with same type
        let another_catalog_ref = SdblType::AnyObjectRef { mdo_type: MdoType::Catalog };
        assert!(catalog_ref.is_compatible_with(&another_catalog_ref));

        // AnyObjectRef not compatible with different type
        assert!(!catalog_ref.is_compatible_with(&document_ref));
        assert!(!document_ref.is_compatible_with(&bp_ref));

        // AnyObjectRef not compatible with primitives
        assert!(!catalog_ref.is_compatible_with(&SdblType::string()));
        assert!(!catalog_ref.is_compatible_with(&SdblType::number()));
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

        // Multiple types should show brief label
        let composite = SdblType::Composite {
            types: vec![
                SdblType::reference(MdoType::Enum, "ВидыУсловийОтбораИсходящихПисем"),
                SdblType::reference(MdoType::Enum, "ВидыДействийПриОбработкеИсходящихПисем"),
                SdblType::reference(MdoType::Enum, "ВидыУсловийОтбораВходящихПисем"),
                SdblType::reference(MdoType::Enum, "ВидыДействийПриОбработкеВходящихПисем"),
            ],
        };

        let display = composite.to_string();
        assert_eq!(display, "Составной тип:");
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
