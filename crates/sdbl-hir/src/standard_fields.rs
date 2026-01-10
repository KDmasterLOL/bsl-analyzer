//! Standard field definitions per MdoType.
//!
//! Defines standard attributes available for each metadata object type.

use bsl_metadata::MdoType;

use crate::hir::FieldDef;
use crate::types::SdblType;

/// Get standard fields for a metadata object type.
pub fn standard_fields_for_mdo(mdo_type: MdoType) -> Vec<FieldDef> {
    match mdo_type {
        MdoType::Catalog => catalog_standard_fields(),
        MdoType::Document => document_standard_fields(),
        MdoType::InformationRegister => information_register_standard_fields(),
        MdoType::AccumulationRegister => accumulation_register_standard_fields(),
        MdoType::AccountingRegister => accounting_register_standard_fields(),
        MdoType::CalculationRegister => calculation_register_standard_fields(),
        MdoType::ChartOfCharacteristicTypes => chart_of_characteristic_types_standard_fields(),
        MdoType::ChartOfAccounts => chart_of_accounts_standard_fields(),
        MdoType::ChartOfCalculationTypes => chart_of_calculation_types_standard_fields(),
        MdoType::BusinessProcess => business_process_standard_fields(),
        MdoType::Task => task_standard_fields(),
        MdoType::Enum => enum_standard_fields(),
        MdoType::ExchangePlan => exchange_plan_standard_fields(),
        MdoType::ExternalDataSource => Vec::new(), // External sources define their own fields
        MdoType::Cube => Vec::new(),
        MdoType::DimensionTable => Vec::new(),
        MdoType::Constant => constant_standard_fields(),
        MdoType::DataProcessor => Vec::new(), // No standard query fields
        MdoType::Report => Vec::new(),
        MdoType::CommonModule => Vec::new(),
    }
}

/// Standard fields for Справочник (Catalog).
fn catalog_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Ссылка", "Ref", SdblType::Unknown), // Type depends on specific catalog
        FieldDef::standard("Код", "Code", SdblType::string()),
        FieldDef::standard("Наименование", "Description", SdblType::string()),
        FieldDef::standard("Родитель", "Parent", SdblType::Unknown), // Ref to same catalog
        FieldDef::standard("Владелец", "Owner", SdblType::Unknown),  // Ref to owner type
        FieldDef::standard("ПометкаУдаления", "DeletionMark", SdblType::Boolean),
        FieldDef::standard("Предопределенный", "Predefined", SdblType::Boolean),
        FieldDef::standard("ЭтоГруппа", "IsFolder", SdblType::Boolean),
    ]
}

/// Standard fields for Документ (Document).
fn document_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Ссылка", "Ref", SdblType::Unknown),
        FieldDef::standard("Номер", "Number", SdblType::string()),
        FieldDef::standard("Дата", "Date", SdblType::DateTime),
        FieldDef::standard("Проведен", "Posted", SdblType::Boolean),
        FieldDef::standard("ПометкаУдаления", "DeletionMark", SdblType::Boolean),
    ]
}

/// Standard fields for РегистрСведений (InformationRegister).
fn information_register_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Период", "Period", SdblType::DateTime),
        FieldDef::standard("Регистратор", "Recorder", SdblType::Unknown),
        FieldDef::standard("НомерСтроки", "LineNumber", SdblType::number()),
        FieldDef::standard("Активность", "Active", SdblType::Boolean),
    ]
}

/// Standard fields for РегистрНакопления (AccumulationRegister).
fn accumulation_register_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Период", "Period", SdblType::DateTime),
        FieldDef::standard("Регистратор", "Recorder", SdblType::Unknown),
        FieldDef::standard("НомерСтроки", "LineNumber", SdblType::number()),
        FieldDef::standard("Активность", "Active", SdblType::Boolean),
        FieldDef::standard("ВидДвижения", "RecordType", SdblType::string()), // Приход/Расход
    ]
}

/// Standard fields for РегистрБухгалтерии (AccountingRegister).
fn accounting_register_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Период", "Period", SdblType::DateTime),
        FieldDef::standard("Регистратор", "Recorder", SdblType::Unknown),
        FieldDef::standard("НомерСтроки", "LineNumber", SdblType::number()),
        FieldDef::standard("Активность", "Active", SdblType::Boolean),
        FieldDef::standard("СчетДт", "AccountDr", SdblType::Unknown),
        FieldDef::standard("СчетКт", "AccountCr", SdblType::Unknown),
        FieldDef::standard("Сумма", "Amount", SdblType::number()),
    ]
}

/// Standard fields for РегистрРасчета (CalculationRegister).
fn calculation_register_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("ПериодРегистрации", "RegistrationPeriod", SdblType::DateTime),
        FieldDef::standard("ПериодДействия", "ActualPeriod", SdblType::DateTime),
        FieldDef::standard("Регистратор", "Recorder", SdblType::Unknown),
        FieldDef::standard("НомерСтроки", "LineNumber", SdblType::number()),
        FieldDef::standard("Активность", "Active", SdblType::Boolean),
        FieldDef::standard("Сторно", "Reversal", SdblType::Boolean),
        FieldDef::standard("ВидРасчета", "CalculationType", SdblType::Unknown),
    ]
}

/// Standard fields for ПланВидовХарактеристик (ChartOfCharacteristicTypes).
fn chart_of_characteristic_types_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Ссылка", "Ref", SdblType::Unknown),
        FieldDef::standard("Код", "Code", SdblType::string()),
        FieldDef::standard("Наименование", "Description", SdblType::string()),
        FieldDef::standard("Родитель", "Parent", SdblType::Unknown),
        FieldDef::standard("ПометкаУдаления", "DeletionMark", SdblType::Boolean),
        FieldDef::standard("Предопределенный", "Predefined", SdblType::Boolean),
        FieldDef::standard("ЭтоГруппа", "IsFolder", SdblType::Boolean),
        FieldDef::standard("ТипЗначения", "ValueType", SdblType::Unknown),
    ]
}

/// Standard fields for ПланСчетов (ChartOfAccounts).
fn chart_of_accounts_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Ссылка", "Ref", SdblType::Unknown),
        FieldDef::standard("Код", "Code", SdblType::string()),
        FieldDef::standard("Наименование", "Description", SdblType::string()),
        FieldDef::standard("Родитель", "Parent", SdblType::Unknown),
        FieldDef::standard("ПометкаУдаления", "DeletionMark", SdblType::Boolean),
        FieldDef::standard("Предопределенный", "Predefined", SdblType::Boolean),
        FieldDef::standard("Порядок", "Order", SdblType::string()),
        FieldDef::standard("ВидСчета", "Type", SdblType::Unknown),
        FieldDef::standard("Забалансовый", "OffBalance", SdblType::Boolean),
    ]
}

/// Standard fields for ПланВидовРасчета (ChartOfCalculationTypes).
fn chart_of_calculation_types_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Ссылка", "Ref", SdblType::Unknown),
        FieldDef::standard("Код", "Code", SdblType::string()),
        FieldDef::standard("Наименование", "Description", SdblType::string()),
        FieldDef::standard("ПометкаУдаления", "DeletionMark", SdblType::Boolean),
        FieldDef::standard("Предопределенный", "Predefined", SdblType::Boolean),
    ]
}

/// Standard fields for БизнесПроцесс (BusinessProcess).
fn business_process_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Ссылка", "Ref", SdblType::Unknown),
        FieldDef::standard("Номер", "Number", SdblType::string()),
        FieldDef::standard("Дата", "Date", SdblType::DateTime),
        FieldDef::standard("ПометкаУдаления", "DeletionMark", SdblType::Boolean),
        FieldDef::standard("Стартован", "Started", SdblType::Boolean),
        FieldDef::standard("Завершен", "Completed", SdblType::Boolean),
    ]
}

/// Standard fields for Задача (Task).
fn task_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Ссылка", "Ref", SdblType::Unknown),
        FieldDef::standard("Номер", "Number", SdblType::string()),
        FieldDef::standard("Дата", "Date", SdblType::DateTime),
        FieldDef::standard("ПометкаУдаления", "DeletionMark", SdblType::Boolean),
        FieldDef::standard("Наименование", "Description", SdblType::string()),
        FieldDef::standard("Выполнена", "Executed", SdblType::Boolean),
        FieldDef::standard("БизнесПроцесс", "BusinessProcess", SdblType::Unknown),
        FieldDef::standard("ТочкаМаршрута", "RoutePoint", SdblType::Unknown),
    ]
}

/// Standard fields for Перечисление (Enum).
fn enum_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Ссылка", "Ref", SdblType::Unknown),
        FieldDef::standard("Порядок", "Order", SdblType::number()),
    ]
}

/// Standard fields for ПланОбмена (ExchangePlan).
fn exchange_plan_standard_fields() -> Vec<FieldDef> {
    vec![
        FieldDef::standard("Ссылка", "Ref", SdblType::Unknown),
        FieldDef::standard("Код", "Code", SdblType::string()),
        FieldDef::standard("Наименование", "Description", SdblType::string()),
        FieldDef::standard("ПометкаУдаления", "DeletionMark", SdblType::Boolean),
        FieldDef::standard("Предопределенный", "Predefined", SdblType::Boolean),
        FieldDef::standard("ЭтотУзел", "ThisNode", SdblType::Boolean),
    ]
}

/// Standard fields для Константа (Constant).
fn constant_standard_fields() -> Vec<FieldDef> {
    vec![FieldDef::standard("Значение", "Value", SdblType::Unknown)]
}

/// Virtual table types and their Russian/English names.
pub const VIRTUAL_TABLES: &[(&str, &str)] = &[
    ("срезпоследних", "slicelast"),
    ("срезпервых", "slicefirst"),
    ("остатки", "balance"),
    ("обороты", "turnovers"),
    ("остаткииобороты", "balanceandturnovers"),
    ("движениясостороккорреспонденциями", "recordswithextdimensions"),
    ("движениясубконто", "extdimensiondr"),
    ("субконто", "extdimensions"),
    ("обороты", "turnovers"),
    ("остатки", "balance"),
];

/// Check if table name part is a virtual table.
pub fn is_virtual_table_name(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    VIRTUAL_TABLES.iter().any(|(ru, en)| *ru == name_lower || *en == name_lower)
}

/// Get virtual table type from name (for diagnostics).
pub fn virtual_table_type(name: &str) -> Option<&'static str> {
    let name_lower = name.to_lowercase();
    for (ru, en) in VIRTUAL_TABLES {
        if *ru == name_lower {
            return Some(ru);
        }
        if *en == name_lower {
            return Some(ru); // Return Russian name for diagnostics
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_standard_fields() {
        let fields = standard_fields_for_mdo(MdoType::Catalog);
        assert!(fields.iter().any(|f| f.name == "Ссылка"));
        assert!(fields.iter().any(|f| f.name == "Код"));
        assert!(fields.iter().any(|f| f.name == "Наименование"));
        assert!(fields.iter().any(|f| f.name == "ПометкаУдаления"));
    }

    #[test]
    fn test_document_standard_fields() {
        let fields = standard_fields_for_mdo(MdoType::Document);
        assert!(fields.iter().any(|f| f.name == "Ссылка"));
        assert!(fields.iter().any(|f| f.name == "Номер"));
        assert!(fields.iter().any(|f| f.name == "Дата"));
        assert!(fields.iter().any(|f| f.name == "Проведен"));
    }

    #[test]
    fn test_virtual_table_detection() {
        assert!(is_virtual_table_name("СрезПоследних"));
        assert!(is_virtual_table_name("срезпоследних"));
        assert!(is_virtual_table_name("SliceLast"));
        assert!(is_virtual_table_name("slicelast"));
        assert!(is_virtual_table_name("Остатки"));
        assert!(is_virtual_table_name("Balance"));

        assert!(!is_virtual_table_name("Справочник"));
        assert!(!is_virtual_table_name("Random"));
    }

    #[test]
    fn test_virtual_table_type() {
        assert_eq!(virtual_table_type("СрезПоследних"), Some("срезпоследних"));
        assert_eq!(virtual_table_type("SliceLast"), Some("срезпоследних"));
        assert_eq!(virtual_table_type("Остатки"), Some("остатки"));
        assert_eq!(virtual_table_type("Balance"), Some("остатки"));
        assert_eq!(virtual_table_type("Unknown"), None);
    }
}
