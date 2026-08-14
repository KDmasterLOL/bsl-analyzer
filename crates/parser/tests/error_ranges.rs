//! Норма привязки диапазонов ошибок разбора: куда указывает диагностика.
//!
//! Норма объявлена в `docs/architecture/adr/ADR-03-error-range-attribution.md`
//! и держится устройством `Sink`. Здесь она проверяется снаружи, на публичном
//! API, потому что именно эти диапазоны уходят пользователю: `parse().errors()`
//! попадает в LSP-диагностику напрямую, а для запросов — ещё и проекцией в
//! текст литерала.
//!
//! **Что этот файл ловит, а что нет — установлено мутантами, а не предположено.**
//!
//! Ловит соответствие «вид восстановления → форма диапазона» из таблицы ADR:
//! реализация, выдавшая на `MissingToken` непустой диапазон, даёт 28 нарушений
//! на этом корпусе.
//!
//! НЕ ловит выбор смещения среди границ значимых токенов. Три мутанта —
//! смещение у лексемы под курсором без нормализации, `BumpToken` на предыдущей
//! лексеме потока вместо последнего слова, нормализация назад вместо вперёд —
//! проходят корпус зелёными. Причина не в слабости утверждения, а в том, что
//! грамматика сама снимает тривию раньше, чем требует токен: на 6 000
//! обрезанных реальных файлов, давших 12 718 ошибок, курсор не стоял на тривии
//! ни разу. Входа, на котором разница видна, в корпусе нет и быть не может.
//!
//! Поэтому направление держат два синтетических контроля в модуле тестов
//! `crates/parser/src/sink.rs`
//! (`a_missing_token_error_after_buffered_trivia_points_at_the_next_word` и
//! `a_recovery_span_starting_on_trivia_begins_at_the_next_word`): им нужен
//! событийный вектор, а `Event` и `Sink` крейт наружу не отдаёт. Оба были
//! увидены падающими на реализации без нормализации — `2..2` вместо `5..5` и
//! `2..5` вместо `3..5`.

use std::collections::HashSet;

const MODULE: &str = include_str!("fixtures/Module.bsl");
const QUERY: &str = include_str!("fixtures/user_query_with_highlighting_issue.sdbl");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Bsl,
    Sdbl,
}

/// Диапазоны значимых токенов: их начала, концы и полные пары.
///
/// Начала и концы держатся раздельно намеренно. Множество «любая граница»
/// проверять бесполезно: прогон тривии ВСЕГДА начинается там, где кончился
/// предыдущий значимый токен, поэтому смещение, уехавшее в промежуток, такому
/// свойству удовлетворяет и нарушение остаётся невидимым. Норма ADR-03
/// направленная, и проверяться должна направленно: о пропущенном токене
/// сообщают на НАЧАЛЕ следующего слова.
struct Significant {
    starts: HashSet<u32>,
    ranges: HashSet<(u32, u32)>,
    len: u32,
}

impl Significant {
    fn of(text: &str, lang: Lang) -> Self {
        let tokens: Vec<(usize, usize)> = match lang {
            Lang::Bsl => lexer::tokenize(text)
                .into_iter()
                .filter(|t| !is_trivia(t.kind))
                .map(|t| (t.offset, t.text.len()))
                .collect(),
            Lang::Sdbl => lexer::sdbl::tokenize_sdbl(text)
                .into_iter()
                .filter(|t| !is_sdbl_trivia(t.kind))
                .map(|t| (t.offset, t.text.len()))
                .collect(),
        };

        let mut starts = HashSet::new();
        let mut ranges = HashSet::new();
        for (offset, len) in tokens {
            starts.insert(offset as u32);
            ranges.insert((offset as u32, offset.saturating_add(len) as u32));
        }
        Self { starts, ranges, len: text.len() as u32 }
    }

    /// Начало слова либо конец входа — то, на что указывает сообщение о том,
    /// чего в тексте нет.
    fn is_word_start(&self, offset: u32) -> bool {
        offset == self.len || self.starts.contains(&offset)
    }
}

fn is_trivia(kind: lexer::TokenKind) -> bool {
    matches!(
        kind,
        lexer::TokenKind::Whitespace
            | lexer::TokenKind::Comment
            | lexer::TokenKind::Newline
            | lexer::TokenKind::Bom
    )
}

fn is_sdbl_trivia(kind: lexer::sdbl::SdblTokenKind) -> bool {
    matches!(
        kind,
        lexer::sdbl::SdblTokenKind::Whitespace
            | lexer::sdbl::SdblTokenKind::Comment
            | lexer::sdbl::SdblTokenKind::Newline
    )
}

fn errors_of(text: &str, lang: Lang) -> Vec<(parser_error::RecoveryKind, u32, u32)> {
    let parse = match lang {
        Lang::Bsl => parser::parse(text),
        Lang::Sdbl => parser::parse_sdbl(text),
    };
    parse
        .errors()
        .iter()
        .map(|e| {
            (e.structured().recovery(), u32::from(e.range().start()), u32::from(e.range().end()))
        })
        .collect()
}

/// Обрезка входа по границе символа: даёт много ошибок разбора из фикстуры,
/// не заводя в репозиторий отдельного корпуса битых файлов.
fn cut(text: &str, num: usize, den: usize) -> &str {
    let mut at = text.len() * num / den;
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    &text[..at]
}

const BROKEN_BSL: &[&str] = &[
    "Процедура П()\n\tА = Б.\nКонецПроцедуры\n",
    "Процедура П(\nКонецПроцедуры\n",
    "Процедура П()\n\tФ( , );\nКонецПроцедуры\n",
    "Процедура П()\n\tА = Б + ;\nКонецПроцедуры\n",
    "Процедура П()\n\tЕсли А \n\t\tБ = 1;\n\tКонецЕсли;\nКонецПроцедуры\n",
    "Процедура П()\n\tА = Новый ;\nКонецПроцедуры\n",
    "#Вставка\n\tА = 1;\n",
    "Процедура Тест(   ",
    "Перем   ;\n",
    "&НаКлиенте   \n",
];

const BROKEN_SDBL: &[&str] = &[
    "ВЫБРАТЬ Т.\nИЗ ИЗ Т",
    "ВЫБРАТЬ   ИЗ Справочник.Товары",
    "ВЫБРАТЬ ЕСТЬNULL( Т.А ,  \nИЗ Т",
    "ВЫБРАТЬ * ИЗ Т {ВЫБРАТЬ \n",
    "ВЫБРАТЬ * ИЗ ( ВЫБРАТЬ \n) КАК Т",
    "ВЫБРАТЬ 1 УПОРЯДОЧИТЬ ПО   \n",
    "ВЫБРАТЬ * ИЗ Т ГДЕ Т.А В ( 1 , \n",
    "ВЫБРАТЬ ВЫБОР КОГДА 1 ТОГДА \n КОНЕЦ ИЗ Т",
    "УНИЧТОЖИТЬ   \n",
    "SELECT A; ) T.\nFROM Products",
];

#[test]
fn an_error_points_where_the_norm_says() {
    let mut corpus: Vec<(String, String, Lang)> = Vec::new();

    for (num, den) in [(1, 3), (1, 2), (2, 3), (9, 10), (1, 1)] {
        corpus.push((format!("module {num}/{den}"), cut(MODULE, num, den).to_string(), Lang::Bsl));
        corpus.push((format!("query {num}/{den}"), cut(QUERY, num, den).to_string(), Lang::Sdbl));
    }
    for (i, text) in BROKEN_BSL.iter().enumerate() {
        corpus.push((format!("bsl #{i}"), (*text).to_string(), Lang::Bsl));
    }
    for (i, text) in BROKEN_SDBL.iter().enumerate() {
        corpus.push((format!("sdbl #{i}"), (*text).to_string(), Lang::Sdbl));
    }

    let mut breaches = Vec::new();
    let mut seen = Vec::new();

    for (name, text, lang) in &corpus {
        let significant = Significant::of(text, *lang);

        for (recovery, start, end) in errors_of(text, *lang) {
            seen.push(recovery);
            let ok = match recovery {
                // Пропущенного токена в тексте нет, поэтому диапазон пуст, а
                // указывает он на начало слова за промежутком.
                parser_error::RecoveryKind::MissingToken => {
                    start == end && significant.is_word_start(start)
                }
                // Спан идёт от слова до слова: обе границы — начала.
                parser_error::RecoveryKind::RecoverySpan => {
                    significant.is_word_start(start) && significant.is_word_start(end)
                }
                // Ошибка потрачена на слово и показывается на нём целиком.
                parser_error::RecoveryKind::BumpToken | parser_error::RecoveryKind::Custom => {
                    significant.ranges.contains(&(start, end))
                        || (start == end && significant.is_word_start(start))
                }
            };

            if !ok {
                breaches.push(format!("{name}: {recovery:?} {start}..{end} не по норме"));
            }
        }
    }

    // Покрытие меряется видами восстановления, а не числом ошибок: у каждого
    // своя ветвь расчёта диапазона, и корпус с одним видом оставляет две
    // другие непроверенными.
    //
    // Видов четыре, требуются три: `Custom` грамматика не производит ни на
    // каком входе — его ставят потребители, строящие `ParseError` сами
    // (`hir-def` при проекции ошибок запроса, `syntax` в запасных ветках).
    // Ту ветвь держит синтетический тест в модуле тестов стока, и другого
    // способа до неё добраться нет.
    for required in [
        parser_error::RecoveryKind::MissingToken,
        parser_error::RecoveryKind::BumpToken,
        parser_error::RecoveryKind::RecoverySpan,
    ] {
        assert!(
            seen.contains(&required),
            "корпус не дал ни одной ошибки вида {required:?}: её ветвь расчёта диапазона не проверена"
        );
    }

    assert!(
        breaches.is_empty(),
        "нарушений нормы: {}\n{}",
        breaches.len(),
        breaches.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}
