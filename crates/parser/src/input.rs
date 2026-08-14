//! Вход парсера: поток лексем, из которого видны только значимые токены.
//!
//! Тривия — пробелы, переводы строки, комментарии, BOM — не отбрасывается: она
//! остаётся в `tokens` и уходит в дерево через сток. Здесь она перестаёт быть
//! адресуемой: у грамматики нет позиции, которая на неё указывает, и нет
//! способа спросить про неё видом токена.
//!
//! Про переводы строки грамматике отвечает единственный предикат —
//! [`Input::a_line_break_precedes`], — и он считает по карте, снятой один раз
//! при разборе входа. Основание нормы объявлено в
//! `docs/architecture/adr/ADR-02-line-sensitivity.md`.

use lexer::{Token, TokenKind};

/// Значимые токены входа и промежутки между ними.
pub(crate) struct Input<'t> {
    tokens: &'t [Token],
    /// Индекс каждого значимого токена в сыром потоке.
    raw: Vec<u32>,
    /// Виды значимых токенов, в том же порядке.
    kinds: Vec<TokenKind>,
    /// Бит `i` — «в промежутке перед значимым токеном `i` есть перевод
    /// строки». Длина карты `n + 1`: последний бит описывает промежуток за
    /// последним значимым токеном, и он не декоративный — на нём отвечает
    /// предикат, когда значимые токены кончились.
    line_break_before: Vec<u64>,
}

impl<'t> Input<'t> {
    pub(crate) fn new(tokens: &'t [Token]) -> Self {
        let mut raw = Vec::new();
        let mut kinds = Vec::new();
        let mut line_break_before = Vec::new();

        let mut gap_has_line_break = false;
        for (index, token) in tokens.iter().enumerate() {
            if is_trivia(token.kind) {
                // Комментарий идёт до конца своей строки, поэтому за ним стоит
                // либо перевод строки, либо конец файла — считать его иначе
                // значило бы разрешить конструкции продолжаться через
                // закомментированный хвост.
                gap_has_line_break |= matches!(token.kind, TokenKind::Newline | TokenKind::Comment);
                continue;
            }

            set_bit(&mut line_break_before, kinds.len(), gap_has_line_break);
            gap_has_line_break = false;
            raw.push(index as u32);
            kinds.push(token.kind);
        }
        set_bit(&mut line_break_before, kinds.len(), gap_has_line_break);

        Self { tokens, raw, kinds, line_break_before }
    }

    /// Сколько значимых токенов во входе.
    pub(crate) fn len(&self) -> usize {
        self.kinds.len()
    }

    pub(crate) fn kind(&self, pos: usize) -> Option<TokenKind> {
        self.kinds.get(pos).copied()
    }

    pub(crate) fn text(&self, pos: usize) -> &str {
        self.token(pos).map_or("", |token| token.text.as_str())
    }

    pub(crate) fn token(&self, pos: usize) -> Option<&'t Token> {
        self.raw.get(pos).and_then(|&index| self.tokens.get(index as usize))
    }

    /// Вид лексемы по СЫРОМУ индексу.
    ///
    /// Единственная дверь к промежуткам, и открыта она не грамматике: ею
    /// пользуется передача сырого потока в сток.
    pub(crate) fn raw_kind(&self, raw: usize) -> TokenKind {
        self.tokens[raw].kind
    }

    /// Индекс в СЫРОМ потоке — тот, которым позиции обмениваются со стоком.
    ///
    /// Позиция за последним значимым токеном штатна: ошибка о пропущенном
    /// токене на конце входа ставит маркер именно там. Для неё отдаётся длина
    /// потока — сток такой индекс уже понимает и отвечает на него концом
    /// текста.
    pub(crate) fn raw_at(&self, pos: usize) -> usize {
        self.raw.get(pos).map_or(self.tokens.len(), |&index| index as usize)
    }

    /// Есть ли вообще промежуток перед значимым токеном `pos`.
    ///
    /// Отдельный вопрос от перевода строки: он про то, соприкасаются ли две
    /// лексемы. Спрашивают его там, где лексер режет одно слово языка на
    /// несколько токенов вплотную, и склеить обратно надо ровно их.
    ///
    /// Считается по индексам, а не по карте: значимые токены идут в сыром
    /// потоке подряд тогда и только тогда, когда между ними ничего нет.
    pub(crate) fn a_gap_precedes(&self, pos: usize) -> bool {
        let ends_here = match pos.checked_sub(1) {
            Some(prev) => match self.raw.get(prev) {
                Some(&raw) => raw as usize + 1,
                None => return false,
            },
            None => 0,
        };
        self.raw_at(pos) != ends_here
    }

    /// Стоит ли перевод строки в промежутке перед значимым токеном `pos`.
    ///
    /// Промежуток перед самым первым значимым токеном — пролог файла: пары
    /// токенов, между которыми стоять, там нет, и ответ ложен.
    pub(crate) fn a_line_break_precedes(&self, pos: usize) -> bool {
        if pos == 0 {
            return false;
        }
        let pos = pos.min(self.kinds.len());
        self.line_break_before
            .get(pos / u64::BITS as usize)
            .is_some_and(|word| word & (1 << (pos % u64::BITS as usize)) != 0)
    }
}

fn set_bit(map: &mut Vec<u64>, index: usize, value: bool) {
    let word = index / u64::BITS as usize;
    if map.len() <= word {
        map.resize(word + 1, 0);
    }
    if value {
        map[word] |= 1 << (index % u64::BITS as usize);
    }
}

/// Единственное место в парсере, где вид токена сверяется с тривией.
pub(crate) fn is_trivia(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Whitespace | TokenKind::Comment | TokenKind::Newline | TokenKind::Bom)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Прежний обратный скан по сырому потоку — эталон, с которым сверяется
    /// карта.
    ///
    /// Он остаётся здесь дословно, а не переписывается через `Input`: карта и
    /// эталон обязаны быть двумя независимыми способами получить один ответ,
    /// иначе сверка проверяет саму себя.
    fn scan_line_break_before(tokens: &[Token], raw_pos: usize) -> bool {
        let anchor = tokens[raw_pos..]
            .iter()
            .position(|t| !is_trivia(t.kind))
            .map_or(tokens.len(), |offset| raw_pos + offset);

        let mut saw_line_break = false;
        for token in tokens[..anchor].iter().rev() {
            if !is_trivia(token.kind) {
                return saw_line_break;
            }
            if matches!(token.kind, TokenKind::Newline | TokenKind::Comment) {
                saw_line_break = true;
            }
        }
        false
    }

    fn agrees_with_the_scan(source: &str) {
        let tokens = lexer::tokenize(source);
        let input = Input::new(&tokens);

        for pos in 0..=input.len() {
            assert_eq!(
                input.a_line_break_precedes(pos),
                scan_line_break_before(&tokens, input.raw_at(pos)),
                "позиция {pos} источника {source:?}: карта разошлась со сканом"
            );
        }
    }

    /// Входы подобраны так, чтобы каждое правило карты было ВИДНО хотя бы на
    /// одном из них:
    ///
    /// - `"А // хвост"` — комментарий без перевода строки за ним: без него
    ///   сборка, не считающая комментарий переводом строки, зелена, потому что
    ///   в остальных входах за комментарием стоит `Newline`;
    /// - `"\nА"` и `"// шапка\nА"` — перевод строки ПЕРЕД первым словом:
    ///   пролога пары токенов не имеет, и без такого входа ответ на нулевой
    ///   позиции ничем не проверяется;
    /// - `"А\n"` — промежуток за последним словом.
    #[test]
    fn the_map_agrees_with_the_backward_scan() {
        for source in [
            "",
            "   ",
            "\n",
            "А",
            "А Б",
            "А\nБ",
            "А\n",
            "\nА",
            "// шапка\nА",
            "А // хвост\nБ",
            "А // хвост",
            "\u{feff}А\nБ",
            "Процедура П()\n\tА = 1;\nКонецПроцедуры\n",
            "Процедура П() А = 1; КонецПроцедуры",
            include_str!("../tests/fixtures/Module.bsl"),
        ] {
            agrees_with_the_scan(source);
        }
    }

    #[test]
    fn no_position_of_the_input_carries_trivia() {
        let source = include_str!("../tests/fixtures/Module.bsl");
        let tokens = lexer::tokenize(source);
        let input = Input::new(&tokens);

        assert!(input.len() > 0, "фикстура обязана дать значимые токены");
        for pos in 0..input.len() {
            let kind = input.kind(pos).expect("позиция внутри длины обязана иметь вид");
            assert!(!is_trivia(kind), "позиция {pos} отдала тривию {kind:?}");
        }
    }

    /// Карта длиной `n + 1`: у промежутка за последним значимым токеном есть
    /// своё значение, и потерять его молча нельзя.
    #[test]
    fn the_gap_after_the_last_word_has_a_value() {
        let tokens = lexer::tokenize("А\n");
        let input = Input::new(&tokens);

        assert_eq!(input.len(), 1);
        assert!(input.a_line_break_precedes(1), "перевод строки за последним словом потерян");
    }

    /// Позиция за концом входа штатна, и отвечать на неё обязан не паникой.
    #[test]
    fn a_position_past_the_end_answers_with_the_end_of_the_stream() {
        let tokens = lexer::tokenize("А Б");
        let input = Input::new(&tokens);

        assert_eq!(input.raw_at(input.len()), tokens.len());
        assert_eq!(input.kind(input.len()), None);
        assert_eq!(input.text(input.len()), "");
    }

    /// Вход без единого значимого токена: карта состоит из одного значения, и
    /// оно описывает весь вход целиком.
    #[test]
    fn an_input_of_pure_trivia_has_one_gap() {
        let tokens = lexer::tokenize("  \n  ");
        let input = Input::new(&tokens);

        assert_eq!(input.len(), 0);
        assert_eq!(input.raw_at(0), tokens.len());
        assert!(!input.a_line_break_precedes(0), "пролог файла пары токенов не имеет");
    }
}
