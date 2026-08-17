//! Перевод потока событий парсера в дерево — и место, где объявлено, какому
//! узлу принадлежит тривия.

use parser_error::{ParseError, RecoveryKind};
use syntax::{SyntaxKind, SyntaxTreeBuilder, TextRange, TextSize};

use crate::{
    event::Event,
    syntax_kind::{node_kind_to_syntax, token_kind_to_syntax},
};

pub struct Sink<'t, 'cache> {
    builder: SyntaxTreeBuilder<'cache>,
    tokens: &'t [lexer::Token],
    /// Курсор потока лексем: двигается на каждом токенном событии, отдана
    /// лексема билдеру или отложена.
    ///
    /// Диапазоны ошибок считаются по нему, потому что позиции, с которыми их
    /// сравнивают, приходят из парсера, а тот ходит по лексемам подряд,
    /// включая тривию.
    token_pos: usize,
    /// Позиция последней ОТДАННОЙ значимой лексемы в том же сыром счёте.
    ///
    /// Ошибка, о которой сообщает потраченный токен, обязана указывать на
    /// токен, а не на пробел перед ним: с буферизацией предыдущая лексема
    /// потока и последняя попавшая в дерево — разные вещи.
    last_significant_pos: Option<usize>,
    /// Начало непрерывного участка тривии, ещё не отданного билдеру; конец —
    /// текущее положение курсора.
    ///
    /// Участок непрерывен по построению: значимый токен буфер сбрасывает,
    /// поэтому между отложенными лексемами другой оказаться не может.
    pending_trivia_start: Option<usize>,
    /// Открытия и закрытия узлов, ещё не выполненные, в порядке поступления.
    ///
    /// Хранится потоком, а не стеком открытий, потому что узел, закрывшийся
    /// без единого значимого токена, обязан сохранить своё место среди
    /// отложенных предков: выполнить их открытия ради него значит отдать им
    /// ведущую тривию, которая по норме принадлежит предку снаружи.
    deferred_nodes: Vec<DeferredNode>,
    /// Сколько отложенных открытий ещё не закрыто.
    deferred_depth: usize,
    /// Сколько узлов открыто в билдере.
    open_nodes: usize,
    errors: Vec<(TextRange, ParseError)>,
}

enum DeferredNode {
    Start(SyntaxKind),
    Finish,
}

impl<'t> Sink<'t, 'static> {
    pub fn new(tokens: &'t [lexer::Token]) -> Self {
        Self::with_builder(SyntaxTreeBuilder::new(), tokens)
    }
}

impl<'t, 'cache> Sink<'t, 'cache> {
    pub fn with_cache(tokens: &'t [lexer::Token], cache: &'cache mut syntax::NodeCache) -> Self {
        Self::with_builder(SyntaxTreeBuilder::with_cache(cache), tokens)
    }

    fn with_builder(builder: SyntaxTreeBuilder<'cache>, tokens: &'t [lexer::Token]) -> Self {
        Self {
            builder,
            tokens,
            token_pos: 0,
            last_significant_pos: None,
            pending_trivia_start: None,
            deferred_nodes: Vec::new(),
            deferred_depth: 0,
            open_nodes: 0,
            errors: Vec::new(),
        }
    }

    /// Строит дерево, объявляя, кому принадлежит тривия.
    ///
    /// Норма: тривия принадлежит общему предку соседних значимых токенов.
    /// Объявлена она здесь, а не в грамматике, потому что `Marker::complete`
    /// закрывает узел по текущей позиции парсера — правило, съевшее пробел
    /// ради заглядывания вперёд, втягивает его в узел независимо от того,
    /// зачем съело. Держать норму перечнем правил значит держать её
    /// дисциплиной авторов.
    ///
    /// Отсюда два отступления от прямой трансляции событий. Тривия копится в
    /// буфере и уходит билдеру только перед значимым токеном — это даёт
    /// хвостовой край, потому что закрытие узла успевает раньше сброса.
    /// Открытие узла откладывается до его первого значимого токена — это даёт
    /// ведущий край. Сброса «перед открытием» не хватило бы: маркер ставится
    /// на позиции, промежуток перед которой ещё не отдан, и узел открылся бы
    /// раньше этого промежутка.
    ///
    /// Узел, у которого своих значимых токенов не оказалось, остаётся пустым;
    /// это узлы ошибок, и диапазоны самих сообщений считаются отдельно.
    pub fn finish(mut self, events: Vec<Event>) -> SyntaxTreeBuilder<'cache> {
        let mut forward_parents = Vec::new();
        let mut skip = vec![false; events.len()];

        for i in 0..events.len() {
            if let Event::Start { forward_parent: Some(fwd), .. } = &events[i] {
                let mut idx = i + fwd;
                while let Event::Start { forward_parent, .. } = &events[idx] {
                    skip[idx] = true;
                    if let Some(next_fwd) = forward_parent {
                        idx += next_fwd;
                    } else {
                        break;
                    }
                }
            }
        }

        for i in 0..events.len() {
            match &events[i] {
                Event::Start { kind, forward_parent } => {
                    if skip[i] {
                        continue;
                    }

                    forward_parents.clear();
                    forward_parents.push(*kind);

                    let mut idx = i;
                    let mut fp = *forward_parent;
                    while let Some(fwd) = fp {
                        idx += fwd;
                        if let Event::Start { kind, forward_parent } = &events[idx] {
                            forward_parents.push(*kind);
                            fp = *forward_parent;
                        } else {
                            unreachable!("forward_parent must point to Start event");
                        }
                    }

                    // Корень открывается сразу: сбрасывать тривию некуда,
                    // пока не открыт ни один узел, и хвост файла иначе
                    // выпал бы из дерева.
                    let defer = self.open_nodes > 0 || !self.deferred_nodes.is_empty();
                    for kind in forward_parents.iter().rev() {
                        let kind = node_kind_to_syntax(*kind);
                        if defer {
                            self.deferred_nodes.push(DeferredNode::Start(kind));
                            self.deferred_depth += 1;
                        } else {
                            self.builder.start_node(kind);
                            self.open_nodes += 1;
                        }
                    }
                }

                // Закрытие идёт раньше любого сброса буфера — этим и держится
                // хвостовой край нормы.
                Event::Finish => {
                    if self.deferred_depth > 0 {
                        self.deferred_nodes.push(DeferredNode::Finish);
                        self.deferred_depth -= 1;
                        continue;
                    }

                    // Уравновешенные отложенные узлы значимых токенов так и
                    // не получили. Место у них внутри закрываемого узла,
                    // поэтому выйти нерождёнными они не могут — но и тривию
                    // им отдавать не за что: они пусты и встают там, где
                    // билдер стоит сейчас.
                    self.perform_deferred_nodes();
                    if self.open_nodes == 1 {
                        self.take_the_tail();
                        self.flush_trivia();
                    }
                    self.builder.finish_node();
                    self.open_nodes -= 1;
                }

                Event::Token { kind } => {
                    self.token(*kind);
                }

                Event::Placeholder => {}

                Event::Error(err) => {
                    let range = self.compute_error_range(err, None);
                    self.errors.push((range, err.clone()));
                }

                Event::ErrorWithSpan { start_token, err } => {
                    let range = self.compute_error_range(err, Some(*start_token));
                    self.errors.push((range, err.clone()));
                }
            }
        }

        for (range, err) in self.errors.drain(..) {
            self.builder.error(range, err);
        }

        self.builder
    }

    /// Ставит очередной значимый токен, забрав по дороге промежуток перед ним.
    ///
    /// Промежутки событий не имеют: грамматика их не видит и выдать не может.
    /// Промотку делает сток, потому что он один ходит по сырому потоку.
    fn token(&mut self, kind: lexer::TokenKind) {
        while self
            .tokens
            .get(self.token_pos)
            .is_some_and(|token| token_kind_to_syntax(token.kind).is_trivia())
        {
            self.pending_trivia_start.get_or_insert(self.token_pos);
            self.token_pos += 1;
        }

        let Some(token) = self.tokens.get(self.token_pos) else {
            return;
        };

        // Порядок здесь и есть ведущий край нормы: тривия достаётся тому,
        // что было открыто до неё, а узлы этого токена открываются уже
        // после неё.
        self.flush_trivia();
        self.perform_deferred_nodes();
        self.builder.token(token_kind_to_syntax(kind), &token.text);
        self.last_significant_pos = Some(self.token_pos);
        self.token_pos += 1;
    }

    /// Досматривает поток до конца, пока корень ещё открыт.
    ///
    /// Хвост за последним значимым токеном событий не имеет: грамматика
    /// значимых токенов там уже не видит и ничего не бампает. Довести его до
    /// дерева может только тот, кто ходит по сырому потоку, — и сделать это
    /// обязан ДО закрытия корня, потому что закрытому узлу токенов не отдать.
    fn take_the_tail(&mut self) {
        while self.token_pos < self.tokens.len() {
            self.pending_trivia_start.get_or_insert(self.token_pos);
            self.token_pos += 1;
        }
    }

    /// Отдаёт накопленную тривию билдеру.
    ///
    /// Вид берётся у самой лексемы, а не у события: текст она отдаёт тот же,
    /// и разойтись они не могут — событие рождается из лексемы под курсором.
    fn flush_trivia(&mut self) {
        let Some(start) = self.pending_trivia_start.take() else {
            return;
        };
        let tokens = self.tokens;
        for token in &tokens[start..self.token_pos] {
            self.builder.token(token_kind_to_syntax(token.kind), &token.text);
        }
    }

    fn perform_deferred_nodes(&mut self) {
        self.deferred_depth = 0;
        let mut deferred = std::mem::take(&mut self.deferred_nodes);
        for node in deferred.drain(..) {
            match node {
                DeferredNode::Start(kind) => {
                    self.builder.start_node(kind);
                    self.open_nodes += 1;
                }
                DeferredNode::Finish => {
                    self.builder.finish_node();
                    self.open_nodes -= 1;
                }
            }
        }
        self.deferred_nodes = deferred;
    }

    fn compute_error_range(&self, err: &ParseError, span_start_token: Option<usize>) -> TextRange {
        match err.recovery() {
            RecoveryKind::BumpToken => self.previous_token_range_or_zero_at_start(),
            RecoveryKind::MissingToken => {
                let offset = self.offset_of_next_significant(self.token_pos);
                TextRange::empty(TextSize::new(offset))
            }
            RecoveryKind::RecoverySpan => {
                let start = span_start_token
                    .map_or_else(|| self.source_len(), |idx| self.offset_of_next_significant(idx));
                let end = self.offset_of_next_significant(self.token_pos);
                self.safe_range(start, end)
            }
            RecoveryKind::Custom => {
                if self.last_significant_pos.is_some() {
                    self.previous_token_range_or_zero_at_start()
                } else {
                    let offset = self.offset_of_next_significant(self.token_pos);
                    TextRange::empty(TextSize::new(offset))
                }
            }
        }
    }

    /// Диапазон токена, которым ошибка была потрачена.
    ///
    /// Берётся последний ЗНАЧИМЫЙ токен, а не предыдущая лексема потока:
    /// ошибку, потраченную на пробел, следует показывать на слове перед ним,
    /// а не на самом пробеле.
    fn previous_token_range_or_zero_at_start(&self) -> TextRange {
        let Some(pos) = self.last_significant_pos else {
            return TextRange::empty(TextSize::new(0));
        };

        self.tokens.get(pos).map_or_else(
            || {
                let offset = self.source_len();
                TextRange::empty(TextSize::new(offset))
            },
            |token| {
                let start = self.clamp_offset(token.offset);
                let end = self.clamp_offset(token.offset.saturating_add(token.text.len()));
                self.safe_range(start, end)
            },
        )
    }

    /// Начало первого значимого токена на позиции `from` или за ней; если
    /// такого нет — конец входа.
    ///
    /// Этим держится норма привязки диапазонов ошибок
    /// (`docs/architecture/adr/ADR-03-error-range-attribution.md`): о
    /// пропущенном токене сообщают на начале следующего слова, а не в
    /// промежутке перед ним. Пропущенный токен места не занимает, и указать на
    /// него можно тремя способами — на конец предыдущего слова, в промежуток,
    /// на начало следующего; выбран третий.
    ///
    /// Нормализация нужна здесь, а не у вызывающих: сегодня промежуток под
    /// курсором почти не встречается, потому что грамматика снимает тривию
    /// раньше, чем требует токен, — но это её привычка, а не устройство стока.
    /// Правило, потребовавшее токен и не снявшее пробел, ставит диагностику на
    /// пробел, и заметить это можно только по жалобе.
    fn offset_of_next_significant(&self, from: usize) -> u32 {
        self.tokens[from.min(self.tokens.len())..]
            .iter()
            .find(|token| !token_kind_to_syntax(token.kind).is_trivia())
            .map_or_else(|| self.source_len(), |token| self.clamp_offset(token.offset))
    }

    fn clamp_offset(&self, offset: usize) -> u32 {
        self.to_u32(offset).min(self.source_len())
    }

    fn safe_range(&self, start: u32, end: u32) -> TextRange {
        let end = end.max(start);
        TextRange::new(TextSize::new(start), TextSize::new(end))
    }

    fn source_len(&self) -> u32 {
        self.tokens
            .last()
            .map_or(0, |token| self.to_u32(token.offset.saturating_add(token.text.len())))
    }

    fn to_u32(&self, offset: usize) -> u32 {
        u32::try_from(offset).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::NodeKind, grammar};

    fn unexpected(recovery: RecoveryKind) -> ParseError {
        ParseError::Unexpected { found: Some(lexer::TokenKind::Ident), recovery }
    }

    fn range(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::new(start), TextSize::new(end))
    }

    #[test]
    fn test_sink_simple() {
        let source = "Процедура Тест() КонецПроцедуры";
        let tokens = lexer::tokenize(source);
        let mut parser = crate::Parser::new(&tokens);
        grammar::source_file(&mut parser);
        let events = parser.finish();

        let sink = Sink::new(&tokens);
        let builder = sink.finish(events);
        let parse = builder.finish();

        assert!(!parse.has_errors());
        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SOURCE_FILE);
    }

    #[test]
    fn error_event_between_forward_parent_starts_preserves_tree_topology() {
        let source = "Тест";
        let tokens = lexer::tokenize(source);
        let events = vec![
            Event::Start { kind: NodeKind::SourceFile, forward_parent: None },
            Event::Start { kind: NodeKind::Ident, forward_parent: Some(4) },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Finish,
            Event::Error(unexpected(RecoveryKind::BumpToken)),
            Event::Start { kind: NodeKind::FieldExpr, forward_parent: None },
            Event::Finish,
            Event::Finish,
        ];

        let parse = Sink::new(&tokens).finish(events).finish();

        assert_eq!(parse.errors().len(), 1);
        let root = parse.syntax_node();
        let field = root.children().next().expect("source should contain field expr");
        assert_eq!(field.kind(), syntax::SyntaxKind::FIELD_EXPR);
        let ident = field.children().next().expect("field expr should contain ident");
        assert_eq!(ident.kind(), syntax::SyntaxKind::IDENT);
    }

    #[test]
    fn multi_byte_cyrillic_error_range_is_byte_correct() {
        // The token has to be one no enclosing rule is waiting for: a block
        // closer is reported at the gap without being consumed, so it never
        // carries a range of its own.
        let source = "Процедура Тест() А = Возврат; КонецПроцедуры";
        let tokens = lexer::tokenize(source);
        let unexpected = tokens
            .iter()
            .find(|token| token.kind == lexer::TokenKind::KwReturn)
            .expect("test input should contain Возврат token");
        let parse = crate::parse(source);
        let expected_range = range(
            unexpected.offset as u32,
            unexpected.offset.saturating_add(unexpected.text.len()) as u32,
        );

        let error = parse
            .errors()
            .iter()
            .find(|error| error.range() == expected_range)
            .expect("unexpected Cyrillic token should produce a byte-exact range");
        assert!(source.is_char_boundary(u32::from(error.range().start()) as usize));
        assert!(source.is_char_boundary(u32::from(error.range().end()) as usize));
    }

    #[test]
    fn missing_token_at_eof_uses_zero_width_source_len_range() {
        let source = "Процедура Тест(";
        let parse = crate::parse(source);
        let expected = TextRange::empty(TextSize::new(source.len() as u32));

        assert!(
            parse.errors().iter().any(|error| {
                error.range() == expected
                    && error.structured().recovery() == RecoveryKind::MissingToken
            }),
            "expected a MissingToken diagnostic at EOF, got {:?}",
            parse.errors()
        );
    }

    /// Тривия перед концом файла копится в буфере, поэтому «текущий токен»
    /// в счёте отданных билдеру лексем и в счёте потока — разные вещи.
    /// Здесь ошибка обязана встать в конец файла, а не перед пробелами.
    #[test]
    fn missing_token_after_buffered_trivia_still_lands_at_eof() {
        let source = "Процедура Тест(   ";
        let parse = crate::parse(source);
        let expected = TextRange::empty(TextSize::new(source.len() as u32));

        assert!(
            parse.errors().iter().any(|error| {
                error.range() == expected
                    && error.structured().recovery() == RecoveryKind::MissingToken
            }),
            "ожидалась MissingToken в конце файла, получено {:?}",
            parse.errors()
        );
    }

    /// Конец спана берётся по потоку лексем, а не по отданным билдеру: иначе
    /// он оборвался бы перед накопленной тривией.
    #[test]
    fn error_span_after_buffered_trivia_covers_it() {
        let source = "А Б ;";
        let tokens = lexer::tokenize(source);
        let events = vec![
            Event::Start { kind: NodeKind::SourceFile, forward_parent: None },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::ErrorWithSpan { start_token: 0, err: unexpected(RecoveryKind::RecoverySpan) },
            Event::Token { kind: lexer::TokenKind::Semicolon },
            Event::Finish,
        ];

        let parse = Sink::new(&tokens).finish(events).finish();

        // Конец — начало `;`, то есть за пробелом. Реализация, считающая
        // отданные билдеру лексемы, оборвала бы спан на начале `Б`.
        assert_eq!(parse.errors().len(), 1);
        assert_eq!(parse.errors()[0].range(), range(0, tokens[4].offset as u32));
        assert_ne!(parse.errors()[0].range(), range(0, tokens[2].offset as u32));
    }

    /// Ветвь `Custom` при непустом буфере: своей проверки у неё нет ни одной,
    /// а сообщить она обязана о слове, а не о пробеле за ним.
    #[test]
    fn custom_error_after_buffered_trivia_points_at_the_last_significant_token() {
        let source = "А   ";
        let tokens = lexer::tokenize(source);
        let events = vec![
            Event::Start { kind: NodeKind::SourceFile, forward_parent: None },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Error(unexpected(RecoveryKind::Custom)),
            Event::Finish,
        ];

        let parse = Sink::new(&tokens).finish(events).finish();

        assert_eq!(parse.errors().len(), 1);
        assert_eq!(parse.errors()[0].range(), range(0, 2));
    }

    /// Ветвь `Custom` при пустом дереве: показывать не на чем, поэтому
    /// диапазон пуст — но и он нормализуется вперёд, к первому слову.
    ///
    /// Вход начинается с тривии: пока перед ошибкой стоит слово, работает
    /// соседняя ветвь, и подмена нормализации смещением текущей лексемы
    /// остаётся невидимой.
    #[test]
    fn custom_error_on_an_empty_tree_points_at_the_first_word() {
        let source = "  А";
        let tokens = lexer::tokenize(source);
        let events = vec![
            Event::Start { kind: NodeKind::SourceFile, forward_parent: None },
            Event::Error(unexpected(RecoveryKind::Custom)),
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Finish,
        ];

        let parse = Sink::new(&tokens).finish(events).finish();

        assert_eq!(parse.errors().len(), 1);
        assert_eq!(
            parse.errors()[0].range(),
            TextRange::empty(TextSize::new(tokens[1].offset as u32)),
            "пустой диапазон остался на промежутке вместо начала первого слова"
        );
    }

    /// Ошибка о пропущенном токене указывает на начало следующего значимого
    /// токена, а не в промежуток перед ним.
    ///
    /// Вход подобран так, чтобы нарушение было ВИДНО: ошибка подана до того,
    /// как тривия сбампана, — то есть ровно в положении, в котором её
    /// оставляет правило, потребовавшее токен и не снявшее пробел. Реализация,
    /// берущая смещение у лексемы под курсором, показывает пробел.
    #[test]
    fn a_missing_token_error_after_buffered_trivia_points_at_the_next_word() {
        let source = "А   Б";
        let tokens = lexer::tokenize(source);
        let events = vec![
            Event::Start { kind: NodeKind::SourceFile, forward_parent: None },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Error(unexpected(RecoveryKind::MissingToken)),
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Finish,
        ];

        let parse = Sink::new(&tokens).finish(events).finish();

        assert_eq!(parse.errors().len(), 1);
        assert_eq!(
            parse.errors()[0].range(),
            TextRange::empty(TextSize::new(tokens[2].offset as u32)),
            "ожидалось начало слова за промежутком, а не сам промежуток"
        );
    }

    /// Начало спана нормализуется ВПЕРЁД, к следующему слову.
    ///
    /// Правило направленное, и проверить его можно только входом, у которого
    /// маркер стоит на тривии: конец предыдущего слова — тоже граница
    /// значимого токена, поэтому свойство «смещение не внутри тривии»
    /// реализацию, нормализующую назад, пропустило бы.
    #[test]
    fn a_recovery_span_starting_on_trivia_begins_at_the_next_word() {
        let source = "А Б;";
        let tokens = lexer::tokenize(source);
        let events = vec![
            Event::Start { kind: NodeKind::SourceFile, forward_parent: None },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::ErrorWithSpan { start_token: 1, err: unexpected(RecoveryKind::RecoverySpan) },
            Event::Token { kind: lexer::TokenKind::Semicolon },
            Event::Finish,
        ];

        let parse = Sink::new(&tokens).finish(events).finish();

        assert_eq!(parse.errors().len(), 1);
        assert_eq!(
            parse.errors()[0].range(),
            range(tokens[2].offset as u32, tokens[3].offset as u32),
            "начало спана уехало назад, на конец предыдущего слова"
        );
    }

    /// Ошибка, потраченная на токен, показывается на последнем СЛОВЕ, а не на
    /// предыдущей лексеме потока.
    ///
    /// Вход с тривией между словом и ошибкой обязателен: пока лексема перед
    /// ошибкой значима, обе реализации дают один и тот же диапазон, и подмена
    /// «последнее слово» → «предыдущая лексема» остаётся невидимой.
    #[test]
    fn a_bump_token_error_after_trivia_points_at_the_last_word() {
        let source = "А   Б";
        let tokens = lexer::tokenize(source);
        let events = vec![
            Event::Start { kind: NodeKind::SourceFile, forward_parent: None },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Error(unexpected(RecoveryKind::BumpToken)),
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Finish,
        ];

        let parse = Sink::new(&tokens).finish(events).finish();

        assert_eq!(parse.errors().len(), 1);
        assert_eq!(
            parse.errors()[0].range(),
            range(tokens[0].offset as u32, tokens[0].text.len() as u32),
            "диапазон уехал на промежуток вместо слова перед ним"
        );
    }

    #[test]
    fn empty_token_stream_returns_clean_parse() {
        let parse = crate::parse("");

        assert!(!parse.has_errors());
        assert_eq!(parse.syntax_node().kind(), syntax::SyntaxKind::SOURCE_FILE);
    }

    #[test]
    fn error_with_span_ranges_from_start_token_to_current_token() {
        let source = "А Б;";
        let tokens = lexer::tokenize(source);
        let events = vec![
            Event::Start { kind: NodeKind::SourceFile, forward_parent: None },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::ErrorWithSpan { start_token: 0, err: unexpected(RecoveryKind::RecoverySpan) },
            Event::Token { kind: lexer::TokenKind::Semicolon },
            Event::Finish,
        ];

        let parse = Sink::new(&tokens).finish(events).finish();

        assert_eq!(parse.errors().len(), 1);
        assert_eq!(
            parse.errors()[0].range(),
            range(tokens[0].offset as u32, tokens[3].offset as u32)
        );
    }

    #[test]
    fn test_sink_with_bom() {
        let source = "\u{FEFF}Процедура Тест() КонецПроцедуры";
        let tokens = lexer::tokenize(source);

        eprintln!("=== Tokens with BOM ===");
        for (i, token) in tokens.iter().enumerate() {
            eprintln!("{}: {:?} @ {} = {:?}", i, token.kind, token.offset, token.text);
        }

        let mut parser = crate::Parser::new(&tokens);
        grammar::source_file(&mut parser);
        let events = parser.finish();

        let sink = Sink::new(&tokens);
        let builder = sink.finish(events);
        let parse = builder.finish();

        eprintln!("=== Syntax tree ===");
        eprintln!("{:#?}", parse.syntax_node());

        assert!(!parse.has_errors(), "File with BOM should parse without errors");
        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SOURCE_FILE);

        let error_nodes: Vec<_> =
            root.descendants().filter(|n| n.kind() == syntax::SyntaxKind::ERROR).collect();
        assert!(error_nodes.is_empty(), "Should have no ERROR nodes, found: {:?}", error_nodes);
    }

    #[test]
    fn test_sink_with_bom_and_region() {
        let source =
            "\u{FEFF}\r\n#Область Test\r\nПроцедура Тест()\r\nКонецПроцедуры\r\n#КонецОбласти";
        let tokens = lexer::tokenize(source);

        eprintln!("=== Tokens with BOM+CRLF+Region ===");
        for (i, token) in tokens.iter().enumerate() {
            eprintln!("{}: {:?} @ {} = {:?}", i, token.kind, token.offset, token.text);
        }

        let mut parser = crate::Parser::new(&tokens);
        grammar::source_file(&mut parser);
        let events = parser.finish();

        let sink = Sink::new(&tokens);
        let builder = sink.finish(events);
        let parse = builder.finish();

        eprintln!("=== Syntax tree ===");
        eprintln!("{:#?}", parse.syntax_node());

        assert!(!parse.has_errors(), "File with BOM+CRLF+Region should parse without errors");
        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SOURCE_FILE);

        let error_nodes: Vec<_> =
            root.descendants().filter(|n| n.kind() == syntax::SyntaxKind::ERROR).collect();
        assert!(error_nodes.is_empty(), "Should have no ERROR nodes, found: {:?}", error_nodes);
    }

    #[test]
    fn test_sink_multiple_variables() {
        let source = r#"
Перем Первая;
Перем Вторая Экспорт;
Перем Третья;
"#;
        eprintln!("=== Source ===\n{}", source);

        let tokens = lexer::tokenize(source);
        eprintln!("=== Tokens from lexer ===");
        for (i, token) in tokens.iter().enumerate() {
            eprintln!(
                "{}: {:?} @ {}..{} = {:?}",
                i,
                token.kind,
                token.offset,
                token.offset + token.text.len(),
                token.text
            );
        }

        let mut parser = crate::Parser::new(&tokens);
        grammar::source_file(&mut parser);
        let events = parser.finish();

        let sink = Sink::new(&tokens);
        let builder = sink.finish(events);
        let parse = builder.finish();

        eprintln!("=== Final syntax tree ===");
        eprintln!("{:#?}", parse.syntax_node());
    }
}
