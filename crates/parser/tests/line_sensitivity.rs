//! Норма строчной чувствительности: какие конструкции могут пересечь перевод
//! строки, и на каком основании. Объявлена в
//! `docs/architecture/adr/ADR-02-line-sensitivity.md`.
//!
//! Основное свойство метаморфное: дерево значимых токенов после замены
//! перевода строки пробелом совпадает с исходным всюду, кроме объявленных
//! строчно-чувствительных конструкций. Правило, прочитавшее перевод строки
//! мимо предиката, свойство ломает.
//!
//! Свойство не может перейти из красного в зелёное: объявленные места ведут
//! себя одинаково и до сведения нормы к одному предикату, и после. Поэтому оно
//! сторожит будущее шестое место, и у каждого утверждения здесь есть вход, на
//! котором нарушение ВИДНО:
//!
//! - равенство проекций без сверки числа замен зелено у преобразования,
//!   которое не заменило ничего, — поэтому рядом с каждым входом стоит
//!   объявленное число замен, и оно сверяется точно, а не «больше нуля»;
//! - правила пригодности самой замены сторожит отдельный вход, на котором
//!   снятие любого из них меняет текст.

use lexer::sdbl::{tokenize_sdbl, SdblTokenKind};
use lexer::{tokenize, TokenKind};
use parser::{parse, parse_sdbl, Parser};
use syntax::{NodeOrToken, SyntaxKind, SyntaxNode, TextRange};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Bsl,
    Sdbl,
}

fn is_trivia(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Whitespace | TokenKind::Comment | TokenKind::Newline | TokenKind::Bom)
}

// ===================================================================
// Преобразование: перевод строки → пробел
// ===================================================================

/// Смещения байтов `\n`, пригодных к замене пробелом.
///
/// Преобразование идёт по потоку токенов, а не по тексту: слепая замена всех
/// `\n` меняет сам разбор, а не только его вход. Непригодны две позиции.
///
/// **Сразу за комментарием.** Комментарий лексируется как `//[^\n]*`, поэтому
/// после замены он поглотил бы остаток строки, и сравнивались бы два разных
/// текста, а не два разбора одного. Слепым пятном это не является: у позиции
/// сразу за комментарием ответ предиката постоянен, и различает она только
/// конец файла.
///
/// **Внутри открытого строкового прогона.** `"Первая\n|Вторая"` приходит как
/// `StringStart · Newline · StringTail`; после замены весь текст матчится одним
/// токеном `String`, а `|` становится обычным содержимым. Прогон закрывает и
/// `StringTail`, и обычный `String` — так их читает и грамматика, а автомат,
/// закрывающий прогон только по `StringTail`, остался бы открытым до конца
/// файла после входа вида `А = "первая\n"вторая";` и молча погасил бы все
/// последующие замены, оставаясь зелёным. На `StringPart` прогон не
/// открывается: вне литерала `|` в начале строки лексится так же и открыл бы
/// призрачный прогон.
///
/// Незакрытый литерал держит прогон открытым до конца файла — это осознанно
/// консервативно: часть замен не делается, ложного падения не возникает.
fn replaceable_newline_offsets(text: &str, lang: Lang) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut inside_string_run = false;
    let mut previous_was_comment = false;

    match lang {
        Lang::Bsl => {
            for token in tokenize(text) {
                match token.kind {
                    TokenKind::StringStart => inside_string_run = true,
                    TokenKind::StringTail | TokenKind::String => inside_string_run = false,
                    TokenKind::Newline if !inside_string_run && !previous_was_comment => {
                        offsets.push(token.offset)
                    }
                    _ => {}
                }
                previous_was_comment = token.kind == TokenKind::Comment;
            }
        }
        // В SDBL переводы строки внутри строкового прогона токенами не выходят
        // вовсе — их проглатывает сканер строк, — поэтому правило прогона
        // здесь срабатывает само собой.
        Lang::Sdbl => {
            for token in tokenize_sdbl(text) {
                if token.kind == SdblTokenKind::Newline && !previous_was_comment {
                    offsets.push(token.offset);
                }
                previous_was_comment = token.kind == SdblTokenKind::Comment;
            }
        }
    }

    offsets
}

/// Текст с заменёнными переводами строки и число замен.
///
/// Число возвращается не для отчёта: без него все утверждения о равенстве
/// проекций проходят вхолостую — преобразование, заменившее ноль байтов,
/// превращает сравнение в `X == X`.
fn replace_line_breaks(text: &str, lang: Lang) -> (String, usize) {
    let offsets = replaceable_newline_offsets(text, lang);
    let mut bytes = text.as_bytes().to_vec();
    for &offset in &offsets {
        assert_eq!(bytes[offset], b'\n', "смещение {offset} не указывает на перевод строки");
        bytes[offset] = b' ';
    }
    let replaced = String::from_utf8(bytes).expect("замена однобайтового `\\n` пробелом");
    (replaced, offsets.len())
}

// ===================================================================
// Проекция разбора
// ===================================================================

/// Запись проекции. Тривия в проекцию не входит — она и есть то, что
/// преобразование двигает.
#[derive(Debug, PartialEq, Eq)]
enum Piece {
    Enter(SyntaxKind),
    Leave,
    Token { kind: SyntaxKind, range: TextRange, text: String },
}

#[derive(Debug, PartialEq, Eq)]
struct Projection {
    pieces: Vec<Piece>,
    errors: Vec<(TextRange, String)>,
}

fn collect_pieces(node: &SyntaxNode, out: &mut Vec<Piece>) {
    out.push(Piece::Enter(node.kind()));
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(child) => collect_pieces(&child, out),
            NodeOrToken::Token(token) => {
                if !token.kind().is_trivia() {
                    out.push(Piece::Token {
                        kind: token.kind(),
                        range: token.text_range(),
                        text: token.text().to_string(),
                    });
                }
            }
        }
    }
    out.push(Piece::Leave);
}

/// Вложенность узлов, значимые токены с видом, диапазоном и текстом, и ошибки
/// разбора.
///
/// Диапазоны включены сознательно: замена сохраняет длину текста, поэтому у
/// неизменившегося разбора они обязаны совпасть байт в байт. Диапазоны самих
/// узлов не нужны — вложенность и диапазоны токенов их уже задают, а узлы
/// пропущенных элементов пусты и своего диапазона не имеют.
fn projection(text: &str, lang: Lang) -> Projection {
    let parse = match lang {
        Lang::Bsl => parse(text),
        Lang::Sdbl => parse_sdbl(text),
    };

    let mut pieces = Vec::new();
    collect_pieces(&parse.syntax_node(), &mut pieces);

    let errors =
        parse.errors().iter().map(|e| (e.range(), e.message().to_string())).collect::<Vec<_>>();

    Projection { pieces, errors }
}

// ===================================================================
// Корпус
// ===================================================================

/// Вход гейта: текст, язык и объявленное число пригодных к замене переводов
/// строки.
struct Input {
    name: &'static str,
    lang: Lang,
    text: String,
    replacements: usize,
}

impl Input {
    fn bsl(name: &'static str, text: &str, replacements: usize) -> Self {
        Self { name, lang: Lang::Bsl, text: text.to_string(), replacements }
    }

    fn sdbl(name: &'static str, text: &str, replacements: usize) -> Self {
        Self { name, lang: Lang::Sdbl, text: text.to_string(), replacements }
    }
}

const MODULE_FIXTURE: &str = include_str!("fixtures/Module.bsl");
const QUERY_FIXTURE: &str = include_str!("fixtures/user_query_with_highlighting_issue.sdbl");

/// Числа сняты замером при авторстве и меняются только вместе с фикстурой.
/// Они и есть положительный контроль корпуса: преобразование, тихо переставшее
/// заменять, ломает их немедленно.
const MODULE_FIXTURE_REPLACEMENTS: usize = 12_077;
const QUERY_FIXTURE_REPLACEMENTS: usize = 131;

/// Места, объявленные строчно-чувствительными. Замена перевода строки здесь
/// обязана менять разбор.
fn line_sensitive_inputs() -> Vec<Input> {
    vec![
        Input::bsl("имя области со следующей строки", "#Область\nИмя\n", 2),
        Input::bsl(
            "имя свойства за переводом строки — не любое ключевое слово",
            "Функция Ф()\n\tВозврат А.\nКонецФункции\n",
            3,
        ),
        Input::bsl(
            "висячая точка перед новым объявлением",
            "Процедура П()\n\tА = Б.\n\tПроцедура В()\n",
            3,
        ),
        Input::sdbl("точка колонки", "ВЫБРАТЬ Т.\nИЗ ИЗ Т", 1),
        Input::sdbl("точка за вызовом", "ВЫБРАТЬ ПРЕДСТАВЛЕНИЕ(Т.А).\nИЗ ИЗ Т", 1),
        Input::sdbl("точка в неразобранном остатке", "SELECT A; ) T.\nFROM Products", 1),
    ]
}

/// Места, объявленные нечувствительными, у которых перевод строки вообще
/// пригоден к замене.
///
/// Инструкция препроцессора здесь не по недосмотру: платформа разрыв
/// отвергает, но разбирается он одинаково, и жалоба принадлежит слою
/// диагностик, а не грамматике. Входы стоят тут, чтобы это решение держалось
/// наблюдением: правило, начавшее ветвиться по переводу строки внутри
/// инструкции, сломает их.
fn line_insensitive_inputs() -> Vec<Input> {
    vec![
        Input::bsl("условие #Если со следующей строки", "#Если\nСервер Тогда\n#КонецЕсли\n", 3),
        Input::bsl(
            "операнд условия со следующей строки",
            "#Если Сервер\nИ Клиент Тогда\n#КонецЕсли\n",
            3,
        ),
        Input::bsl("Тогда со следующей строки", "#Если Сервер\nТогда\n#КонецЕсли\n", 3),
        Input::bsl(
            "условие #ИначеЕсли со следующей строки",
            "#Если Сервер Тогда\n#ИначеЕсли\nКлиент Тогда\n#КонецЕсли\n",
            4,
        ),
        Input::bsl(
            "склейка соседних литералов",
            "Процедура П()\n\tА = \"а\"\n\t\"б\";\nКонецПроцедуры\n",
            4,
        ),
        Input::bsl(
            "привязка аннотации к объявлению",
            "&НаКлиенте\nПроцедура П()\nКонецПроцедуры\n",
            3,
        ),
        Input::bsl(
            "выражение, разорванное после операции",
            "Процедура П()\n\tА = Б +\n\t\tВ;\nКонецПроцедуры\n",
            4,
        ),
    ]
}

// ===================================================================
// Предикат
// ===================================================================

/// Значение предиката не зависит от того, звало ли правило `skip_trivia`.
///
/// Предикат, смотрящий только вперёд, после пропуска тривии всегда ложен;
/// смотрящий только назад — ложен до пропуска. Разойтись эти две формы могут
/// тихо, и правило получит разный ответ на один и тот же текст в зависимости
/// от того, в каком порядке оно написано.
///
/// Второе утверждение — покрытие: без него свойство зелено у реализации,
/// которая всегда возвращает `false`.
#[test]
fn the_predicate_reads_the_same_before_and_after_skipping_trivia() {
    let mut saw_true = false;
    let mut saw_false = false;

    let corpus = [MODULE_FIXTURE.to_string()]
        .into_iter()
        .chain(line_sensitive_inputs().into_iter().filter(|i| i.lang == Lang::Bsl).map(|i| i.text))
        .chain(line_insensitive_inputs().into_iter().map(|i| i.text))
        .collect::<Vec<_>>();

    for text in &corpus {
        let tokens = tokenize(text);
        let mut p = Parser::new(&tokens);

        while let Some(kind) = p.current() {
            if !is_trivia(kind) {
                let value = p.a_line_break_precedes();
                saw_true |= value;
                saw_false |= !value;
                p.bump();
                continue;
            }

            let at_run_start = p.a_line_break_precedes();
            while p.current().is_some_and(is_trivia) {
                assert_eq!(
                    p.a_line_break_precedes(),
                    at_run_start,
                    "предикат разошёлся внутри прогона тривии"
                );
                p.bump();
            }
            assert_eq!(
                p.a_line_break_precedes(),
                at_run_start,
                "предикат разошёлся до и после пропуска тривии"
            );

            saw_true |= at_run_start;
            saw_false |= !at_run_start;
        }
    }

    assert!(saw_true, "на корпусе не встретилось ни одного пересечения строки");
    assert!(saw_false, "на корпусе не встретилось ни одной позиции без пересечения строки");
}

/// Комментарий в промежутке — это пересечение строки, и основание тут
/// лексическое, а не грамматическое: комментарий тянется до конца своей
/// строки. Под позицией стоит `Comment`, а не `Newline`, поэтому правило,
/// смотрящее на вид токена, здесь ответило бы «нет».
#[test]
fn a_comment_in_the_gap_counts_as_a_line_break() {
    let tokens = tokenize("Т.// c\nИЗ");
    let mut p = Parser::new(&tokens);

    while !p.at(TokenKind::Dot) {
        p.bump();
    }
    p.bump();

    assert_eq!(p.current(), Some(TokenKind::Comment), "проба стоит не на комментарии");
    assert!(p.a_line_break_precedes(), "комментарий в промежутке не признан пересечением строки");
}

// ===================================================================
// Метаморфное свойство
// ===================================================================

/// Замена перевода строки пробелом не меняет разбор вне объявленных мест.
#[test]
fn replacing_a_line_break_does_not_change_the_parse() {
    let mut corpus = vec![
        Input::bsl("фикстура модуля", MODULE_FIXTURE, MODULE_FIXTURE_REPLACEMENTS),
        Input::sdbl("фикстура запроса", QUERY_FIXTURE, QUERY_FIXTURE_REPLACEMENTS),
    ];
    corpus.extend(line_insensitive_inputs());

    for input in &corpus {
        let (replaced, count) = replace_line_breaks(&input.text, input.lang);

        assert_eq!(
            count, input.replacements,
            "«{}»: замен {count}, объявлено {}",
            input.name, input.replacements
        );
        assert_eq!(
            projection(&input.text, input.lang),
            projection(&replaced, input.lang),
            "«{}»: замена перевода строки изменила разбор",
            input.name
        );
    }
}

/// У каждой строки таблицы со значением «да» есть вход, на котором
/// чувствительность ВИДНА. Строка, для которой такого входа нет, — это
/// строка, которую никто не проверял.
#[test]
fn the_declared_line_sensitive_places_do_change() {
    for input in line_sensitive_inputs() {
        let (replaced, count) = replace_line_breaks(&input.text, input.lang);

        assert_eq!(
            count, input.replacements,
            "«{}»: замен {count}, объявлено {}",
            input.name, input.replacements
        );
        assert_ne!(
            projection(&input.text, input.lang),
            projection(&replaced, input.lang),
            "«{}»: объявлено чувствительным, но разбор не изменился",
            input.name
        );
    }
}

/// Объявленные нечувствительными места остаются нечувствительными — при том,
/// что замена у них состоялась.
#[test]
fn the_declared_insensitive_places_do_not_change() {
    for input in line_insensitive_inputs() {
        let (replaced, count) = replace_line_breaks(&input.text, input.lang);

        assert_eq!(
            count, input.replacements,
            "«{}»: замен {count}, объявлено {}",
            input.name, input.replacements
        );
        assert_ne!(replaced, input.text, "«{}»: текст не изменился", input.name);
        assert_eq!(
            projection(&input.text, input.lang),
            projection(&replaced, input.lang),
            "«{}»: объявлено нечувствительным, но разбор изменился",
            input.name
        );
    }
}

/// Гейт самих правил пригодности, а не разбора: у каждого правила есть вход,
/// на котором его отсутствие видно. Сними исключение строкового прогона —
/// литерал после замены слипается; сними исключение перевода строки за
/// комментарием — комментарий поглощает следующий оператор. В обоих случаях
/// число замен перестаёт быть нулём, а текст — совпадать с исходным.
#[test]
fn the_transformation_rules_are_observable() {
    let cases = [
        ("перевод строки внутри многострочного литерала", "А = \"начало\n|конец\";"),
        ("перевод строки сразу за комментарием", "А = 1; // c\nБ = 2;"),
    ];

    for (name, text) in cases {
        let (replaced, count) = replace_line_breaks(text, Lang::Bsl);

        assert_eq!(count, 0, "«{name}»: перевод строки признан пригодным к замене");
        assert_eq!(replaced, text, "«{name}»: текст изменился");
    }
}

// ===================================================================
// Тривия вне алфавита грамматики
// ===================================================================
//
// Проверок три, по одной на функцию, а не одна на всех: в грамматике нет ни
// одного вызова с тривиальным видом, поэтому забытая или перепутанная проверка
// не всплыла бы ни в одном прогоне.
//
// У `expect` и `expect_no_bump` перед вызовом объявляется граница: при истинной
// границе обе возвращаются, не дойдя до `eat`, а значит и до `at`, — и
// сработать может только их собственная проверка. Без этой подготовки тест был
// бы зелен из-за паники в `at` и своей проверки не наблюдал бы вовсе.

#[test]
#[should_panic(expected = "о переводе строки")]
#[cfg(debug_assertions)]
fn at_refuses_a_trivia_kind() {
    let tokens = tokenize("А = 1;\n");
    let p = Parser::new(&tokens);

    let _ = p.at(TokenKind::Newline);
}

#[test]
#[should_panic(expected = "о переводе строки")]
#[cfg(debug_assertions)]
fn expect_refuses_a_trivia_kind() {
    let tokens = tokenize("А = 1;\n");
    let mut p = Parser::new(&tokens);
    p.set_grammar_boundary(|_| true);

    p.expect(TokenKind::Newline);
}

#[test]
#[should_panic(expected = "о переводе строки")]
#[cfg(debug_assertions)]
fn expect_no_bump_refuses_a_trivia_kind() {
    let tokens = tokenize("А = 1;\n");
    let mut p = Parser::new(&tokens);
    p.set_grammar_boundary(|_| true);

    p.expect_no_bump(TokenKind::Newline);
}

/// Перевод строки внутри многострочного литерала принадлежит литералу.
///
/// Отдельной проверкой, а не метаморфной: этот перевод строки по построению
/// непригоден к замене, и метаморфное сравнение на нём выродилось бы в
/// `X == X`.
#[test]
fn a_line_break_inside_a_literal_belongs_to_the_literal() {
    let text = "Процедура П()\n\tА = \"начало\n\t|конец\";\nКонецПроцедуры\n";
    let tree = parse(text).syntax_node();

    let literal = tree
        .descendants()
        .find(|node| node.kind() == SyntaxKind::LITERAL)
        .expect("узел литерала не найден");

    let kinds = literal
        .children_with_tokens()
        .filter_map(|child| child.into_token().map(|token| token.kind()))
        .collect::<Vec<_>>();

    assert!(
        kinds.contains(&SyntaxKind::STRING_START) && kinds.contains(&SyntaxKind::STRING_TAIL),
        "литерал собран не из частей: {kinds:?}"
    );
    assert!(kinds.contains(&SyntaxKind::NEWLINE), "перевод строки вышел за пределы литерала");
}
