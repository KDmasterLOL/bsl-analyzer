//! Basic metadata object types
//!
//! Simplified metadata object structure for query validation and diagnostics

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Metadata object type (MDO Type)
///
/// Represents different types of 1C:Enterprise metadata objects
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MdoType {
    /// Справочник / Catalog
    Catalog,
    /// Документ / Document
    Document,
    /// Регистр сведений / InformationRegister
    InformationRegister,
    /// Регистр накопления / AccumulationRegister
    AccumulationRegister,
    /// Регистр бухгалтерии / AccountingRegister
    AccountingRegister,
    /// Регистр расчета / CalculationRegister
    CalculationRegister,
    /// План видов характеристик / ChartOfCharacteristicTypes
    ChartOfCharacteristicTypes,
    /// План счетов / ChartOfAccounts
    ChartOfAccounts,
    /// План видов расчета / ChartOfCalculationTypes
    ChartOfCalculationTypes,
    /// Бизнес-процесс / BusinessProcess
    BusinessProcess,
    /// Задача / Task
    Task,
    /// Перечисление / Enum
    Enum,
    /// План обмена / ExchangePlan
    ExchangePlan,
    /// Внешний источник данных / ExternalDataSource
    ExternalDataSource,
    /// Куб внешнего источника данных / Cube (nested in ExternalDataSource)
    Cube,
    /// Таблица измерения куба / DimensionTable (nested in Cube)
    DimensionTable,
    /// Константа / Constant
    Constant,
    /// Обработка / DataProcessor
    DataProcessor,
    /// Отчет / Report
    Report,
    /// Общий модуль / CommonModule
    CommonModule,
}

impl FromStr for MdoType {
    type Err = String;

    /// Parse MDO type from string (case-insensitive, both Russian and English)
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "справочник" | "catalog" => Ok(Self::Catalog),
            "документ" | "document" => Ok(Self::Document),
            "регистрсведений" | "informationregister" => {
                Ok(Self::InformationRegister)
            }
            "регистрнакопления" | "accumulationregister" => {
                Ok(Self::AccumulationRegister)
            }
            "регистрбухгалтерии" | "accountingregister" => {
                Ok(Self::AccountingRegister)
            }
            "регистррасчета" | "calculationregister" => Ok(Self::CalculationRegister),
            "планвидовхарактеристик" | "chartofcharacteristictypes" => {
                Ok(Self::ChartOfCharacteristicTypes)
            }
            "плансчетов" | "chartofaccounts" => Ok(Self::ChartOfAccounts),
            "планвидоврасчета" | "chartofcalculationtypes" => {
                Ok(Self::ChartOfCalculationTypes)
            }
            "бизнеспроцесс" | "businessprocess" => Ok(Self::BusinessProcess),
            "задача" | "task" => Ok(Self::Task),
            "перечисление" | "enum" => Ok(Self::Enum),
            "планобмена" | "exchangeplan" => Ok(Self::ExchangePlan),
            "внешнийисточникданных" | "externaldatasource" => {
                Ok(Self::ExternalDataSource)
            }
            "куб" | "cube" => Ok(Self::Cube),
            "таблицаизмерения" | "dimensiontable" => Ok(Self::DimensionTable),
            "константа" | "constant" => Ok(Self::Constant),
            "обработка" | "dataprocessor" => Ok(Self::DataProcessor),
            "отчет" | "report" => Ok(Self::Report),
            "общиймодуль" | "commonmodule" => Ok(Self::CommonModule),
            _ => Err(format!("Unknown MDO type: {}", s)),
        }
    }
}

impl MdoType {
    /// Get Russian name for this MDO type
    pub fn russian_name(&self) -> &'static str {
        match self {
            Self::Catalog => "Справочник",
            Self::Document => "Документ",
            Self::InformationRegister => "РегистрСведений",
            Self::AccumulationRegister => "РегистрНакопления",
            Self::AccountingRegister => "РегистрБухгалтерии",
            Self::CalculationRegister => "РегистрРасчета",
            Self::ChartOfCharacteristicTypes => "ПланВидовХарактеристик",
            Self::ChartOfAccounts => "ПланСчетов",
            Self::ChartOfCalculationTypes => "ПланВидовРасчета",
            Self::BusinessProcess => "БизнесПроцесс",
            Self::Task => "Задача",
            Self::Enum => "Перечисление",
            Self::ExchangePlan => "ПланОбмена",
            Self::ExternalDataSource => "ВнешнийИсточникДанных",
            Self::Cube => "Куб",
            Self::DimensionTable => "ТаблицаИзмерения",
            Self::Constant => "Константа",
            Self::DataProcessor => "Обработка",
            Self::Report => "Отчет",
            Self::CommonModule => "ОбщийМодуль",
        }
    }

    /// Get English name for this MDO type
    pub fn english_name(&self) -> &'static str {
        match self {
            Self::Catalog => "Catalog",
            Self::Document => "Document",
            Self::InformationRegister => "InformationRegister",
            Self::AccumulationRegister => "AccumulationRegister",
            Self::AccountingRegister => "AccountingRegister",
            Self::CalculationRegister => "CalculationRegister",
            Self::ChartOfCharacteristicTypes => "ChartOfCharacteristicTypes",
            Self::ChartOfAccounts => "ChartOfAccounts",
            Self::ChartOfCalculationTypes => "ChartOfCalculationTypes",
            Self::BusinessProcess => "BusinessProcess",
            Self::Task => "Task",
            Self::Enum => "Enum",
            Self::ExchangePlan => "ExchangePlan",
            Self::ExternalDataSource => "ExternalDataSource",
            Self::Cube => "Cube",
            Self::DimensionTable => "DimensionTable",
            Self::Constant => "Constant",
            Self::DataProcessor => "DataProcessor",
            Self::Report => "Report",
            Self::CommonModule => "CommonModule",
        }
    }

    /// Get all MDO types
    pub fn all() -> &'static [MdoType] {
        &[
            Self::Catalog,
            Self::Document,
            Self::InformationRegister,
            Self::AccumulationRegister,
            Self::AccountingRegister,
            Self::CalculationRegister,
            Self::ChartOfCharacteristicTypes,
            Self::ChartOfAccounts,
            Self::ChartOfCalculationTypes,
            Self::BusinessProcess,
            Self::Task,
            Self::Enum,
            Self::ExchangePlan,
            Self::ExternalDataSource,
            Self::Cube,
            Self::DimensionTable,
            Self::Constant,
            Self::DataProcessor,
            Self::Report,
            Self::CommonModule,
        ]
    }

    /// Parse MDO type from plural form keyword (case-insensitive, bilingual)
    ///
    /// Used for object model calls like `Документы.ПКО.Method()` or `Catalogs.Name.Method()`
    ///
    /// # Examples
    /// ```
    /// use bsl_metadata::MdoType;
    ///
    /// assert_eq!(MdoType::from_plural("Документы"), Some(MdoType::Document));
    /// assert_eq!(MdoType::from_plural("documents"), Some(MdoType::Document));
    /// assert_eq!(MdoType::from_plural("Справочники"), Some(MdoType::Catalog));
    /// assert_eq!(MdoType::from_plural("catalogs"), Some(MdoType::Catalog));
    /// ```
    pub fn from_plural(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "документы" | "documents" => Some(Self::Document),
            "справочники" | "catalogs" => Some(Self::Catalog),
            "регистрысведений" | "informationregisters" => {
                Some(Self::InformationRegister)
            }
            "регистрынакопления" | "accumulationregisters" => {
                Some(Self::AccumulationRegister)
            }
            "регистрыбухгалтерии" | "accountingregisters" => {
                Some(Self::AccountingRegister)
            }
            "регистрырасчета" | "calculationregisters" => {
                Some(Self::CalculationRegister)
            }
            "планывидовхарактеристик" | "chartsofcharacteristictypes" => {
                Some(Self::ChartOfCharacteristicTypes)
            }
            "планысчетов" | "chartsofaccounts" => Some(Self::ChartOfAccounts),
            "планывидоврасчета" | "chartsofcalculationtypes" => {
                Some(Self::ChartOfCalculationTypes)
            }
            "бизнеспроцессы" | "businessprocesses" => Some(Self::BusinessProcess),
            "задачи" | "tasks" => Some(Self::Task),
            "перечисления" | "enums" => Some(Self::Enum),
            "планыобмена" | "exchangeplans" => Some(Self::ExchangePlan),
            "внешниеисточникиданных" | "externaldatasources" => {
                Some(Self::ExternalDataSource)
            }
            "кубы" | "cubes" => Some(Self::Cube),
            "таблицыизмерения" | "dimensiontables" => Some(Self::DimensionTable),
            "константы" | "constants" => Some(Self::Constant),
            "обработки" | "dataprocessors" => Some(Self::DataProcessor),
            "отчеты" | "reports" => Some(Self::Report),
            "общиемодули" | "commonmodules" => Some(Self::CommonModule),
            _ => None,
        }
    }
}

/// Simple metadata object
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataObject {
    /// Object type
    pub mdo_type: MdoType,
    /// Object name (Russian)
    pub name: String,
    /// English name (optional, for bilingual completion)
    #[serde(default)]
    pub name_en: Option<String>,
    /// Custom attributes (fields) for this object
    #[serde(default)]
    pub attributes: Vec<Attribute>,
    /// Tabular sections (child collections) for this object
    #[serde(default)]
    pub tabular_sections: Vec<crate::tabular_section::TabularSection>,
    /// Child objects (e.g., Cubes for ExternalDataSource, DimensionTables for Cube)
    #[serde(default)]
    pub children: Vec<MetadataObject>,
}

/// Metadata object attribute (custom field).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    /// Russian name
    pub name: String,
    /// English name (optional)
    #[serde(default)]
    pub name_en: Option<String>,
    /// Attribute type
    pub attr_type: AttributeType,
}

/// Attribute type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttributeType {
    /// String with optional length
    String {
        /// Maximum string length
        length: Option<u32>,
    },
    /// Number with precision and scale
    Number {
        /// Total number of digits
        precision: u8,
        /// Number of digits after decimal point
        scale: u8,
    },
    /// Boolean
    Boolean,
    /// Date (without time)
    Date,
    /// DateTime (with time)
    DateTime,
    /// Reference to another metadata object
    Ref {
        /// Referenced metadata object type
        mdo_type: MdoType,
        /// Referenced metadata object name
        name: String,
    },
    /// Any reference (ЛюбаяСсылка)
    AnyRef,
    /// UUID
    Uuid,
    /// ValueStorage (ХранилищеЗначения)
    ValueStorage,
    /// DefinedType (ОпределяемыйТип)
    DefinedType {
        /// Name of the defined type
        name: String,
    },
    /// Unknown or unsupported type
    Unknown,
}

impl MetadataObject {
    /// Create new metadata object
    pub fn new(mdo_type: MdoType, name: impl Into<String>) -> Self {
        Self {
            mdo_type,
            name: name.into(),
            name_en: None,
            attributes: Vec::new(),
            tabular_sections: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Create with children
    pub fn with_children(
        mdo_type: MdoType,
        name: impl Into<String>,
        children: Vec<MetadataObject>,
    ) -> Self {
        Self {
            mdo_type,
            name: name.into(),
            name_en: None,
            attributes: Vec::new(),
            tabular_sections: Vec::new(),
            children,
        }
    }

    /// Create with full details
    pub fn with_details(
        mdo_type: MdoType,
        name: impl Into<String>,
        name_en: Option<String>,
        attributes: Vec<Attribute>,
    ) -> Self {
        Self {
            mdo_type,
            name: name.into(),
            name_en,
            attributes,
            tabular_sections: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Add child object
    pub fn add_child(&mut self, child: MetadataObject) {
        self.children.push(child);
    }

    /// Add attribute
    pub fn add_attribute(&mut self, attribute: Attribute) {
        self.attributes.push(attribute);
    }

    /// Add a tabular section
    pub fn add_tabular_section(&mut self, tabular_section: crate::tabular_section::TabularSection) {
        self.tabular_sections.push(tabular_section);
    }

    /// Find child by name (case-insensitive)
    pub fn find_child(&self, name: &str) -> Option<&MetadataObject> {
        let name_lower = name.to_lowercase();
        self.children.iter().find(|child| child.name.to_lowercase() == name_lower)
    }

    /// Check if has child with given name (case-insensitive)
    pub fn has_child(&self, name: &str) -> bool {
        self.find_child(name).is_some()
    }

    /// Find attribute by name (case-insensitive)
    pub fn find_attribute(&self, name: &str) -> Option<&Attribute> {
        let name_lower = name.to_lowercase();
        self.attributes.iter().find(|attr| attr.name.to_lowercase() == name_lower)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mdo_type_from_str_russian() {
        assert_eq!("Справочник".parse::<MdoType>().ok(), Some(MdoType::Catalog));
        assert_eq!("РегистрСведений".parse::<MdoType>().ok(), Some(MdoType::InformationRegister));
        assert_eq!("Документ".parse::<MdoType>().ok(), Some(MdoType::Document));
    }

    #[test]
    fn test_mdo_type_from_str_english() {
        assert_eq!("Catalog".parse::<MdoType>().ok(), Some(MdoType::Catalog));
        assert_eq!(
            "InformationRegister".parse::<MdoType>().ok(),
            Some(MdoType::InformationRegister)
        );
        assert_eq!("Document".parse::<MdoType>().ok(), Some(MdoType::Document));
    }

    #[test]
    fn test_mdo_type_case_insensitive() {
        assert_eq!("справочник".parse::<MdoType>().ok(), Some(MdoType::Catalog));
        assert_eq!("CATALOG".parse::<MdoType>().ok(), Some(MdoType::Catalog));
        assert_eq!("CaTaLoG".parse::<MdoType>().ok(), Some(MdoType::Catalog));
    }

    #[test]
    fn test_mdo_type_names() {
        assert_eq!(MdoType::Catalog.russian_name(), "Справочник");
        assert_eq!(MdoType::Catalog.english_name(), "Catalog");
    }

    #[test]
    fn test_mdo_type_from_plural() {
        // Russian plural forms
        assert_eq!(MdoType::from_plural("Документы"), Some(MdoType::Document));
        assert_eq!(MdoType::from_plural("Справочники"), Some(MdoType::Catalog));
        assert_eq!(MdoType::from_plural("РегистрыСведений"), Some(MdoType::InformationRegister));
        assert_eq!(MdoType::from_plural("РегистрыНакопления"), Some(MdoType::AccumulationRegister));
        assert_eq!(MdoType::from_plural("ПланыСчетов"), Some(MdoType::ChartOfAccounts));

        // English plural forms
        assert_eq!(MdoType::from_plural("documents"), Some(MdoType::Document));
        assert_eq!(MdoType::from_plural("catalogs"), Some(MdoType::Catalog));
        assert_eq!(
            MdoType::from_plural("InformationRegisters"),
            Some(MdoType::InformationRegister)
        );

        // Case insensitive
        assert_eq!(MdoType::from_plural("ДОКУМЕНТЫ"), Some(MdoType::Document));
        assert_eq!(MdoType::from_plural("CaTaLoGs"), Some(MdoType::Catalog));

        // Invalid input
        assert_eq!(MdoType::from_plural("InvalidType"), None);
        assert_eq!(MdoType::from_plural(""), None);
    }

    #[test]
    fn test_metadata_object_with_attributes() {
        let mut obj = MetadataObject::new(MdoType::Catalog, "Валюты");

        assert_eq!(obj.name, "Валюты");
        assert_eq!(obj.name_en, None);
        assert!(obj.attributes.is_empty());

        // Add attributes
        obj.add_attribute(Attribute {
            name: "Код".to_string(),
            name_en: Some("Code".to_string()),
            attr_type: AttributeType::String { length: Some(10) },
        });

        obj.add_attribute(Attribute {
            name: "Курс".to_string(),
            name_en: Some("Rate".to_string()),
            attr_type: AttributeType::Number { precision: 15, scale: 4 },
        });

        assert_eq!(obj.attributes.len(), 2);

        // Find attribute by Russian name (case-insensitive)
        let code_attr = obj.find_attribute("Код").unwrap();
        assert_eq!(code_attr.name, "Код");
        assert_eq!(code_attr.name_en, Some("Code".to_string()));
        assert_eq!(code_attr.attr_type, AttributeType::String { length: Some(10) });

        // Find by Russian name (case-insensitive)
        let rate_attr = obj.find_attribute("курс").unwrap();
        assert_eq!(rate_attr.name, "Курс");

        // Not found
        assert!(obj.find_attribute("НесуществующееПоле").is_none());
    }

    #[test]
    fn test_metadata_object_with_details() {
        let attributes = vec![
            Attribute {
                name: "Активен".to_string(),
                name_en: Some("Active".to_string()),
                attr_type: AttributeType::Boolean,
            },
            Attribute {
                name: "Дата".to_string(),
                name_en: Some("Date".to_string()),
                attr_type: AttributeType::Date,
            },
        ];

        let obj = MetadataObject::with_details(
            MdoType::Document,
            "ПриходнаяНакладная",
            Some("GoodsReceipt".to_string()),
            attributes,
        );

        assert_eq!(obj.name, "ПриходнаяНакладная");
        assert_eq!(obj.name_en, Some("GoodsReceipt".to_string()));
        assert_eq!(obj.attributes.len(), 2);
    }

    #[test]
    fn test_attribute_types() {
        let attr_string = AttributeType::String { length: Some(100) };
        assert_eq!(attr_string, AttributeType::String { length: Some(100) });

        let attr_number = AttributeType::Number { precision: 10, scale: 2 };
        assert_eq!(attr_number, AttributeType::Number { precision: 10, scale: 2 });

        let attr_ref =
            AttributeType::Ref { mdo_type: MdoType::Catalog, name: "Валюты".to_string() };
        assert_eq!(
            attr_ref,
            AttributeType::Ref { mdo_type: MdoType::Catalog, name: "Валюты".to_string() }
        );

        let attr_boolean = AttributeType::Boolean;
        assert_eq!(attr_boolean, AttributeType::Boolean);

        let attr_date = AttributeType::Date;
        assert_eq!(attr_date, AttributeType::Date);

        let attr_datetime = AttributeType::DateTime;
        assert_eq!(attr_datetime, AttributeType::DateTime);

        let attr_unknown = AttributeType::Unknown;
        assert_eq!(attr_unknown, AttributeType::Unknown);
    }
}
