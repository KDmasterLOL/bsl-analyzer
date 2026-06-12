use stdx::case::CaseExt;
const PLATFORM_EVENT_HANDLERS: &[&str] = &[
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
    "обработкаполученияданныхвыбора",
    "choicedatagetprocessing",
    "обработкаполученияформы",
    "formgetprocessing",
    "обработкаполученияпредставления",
    "presentationgetprocessing",
    "обработкаполученияполейпредставления",
    "presentationfieldsgetprocessing",
    "обработкакоманды",
    "commandprocessing",
];

// Application-family event handlers the platform invokes with no local call site,
// so `UnusedLocalMethod` must exempt them — but each set is gated to the module kind
// that actually exposes it (see the call site). The sets are scoped narrowly on
// purpose: a name absent here is at most a residual false positive, while a name
// exempted in a module that does NOT invoke it would mask a real unused method.

/// Start/exit hooks present in every application-family module, including the
/// non-interactive external-connection module.
const APP_RUN_LIFECYCLE_HANDLERS: &[&str] =
    &["приначалеработысистемы", "onstart", "призавершенииработысистемы", "onexit"];

/// Interactive start/exit hooks (a cancellable "before" phase) — managed and
/// ordinary application modules, but NOT the non-interactive external connection.
const APP_INTERACTIVE_LIFECYCLE_HANDLERS: &[&str] =
    &["передначаломработысистемы", "beforestart", "передзавершениемработысистемы", "beforeexit"];

/// External-event hook — managed and ordinary application modules.
const APP_EXTERNAL_EVENT_HANDLERS: &[&str] =
    &["обработкавнешнегособытия", "externaleventprocessing"];

/// Interaction-system, global-search and navigation hooks exposed ONLY by the
/// managed application module. Russian-only: the canonical English spellings of
/// these newer handlers are not asserted here, so English-spelled code keeps a
/// residual FP rather than risk minting a wrong alias.
const MANAGED_APP_UI_HANDLERS: &[&str] = &[
    "обработкаполученияформывыборапользователейсистемывзаимодействия",
    "приглобальномпоиске",
    "привыборерезультатаглобальногопоиска",
    "привыборедействиярезультатаглобальногопоиска",
    "обработкапереходапонавигационнойссылке",
    "приизменениидоступностиосновногосервера",
];

/// Event handlers the platform invokes in the session module.
const SESSION_MODULE_EVENT_HANDLERS: &[&str] =
    &["установкапараметровсеанса", "sessionparameterssetting"];

pub fn is_platform_event_handler(name: &str) -> bool {
    let lower = name.fold_lower();
    PLATFORM_EVENT_HANDLERS.contains(&lower.as_str())
}

pub fn is_managed_application_module_event_handler(name: &str) -> bool {
    let lower = name.fold_lower();
    is_ordinary_application_module_event_handler(&lower)
        || MANAGED_APP_UI_HANDLERS.contains(&lower.as_str())
}

pub fn is_ordinary_application_module_event_handler(name: &str) -> bool {
    let lower = name.fold_lower();
    APP_RUN_LIFECYCLE_HANDLERS.contains(&lower.as_str())
        || APP_INTERACTIVE_LIFECYCLE_HANDLERS.contains(&lower.as_str())
        || APP_EXTERNAL_EVENT_HANDLERS.contains(&lower.as_str())
}

pub fn is_external_connection_module_event_handler(name: &str) -> bool {
    let lower = name.fold_lower();
    APP_RUN_LIFECYCLE_HANDLERS.contains(&lower.as_str())
}

pub fn is_session_module_event_handler(name: &str) -> bool {
    let lower = name.fold_lower();
    SESSION_MODULE_EVENT_HANDLERS.contains(&lower.as_str())
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
