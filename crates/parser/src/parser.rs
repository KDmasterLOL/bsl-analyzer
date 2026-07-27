use lexer::{Token, TokenKind};
use parser_error::{ParseError, RecoveryKind};
use smallvec::smallvec;

use crate::event::{Event, NodeKind};

const MAX_ITERATIONS: usize = 1_000_000;

const POSITION_HISTORY_SIZE: usize = 100;

pub struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    events: Vec<Event>,
    iteration_count: usize,
    recent_positions: Vec<usize>,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            pos: 0,
            events: Vec::new(),
            iteration_count: 0,
            recent_positions: Vec::with_capacity(POSITION_HISTORY_SIZE),
        }
    }

    pub fn finish(self) -> Vec<Event> {
        self.events
    }

    pub fn current(&self) -> Option<TokenKind> {
        self.nth(0)
    }

    pub fn nth(&self, n: usize) -> Option<TokenKind> {
        self.tokens.get(self.pos + n).map(|t| t.kind)
    }

    pub fn nth_non_trivia(&self, n: usize) -> Option<TokenKind> {
        let mut count = 0;
        let mut offset = 1;
        while let Some(t) = self.tokens.get(self.pos + offset) {
            match t.kind {
                TokenKind::Whitespace
                | TokenKind::Comment
                | TokenKind::Newline
                | TokenKind::Bom => {
                    offset += 1;
                }
                _ => {
                    if count == n {
                        return Some(t.kind);
                    }
                    count += 1;
                    offset += 1;
                }
            }
        }
        None
    }

    pub fn at(&self, kind: TokenKind) -> bool {
        self.current() == Some(kind)
    }

    pub fn at_ts(&self, set: crate::token_set::TokenSet) -> bool {
        self.current().is_some_and(|k| set.contains(k))
    }

    pub fn at_keyword(&self, text: &str) -> bool {
        if let Some(token) = self.tokens.get(self.pos) {
            token.kind == TokenKind::Ident && stdx::case::eq_ignore_case(&token.text, text)
        } else {
            false
        }
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    pub(crate) fn token_cursor(&self) -> usize {
        self.pos
    }

    /// Net nesting introduced between `from` and the current position:
    /// opened groups minus closed ones. A grammar rule that gave up inside
    /// an unclosed group leaves this above zero, which is the only way a
    /// caller can tell that the position it was handed is not top level.
    pub(crate) fn nesting_delta_since(&self, from: usize) -> i32 {
        self.tokens[from.min(self.tokens.len())..self.pos.min(self.tokens.len())].iter().fold(
            0i32,
            |depth, t| match t.kind {
                TokenKind::LParen | TokenKind::LBrace => depth + 1,
                TokenKind::RParen | TokenKind::RBrace => depth - 1,
                _ => depth,
            },
        )
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
        if let Some(kind) = self.current() {
            self.events.push(Event::Token { kind });
            self.pos += 1;
        }
    }

    pub fn expect(&mut self, kind: TokenKind) -> bool {
        if self.eat(kind) {
            return true;
        }

        let found = self.current();
        let recovery =
            if found.is_none() { RecoveryKind::MissingToken } else { RecoveryKind::BumpToken };
        let err = ParseError::Expected { expected: smallvec![kind], found, recovery };

        if recovery == RecoveryKind::MissingToken {
            self.emit_missing(err);
        } else {
            self.emit_error(err);
        }

        false
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
        let start_token_pos = self.pos;
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

    /// A statement separator is never the token an error is *about*. It is
    /// the boundary that lets whatever comes after it still be parsed, so
    /// reporting at one behaves like reporting at end of input: the
    /// complaint is recorded where the missing element should have been,
    /// and the separator stays for the caller.
    fn at_statement_separator(&self) -> bool {
        self.at(TokenKind::Semicolon)
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

    pub fn skip_trivia(&mut self) {
        while let Some(kind) = self.current() {
            match kind {
                TokenKind::Whitespace
                | TokenKind::Comment
                | TokenKind::Newline
                | TokenKind::Bom => self.bump(),
                _ => break,
            }
        }
    }

    pub fn skip_trivia_crossing_newline(&mut self) -> bool {
        let mut crossed_newline = false;
        while let Some(kind) = self.current() {
            match kind {
                TokenKind::Newline => {
                    crossed_newline = true;
                    self.bump();
                }
                TokenKind::Whitespace | TokenKind::Comment | TokenKind::Bom => self.bump(),
                _ => break,
            }
        }
        crossed_newline
    }

    pub fn at_declaration_start(&self) -> bool {
        matches!(self.current(), Some(TokenKind::KwFunction | TokenKind::KwProcedure))
            && self.nth_non_trivia(0) == Some(TokenKind::Ident)
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
