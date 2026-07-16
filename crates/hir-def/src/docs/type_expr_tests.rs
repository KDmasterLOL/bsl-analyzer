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
