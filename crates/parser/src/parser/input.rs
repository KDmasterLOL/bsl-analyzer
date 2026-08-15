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

/// Вид значимого токена — алфавит грамматики.
///
/// Обёртка над [`TokenKind`] с приватным полем и без конструктора наружу.
/// Записи выходят ассоциированными константами, по одной на значимый вид, и
/// у тривиального вида такой записи не существует: не «запрещена», а нет.
/// Поэтому `p.at(T![Newline])` не проходит проверку типов, а не оказывается
/// тихо ложным, как было при сыром виде.
///
/// Конструктора наружу нет намеренно, и это установлено пробой, а не выбрано.
/// Охраняемый конструктор годился бы, только будь он невидим грамматике, —
/// но макрос `T![…]` разворачивается в её коде, значит конструктор ей виден,
/// а видимый конструктор зовётся и мимо макроса. Охранник внутри него ловит
/// лишь константное вычисление: в `const` это отказ сборки, вне `const` тот
/// же вызов собирается и падает только на прогоне.
///
/// # Гейт И1: тривиального вида в алфавите нет
///
/// Ветви `T![…]` для тривиального вида не существует, и это отказ сборки,
/// а не тихая ложь, какой было `p.current() == Some(TokenKind::Newline)`.
///
/// ```compile_fail
/// let _ = parser::T![Newline];
/// ```
///
/// ```compile_fail
/// let _ = parser::T![Whitespace];
/// ```
///
/// ```compile_fail
/// let _ = parser::T![Comment];
/// ```
///
/// ```compile_fail
/// let _ = parser::T![Bom];
/// ```
///
/// Положительный контроль: у значимого вида ветвь есть и даёт рабочую
/// запись. Без него все четыре отказа выше объяснялись бы и ненайденным
/// макросом, и опечаткой в пути.
///
/// ```
/// assert_eq!(parser::T![Comma], parser::T![Comma]);
/// assert_ne!(parser::T![Comma], parser::T![Semicolon]);
/// ```
///
/// # Гейт И5: ни одна дверь не работает с сырым видом
///
/// По одной паре на КАЖДУЮ дверь, без выборки. Вид взят ЗНАЧИМЫЙ
/// намеренно: с тривиальным отказ неразличим — он означал бы и запрет
/// тривии, и смену типа. На `Comma` объяснение остаётся одно — дверь
/// больше не принимает сырой вид, — а контроль отделяет это от
/// «дверь сломана вовсе».
///
/// `at`:
///
/// ```compile_fail
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// p.at(lexer::TokenKind::Comma);
/// ```
///
/// ```
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// assert!(!p.at(parser::T![Comma]));
/// ```
///
/// `eat`:
///
/// ```compile_fail
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// p.eat(lexer::TokenKind::Comma);
/// ```
///
/// ```
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// assert!(!p.eat(parser::T![Comma]));
/// ```
///
/// `expect`:
///
/// ```compile_fail
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// p.expect(lexer::TokenKind::Comma);
/// ```
///
/// ```
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// p.expect(parser::T![Comma]);
/// ```
///
/// `expect_no_bump`:
///
/// ```compile_fail
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// p.expect_no_bump(lexer::TokenKind::Comma);
/// ```
///
/// ```
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// p.expect_no_bump(parser::T![Comma]);
/// ```
///
/// `TokenSet::new`:
///
/// ```compile_fail
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// parser::token_set::TokenSet::new(&[lexer::TokenKind::Comma]);
/// ```
///
/// ```
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// parser::token_set::TokenSet::new(&[parser::T![Comma]]);
/// ```
///
/// `TokenSet::contains`:
///
/// ```compile_fail
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// parser::token_set::TokenSet::empty().contains(lexer::TokenKind::Comma);
/// ```
///
/// ```
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// assert!(!parser::token_set::TokenSet::empty().contains(parser::T![Comma]));
/// ```
///
/// `current`:
///
/// ```compile_fail
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// let _ = p.current() == Some(lexer::TokenKind::Comma);
/// ```
///
/// ```
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// let _ = p.current() == Some(parser::T![Comma]);
/// ```
///
/// `nth`:
///
/// ```compile_fail
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// let _ = p.nth(1) == Some(lexer::TokenKind::Comma);
/// ```
///
/// ```
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// let _ = p.nth(1) == Some(parser::T![Comma]);
/// ```
///
/// `prev_significant`:
///
/// ```compile_fail
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// let _ = p.prev_significant() == Some(lexer::TokenKind::Comma);
/// ```
///
/// ```
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// let _ = p.prev_significant() == Some(parser::T![Comma]);
/// ```
///
/// `error_expected`:
///
/// ```compile_fail
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// p.error_expected(lexer::TokenKind::Comma);
/// ```
///
/// ```
/// let tokens = lexer::tokenize("А = 1;");
/// let mut p = parser::Parser::new(&tokens);
/// p.error_expected(parser::T![Comma]);
/// ```
///
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sig(TokenKind);

impl Sig {
    /// Виден только этому модулю: снаружи `Sig` строится исключительно
    /// готовой константой.
    const fn new(kind: TokenKind) -> Self {
        Sig(kind)
    }

    /// Вид под записью — для стока, сообщений об ошибках и разряда бита в
    /// наборе. Грамматике невидим: она сосед `parser` в дереве модулей и
    /// получает здесь E0624.
    pub(in crate::parser) const fn kind(self) -> TokenKind {
        self.0
    }

    pub fn is_keyword(self) -> bool {
        self.0.is_keyword()
    }
}

impl std::fmt::Debug for Sig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

/// Порождает записи алфавита и ветви `T![…]` из ОДНОГО списка.
///
/// Из одного — потому что иначе `Sig::ALL` отстаёт от набора констант, и
/// перебор по нему становится вхолостую зелёным: он перечисляет ровно то,
/// что сам же и породил.
macro_rules! define_the_alphabet {
    ($($variant:ident => $konst:ident),* $(,)?) => {
        impl Sig {
            $(pub const $konst: Sig = Sig::new(TokenKind::$variant);)*

            /// Все записи алфавита, в порядке объявления вида.
            pub const ALL: &'static [Sig] = &[$(Sig::$konst),*];
        }

        /// Запись алфавита грамматики по имени вида: `T![Comma]`, `T![KwIf]`.
        ///
        /// У тривиального вида ветви нет, поэтому `T![Newline]` — отказ
        /// сборки. Ветви порождаются тем же списком, что и константы.
        #[macro_export]
        macro_rules! T {
            $(($variant) => { $crate::Sig::$konst };)*
        }
    };
}

define_the_alphabet! {
    KwProcedure => KW_PROCEDURE,
    KwEndProcedure => KW_END_PROCEDURE,
    KwFunction => KW_FUNCTION,
    KwEndFunction => KW_END_FUNCTION,
    KwExport => KW_EXPORT,
    KwVal => KW_VAL,
    KwIf => KW_IF,
    KwThen => KW_THEN,
    KwElsIf => KW_ELS_IF,
    KwElse => KW_ELSE,
    KwEndIf => KW_END_IF,
    KwFor => KW_FOR,
    KwEach => KW_EACH,
    KwIn => KW_IN,
    KwTo => KW_TO,
    KwWhile => KW_WHILE,
    KwDo => KW_DO,
    KwEndDo => KW_END_DO,
    KwReturn => KW_RETURN,
    KwContinue => KW_CONTINUE,
    KwBreak => KW_BREAK,
    KwGoto => KW_GOTO,
    KwTry => KW_TRY,
    KwExcept => KW_EXCEPT,
    KwEndTry => KW_END_TRY,
    KwRaise => KW_RAISE,
    KwVar => KW_VAR,
    KwNew => KW_NEW,
    KwExecute => KW_EXECUTE,
    KwAddHandler => KW_ADD_HANDLER,
    KwRemoveHandler => KW_REMOVE_HANDLER,
    KwAsync => KW_ASYNC,
    KwAwait => KW_AWAIT,
    KwAnd => KW_AND,
    KwOr => KW_OR,
    KwNot => KW_NOT,
    KwTrue => KW_TRUE,
    KwFalse => KW_FALSE,
    KwUndefined => KW_UNDEFINED,
    KwNull => KW_NULL,
    PreIf => PRE_IF,
    PreElsIf => PRE_ELS_IF,
    PreElse => PRE_ELSE,
    PreEndIf => PRE_END_IF,
    PreRegion => PRE_REGION,
    PreEndRegion => PRE_END_REGION,
    PreUse => PRE_USE,
    PreInsert => PRE_INSERT,
    PreEndInsert => PRE_END_INSERT,
    PreDelete => PRE_DELETE,
    PreEndDelete => PRE_END_DELETE,
    AnnAtClient => ANN_AT_CLIENT,
    AnnAtServer => ANN_AT_SERVER,
    AnnAtServerNoContext => ANN_AT_SERVER_NO_CONTEXT,
    AnnAtClientAtServerNoContext => ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT,
    AnnAtClientAtServer => ANN_AT_CLIENT_AT_SERVER,
    AnnBefore => ANN_BEFORE,
    AnnAfter => ANN_AFTER,
    AnnAround => ANN_AROUND,
    AnnChangeAndValidate => ANN_CHANGE_AND_VALIDATE,
    AnnCustom => ANN_CUSTOM,
    Eq => EQ,
    Neq => NEQ,
    Le => LE,
    Lt => LT,
    Ge => GE,
    Gt => GT,
    Plus => PLUS,
    Minus => MINUS,
    Star => STAR,
    Slash => SLASH,
    Percent => PERCENT,
    LParen => L_PAREN,
    RParen => R_PAREN,
    LBrace => L_BRACE,
    RBrace => R_BRACE,
    LBracket => L_BRACKET,
    RBracket => R_BRACKET,
    Dot => DOT,
    Comma => COMMA,
    Semicolon => SEMICOLON,
    Colon => COLON,
    Question => QUESTION,
    Tilde => TILDE,
    Bar => BAR,
    Hash => HASH,
    Ampersand => AMPERSAND,
    Exclamation => EXCLAMATION,
    Float => FLOAT,
    Decimal => DECIMAL,
    String => STRING,
    StringStart => STRING_START,
    StringTail => STRING_TAIL,
    StringPart => STRING_PART,
    Date => DATE,
    Ident => IDENT,
    Error => ERROR,
}

/// Значимые токены входа и промежутки между ними.
pub(crate) struct Input<'t> {
    tokens: &'t [Token],
    /// Индекс каждого значимого токена в сыром потоке.
    raw: Vec<u32>,
    /// Записи алфавита для значимых токенов, в том же порядке.
    kinds: Vec<Sig>,
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
            if token.kind.is_trivia() {
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
            kinds.push(Sig::new(token.kind));
        }
        set_bit(&mut line_break_before, kinds.len(), gap_has_line_break);

        Self { tokens, raw, kinds, line_break_before }
    }

    /// Сколько значимых токенов во входе.
    pub(crate) fn len(&self) -> usize {
        self.kinds.len()
    }

    pub(crate) fn kind(&self, pos: usize) -> Option<Sig> {
        self.kinds.get(pos).copied()
    }

    pub(crate) fn text(&self, pos: usize) -> &str {
        self.token(pos).map_or("", |token| token.text.as_str())
    }

    pub(crate) fn token(&self, pos: usize) -> Option<&'t Token> {
        self.raw.get(pos).and_then(|&index| self.tokens.get(index as usize))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Запись есть у вида тогда и только тогда, когда вид не тривиален.
    ///
    /// Свойство направлено в обе стороны, и одной стороны здесь мало.
    /// Полнота — перебор `TokenKind::ALL` — ловит вид, потерянный при правке
    /// списка. Чистота — перебор `Sig::ALL` — ловит лишнюю запись для тривии,
    /// и без неё её не увидеть ничем: отражения ассоциированных констант в
    /// языке нет, поэтому «со стороны видов» лишняя запись невидима.
    ///
    /// Обе стороны падают, называя разошедшийся вид, а не просто краснеют.
    #[test]
    fn a_record_exists_exactly_for_the_significant_kinds() {
        let recorded: Vec<TokenKind> = Sig::ALL.iter().map(|sig| sig.kind()).collect();

        for &kind in TokenKind::ALL {
            assert_eq!(
                recorded.contains(&kind),
                !kind.is_trivia(),
                "{kind:?}: запись в алфавите есть — {}, вид значим — {}",
                recorded.contains(&kind),
                !kind.is_trivia()
            );
        }

        assert_eq!(
            recorded.len(),
            TokenKind::ALL.iter().filter(|kind| !kind.is_trivia()).count(),
            "записей в алфавите столько же, сколько значимых видов: дубль записи \
             перебор по видам пропустил бы"
        );
    }

    /// Прежний обратный скан по сырому потоку — эталон, с которым сверяется
    /// карта.
    ///
    /// Он остаётся здесь дословно, а не переписывается через `Input`: карта и
    /// эталон обязаны быть двумя независимыми способами получить один ответ,
    /// иначе сверка проверяет саму себя.
    fn scan_line_break_before(tokens: &[Token], raw_pos: usize) -> bool {
        let anchor = tokens[raw_pos..]
            .iter()
            .position(|t| !t.kind.is_trivia())
            .map_or(tokens.len(), |offset| raw_pos + offset);

        let mut saw_line_break = false;
        for token in tokens[..anchor].iter().rev() {
            if !token.kind.is_trivia() {
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
            include_str!("../../tests/fixtures/Module.bsl"),
        ] {
            agrees_with_the_scan(source);
        }
    }

    #[test]
    fn no_position_of_the_input_carries_trivia() {
        let source = include_str!("../../tests/fixtures/Module.bsl");
        let tokens = lexer::tokenize(source);
        let input = Input::new(&tokens);

        assert!(input.len() > 0, "фикстура обязана дать значимые токены");
        for pos in 0..input.len() {
            let kind = input.kind(pos).expect("позиция внутри длины обязана иметь вид");
            assert!(!kind.kind().is_trivia(), "позиция {pos} отдала тривию {kind:?}");
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
