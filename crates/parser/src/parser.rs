use lexer::{Token, TokenKind};
use parser_error::{ParseError, RecoveryKind};
use smallvec::smallvec;

use crate::event::{Event, NodeKind};
use crate::input::Input;

const MAX_ITERATIONS: usize = 1_000_000;

const POSITION_HISTORY_SIZE: usize = 100;

/// Asking for trivia by kind is asking the grammar to decide something about
/// lines through a channel that does not carry that decision. The one channel
/// that does is [`Parser::a_line_break_precedes`]; which constructs may cross a
/// line, and on what grounds, is declared in
/// `docs/architecture/adr/ADR-02-line-sensitivity.md`.
const TRIVIA_IS_NOT_THE_CHANNEL: &str =
    "о переводе строки грамматике сообщает предикат, а не вид токена";

pub struct Parser<'a> {
    input: Input<'a>,
    /// Позиция среди ЗНАЧИМЫХ токенов, а не среди лексем.
    pos: usize,
    /// Докуда сырой поток уже отдан событиями.
    ///
    /// Держится отдельно от `pos`, потому что промежутки в дерево всё ещё
    /// уходят: грамматика их не видит, но текст дерева обязан равняться входу.
    raw_pos: usize,
    events: Vec<Event>,
    iteration_count: usize,
    recent_positions: Vec<usize>,
    paren_depth: u32,
    brace_depth: u32,
    at_grammar_boundary: Option<fn(&Parser) -> bool>,
    enclosing_boundaries: Vec<fn(&Parser) -> bool>,
    names_are_fields: bool,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token]) -> Self {
        Self {
            input: Input::new(tokens),
            pos: 0,
            raw_pos: 0,
            events: Vec::new(),
            iteration_count: 0,
            recent_positions: Vec::with_capacity(POSITION_HISTORY_SIZE),
            paren_depth: 0,
            brace_depth: 0,
            at_grammar_boundary: None,
            names_are_fields: false,
            enclosing_boundaries: Vec::new(),
        }
    }

    pub fn finish(self) -> Vec<Event> {
        self.events
    }

    pub fn current(&self) -> Option<TokenKind> {
        self.nth(0)
    }

    /// Вид `n`-го значимого токена, считая от текущего.
    pub fn nth(&self, n: usize) -> Option<TokenKind> {
        self.input.kind(self.pos + n)
    }

    pub fn at(&self, kind: TokenKind) -> bool {
        debug_assert!(!Self::is_trivia_kind(kind), "{}", TRIVIA_IS_NOT_THE_CHANNEL);
        self.current() == Some(kind)
    }

    /// No check for trivia here, unlike [`Parser::at`]: a set cannot hold any.
    /// The bits of a [`TokenSet`](crate::token_set::TokenSet) are private and
    /// every way to make one refuses trivia or preserves that refusal, so a
    /// check placed here could never be seen failing.
    pub fn at_ts(&self, set: crate::token_set::TokenSet) -> bool {
        self.current().is_some_and(|k| set.contains(k))
    }

    pub fn current_text(&self) -> &str {
        self.input.text(self.pos)
    }

    pub fn at_keyword(&self, text: &str) -> bool {
        self.input.token(self.pos).is_some_and(|t| {
            t.kind == TokenKind::Ident && stdx::case::eq_ignore_case(&t.text, text)
        })
    }

    /// Значимых токенов больше не осталось.
    ///
    /// Хвост промежутков за последним словом сюда не входит: грамматике там
    /// делать нечего, а до дерева его доводит сток.
    pub fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    pub fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub fn bump(&mut self) {
        let Some(kind) = self.current() else {
            return;
        };
        self.track_group(kind);

        let raw = self.input.raw_at(self.pos);
        self.emit_gap_before(raw);
        self.events.push(Event::Token { kind });
        self.raw_pos = raw + 1;
        self.pos += 1;
    }

    /// Отдаёт событиями промежуток перед сырой позицией `raw`.
    ///
    /// Промежутки уходят в дерево здесь, а не по просьбе грамматики: правило,
    /// которому пришлось бы просить, тем самым знало бы о тривии. Событие на
    /// каждую лексему — потому что сток идёт по сырому потоку в такт событиям,
    /// и пропуск одного события сдвинул бы весь остаток.
    fn emit_gap_before(&mut self, raw: usize) {
        while self.raw_pos < raw {
            let kind = self.input.raw_kind(self.raw_pos);
            self.events.push(Event::Token { kind });
            self.raw_pos += 1;
        }
    }

    /// Maintains the open-group count as tokens go by.
    ///
    /// Kept here because `bump` is the only way a token is consumed and the
    /// position never rewinds, so this is the one place that sees every
    /// token exactly once. Counting it anywhere else means counting it twice
    /// or rescanning, and rescanning a growing prefix on every decision is
    /// quadratic on a long malformed input.
    ///
    /// The two kinds are counted apart so that a closer of one kind cannot
    /// cancel an opener of the other. Parens inside a brace region are not
    /// counted at all: that region's content is taken verbatim, so its
    /// brackets say nothing about the structure around it.
    fn track_group(&mut self, kind: TokenKind) {
        match kind {
            // A group cannot span a statement separator, so consuming one
            // closes whatever was open — wherever it is consumed. Tying this
            // to the one caller that happens to notice the separator would
            // leave every rule that swallows one holding stale depth.
            TokenKind::Semicolon => {
                self.paren_depth = 0;
                self.brace_depth = 0;
            }
            TokenKind::LBrace => self.brace_depth += 1,
            TokenKind::RBrace => self.brace_depth = self.brace_depth.saturating_sub(1),
            TokenKind::LParen if self.brace_depth == 0 => self.paren_depth += 1,
            TokenKind::RParen if self.brace_depth == 0 => {
                self.paren_depth = self.paren_depth.saturating_sub(1)
            }
            _ => {}
        }
    }

    /// How many groups are open at the current position.
    ///
    /// Tells a caller whether the rule it just ran left something open —
    /// which is the only way to know that the position it was handed is not
    /// top level.
    pub(crate) fn open_group_count(&self) -> u32 {
        self.paren_depth + self.brace_depth
    }

    /// Declares that whatever was open before this point is closed as far as
    /// grouping is concerned.
    ///
    /// A statement separator is that point: a group cannot span one. Without
    /// saying so, an unclosed group in a bad statement outlives it — an
    /// unclosed `{` would go on suppressing the paren count in every
    /// statement after it, and a depth left standing would let the next
    /// statement close it and open its own without the total ever changing.
    pub(crate) fn reset_group_tracking(&mut self) {
        self.paren_depth = 0;
        self.brace_depth = 0;
    }

    pub fn expect(&mut self, kind: TokenKind) -> bool {
        // Its own check, not the one in `at`: a declared boundary returns from
        // here without ever reaching it.
        debug_assert!(!Self::is_trivia_kind(kind), "{}", TRIVIA_IS_NOT_THE_CHANNEL);

        // A grammar boundary belongs to a rule further out, and no rule may
        // require it here. It matters because kinds are coarser than words:
        // every keyword of a language whose keywords are not reserved
        // arrives as an identifier, so `expect(Ident)` would take the word
        // that begins the next construct and cost the caller that construct.
        // The separator is not covered — a rule asking for `;` means it.
        if !self.at_declared_boundary() && self.eat(kind) {
            return true;
        }

        let found = self.current();
        let recovery = if found.is_none() || self.at_statement_separator() {
            RecoveryKind::MissingToken
        } else {
            RecoveryKind::BumpToken
        };
        let err = ParseError::Expected { expected: smallvec![kind], found, recovery };

        if recovery == RecoveryKind::MissingToken {
            self.emit_missing(err);
        } else {
            self.emit_error(err);
        }

        false
    }

    /// Runs `f` with a bare name read as a field of a source.
    ///
    /// Where the query says what to read — the field list, a filter, a join
    /// condition — a bare name is a field, and the source closes that position
    /// to keywords. Where it says how to arrange what was read, the same word
    /// may be an alias the select list declared, and an alias is allowed to
    /// spell a keyword; those rules leave this off, so `УПОРЯДОЧИТЬ ПО Итоги`
    /// stays a reference rather than becoming an error.
    ///
    /// Scoped like a boundary, and for the same reason: the fact belongs to
    /// the rule that knows it, and has to end when that rule returns.
    pub fn within_field_names<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let outer = self.names_are_fields;
        self.names_are_fields = true;
        let result = f(self);
        self.names_are_fields = outer;
        result
    }

    /// Runs `f` with the field-name scope of whatever holds it dropped.
    ///
    /// A nested query is its own query. The scope belongs to the clause that
    /// declared it, and a subquery standing inside a filter must read its own
    /// `УПОРЯДОЧИТЬ ПО` the way a top-level one does — by an alias its own
    /// selection declared, which may spell a keyword.
    pub fn outside_field_names<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let outer = self.names_are_fields;
        self.names_are_fields = false;
        let result = f(self);
        self.names_are_fields = outer;
        result
    }

    pub fn names_are_fields(&self) -> bool {
        self.names_are_fields
    }

    /// Requires `kind`, and consumes nothing when it is absent.
    ///
    /// A rule that gives up once it sees its opener is missing must not spend
    /// a token on the way out. Whatever stands there is the continuation of
    /// something else — an operator of the expression around it, the bracket
    /// of the construct holding it — and plain `expect` would take it as its
    /// recovery, which costs the caller exactly what the caller was about to
    /// parse.
    pub fn expect_no_bump(&mut self, kind: TokenKind) -> bool {
        // Its own check, for the same reason as in [`Parser::expect`].
        debug_assert!(!Self::is_trivia_kind(kind), "{}", TRIVIA_IS_NOT_THE_CHANNEL);

        if !self.at_declared_boundary() && self.eat(kind) {
            return true;
        }

        let err = ParseError::Expected {
            expected: smallvec![kind],
            found: self.current(),
            recovery: RecoveryKind::MissingToken,
        };
        self.emit_missing(err);
        false
    }

    /// The last token before this one that was not trivia.
    ///
    /// A rule that must know what it is standing behind — whether a dot
    /// made the word here a field name, say — would otherwise walk the
    /// token list itself, and every rule that walked it got a different
    /// part of it wrong.
    pub fn prev_significant(&self) -> Option<TokenKind> {
        self.pos.checked_sub(1).and_then(|prev| self.input.kind(prev))
    }

    /// Whether a line break stands between the previous significant token and
    /// the next one.
    ///
    /// This is the whole of what the grammar is told about lines. The kind of
    /// a token is not that channel: a rule that reads one gets the norm from
    /// nowhere, and the next rule to need the same decision writes it a fourth
    /// way. Which constructs may cross a line, and on what grounds, is
    /// declared in `docs/architecture/adr/ADR-02-line-sensitivity.md`.
    ///
    /// A comment in the gap counts as a line break: it runs to the end of its
    /// line, so behind it stands either a newline or the end of the file.
    ///
    /// Nothing is consumed. Where no significant token precedes the gap —
    /// the prologue of a file — there is no pair of tokens to stand between,
    /// and the answer is `false`.
    pub fn a_line_break_precedes(&self) -> bool {
        self.input.a_line_break_precedes(self.pos)
    }

    /// Соприкасается ли токен под курсором с предыдущим.
    ///
    /// Нужен там, где лексер режет одно слово языка на несколько токенов
    /// вплотную: склеить обратно надо ровно их, а соседнее слово, отделённое
    /// хоть пробелом, — уже другое слово.
    pub fn a_gap_precedes(&self) -> bool {
        self.input.a_gap_precedes(self.pos)
    }

    fn is_trivia_kind(kind: TokenKind) -> bool {
        crate::input::is_trivia(kind)
    }

    pub fn eat_keyword(&mut self, text: &str) -> bool {
        if self.at_keyword(text) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub fn start(&mut self) -> Marker {
        let pos = self.events.len();
        // Индекс СЫРОЙ: сток считает диапазоны по потоку лексем, а `self.pos`
        // считает слова. Позиция за последним словом штатна — ошибка о
        // пропущенном токене на конце входа ставит маркер именно там.
        let start_token_pos = self.input.raw_at(self.pos);
        self.events.push(Event::Placeholder);
        Marker { pos, start_token_pos }
    }

    pub fn error(&mut self) {
        let found = self.current();
        let recovery = if found.is_none() || self.at_statement_separator() {
            RecoveryKind::MissingToken
        } else {
            RecoveryKind::BumpToken
        };
        let err = ParseError::Unexpected { found, recovery };

        if recovery == RecoveryKind::MissingToken {
            self.emit_missing(err);
        } else {
            self.emit_error(err);
        }
    }

    pub fn error_unexpected(&mut self) {
        self.error();
    }

    pub fn error_custom(&mut self, msg: &'static str) {
        let recovery = if self.current().is_none() || self.at_statement_separator() {
            RecoveryKind::MissingToken
        } else {
            RecoveryKind::BumpToken
        };
        let err = ParseError::Custom { message: msg, recovery };
        if recovery == RecoveryKind::MissingToken {
            self.emit_missing(err);
        } else {
            self.emit_error(err);
        }
    }

    pub fn error_custom_no_bump(&mut self, msg: &'static str) {
        let err = ParseError::Custom { message: msg, recovery: RecoveryKind::MissingToken };
        self.emit_missing(err);
    }

    pub(crate) fn emit_error_at_marker(&mut self, m: Marker, err: ParseError) {
        debug_assert!(err.recovery() == RecoveryKind::RecoverySpan);
        self.events.push(Event::ErrorWithSpan { start_token: m.start_token_pos, err });
        m.complete(self, NodeKind::Error);
    }

    /// Names the words that are never anything but a boundary, for the whole
    /// parse.
    ///
    /// A rule deep inside a construct cannot know what encloses it, so it
    /// cannot know which token it must not take. What holds everywhere the
    /// grammar can say once, here — and only what holds everywhere belongs
    /// here, because this reaches even the positions where a rule is asking
    /// for a name.
    pub fn set_grammar_boundary(&mut self, at_boundary: fn(&Parser) -> bool) {
        self.at_grammar_boundary = Some(at_boundary);
    }

    /// Runs `f` with one more token pattern that no rule inside it may report
    /// an error by consuming.
    ///
    /// This is what the whole-parse boundary cannot express: the closer of
    /// *this* block, the clause keywords of *this* query. The rule that owns
    /// the construct knows them, states them where it descends into the body,
    /// and they stop applying when it returns — which is why they are pushed
    /// here rather than by hand, since a rule with an early return would
    /// otherwise leave one behind.
    ///
    /// Unlike the whole-parse boundary, an enclosing one does not reach a
    /// rule that is asking for a name: a clause keyword is a boundary where a
    /// clause may start and an ordinary alias where a name may stand.
    pub fn within_boundary<R>(
        &mut self,
        at_boundary: fn(&Parser) -> bool,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.enclosing_boundaries.push(at_boundary);
        let result = f(self);
        self.enclosing_boundaries.pop();
        result
    }

    /// Whether the current token belongs to a construct further out.
    ///
    /// A loop that parses items until its own terminator has to ask this too.
    /// Rules inside it refuse to consume such a token, so a loop that waits
    /// only for its own terminator waits for a token nothing will reach — the
    /// boundary turns a cascade of false errors into a parse that never ends.
    pub fn at_enclosing_boundary(&self) -> bool {
        self.enclosing_boundaries.iter().any(|at_boundary| at_boundary(self))
    }

    /// A boundary token is never the token an error is *about*. It is what
    /// lets whatever comes after it still be parsed, so reporting at one
    /// behaves like reporting at end of input: the complaint is recorded
    /// where the missing element should have been, and the token stays for
    /// the caller.
    fn at_statement_separator(&self) -> bool {
        self.at(TokenKind::Semicolon) || self.at_declared_boundary() || self.at_enclosing_boundary()
    }

    /// The part of the boundary that holds for the whole parse, without the
    /// separator every grammar has.
    ///
    /// Kept apart from the enclosing ones because this is the only part a
    /// successful `expect` may refuse: a word that is never a name is safe to
    /// decline anywhere, and a word that is a name somewhere is not.
    fn at_declared_boundary(&self) -> bool {
        self.at_grammar_boundary.is_some_and(|at_boundary| at_boundary(self))
    }

    fn emit_error(&mut self, err: ParseError) {
        debug_assert!(matches!(err.recovery(), RecoveryKind::BumpToken | RecoveryKind::Custom));
        let m = self.start();
        if !self.at_end() {
            self.bump();
        }
        self.events.push(Event::Error(err));
        m.complete(self, NodeKind::Error);
    }

    fn emit_missing(&mut self, err: ParseError) {
        debug_assert!(err.recovery() == RecoveryKind::MissingToken);
        let m = self.start();
        self.events.push(Event::Error(err));
        m.complete(self, NodeKind::Error);
    }

    pub fn at_declaration_start(&self) -> bool {
        matches!(self.current(), Some(TokenKind::KwFunction | TokenKind::KwProcedure))
            && self.nth(1) == Some(TokenKind::Ident)
    }

    pub fn check_iteration_limit(&mut self) {
        self.iteration_count += 1;

        if self.recent_positions.len() >= POSITION_HISTORY_SIZE {
            self.recent_positions.remove(0);
        }
        self.recent_positions.push(self.pos);

        if self.iteration_count < MAX_ITERATIONS {
            return;
        }

        let unique_positions: std::collections::HashSet<_> = self.recent_positions.iter().collect();
        let stuck = unique_positions.len() < 5;

        if !stuck {
            tracing::debug!(
                position = self.pos,
                unique_in_window = unique_positions.len(),
                "parser guard: large input, resetting iteration counter"
            );
            self.iteration_count = 0;
            return;
        }

        let last_10: Vec<_> = self.recent_positions.iter().rev().take(10).rev().copied().collect();

        panic!(
            "Parser exceeded maximum iteration limit ({} iterations).\n\
            Position: {}, Token: {:?}\n\
            Status: STUCK (infinite loop)\n\
            Last 10 positions: {:?}\n\
            Unique positions in last {}: {}\n\
            This is a bug - the parser should always make progress.",
            MAX_ITERATIONS,
            self.pos,
            self.current(),
            last_10,
            POSITION_HISTORY_SIZE,
            unique_positions.len()
        );
    }
}

pub struct Marker {
    pos: usize,
    start_token_pos: usize,
}

impl Marker {
    fn at_event_pos(pos: usize, start_token_pos: usize) -> Self {
        Self { pos, start_token_pos }
    }

    pub fn complete(self, p: &mut Parser, kind: NodeKind) -> CompletedMarker {
        let event = &mut p.events[self.pos];
        *event = Event::Start { kind, forward_parent: None };
        p.events.push(Event::Finish);
        CompletedMarker::at_event_pos(self.pos, self.start_token_pos)
    }

    pub fn abandon(self, p: &mut Parser) {
        if self.pos == p.events.len() - 1 {
            if let Some(Event::Placeholder) = p.events.last() {
                p.events.pop();
            }
        }
    }
}

pub struct CompletedMarker {
    pos: usize,
    start_token_pos: usize,
}

impl CompletedMarker {
    fn at_event_pos(pos: usize, start_token_pos: usize) -> Self {
        Self { pos, start_token_pos }
    }

    pub fn precede(self, p: &mut Parser) -> Marker {
        let new_pos = p.events.len();
        p.events.push(Event::Placeholder);

        if let Event::Start { forward_parent, .. } = &mut p.events[self.pos] {
            *forward_parent = Some(new_pos - self.pos);
        }

        Marker::at_event_pos(new_pos, self.start_token_pos)
    }
}
