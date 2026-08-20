//! Инвентарь видов лексем: каждый вид порождается входом и каждый значимый
//! вид читается правилом.
//!
//! Обе проверки живут на ОДНОЙ таблице свидетелей. Две копии таблицы в разных
//! крейтах разошлись бы молча, а канал SDBL наблюдаем только отсюда:
//! `sdbl_token_converter` объявлен приватным, и снаружи крейта его не видно.
//!
//! Provenance: `docs/legal/bsl-clean-room-slice-b1.md`.

use crate::syntax_kind::token_kind_to_syntax;
use lexer::TokenKind;
use syntax::SyntaxKind;

/// Канал, которым вид попадает в дерево.
///
/// `TokenKind` — общий алфавит двух лексеров. Вид, который BSL-лексер не
/// порождает, не обязан быть мёртвым: он может приходить преобразованием
/// лексем SDBL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    /// Вид порождает BSL-лексер на `input`.
    Bsl,
    /// Вид приходит преобразованием лексем SDBL; BSL-лексер его не порождает.
    Sdbl,
}

struct Witness {
    kind: TokenKind,
    channel: Channel,
    input: &'static str,
    /// Текст BSL, на котором вид канала SDBL мог бы появиться, если бы
    /// BSL-лексер его порождал.
    ///
    /// Без такого зонда проверка непорождаемости зелена вхолостую: знаков
    /// `#`, `&`, `{`, `}`, `|` нет ни в одном законном входе таблицы, и
    /// перебор по ним не встретил бы нарушения ни при какой реализации.
    bsl_probe: Option<&'static str>,
}

const fn bsl(kind: TokenKind, input: &'static str) -> Witness {
    Witness { kind, channel: Channel::Bsl, input, bsl_probe: None }
}

const fn sdbl(kind: TokenKind, input: &'static str, bsl_probe: &'static str) -> Witness {
    Witness { kind, channel: Channel::Sdbl, input, bsl_probe: Some(bsl_probe) }
}

/// Свидетель на каждый вид, в порядке объявления перечисления.
///
/// Входы — законные конструкции языка, а не голые обрывки: тот же вход
/// отвечает и на вопрос «порождается ли вид», и на вопрос «читает ли его
/// правило», а на обрывке второй вопрос смысла не имеет.
const WITNESSES: &[Witness] = &[
    bsl(TokenKind::KwProcedure, "Процедура П() КонецПроцедуры"),
    bsl(TokenKind::KwEndProcedure, "Процедура П() КонецПроцедуры"),
    bsl(TokenKind::KwFunction, "Функция Ф() Возврат 1; КонецФункции"),
    bsl(TokenKind::KwEndFunction, "Функция Ф() Возврат 1; КонецФункции"),
    bsl(TokenKind::KwExport, "Процедура П() Экспорт КонецПроцедуры"),
    bsl(TokenKind::KwVal, "Процедура П(Знач А) КонецПроцедуры"),
    bsl(TokenKind::KwIf, "Процедура П() Если А Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwThen, "Процедура П() Если А Тогда КонецЕсли; КонецПроцедуры"),
    bsl(
        TokenKind::KwElsIf,
        "Процедура П() Если А Тогда ИначеЕсли Б Тогда КонецЕсли; КонецПроцедуры",
    ),
    bsl(TokenKind::KwElse, "Процедура П() Если А Тогда Иначе КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwEndIf, "Процедура П() Если А Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwFor, "Процедура П() Для С = 1 По 3 Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwEach, "Процедура П() Для Каждого Э Из К Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwIn, "Процедура П() Для Каждого Э Из К Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwTo, "Процедура П() Для С = 1 По 3 Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwWhile, "Процедура П() Пока А Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwDo, "Процедура П() Пока А Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwEndDo, "Процедура П() Пока А Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwReturn, "Функция Ф() Возврат 1; КонецФункции"),
    bsl(TokenKind::KwContinue, "Процедура П() Пока А Цикл Продолжить; КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwBreak, "Процедура П() Пока А Цикл Прервать; КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwGoto, "Процедура П() Перейти ~М; ~М: Возврат; КонецПроцедуры"),
    bsl(TokenKind::KwTry, "Процедура П() Попытка Исключение КонецПопытки; КонецПроцедуры"),
    bsl(TokenKind::KwExcept, "Процедура П() Попытка Исключение КонецПопытки; КонецПроцедуры"),
    bsl(TokenKind::KwEndTry, "Процедура П() Попытка Исключение КонецПопытки; КонецПроцедуры"),
    bsl(
        TokenKind::KwRaise,
        "Процедура П() Попытка Исключение ВызватьИсключение; КонецПопытки; КонецПроцедуры",
    ),
    bsl(TokenKind::KwVar, "Перем А;"),
    bsl(TokenKind::KwNew, "Процедура П() А = Новый Массив; КонецПроцедуры"),
    bsl(TokenKind::KwExecute, "Процедура П() Выполнить(\"А\"); КонецПроцедуры"),
    bsl(TokenKind::KwAddHandler, "Процедура П() ДобавитьОбработчик О.С, Обр; КонецПроцедуры"),
    bsl(TokenKind::KwRemoveHandler, "Процедура П() УдалитьОбработчик О.С, Обр; КонецПроцедуры"),
    bsl(TokenKind::KwAsync, "Асинх Функция Ф() Возврат Ждать Г(); КонецФункции"),
    bsl(TokenKind::KwAwait, "Асинх Функция Ф() Возврат Ждать Г(); КонецФункции"),
    bsl(TokenKind::KwAnd, "Процедура П() Если А И Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwOr, "Процедура П() Если А Или Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwNot, "Процедура П() Если Не А Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwTrue, "Процедура П() А = Истина; КонецПроцедуры"),
    bsl(TokenKind::KwFalse, "Процедура П() А = Ложь; КонецПроцедуры"),
    bsl(TokenKind::KwUndefined, "Процедура П() А = Неопределено; КонецПроцедуры"),
    bsl(TokenKind::KwNull, "Процедура П() А = NULL; КонецПроцедуры"),
    bsl(TokenKind::PreIf, "#Если Клиент Тогда\n#КонецЕсли"),
    bsl(TokenKind::PreElsIf, "#Если Клиент Тогда\n#ИначеЕсли Сервер Тогда\n#КонецЕсли"),
    bsl(TokenKind::PreElse, "#Если Клиент Тогда\n#Иначе\n#КонецЕсли"),
    bsl(TokenKind::PreEndIf, "#Если Клиент Тогда\n#КонецЕсли"),
    bsl(TokenKind::PreRegion, "#Область О\n#КонецОбласти"),
    bsl(TokenKind::PreEndRegion, "#Область О\n#КонецОбласти"),
    bsl(TokenKind::PreInsert, "#Вставка\nПроцедура П() КонецПроцедуры\n#КонецВставки"),
    bsl(TokenKind::PreEndInsert, "#Вставка\nПроцедура П() КонецПроцедуры\n#КонецВставки"),
    bsl(TokenKind::PreDelete, "#Удаление\nПроцедура П() КонецПроцедуры\n#КонецУдаления"),
    bsl(TokenKind::PreEndDelete, "#Удаление\nПроцедура П() КонецПроцедуры\n#КонецУдаления"),
    bsl(TokenKind::AnnAtClient, "&НаКлиенте\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::AnnAtServer, "&НаСервере\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::AnnAtServerNoContext, "&НаСервереБезКонтекста\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::AnnAtClientAtServerNoContext, "&НаКлиентеНаСервереБезКонтекста\nПеремА;"),
    bsl(TokenKind::AnnAtClientAtServer, "&НаКлиентеНаСервере\nПерем А;"),
    bsl(TokenKind::AnnBefore, "&Перед(\"М\")\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::AnnAfter, "&После(\"М\")\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::AnnAround, "&Вместо(\"М\")\nПроцедура П() КонецПроцедуры"),
    bsl(
        TokenKind::AnnChangeAndValidate,
        "&ИзменениеИКонтроль(\"М\")\nПроцедура П() КонецПроцедуры",
    ),
    bsl(TokenKind::AnnCustom, "&МояАннотация\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::Eq, "Процедура П() А = 1; КонецПроцедуры"),
    bsl(TokenKind::Neq, "Процедура П() Если А <> Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::Le, "Процедура П() Если А <= Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::Lt, "Процедура П() Если А < Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::Ge, "Процедура П() Если А >= Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::Gt, "Процедура П() Если А > Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::Plus, "Процедура П() А = Б + В; КонецПроцедуры"),
    bsl(TokenKind::Minus, "Процедура П() А = Б - В; КонецПроцедуры"),
    bsl(TokenKind::Star, "Процедура П() А = Б * В; КонецПроцедуры"),
    bsl(TokenKind::Slash, "Процедура П() А = Б / В; КонецПроцедуры"),
    bsl(TokenKind::Percent, "Процедура П() А = Б % В; КонецПроцедуры"),
    bsl(TokenKind::LParen, "Процедура П() Ф(А, Б); КонецПроцедуры"),
    bsl(TokenKind::RParen, "Процедура П() Ф(А, Б); КонецПроцедуры"),
    sdbl(TokenKind::LBrace, "ВЫБРАТЬ {Т.П} ИЗ Т", "Процедура П() А = {; КонецПроцедуры"),
    sdbl(TokenKind::RBrace, "ВЫБРАТЬ {Т.П} ИЗ Т", "Процедура П() А = }; КонецПроцедуры"),
    bsl(TokenKind::LBracket, "Процедура П() А = К[0]; КонецПроцедуры"),
    bsl(TokenKind::RBracket, "Процедура П() А = К[0]; КонецПроцедуры"),
    bsl(TokenKind::Dot, "Процедура П() А = О.С; КонецПроцедуры"),
    bsl(TokenKind::Comma, "Процедура П() Ф(А, Б); КонецПроцедуры"),
    bsl(TokenKind::Semicolon, "Процедура П() А = 1; КонецПроцедуры"),
    bsl(TokenKind::Colon, "Процедура П() Перейти ~М; ~М: Возврат; КонецПроцедуры"),
    bsl(TokenKind::Question, "Процедура П() А = ?(Б, 1, 2); КонецПроцедуры"),
    bsl(TokenKind::Tilde, "Процедура П() Перейти ~М; ~М: Возврат; КонецПроцедуры"),
    sdbl(TokenKind::Bar, "ВЫБРАТЬ |", "Процедура П() А = |; КонецПроцедуры"),
    sdbl(TokenKind::Hash, "ВЫБРАТЬ #", "#Неизвестная"),
    sdbl(TokenKind::Ampersand, "ВЫБРАТЬ * ИЗ Т ГДЕ Т.П = &Пар", "&1"),
    bsl(TokenKind::Float, "Процедура П() А = 1.5; КонецПроцедуры"),
    bsl(TokenKind::Decimal, "Процедура П() А = 1; КонецПроцедуры"),
    bsl(TokenKind::String, "Процедура П() А = \"с\"; КонецПроцедуры"),
    bsl(TokenKind::StringStart, "Процедура П() А = \"п\n|т\"; КонецПроцедуры"),
    bsl(TokenKind::StringTail, "Процедура П() А = \"п\n|т\"; КонецПроцедуры"),
    bsl(TokenKind::StringPart, "Процедура П() А = \"п\n|р\n|т\"; КонецПроцедуры"),
    bsl(TokenKind::Date, "Процедура П() А = '20240101'; КонецПроцедуры"),
    bsl(TokenKind::Ident, "Процедура П() А = Б; КонецПроцедуры"),
    bsl(TokenKind::Comment, "// комментарий\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::Newline, "Процедура П()\nКонецПроцедуры"),
    bsl(TokenKind::Whitespace, "Процедура П() КонецПроцедуры"),
    bsl(TokenKind::Bom, "\u{FEFF}Процедура П() КонецПроцедуры"),
    bsl(TokenKind::Error, "Процедура П() А = 1;\u{2003} КонецПроцедуры"),
];

fn witness_of(kind: TokenKind) -> &'static Witness {
    WITNESSES
        .iter()
        .find(|w| w.kind == kind)
        .unwrap_or_else(|| panic!("{kind:?}: вида нет в таблице свидетелей"))
}

/// Таблица покрывает перечисление целиком и ровно по разу.
///
/// Без этого J1 и J2 молча сужаются до тех видов, о которых кто-то вспомнил:
/// свойство, проверенное на выборке, зелено и у инвентаря, разошедшегося на
/// не вошедшем в выборку виде.
#[test]
fn the_witness_table_covers_every_kind_exactly_once() {
    for kind in TokenKind::ALL {
        let hits = WITNESSES.iter().filter(|w| w.kind == *kind).count();
        assert_eq!(hits, 1, "{kind:?}: строк в таблице свидетелей {hits}, а должна быть одна");
    }
    assert_eq!(
        WITNESSES.len(),
        TokenKind::ALL.len(),
        "в таблице свидетелей есть строки, которым не отвечает ни один вид"
    );
}

/// J1 — каждый вид порождается хотя бы одним входом.
///
/// `TokenKind` — общий алфавит двух каналов, поэтому вход берётся из того
/// канала, который питает вид. Канал SDBL проверяется через публичный
/// `parse_sdbl`: это единственная проверка, что преобразование лексем SDBL
/// живо, и она падает, если конвертер или SDBL-лексер потеряют вид.
#[test]
fn every_kind_is_produced_by_some_input() {
    let mut unreachable = Vec::new();
    for kind in TokenKind::ALL {
        let w = witness_of(*kind);
        let produced = match w.channel {
            Channel::Bsl => lexer::tokenize(w.input).iter().any(|t| t.kind == *kind),
            Channel::Sdbl => {
                let want = token_kind_to_syntax(*kind);
                crate::parse_sdbl(w.input)
                    .syntax_node()
                    .descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|t| t.kind() == want)
            }
        };
        if !produced {
            unreachable.push(format!("{kind:?} ({:?}) на {:?}", w.channel, w.input));
        }
    }
    assert!(
        unreachable.is_empty(),
        "виды, которых не даёт ни один вход:\n  {}",
        unreachable.join("\n  ")
    );
}

/// Вид, объявленный каналом SDBL, BSL-лексер не порождает.
///
/// Это утверждение и есть решение «перестать порождать», записанное так,
/// чтобы оно могло упасть: вернули образец в BSL-лексер — тест красен.
/// Проверяется на именном зонде каждой такой строки И на всех входах канала
/// BSL, потому что зонд отвечает за нарушение в лоб, а входы — за то, что вид
/// не просочился в обычный код.
#[test]
fn a_kind_owned_by_the_sdbl_channel_is_never_lexed_from_bsl() {
    let sdbl_only: Vec<TokenKind> =
        WITNESSES.iter().filter(|w| w.channel == Channel::Sdbl).map(|w| w.kind).collect();

    let bsl_texts: Vec<&'static str> = WITNESSES
        .iter()
        .filter(|w| w.channel == Channel::Bsl)
        .map(|w| w.input)
        .chain(WITNESSES.iter().filter_map(|w| w.bsl_probe))
        .collect();

    let mut breaches = Vec::new();
    for text in bsl_texts {
        for token in lexer::tokenize(text) {
            if sdbl_only.contains(&token.kind) {
                breaches.push(format!("{:?} на {text:?}", token.kind));
            }
        }
    }

    assert!(
        breaches.is_empty(),
        "BSL-лексер порождает виды канала SDBL:\n  {}",
        breaches.join("\n  ")
    );
}

/// J2 — значимый вид читается правилом, а не просто лежит в дереве.
///
/// Форма «родитель токена не `ERROR`» выбрана потому, что две очевидные
/// формулировки дают гейт, который не может упасть. Покрытие текста сходится
/// всегда: `Sink::finish` сметает остаток входа в дерево. Наличие родителя
/// тоже: `Parser::emit_error` бампает нечитаемый токен в узел `Error`.
///
/// Исключения названы по построению, а не по удобству:
/// - `TokenKind::Error` несёт текст, который не может назвать ни одно
///   правило, и `ERROR` — его единственный законный родитель;
/// - тривия по `TokenKind::is_trivia` привязывается `Sink`, а не правилом;
///   `T![…]` у тривиального вида ветви не имеет, то есть правило физически
///   не может её потребовать.
///
/// Область — виды канала BSL. Потребление видов канала SDBL держит слайс 12
/// SDBL, и эта граница здесь не двигается.
#[test]
fn every_significant_kind_is_consumed_by_a_rule() {
    let mut unread = Vec::new();
    for kind in TokenKind::ALL {
        if *kind == TokenKind::Error || kind.is_trivia() {
            continue;
        }
        let w = witness_of(*kind);
        if w.channel != Channel::Bsl {
            continue;
        }

        let want = token_kind_to_syntax(*kind);
        let parse = crate::parse(w.input);
        let consumed = parse
            .syntax_node()
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == want)
            .any(|t| t.parent().is_some_and(|p| p.kind() != SyntaxKind::ERROR));

        if !consumed {
            unread.push(format!("{kind:?} на {:?}", w.input));
        }
    }
    assert!(
        unread.is_empty(),
        "виды, попадающие в дерево только под узлом ошибки, то есть не читаемые правилом:\n  {}",
        unread.join("\n  ")
    );
}
