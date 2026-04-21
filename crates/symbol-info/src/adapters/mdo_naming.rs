//! Single source for MDO type → Russian plural form used as the manager
//! collection prefix in BSL source (`Справочники`, `Документы`, …).

use bsl_metadata::MdoType;

/// Russian plural form of an MDO type, suitable as a callee qualifier
/// prefix. Returns `""` for MDO kinds that have no manager collection
/// (`Cube`, `DimensionTable`, `CommonModule`).
pub(super) fn russian_plural(mdo_type: MdoType) -> &'static str {
    use MdoType::*;
    match mdo_type {
        Catalog => "Справочники",
        Document => "Документы",
        DataProcessor => "Обработки",
        Report => "Отчеты",
        InformationRegister => "РегистрыСведений",
        AccumulationRegister => "РегистрыНакопления",
        AccountingRegister => "РегистрыБухгалтерии",
        CalculationRegister => "РегистрыРасчета",
        ChartOfCharacteristicTypes => "ПланыВидовХарактеристик",
        ChartOfAccounts => "ПланыСчетов",
        ChartOfCalculationTypes => "ПланыВидовРасчета",
        BusinessProcess => "БизнесПроцессы",
        Task => "Задачи",
        Enum => "Перечисления",
        ExchangePlan => "ПланыОбмена",
        ExternalDataSource => "ВнешниеИсточникиДанных",
        Constant => "Константы",
        Cube | DimensionTable | CommonModule => "",
    }
}
