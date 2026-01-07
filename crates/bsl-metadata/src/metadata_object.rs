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
    /// Object name
    pub name: String,
    /// Child objects (e.g., Cubes for ExternalDataSource, DimensionTables for Cube)
    #[serde(default)]
    pub children: Vec<MetadataObject>,
}

impl MetadataObject {
    /// Create new metadata object
    pub fn new(mdo_type: MdoType, name: impl Into<String>) -> Self {
        Self { mdo_type, name: name.into(), children: Vec::new() }
    }

    /// Create with children
    pub fn with_children(
        mdo_type: MdoType,
        name: impl Into<String>,
        children: Vec<MetadataObject>,
    ) -> Self {
        Self { mdo_type, name: name.into(), children }
    }

    /// Add child object
    pub fn add_child(&mut self, child: MetadataObject) {
        self.children.push(child);
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
}
