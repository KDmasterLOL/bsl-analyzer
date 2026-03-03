//! Basic metadata object types
//!
//! Simplified metadata object structure for query validation and diagnostics

use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::OnceLock;

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

    /// Check if the given keyword is the Russian variant (case-insensitive)
    pub fn is_russian_keyword(&self, keyword: &str) -> bool {
        let keyword_lower = keyword.to_lowercase();
        let russian_lower = self.russian_name().to_lowercase();
        keyword_lower == russian_lower
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

    /// Check if a given name is an MDO plural form (case-insensitive).
    /// Uses O(1) lookup via FxHashSet with lazy static initialization.
    ///
    /// # Examples
    /// ```
    /// use bsl_metadata::MdoType;
    ///
    /// assert!(MdoType::is_plural_form("Документы"));
    /// assert!(MdoType::is_plural_form("documents"));
    /// assert!(MdoType::is_plural_form("СПРАВОЧНИКИ"));
    /// assert!(!MdoType::is_plural_form("Документ"));
    /// ```
    pub fn is_plural_form(s: &str) -> bool {
        static PLURAL_FORMS: OnceLock<FxHashSet<String>> = OnceLock::new();

        let set = PLURAL_FORMS.get_or_init(|| {
            let mut set = FxHashSet::default();
            // All 20 MDO types, both Russian and English, lowercase
            set.insert("документы".to_string());
            set.insert("documents".to_string());
            set.insert("справочники".to_string());
            set.insert("catalogs".to_string());
            set.insert("регистрысведений".to_string());
            set.insert("informationregisters".to_string());
            set.insert("регистрынакопления".to_string());
            set.insert("accumulationregisters".to_string());
            set.insert("регистрыбухгалтерии".to_string());
            set.insert("accountingregisters".to_string());
            set.insert("регистрырасчета".to_string());
            set.insert("calculationregisters".to_string());
            set.insert("планывидовхарактеристик".to_string());
            set.insert("chartsofcharacteristictypes".to_string());
            set.insert("планысчетов".to_string());
            set.insert("chartsofaccounts".to_string());
            set.insert("планывидоврасчета".to_string());
            set.insert("chartsofcalculationtypes".to_string());
            set.insert("бизнеспроцессы".to_string());
            set.insert("businessprocesses".to_string());
            set.insert("задачи".to_string());
            set.insert("tasks".to_string());
            set.insert("перечисления".to_string());
            set.insert("enums".to_string());
            set.insert("планыобмена".to_string());
            set.insert("exchangeplans".to_string());
            set.insert("внешниеисточникиданных".to_string());
            set.insert("externaldatasources".to_string());
            set.insert("кубы".to_string());
            set.insert("cubes".to_string());
            set.insert("таблицыизмерения".to_string());
            set.insert("dimensiontables".to_string());
            set.insert("константы".to_string());
            set.insert("constants".to_string());
            set.insert("обработки".to_string());
            set.insert("dataprocessors".to_string());
            set.insert("отчеты".to_string());
            set.insert("отчёты".to_string());
            set.insert("reports".to_string());
            set.insert("общиемодули".to_string());
            set.insert("commonmodules".to_string());
            set
        });

        set.contains(&s.to_lowercase())
    }
}

/// Enumeration value (element of Enum)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumValue {
    /// Russian name
    pub name: String,
    /// English name (optional)
    #[serde(default)]
    pub name_en: Option<String>,
    /// UUID
    pub uuid: String,
}

/// Predefined item (for Catalogs, Documents, etc.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PredefinedItem {
    /// Russian name
    pub name: String,
    /// English name (optional)
    #[serde(default)]
    pub name_en: Option<String>,
    /// UUID
    pub uuid: String,
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
    /// Enumeration values (for Enum type only)
    #[serde(default)]
    pub enum_values: Vec<EnumValue>,
    /// Predefined items (for Catalog, Document, etc.)
    #[serde(default)]
    pub predefined_items: Vec<PredefinedItem>,
    /// Check code uniqueness (for Catalog, ChartOfCharacteristicTypes, ChartOfAccounts)
    #[serde(default)]
    pub check_unique: bool,
    /// Code series mode (for Catalog, ChartOfCharacteristicTypes, ChartOfAccounts)
    #[serde(default)]
    pub code_series: crate::enums::CodeSeries,
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

/// Standard attribute (built-in by platform)
///
/// Standard attributes are predefined by the 1C platform and available on all objects
/// of certain types. Their presence is controlled by object properties (e.g., CodeLength,
/// Hierarchical) rather than explicit XML declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StandardAttributeKind {
    // Catalog/Document standard attributes
    /// Code attribute (Код) - present if CodeLength > 0
    Code {
        /// Maximum length of code
        length: u32,
    },
    /// Description attribute (Наименование) - present if DescriptionLength > 0
    Description {
        /// Maximum length of description
        length: u32,
    },
    /// Reference attribute (Ссылка) - always present
    Ref,
    /// Deletion mark attribute (ПометкаУдаления) - always present
    DeletionMark,
    /// Is folder attribute (ЭтоГруппа) - present if Hierarchical=true
    IsFolder,
    /// Owner attribute (Владелец) - present if Owners is not empty
    Owner,
    /// Parent attribute (Родитель) - present if Hierarchical=true
    Parent,
    /// Predefined attribute (Предопределенный) - always present
    Predefined,
    /// Predefined data name attribute (ИмяПредопределенныхДанных) - always present
    PredefinedDataName,

    // Document/BusinessProcess/Task standard attributes
    /// Number attribute (Номер) - present if NumberLength > 0
    Number {
        /// Maximum length of number
        length: u32,
    },
    /// Date attribute (Дата) - always present on Document/BusinessProcess/Task
    Date,
    /// Posted attribute (Проведен) - Document only
    Posted,
    /// Started attribute (Стартован) - BusinessProcess only
    Started,
    /// Completed attribute (Завершен) - BusinessProcess only
    Completed,
    /// HeadTask attribute (ГлавнаяЗадача) - BusinessProcess only
    HeadTask,
    /// Executed attribute (Выполнена) - Task only
    Executed,
    /// BusinessProcess attribute (БизнесПроцесс) - Task only
    TaskBusinessProcess,
    /// RoutePoint attribute (ТочкаМаршрута) - Task only
    RoutePoint,

    // ExchangePlan standard attributes
    /// ThisNode attribute (ЭтотУзел) - ExchangePlan only
    ThisNode,

    // ChartOfCharacteristicTypes standard attributes
    /// ValueType attribute (ТипЗначения) - ChartOfCharacteristicTypes only
    ValueType,

    // ChartOfAccounts standard attributes
    /// Order attribute (Порядок) - ChartOfAccounts only
    Order,

    // Information Register standard attributes
    /// Active attribute (Активность) - always present
    Active,
    /// Line number attribute (НомерСтроки) - always present
    LineNumber,
    /// Recorder attribute (Регистратор) - always present
    Recorder,
    /// Period attribute (Период) - present if periodicity != Nonperiodical
    Period,
}

impl StandardAttributeKind {
    /// Get the AttributeType for this standard attribute
    ///
    /// # Arguments
    ///
    /// * `mdo_type` - Type of metadata object (for reference types)
    /// * `object_name` - Name of the object (for reference types)
    ///
    /// # Returns
    ///
    /// The platform-defined type for this standard attribute
    pub fn attribute_type(&self, mdo_type: MdoType, object_name: &str) -> AttributeType {
        match self {
            Self::Code { length } => AttributeType::String { length: Some(*length) },
            Self::Description { length } => AttributeType::String { length: Some(*length) },
            Self::Ref => AttributeType::Ref { mdo_type, name: object_name.to_string() },
            Self::DeletionMark => AttributeType::Boolean,
            Self::IsFolder => AttributeType::Boolean,
            Self::Owner => AttributeType::Unknown, // Type determined from Owners property
            Self::Parent => AttributeType::Ref { mdo_type, name: object_name.to_string() },
            Self::Predefined => AttributeType::Boolean,
            Self::PredefinedDataName => AttributeType::String { length: None },
            Self::Number { length } => AttributeType::String { length: Some(*length) },
            Self::Date => AttributeType::DateTime,
            Self::Posted => AttributeType::Boolean,
            Self::Started => AttributeType::Boolean,
            Self::Completed => AttributeType::Boolean,
            Self::HeadTask => AttributeType::Unknown,
            Self::Executed => AttributeType::Boolean,
            Self::TaskBusinessProcess => AttributeType::Unknown,
            Self::RoutePoint => AttributeType::Unknown,
            Self::ThisNode => AttributeType::Boolean,
            Self::ValueType => AttributeType::Unknown,
            Self::Order => AttributeType::String { length: None },
            Self::Active => AttributeType::Boolean,
            Self::LineNumber => AttributeType::Number { precision: 10, scale: 0 },
            Self::Recorder => AttributeType::AnyObjectRef { mdo_type: MdoType::Document },
            Self::Period => AttributeType::DateTime,
        }
    }

    /// Get the Russian name for this standard attribute
    pub fn russian_name(&self) -> &'static str {
        match self {
            Self::Code { .. } => "Код",
            Self::Description { .. } => "Наименование",
            Self::Ref => "Ссылка",
            Self::DeletionMark => "ПометкаУдаления",
            Self::IsFolder => "ЭтоГруппа",
            Self::Owner => "Владелец",
            Self::Parent => "Родитель",
            Self::Predefined => "Предопределенный",
            Self::PredefinedDataName => "ИмяПредопределенныхДанных",
            Self::Number { .. } => "Номер",
            Self::Date => "Дата",
            Self::Posted => "Проведен",
            Self::Started => "Стартован",
            Self::Completed => "Завершен",
            Self::HeadTask => "ГлавнаяЗадача",
            Self::Executed => "Выполнена",
            Self::TaskBusinessProcess => "БизнесПроцесс",
            Self::RoutePoint => "ТочкаМаршрута",
            Self::ThisNode => "ЭтотУзел",
            Self::ValueType => "ТипЗначения",
            Self::Order => "Порядок",
            Self::Active => "Активность",
            Self::LineNumber => "НомерСтроки",
            Self::Recorder => "Регистратор",
            Self::Period => "Период",
        }
    }

    /// Get the English name for this standard attribute
    pub fn english_name(&self) -> &'static str {
        match self {
            Self::Code { .. } => "Code",
            Self::Description { .. } => "Description",
            Self::Ref => "Ref",
            Self::DeletionMark => "DeletionMark",
            Self::IsFolder => "IsFolder",
            Self::Owner => "Owner",
            Self::Parent => "Parent",
            Self::Predefined => "Predefined",
            Self::PredefinedDataName => "PredefinedDataName",
            Self::Number { .. } => "Number",
            Self::Date => "Date",
            Self::Posted => "Posted",
            Self::Started => "Started",
            Self::Completed => "Completed",
            Self::HeadTask => "HeadTask",
            Self::Executed => "Executed",
            Self::TaskBusinessProcess => "BusinessProcess",
            Self::RoutePoint => "RoutePoint",
            Self::ThisNode => "ThisNode",
            Self::ValueType => "ValueType",
            Self::Order => "Order",
            Self::Active => "Active",
            Self::LineNumber => "LineNumber",
            Self::Recorder => "Recorder",
            Self::Period => "Period",
        }
    }
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
    /// Any object of specific type (e.g., any Catalog, any Document, any BusinessProcess)
    ///
    /// From TypeSet like cfg:CatalogRef, cfg:DocumentRef, cfg:BusinessProcessRef.
    /// Means "any object of this type" without specifying concrete object name.
    AnyObjectRef {
        /// Type of metadata object
        mdo_type: MdoType,
    },
    /// UUID
    Uuid,
    /// ValueStorage (ХранилищеЗначения)
    ValueStorage,
    /// DefinedType (ОпределяемыйТип)
    DefinedType {
        /// Name of the defined type
        name: String,
    },
    /// Composite type (multiple types allowed)
    ///
    /// Used when a field can hold values of different types.
    /// Example: Dimension "ВидДействия" in 1C can be one of 4 enum types.
    Composite {
        /// List of allowed types (union type)
        types: Vec<AttributeType>,
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
            enum_values: Vec::new(),
            predefined_items: Vec::new(),
            check_unique: false,
            code_series: crate::enums::CodeSeries::default(),
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
            enum_values: Vec::new(),
            predefined_items: Vec::new(),
            check_unique: false,
            code_series: crate::enums::CodeSeries::default(),
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
            enum_values: Vec::new(),
            predefined_items: Vec::new(),
            check_unique: false,
            code_series: crate::enums::CodeSeries::default(),
        }
    }

    /// Set check_unique property
    pub fn set_check_unique(&mut self, value: bool) {
        self.check_unique = value;
    }

    /// Set code_series property
    pub fn set_code_series(&mut self, value: crate::enums::CodeSeries) {
        self.code_series = value;
    }

    /// Check if FindByCode is safe for this object
    ///
    /// Returns `true` if both conditions are met:
    /// - check_unique is true
    /// - code_series is WholeCatalog (or equivalent)
    ///
    /// For objects where these properties don't apply, returns `true`.
    pub fn is_find_by_code_safe(&self) -> bool {
        match self.mdo_type {
            MdoType::Catalog | MdoType::ChartOfCharacteristicTypes | MdoType::ChartOfAccounts => {
                self.check_unique && self.code_series.is_whole()
            }
            _ => true,
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

    /// Find tabular section by name (case-insensitive, bilingual)
    pub fn find_tabular_section(
        &self,
        name: &str,
    ) -> Option<&crate::tabular_section::TabularSection> {
        let name_lower = name.to_lowercase();
        self.tabular_sections.iter().find(|ts| {
            ts.name().to_lowercase() == name_lower
                || ts.name_en().map(|en| en.to_lowercase() == name_lower).unwrap_or(false)
        })
    }

    /// Find enum value by name (case-insensitive, bilingual)
    pub fn find_enum_value(&self, name: &str) -> Option<&EnumValue> {
        let name_lower = name.to_lowercase();
        self.enum_values.iter().find(|ev| {
            ev.name.to_lowercase() == name_lower
                || ev.name_en.as_ref().map(|en| en.to_lowercase() == name_lower).unwrap_or(false)
        })
    }

    /// Find predefined item by name (case-insensitive, bilingual)
    pub fn find_predefined_item(&self, name: &str) -> Option<&PredefinedItem> {
        let name_lower = name.to_lowercase();
        self.predefined_items.iter().find(|pi| {
            pi.name.to_lowercase() == name_lower
                || pi.name_en.as_ref().map(|en| en.to_lowercase() == name_lower).unwrap_or(false)
        })
    }
}

impl std::fmt::Display for AttributeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String { length } => {
                write!(f, "Строка")?;
                if let Some(len) = length {
                    write!(f, "({})", len)?;
                }
                Ok(())
            }
            Self::Number { precision, scale } => {
                write!(f, "Число({}, {})", precision, scale)
            }
            Self::Boolean => write!(f, "Булево"),
            Self::Date => write!(f, "Дата"),
            Self::DateTime => write!(f, "ДатаВремя"),
            Self::Ref { mdo_type, name } => {
                write!(f, "{}.{}", mdo_type.russian_name(), name)
            }
            Self::AnyRef => write!(f, "ЛюбаяСсылка"),
            Self::AnyObjectRef { mdo_type } => {
                write!(f, "{}", mdo_type.russian_name())
            }
            Self::Uuid => write!(f, "УникальныйИдентификатор"),
            Self::ValueStorage => write!(f, "ХранилищеЗначения"),
            Self::DefinedType { name } => {
                write!(f, "ОпределяемыйТип.{}", name)
            }
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
            Self::Unknown => write!(f, "Неизвестно"),
        }
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

    #[test]
    fn test_composite_type_display() {
        // Single type composite should display as single type
        let single = AttributeType::Composite {
            types: vec![AttributeType::Ref {
                mdo_type: MdoType::Enum,
                name: "ВидДействия".to_string(),
            }],
        };
        assert_eq!(single.to_string(), "Перечисление.ВидДействия");

        // Multiple types composite should show brief label
        let composite = AttributeType::Composite {
            types: vec![
                AttributeType::Ref {
                    mdo_type: MdoType::Enum,
                    name: "ВидыУсловийОтбораИсходящихПисем".to_string(),
                },
                AttributeType::Ref {
                    mdo_type: MdoType::Enum,
                    name: "ВидыДействийПриОбработкеИсходящихПисем".to_string(),
                },
                AttributeType::Ref {
                    mdo_type: MdoType::Enum,
                    name: "ВидыУсловийОтбораВходящихПисем".to_string(),
                },
                AttributeType::Ref {
                    mdo_type: MdoType::Enum,
                    name: "ВидыДействийПриОбработкеВходящихПисем".to_string(),
                },
            ],
        };

        let display = composite.to_string();
        assert_eq!(display, "Составной тип:");
    }

    #[test]
    fn test_any_object_ref_display() {
        // Test AnyObjectRef Display implementation
        let catalog_ref = AttributeType::AnyObjectRef { mdo_type: MdoType::Catalog };
        assert_eq!(catalog_ref.to_string(), "Справочник");

        let document_ref = AttributeType::AnyObjectRef { mdo_type: MdoType::Document };
        assert_eq!(document_ref.to_string(), "Документ");

        let bp_ref = AttributeType::AnyObjectRef { mdo_type: MdoType::BusinessProcess };
        assert_eq!(bp_ref.to_string(), "БизнесПроцесс");

        let enum_ref = AttributeType::AnyObjectRef { mdo_type: MdoType::Enum };
        assert_eq!(enum_ref.to_string(), "Перечисление");
    }

    #[test]
    fn test_is_plural_form() {
        // Russian plural forms
        assert!(MdoType::is_plural_form("Документы"));
        assert!(MdoType::is_plural_form("Справочники"));
        assert!(MdoType::is_plural_form("РегистрыСведений"));

        // English plural forms
        assert!(MdoType::is_plural_form("Documents"));
        assert!(MdoType::is_plural_form("Catalogs"));
        assert!(MdoType::is_plural_form("InformationRegisters"));

        // Case-insensitive
        assert!(MdoType::is_plural_form("ДОКУМЕНТЫ"));
        assert!(MdoType::is_plural_form("документы"));
        assert!(MdoType::is_plural_form("ДоКуМеНтЫ"));
        assert!(MdoType::is_plural_form("CATALOGS"));
        assert!(MdoType::is_plural_form("catalogs"));

        // Singular forms should return false
        assert!(!MdoType::is_plural_form("Документ"));
        assert!(!MdoType::is_plural_form("Document"));
        assert!(!MdoType::is_plural_form("Справочник"));

        // Invalid forms
        assert!(!MdoType::is_plural_form(""));
        assert!(!MdoType::is_plural_form("InvalidType"));
    }
}
