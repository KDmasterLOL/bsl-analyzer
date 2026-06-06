use bsl_metadata::MdoType;

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
        Cube | DimensionTable | CommonModule | EventSubscription | Subsystem => "",
    }
}
