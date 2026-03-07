//! Shared list of platform event handler names.
//!
//! These methods have a fixed signature defined by the 1C platform —
//! their parameters cannot be removed and the methods are called
//! by the platform, not by user code.

const PLATFORM_EVENT_HANDLERS: &[&str] = &[
    // Object module handlers
    "передзаписью",
    "beforewrite",
    "призаписи",
    "onwrite",
    "передудалением",
    "beforedelete",
    "прикопировании",
    "oncopy",
    "обработказаполнения",
    "filling",
    "обработкапроверкизаполнения",
    "fillcheckprocessing",
    "присозданииобъекта",
    "onobjectcreate",
    "приустановкеновогономера",
    "onsetnewnumber",
    "приустановкеновогокода",
    "onsetnewcode",
    "обработкапроведения",
    "posting",
    "обработкаудаленияпроведения",
    "undoposting",
    // Manager module handlers
    "обработкаполученияданныхвыбора",
    "choicedatagetprocessing",
    "обработкаполученияформы",
    "formgetprocessing",
    "обработкаполученияпредставления",
    "presentationgetprocessing",
    "обработкаполученияполейпредставления",
    "presentationfieldsgetprocessing",
    // Command module handlers
    "обработкакоманды",
    "commandprocessing",
];

pub fn is_platform_event_handler(name: &str) -> bool {
    let lower = name.to_lowercase();
    PLATFORM_EVENT_HANDLERS.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_module_handlers() {
        assert!(is_platform_event_handler("ПередЗаписью"));
        assert!(is_platform_event_handler("BeforeWrite"));
        assert!(is_platform_event_handler("ПриЗаписи"));
        assert!(is_platform_event_handler("OnWrite"));
        assert!(is_platform_event_handler("ПередУдалением"));
        assert!(is_platform_event_handler("BeforeDelete"));
        assert!(is_platform_event_handler("ПриКопировании"));
        assert!(is_platform_event_handler("OnCopy"));
        assert!(is_platform_event_handler("ОбработкаЗаполнения"));
        assert!(is_platform_event_handler("Filling"));
        assert!(is_platform_event_handler("ОбработкаПроверкиЗаполнения"));
        assert!(is_platform_event_handler("FillCheckProcessing"));
        assert!(is_platform_event_handler("ПриСозданииОбъекта"));
        assert!(is_platform_event_handler("OnObjectCreate"));
        assert!(is_platform_event_handler("ПриУстановкеНовогоНомера"));
        assert!(is_platform_event_handler("OnSetNewNumber"));
        assert!(is_platform_event_handler("ПриУстановкеНовогоКода"));
        assert!(is_platform_event_handler("OnSetNewCode"));
        assert!(is_platform_event_handler("ОбработкаПроведения"));
        assert!(is_platform_event_handler("Posting"));
        assert!(is_platform_event_handler("ОбработкаУдаленияПроведения"));
        assert!(is_platform_event_handler("UndoPosting"));
    }

    #[test]
    fn test_manager_module_handlers() {
        assert!(is_platform_event_handler("ОбработкаПолученияДанныхВыбора"));
        assert!(is_platform_event_handler("ChoiceDataGetProcessing"));
        assert!(is_platform_event_handler("ОбработкаПолученияФормы"));
        assert!(is_platform_event_handler("FormGetProcessing"));
        assert!(is_platform_event_handler("ОбработкаПолученияПредставления"));
        assert!(is_platform_event_handler("PresentationGetProcessing"));
        assert!(is_platform_event_handler("ОбработкаПолученияПолейПредставления"));
        assert!(is_platform_event_handler("PresentationFieldsGetProcessing"));
    }

    #[test]
    fn test_command_module_handlers() {
        assert!(is_platform_event_handler("ОбработкаКоманды"));
        assert!(is_platform_event_handler("CommandProcessing"));
    }

    #[test]
    fn test_case_insensitive() {
        assert!(is_platform_event_handler("передзаписью"));
        assert!(is_platform_event_handler("ПЕРЕДЗАПИСЬЮ"));
        assert!(is_platform_event_handler("beforewrite"));
        assert!(is_platform_event_handler("BEFOREWRITE"));
    }

    #[test]
    fn test_non_handler() {
        assert!(!is_platform_event_handler("МойМетод"));
        assert!(!is_platform_event_handler("MyMethod"));
        assert!(!is_platform_event_handler(""));
    }
}
