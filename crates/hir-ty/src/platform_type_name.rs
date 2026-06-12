//! Single source of truth for platform type NAMES recognised beyond the generated
//! corpus. Both the concrete→generic subtype bridges ([`crate::subtype`]) and the
//! type-ref lowering gate ([`crate::lower`]) consult these tables, so a parameter
//! documented with a known generic / real-but-uncorpused name keeps a nominal
//! `PlatformObject` (the bridges can then match the concrete value), while a name
//! that resolves to nothing — a doc-comment typo (`СпискоЗначений`) or free prose
//! (`документ`, `если`) — degrades to Unknown rather than minting a phantom that
//! only ever produces false positives.
//!
//! BSL identifiers fold case (Cyrillic included), so all comparison goes through
//! [`name_eq_ci`], never `eq_ignore_ascii_case` alone.

use bsl_metadata::{FormElementKind, MdoType};
use stdx::case::CaseExt;

/// Case-insensitive match of `actual` against a `(ru, en)` canonical name pair.
pub(crate) fn name_eq_ci(actual: &str, ru: &str, en: &str) -> bool {
    actual.eq_ignore_ascii_case(en) || actual.fold_lower() == ru.fold_lower()
}

/// Generic manager type name per metadata kind. The single definition behind both
/// `subtype::generic_manager_names` and the lowering gate.
pub(crate) const MANAGER_NAMES: &[(MdoType, &str, &str)] = &[
    (MdoType::Catalog, "СправочникМенеджер", "CatalogManager"),
    (MdoType::Document, "ДокументМенеджер", "DocumentManager"),
    (MdoType::Enum, "ПеречислениеМенеджер", "EnumManager"),
    (MdoType::InformationRegister, "РегистрСведенийМенеджер", "InformationRegisterManager"),
    (MdoType::AccumulationRegister, "РегистрНакопленияМенеджер", "AccumulationRegisterManager"),
    (MdoType::AccountingRegister, "РегистрБухгалтерииМенеджер", "AccountingRegisterManager"),
    (MdoType::CalculationRegister, "РегистрРасчетаМенеджер", "CalculationRegisterManager"),
    (MdoType::ExchangePlan, "ПланОбменаМенеджер", "ExchangePlanManager"),
    (MdoType::ChartOfAccounts, "ПланСчетовМенеджер", "ChartOfAccountsManager"),
    (
        MdoType::ChartOfCharacteristicTypes,
        "ПланВидовХарактеристикМенеджер",
        "ChartOfCharacteristicTypesManager",
    ),
    (
        MdoType::ChartOfCalculationTypes,
        "ПланВидовРасчетаМенеджер",
        "ChartOfCalculationTypesManager",
    ),
    (MdoType::Task, "ЗадачаМенеджер", "TaskManager"),
    (MdoType::BusinessProcess, "БизнесПроцессМенеджер", "BusinessProcessManager"),
    (MdoType::DataProcessor, "ОбработкаМенеджер", "DataProcessorManager"),
    (MdoType::Report, "ОтчетМенеджер", "ReportManager"),
];

/// Generic object type name per metadata kind. SSoT behind both
/// `subtype::generic_object_names` and the lowering gate.
pub(crate) const OBJECT_NAMES: &[(MdoType, &str, &str)] = &[
    (MdoType::Catalog, "СправочникОбъект", "CatalogObject"),
    (MdoType::Document, "ДокументОбъект", "DocumentObject"),
    (MdoType::Task, "ЗадачаОбъект", "TaskObject"),
    (MdoType::BusinessProcess, "БизнесПроцессОбъект", "BusinessProcessObject"),
    (MdoType::ExchangePlan, "ПланОбменаОбъект", "ExchangePlanObject"),
    (MdoType::ChartOfAccounts, "ПланСчетовОбъект", "ChartOfAccountsObject"),
    (
        MdoType::ChartOfCharacteristicTypes,
        "ПланВидовХарактеристикОбъект",
        "ChartOfCharacteristicTypesObject",
    ),
    (MdoType::ChartOfCalculationTypes, "ПланВидовРасчетаОбъект", "ChartOfCalculationTypesObject"),
    (MdoType::DataProcessor, "ОбработкаОбъект", "DataProcessorObject"),
    (MdoType::Report, "ОтчетОбъект", "ReportObject"),
];

pub(crate) fn manager_name_for(mdo: MdoType) -> Option<(&'static str, &'static str)> {
    MANAGER_NAMES.iter().find(|(m, _, _)| *m == mdo).map(|(_, ru, en)| (*ru, *en))
}

pub(crate) fn object_name_for(mdo: MdoType) -> Option<(&'static str, &'static str)> {
    OBJECT_NAMES.iter().find(|(m, _, _)| *m == mdo).map(|(_, ru, en)| (*ru, *en))
}

/// Generic form-control name per element kind. SSoT behind
/// `subtype::generic_form_control_names` and the lowering gate.
pub(crate) fn form_control_name_for(kind: FormElementKind) -> Option<(&'static str, &'static str)> {
    use FormElementKind as K;
    Some(match kind {
        K::Table => ("ТаблицаФормы", "FormTable"),
        K::Field => ("ПолеФормы", "FormField"),
        K::Group | K::UsualGroup | K::Pages | K::Page | K::CommandBar | K::ButtonGroup => {
            ("ГруппаФормы", "FormGroup")
        }
        K::Button => ("КнопкаФормы", "FormButton"),
        K::Decoration => ("ДекорацияФормы", "FormDecoration"),
        K::Addition => ("ДополнениеЭлементаФормы", "FormItemAddition"),
        K::Other => return None,
    })
}

const FORM_CONTROL_KINDS: &[FormElementKind] = &[
    FormElementKind::Table,
    FormElementKind::Field,
    FormElementKind::Group,
    FormElementKind::Button,
    FormElementKind::Decoration,
    FormElementKind::Addition,
];

/// The four form-data container family names. SSoT behind the FormData arm of
/// `subtype::is_concrete_to_generic_platform_bridge` and the lowering gate.
pub(crate) const FORM_DATA_NAMES: &[(&str, &str)] = &[
    ("ДанныеФормыСтруктура", "FormDataStructure"),
    ("ДанныеФормыКоллекция", "FormDataCollection"),
    ("ДанныеФормыСтруктураСКоллекцией", "FormDataStructureAndCollection"),
    ("ДанныеФормыДерево", "FormDataTree"),
];

/// Tabular-section row, allow-listed separately because the row rewrite in method
/// lookup and the subtype bridge key on it while the corpus does not list it.
/// Both the spaced platform spelling and the compact doc-comment spelling are
/// recognised so the row bridge ([`crate::subtype`]) matches whichever a parameter
/// is documented with — keeping the compact form nominal in the lowering gate
/// would otherwise reintroduce a false mismatch the bridge cannot clear.
pub(crate) fn is_tabular_row_name(name: &str) -> bool {
    let lc = name.trim().fold_lower();
    lc == "строка табличной части"
        || lc == "line of a tabular section"
        || lc == "строкатабличнойчасти"
        || lc == "tabularsectionrow"
}

/// Names every concrete→generic bridge accepts as a target (manager, object,
/// form control, form-data container, tabular section/row). Reused by the lowering
/// gate so a parameter documented with one of them keeps its nominal phantom.
pub(crate) fn is_generic_bridge_target_name(name: &str) -> bool {
    let pair = |(ru, en): &(&str, &str)| name_eq_ci(name, ru, en);
    MANAGER_NAMES.iter().any(|(_, ru, en)| name_eq_ci(name, ru, en))
        || OBJECT_NAMES.iter().any(|(_, ru, en)| name_eq_ci(name, ru, en))
        || FORM_CONTROL_KINDS
            .iter()
            .filter_map(|k| form_control_name_for(*k))
            .any(|(ru, en)| name_eq_ci(name, ru, en))
        || FORM_DATA_NAMES.iter().any(pair)
        || name_eq_ci(name, "ТабличнаяЧасть", "TabularSection")
        || is_tabular_row_name(name)
}

/// Real platform types the generated corpus omits but the type system still
/// produces by other means — form controls/collections, managed-form family and
/// its extensions, external components, metadata-object descriptions, register
/// records / record sets / keys. A doc-typed parameter naming one of these must
/// keep a nominal `PlatformObject`, never degrade to Unknown.
pub(crate) fn is_known_non_corpus_type_name(name: &str) -> bool {
    if is_generic_bridge_target_name(name) {
        return true;
    }

    const SINGLETONS: &[(&str, &str)] = &[
        // Form control base + collections + the legacy "control".
        ("ЭлементФормы", "FormControl"),
        ("КоллекцияЭлементовФормы", "FormItemsCollection"),
        ("ЭлементУправления", "ManagedFormControl"),
        // Managed form and its version-older / application-scoped spellings.
        ("УправляемаяФорма", "ManagedForm"),
        ("ФормаКлиентскогоПриложения", "ClientApplicationForm"),
        ("ФормаУправляемогоПриложения", "ManagedApplicationForm"),
        // External component.
        ("ВнешняяКомпонента", "AddIn"),
        ("ОбъектВнешнейКомпоненты", "ExternalComponentObject"),
        // Bare register record set.
        ("НаборЗаписей", "RecordSet"),
        ("НаборЗаписейРегистра", "RegisterRecordSet"),
        // Constant value manager (the corpus enumerates the metadata-kind managers
        // but not the constant one).
        ("КонстантаМенеджерЗначения", "ConstantValueManager"),
        // Form attribute collection.
        ("РеквизитФормыКоллекция", "FormAttributeCollection"),
    ];
    if SINGLETONS.iter().any(|(ru, en)| name_eq_ci(name, ru, en)) {
        return true;
    }

    let lc = name.fold_lower();
    // Serialisation / DOM type families the corpus enumerates only partially: a
    // platform name glues the latin XDTO / XML / DOM tag onto a Cyrillic root
    // (ТипXDTO, ФабрикаXDTO, СтрокаXML, УзелDOM, …). Requiring a Cyrillic letter
    // immediately before the tag keeps those while rejecting pure-latin prose that
    // merely ends in the same letters (`Random`, `Freedom`); a Cyrillic-only typo
    // like `ОбъектХДТО` carries no latin tag and degrades to Unknown.
    if has_cyrillic_rooted_suffix(&lc) {
        return true;
    }
    // Metadata-object descriptions: ОбъектМетаданных, ОбъектМетаданныхКонфигурация,
    // ОбъектМетаданных<Вид> (ОбъектМетаданныхСправочник, …).
    if lc.starts_with("объектметаданных") || lc.starts_with("metadataobject") {
        return true;
    }
    // Managed-form extensions: РасширениеУправляемойФормыДля*,
    // Расширение(Поля|Таблицы|Группы…)Формы*,
    // РасширениеФормыКлиентскогоПриложенияДля*.
    if (lc.starts_with("расширение") && lc.contains("форм")) || lc.contains("formextension")
    {
        return true;
    }
    // Register records / record sets / keys: <register-kind><record-suffix> with no
    // trailing «Имя…» template placeholder (those are doc-comment scaffolding, not
    // types, and must degrade).
    is_register_record_family(name)
}

const REGISTER_PREFIXES: &[(&str, &str)] = &[
    ("РегистрСведений", "InformationRegister"),
    ("РегистрНакопления", "AccumulationRegister"),
    ("РегистрБухгалтерии", "AccountingRegister"),
    ("РегистрРасчета", "CalculationRegister"),
    ("Перерасчет", "Recalculation"),
    ("Последовательность", "Sequence"),
];

const RECORD_SUFFIXES: &[(&str, &str)] = &[
    ("НаборЗаписей", "RecordSet"),
    ("Запись", "Record"),
    ("КлючЗаписи", "RecordKey"),
    ("МенеджерЗаписи", "RecordManager"),
];

/// True when `lc` (already lower-cased) ends in a latin XDTO/XML/DOM tag that is
/// glued to a Cyrillic root — the shape of the platform serialisation/DOM types.
fn has_cyrillic_rooted_suffix(lc: &str) -> bool {
    ["xdto", "xml", "dom"].iter().any(|tag| {
        lc.strip_suffix(tag)
            .and_then(|stem| stem.chars().next_back())
            .is_some_and(|c| c.is_alphabetic() && !c.is_ascii())
    })
}

fn is_register_record_family(name: &str) -> bool {
    REGISTER_PREFIXES.iter().any(|(p_ru, p_en)| {
        RECORD_SUFFIXES.iter().any(|(s_ru, s_en)| {
            name_eq_ci(name, &format!("{p_ru}{s_ru}"), &format!("{p_en}{s_en}"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_and_object_tables_round_trip() {
        assert_eq!(
            manager_name_for(MdoType::Document),
            Some(("ДокументМенеджер", "DocumentManager"))
        );
        assert_eq!(object_name_for(MdoType::Catalog), Some(("СправочникОбъект", "CatalogObject")));
        assert!(manager_name_for(MdoType::Constant).is_none());
    }

    #[test]
    fn generic_bridge_targets_recognised_case_insensitively() {
        for n in [
            "ДокументМенеджер",
            "documentmanager",
            "СправочникОбъект",
            "ТаблицаФормы",
            "ДанныеФормыДерево",
            "ТабличнаяЧасть",
        ] {
            assert!(is_generic_bridge_target_name(n), "{n} must be a bridge target");
        }
    }

    #[test]
    fn real_uncorpused_families_recognised() {
        for n in [
            "ОбъектМетаданных",
            "ОбъектМетаданныхСправочник",
            "РасширениеУправляемойФормыДляОбъектов",
            "ВнешняяКомпонента",
            "ОбъектВнешнейКомпоненты",
            "УправляемаяФорма",
            "ЭлементФормы",
            "РегистрСведенийНаборЗаписей",
            "РегистрБухгалтерииЗапись",
            "РегистрНакопленияКлючЗаписи",
            "НаборЗаписей",
            "ТипXDTO",
            "СтрокаXML",
            "УзелDOM",
            "СтрокаТабличнойЧасти",
        ] {
            assert!(is_known_non_corpus_type_name(n), "{n} must be recognised as a real type");
        }
    }

    #[test]
    fn typos_prose_and_placeholders_are_not_recognised() {
        for n in [
            "СпискоЗначений",
            "документ",
            "Структра",
            "Соответсвие",
            "если",
            "истина",
            "СправочникОбъектИмяСправочника",
            "РегистрСведенийНаборЗаписейИмяРегистраСведений",
            "Контрагент",
            "Номенклатура",
            // Pure-latin prose that merely ends in a serialisation/DOM tag must
            // NOT be mistaken for an XDTO/XML/DOM type.
            "Random",
            "Freedom",
            "ОбъектХДТО",
        ] {
            assert!(
                !is_known_non_corpus_type_name(n),
                "{n} must NOT be recognised (degrades to Unknown)"
            );
        }
    }
}
