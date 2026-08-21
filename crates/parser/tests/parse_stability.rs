//! Разбор устойчив: тот же вход даёт то же дерево.
//!
//! Снятие ветки, принимавшей вложенную аннотацию значением параметра, изменило
//! классификацию одного класса входов. Правка поведения обязана предъявить не
//! только то, ЧТО изменилось, но и то, что не изменилось ничего сверх этого:
//! классификацию корпуса закрывает замер в аттестации, устойчивость самого
//! разбора — эта проверка.
//!
//! Provenance: `docs/legal/bsl-clean-room-slice-b3.md`.

/// Входы подобраны так, чтобы среди них были задетые правкой и соседние с ней:
/// проверка на одних лишь безошибочных входах зелена при любом восстановлении.
const INPUTS: &[&str] = &[
    "&Перед(&НаКлиенте)\nПроцедура Т() КонецПроцедуры",
    "&Перед(\"Тест\")\nПроцедура Т() КонецПроцедуры",
    "&НаКлиенте\n&Перед(\"Т\")\nПроцедура Т() КонецПроцедуры",
    "Процедура П(Знач А = 1, Б)\n  Х = ?(А > 0, -Б.В[0], Новый Массив(2));\nКонецПроцедуры",
    "#Если (Сервер Или Клиент) И Не ВебКлиент Тогда\nПерем А, Б Экспорт;\n#КонецЕсли",
    "Функция Ф()\n  Попытка\n    Выполнить Х;\n  Исключение\n    ВызватьИсключение;\n  КонецПопытки;\nКонецФункции",
    "Х = \"первая\"\n  \"вторая\";",
    "Перейти ~М;\n~М: Х = 1;",
];

#[test]
fn parsing_the_same_input_twice_gives_the_same_tree() {
    let mut breaches = Vec::new();

    for input in INPUTS {
        let first = format!("{:#?}", parser::parse(input).syntax_node());
        let second = format!("{:#?}", parser::parse(input).syntax_node());
        if first != second {
            breaches.push(format!("дерево разошлось между прогонами: {input:?}"));
        }

        let errors_first = parser::parse(input).errors().len();
        let errors_second = parser::parse(input).errors().len();
        if errors_first != errors_second {
            breaches.push(format!(
                "число ошибок разошлось между прогонами ({errors_first} против {errors_second}): {input:?}"
            ));
        }
    }

    assert!(!INPUTS.is_empty(), "список входов пуст — проверка была бы зелена вхолостую");
    assert!(breaches.is_empty(), "разбор неустойчив:\n  {}", breaches.join("\n  "));
}

/// Общий кеш разбора даёт то же дерево, что и разбор без него.
///
/// Это второй способ прочитать «независимость от порядка обхода»: путь через
/// общий кеш видит вход после других входов, а не в одиночестве.
#[test]
fn the_shared_cache_does_not_change_the_tree() {
    let mut breaches = Vec::new();

    for input in INPUTS {
        let plain = format!("{:#?}", parser::parse(input).syntax_node());
        let shared = format!("{:#?}", parser::parse_with_shared_cache(input).syntax_node());
        if plain != shared {
            breaches.push(format!("дерево зависит от кеша: {input:?}"));
        }
    }

    assert!(breaches.is_empty(), "разбор зависит от порядка:\n  {}", breaches.join("\n  "));
}
