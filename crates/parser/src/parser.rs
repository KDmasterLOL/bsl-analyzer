//! The parser state machine.

use lexer::{Token, TokenKind};
use parser_error::{ParseError, RecoveryKind};
use smallvec::smallvec;

use crate::event::{Event, NodeKind};

/// Maximum number of iterations to prevent infinite loops
const MAX_ITERATIONS: usize = 1_000_000;

/// How many recent positions to track for debugging infinite loops
const POSITION_HISTORY_SIZE: usize = 100;

/// Parser for BSL language.
pub struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    events: Vec<Event>,
    /// Iteration counter to detect infinite loops
    iteration_count: usize,
    /// Recent positions to detect stuck loops
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

    /// Returns the current token kind.
    pub fn current(&self) -> Option<TokenKind> {
        self.nth(0)
    }

    /// Returns the current token text for debugging.
    #[allow(dead_code)]
    pub(crate) fn current_text(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(|t| t.text.as_str())
    }

    /// Returns the nth token kind (0-indexed).
    pub fn nth(&self, n: usize) -> Option<TokenKind> {
        self.tokens.get(self.pos + n).map(|t| t.kind)
    }

    /// Returns the nth non-trivia token kind (0-indexed), skipping whitespace/comments/newlines.
    pub fn nth_non_trivia(&self, n: usize) -> Option<TokenKind> {
        let mut count = 0;
        let mut offset = 1; // start from next token
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

    /// Checks if the current token matches the given kind.
    pub fn at(&self, kind: TokenKind) -> bool {
        self.current() == Some(kind)
    }

    /// Checks if the current token is in the given set.
    ///
    /// Mirrors rust-analyzer's `Parser::at_ts` — the canonical way to test
    /// position-specific allowlists (e.g. tokens accepted as a property
    /// name after `.`). Returns false at EOF.
    pub fn at_ts(&self, set: crate::token_set::TokenSet) -> bool {
        self.current().is_some_and(|k| set.contains(k))
    }

    /// Checks if the current token matches the given kind and text (case-insensitive).
    ///
    /// This is useful for SDBL keywords that are mapped to TokenKind::Ident.
    /// Uses Unicode-aware case comparison to support Russian keywords.
    pub fn at_keyword(&self, text: &str) -> bool {
        if let Some(token) = self.tokens.get(self.pos) {
            token.kind == TokenKind::Ident && token.text.to_lowercase() == text.to_lowercase()
        } else {
            false
        }
    }

    /// Checks if at the end of input.
    pub fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Consumes the current token if it matches the given kind.
    pub fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consumes the current token.
    pub fn bump(&mut self) {
        if let Some(kind) = self.current() {
            self.events.push(Event::Token { kind });
            self.pos += 1;
        }
    }

    /// Expects the current token to be of the given kind.
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

    /// Consumes the current token if it matches the keyword text (case-insensitive).
    ///
    /// This is useful for SDBL keywords that are mapped to TokenKind::Ident.
    pub fn eat_keyword(&mut self, text: &str) -> bool {
        if self.at_keyword(text) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Starts a new node.
    pub fn start(&mut self) -> Marker {
        let pos = self.events.len();
        let start_token_pos = self.pos;
        self.events.push(Event::Placeholder);
        Marker { pos, start_token_pos }
    }

    /// Adds an error node.
    pub fn error(&mut self) {
        let found = self.current();
        let recovery =
            if found.is_none() { RecoveryKind::MissingToken } else { RecoveryKind::BumpToken };
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
        let recovery = if self.current().is_none() {
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

    /// Emits a custom error at the current position WITHOUT consuming the
    /// lookahead token.
    ///
    /// Use this when the diagnostic itself is enough and the outer grammar
    /// should keep the current token for its own recovery (e.g. block
    /// terminators that must close the enclosing function). `error_custom`
    /// always bumps the next token into an `ERROR` child — that is correct
    /// for "expected `X`, got garbage" cases but wrong here.
    pub fn error_custom_no_bump(&mut self, msg: &'static str) {
        let err = ParseError::Custom { message: msg, recovery: RecoveryKind::MissingToken };
        self.emit_missing(err);
    }

    pub(crate) fn emit_error_at_marker(&mut self, m: Marker, err: ParseError) {
        debug_assert!(err.recovery() == RecoveryKind::RecoverySpan);
        self.events.push(Event::ErrorWithSpan { start_token: m.start_token_pos, err });
        m.complete(self, NodeKind::Error);
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

    /// Skips whitespace, comments, and BOM.
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

    /// Checks whether the parser appears stuck in the recent token window.
    /// The guard panics only when the position barely moves; large-but-progressing
    /// inputs (e.g. a multi-megabyte file fed by mistake) merely reset the counter.
    pub fn check_iteration_limit(&mut self) {
        self.iteration_count += 1;

        // Track recent positions
        if self.recent_positions.len() >= POSITION_HISTORY_SIZE {
            self.recent_positions.remove(0);
        }
        self.recent_positions.push(self.pos);

        if self.iteration_count < MAX_ITERATIONS {
            return;
        }

        let unique_positions: std::collections::HashSet<_> = self.recent_positions.iter().collect();
        let stuck = unique_positions.len() < 5; // Less than 5 unique positions = real loop

        if !stuck {
            // Large input that makes genuine progress: reset the counter and keep parsing.
            // `recent_positions` keeps sliding so a later stall is still caught.
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

/// A marker for a started node.
pub struct Marker /* RecoverySpan carrier */ {
    pos: usize,
    start_token_pos: usize,
}

impl Marker {
    fn at_event_pos(pos: usize, start_token_pos: usize) -> Self {
        Self { pos, start_token_pos }
    }

    /// Completes the node with the given kind.
    pub fn complete(self, p: &mut Parser, kind: NodeKind) -> CompletedMarker {
        let event = &mut p.events[self.pos];
        *event = Event::Start { kind, forward_parent: None };
        p.events.push(Event::Finish);
        CompletedMarker::at_event_pos(self.pos, self.start_token_pos)
    }

    /// Abandons this marker.
    pub fn abandon(self, p: &mut Parser) {
        if self.pos == p.events.len() - 1 {
            if let Some(Event::Placeholder) = p.events.last() {
                p.events.pop();
            }
        }
    }
}

/// A completed marker that can be used for precede.
pub struct CompletedMarker {
    pos: usize,
    start_token_pos: usize,
}

impl CompletedMarker {
    fn at_event_pos(pos: usize, start_token_pos: usize) -> Self {
        Self { pos, start_token_pos }
    }

    /// Wraps the completed node in a new parent.
    pub fn precede(self, p: &mut Parser) -> Marker {
        let new_pos = p.events.len();
        p.events.push(Event::Placeholder);

        if let Event::Start { forward_parent, .. } = &mut p.events[self.pos] {
            *forward_parent = Some(new_pos - self.pos);
        }

        Marker::at_event_pos(new_pos, self.start_token_pos)
    }
}
