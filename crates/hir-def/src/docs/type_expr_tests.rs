use super::{parse_type_expr, DocTypeExpr};
use crate::docs::{parse_method_docs, TypeDoc};

fn parse_docs(lines: &[&str]) -> crate::docs::MethodDocs {
    let comments = lines.iter().map(|line| (*line).to_string()).collect::<Vec<_>>();
    parse_method_docs(&comments).expect("test documentation must parse")
}

#[test]
fn parses_russian_and_english_method_collection_see_returns() {
    let russian = parse_docs(&[
        "Параметры:",
        "  Данные - см. ОбщийМодуль.ДанныеФайла",
        "Возвращаемое значение:",
        "  Массив из см. РаботаСФайлами.ДанныеФайла",
    ]);
    let english = parse_docs(&[
        "Parameters:",
        "  Data - See CommonModule.FileData",
        "Returns:",
        "  Array of See FileService.FileData",
    ]);

    assert_eq!(
        russian.returned_value[0].description.as_deref(),
        Some("из см. РаботаСФайлами.ДанныеФайла")
    );
    assert_eq!(
        english.returned_value[0].description.as_deref(),
        Some("of See FileService.FileData")
    );
    assert!(matches!(parse_type_expr(&russian.parameters[0].types[0]), Some(DocTypeExpr::See(_))));
    assert!(matches!(
        parse_type_expr(&russian.returned_value[0]),
        Some(DocTypeExpr::Array(element)) if matches!(*element, DocTypeExpr::See(_))
    ));
    assert!(matches!(parse_type_expr(&english.parameters[0].types[0]), Some(DocTypeExpr::See(_))));
    assert!(matches!(
        parse_type_expr(&english.returned_value[0]),
        Some(DocTypeExpr::Array(element)) if matches!(*element, DocTypeExpr::See(_))
    ));
}

#[test]
fn parses_structure_with_direct_fields_only() {
    let docs = parse_docs(&[
        "Возвращаемое значение:",
        "  Структура:",
        "    * Имя - Строка - имя.",
        "    ** Вложенное - Число - не поддерживается.",
    ]);

    let Some(DocTypeExpr::Structure { fields }) = parse_type_expr(&docs.returned_value[0]) else {
        panic!("expected documented structure");
    };

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "Имя");
    assert!(matches!(fields[0].types.as_slice(), [DocTypeExpr::TypeRef(_)]));
}

#[test]
fn parses_full_name_collections_and_merged_descriptions() {
    let docs = parse_docs(&[
        "Параметры:",
        "  Русские - МаСсИв ИЗ Строка",
        "  English - aRrAy OF Number",
        "Возвращаемое значение:",
        "  МаСсИв ИЗ Структура - элементы результата:",
        "    * РусскоеПоле - Массив из Строка",
        "    * EnglishField - Array of Number",
    ]);

    assert!(matches!(
        parse_type_expr(&docs.parameters[0].types[0]),
        Some(DocTypeExpr::Array(element)) if matches!(*element, DocTypeExpr::TypeRef(_))
    ));
    assert!(matches!(
        parse_type_expr(&docs.parameters[1].types[0]),
        Some(DocTypeExpr::Array(element)) if matches!(*element, DocTypeExpr::TypeRef(_))
    ));

    let Some(DocTypeExpr::Array(element)) = parse_type_expr(&docs.returned_value[0]) else {
        panic!("expected documented array");
    };
    let DocTypeExpr::Structure { fields } = *element else {
        panic!("expected array element structure");
    };
    assert_eq!(
        docs.returned_value[0].description.as_deref(),
        Some("из Структура - элементы результата:")
    );
    assert_eq!(fields.len(), 2);
    assert!(fields.iter().all(|field| {
        matches!(field.types.as_slice(), [DocTypeExpr::Array(element)] if matches!(**element, DocTypeExpr::TypeRef(_)))
    }));
}

#[test]
fn rejects_prose_and_malformed_see_targets() {
    for invalid in [
        "См. 5",
        "See Service.Type trailing prose",
        "5 см.",
        "(см. Модуль.Тип)",
        "описание полей см. Модуль.Тип. Иначе текст",
    ] {
        assert_eq!(parse_type_expr(&TypeDoc::simple(invalid.to_string(), None)), None);
    }

    for type_name in ["СметнаяСтрока", "Смещение"] {
        assert!(matches!(
            parse_type_expr(&TypeDoc::simple(type_name.to_string(), None)),
            Some(DocTypeExpr::TypeRef(_))
        ));
    }
}

#[test]
fn structure_of_a_value_type_is_a_structure_with_its_documented_fields() {
    // A parameter keeps the `из <Тип>` tail inside the type name, and the tail names the values,
    // not the container: the slot is still the structure its bullets describe.
    let docs = parse_docs(&[
        "Параметры:",
        "  Данные - Структура из КлючИЗначение:",
        "   * Ключ - Строка - имя таблицы.",
        "Возвращаемое значение:",
        "  Structure of KeyAndValue - таблицы:",
        "   * Значение - Число - число строк.",
    ]);

    let parameter = parse_type_expr(&docs.parameters[0].types[0]);
    let Some(DocTypeExpr::Structure { fields }) = parameter else {
        panic!("parameter must lower to a structure, got {parameter:?}");
    };
    assert_eq!(fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Ключ"]);

    let returned = parse_type_expr(&docs.returned_value[0]);
    let Some(DocTypeExpr::Structure { fields }) = returned else {
        panic!("return must lower to a structure, got {returned:?}");
    };
    assert_eq!(fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Значение"]);
}

#[test]
fn a_collection_that_is_not_a_structure_keeps_its_own_lowering() {
    // The `из` tail alone must not turn every collection into a structure.
    assert!(matches!(
        parse_type_expr(&TypeDoc::simple("Массив из Строка".to_string(), None)),
        Some(DocTypeExpr::Array(_))
    ));
    assert!(parse_type_expr(&TypeDoc::simple("Соответствие из Строка".to_string(), None)).is_none());
}

/// The slot's alternatives, in the order the documentation declares them.
fn param_types(docs: &crate::docs::MethodDocs, name: &str) -> Vec<DocTypeExpr> {
    docs.parameters
        .iter()
        .find(|param| param.name == name)
        .expect("test documentation must declare the parameter")
        .types
        .iter()
        .filter_map(parse_type_expr)
        .collect()
}

fn names_target(docs: &crate::docs::MethodDocs, param: &str) -> bool {
    param_types(docs, param).iter().any(DocTypeExpr::names_documentation_target)
}

#[test]
fn a_reference_is_found_wherever_the_slot_puts_it() {
    // A reference is not only ever the whole slot: an array holds it as its element, and a
    // documented structure holds it in a field. A predicate reading only the root of the
    // expression answers `false` for both, and the slots that need resolving most go unnoticed.
    let docs = parse_docs(&[
        "Параметры:",
        "  Прямая - см. База.Создать",
        "  Параметр - см. База.Создать.Настройки",
        "  ВМассиве - Массив из см. База.Создать",
        "  ВПоле - Структура:",
        "   * Ключ - см. База.Создать - вложенная ссылка.",
    ]);

    assert!(names_target(&docs, "Прямая"), "две части — метод и его возврат");
    assert!(names_target(&docs, "Параметр"), "три части — параметр названного метода");
    assert!(names_target(&docs, "ВМассиве"), "ссылка под массивом");
    assert!(names_target(&docs, "ВПоле"), "ссылка в поле документированной структуры");
}

#[test]
fn prose_and_plain_types_name_no_target() {
    // `см. в`, `см. также` and their kin parse into a one-segment reference — there are 16 291 of
    // them in one configuration alone. Counting those as cross-references would claim a target for
    // slots that never had one. `Произвольный` matters for the opposite reason: it lowers to the
    // same permissive top type a reference does, so a predicate that looked at the lowered type
    // instead of the parse could not tell the two apart.
    let docs = parse_docs(&[
        "Параметры:",
        "  Проза - см. в описании",
        "  Ещё - см. также",
        "  Любой - Произвольный",
        "  Обычный - Строка",
        "  Своя - Структура:",
        "   * Ключ - Строка - без ссылок.",
    ]);

    for param in ["Проза", "Ещё", "Любой", "Обычный", "Своя"] {
        assert!(!names_target(&docs, param), "{param} не называет цель");
    }
}
