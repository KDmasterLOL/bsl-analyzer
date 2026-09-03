//! Плита метода: целые строки файла, которыми метод владеет единолично.
//!
//! Строчные проверки судят строку, а не узел: длину строки, соседство токенов
//! на ней, серию комментариев на соседних строках. Узел метода своей строкой
//! не владеет — отступ, docstring и хвост после `КонецПроцедуры` принадлежат
//! родителю, — поэтому единица здесь строка, и каждая строка файла
//! принадлежит ровно одному владельцу: плите одного метода или остатку файла.
//!
//! Разметка ([`SlabLayout`]) — файловая: она знает номера строк и
//! пересчитывается на каждую ревизию. Плита ([`MethodSlab`]) — значение без
//! файловых координат: текст своих строк и ровно тот контекст соседей, от
//! которого зависит исход проверок. Неизменённый метод даёт равную плиту, и
//! всё, что читает плиту, остаётся стоять.

use std::sync::Arc;

use base_db::FileIdInput;
use line_index::LineIndex;
use rustc_hash::FxHashMap;
use syntax::{
    NodeOrToken, Parse, SyntaxKind, SyntaxNode, SyntaxToken, TextRange, TextSize, TokenAtOffset,
};

use crate::item_tree::ItemTree;
use crate::{DefDatabase, MethodIdInput, MethodKey};

/// Правила владения, каждое из которых можно выключить в тесте, чтобы
/// показать вход, на котором без него плиты судят иначе, чем файл.
///
/// Строка, пересекающая узлы двух методов, достаётся первому из них, серия
/// docstring над узлом — своему методу, а всё, что от этого могло бы
/// пересечь границу владельцев — серия комментариев или открытый
/// многострочный литерал, — снимает отщипывание. Отдельные правила для шва
/// двух узлов и для открытой сверху серии были в замысле, но контроль
/// показал, что без них ни один вход не судится иначе: контексты блока
/// делают разрез между любыми строками точным, а то, что блок обязан
/// видеть целиком, сводит к одному владельцу отщипывание.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules(u8);

impl Rules {
    /// Крайняя строка-комментарий плиты рядом с чужой строкой-комментарием —
    /// остаток, пока серия не перестанет пересекать границу.
    pub const PEEL_RUNS: Rules = Rules(1);
    /// Крайняя строка плиты, к концу которой (или к концу строки над ней)
    /// открыт многострочный литерал, — остаток: `//`-строку внутри литерала
    /// блок узнаёт только по открывшему её токену слева.
    pub const OPEN_LITERALS: Rules = Rules(2);
    pub const ALL: Rules = Rules(3);

    pub fn without(self, rule: Rules) -> Rules {
        Rules(self.0 & !rule.0)
    }

    fn has(self, rule: Rules) -> bool {
        self.0 & rule.0 != 0
    }
}

/// Сосед слева, от которого зависит исход строчной проверки: вид ближайшего
/// значимого токена перед блоком (`None` — блок начинает файл). Записывается
/// только когда первый значимый токен блока — знак `+`/`-` (унарный или
/// бинарный решает сосед) либо `;` (после хвоста строкового литерала он
/// длину строки не считает). Для любого другого первого токена исход от
/// соседа не зависит, и блок его не знает.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeadingContext {
    pub prev: Option<SyntaxKind>,
}

/// Строки плиты в файле и контекст, который войдёт в её значение.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlabSpan {
    pub first_line: u32,
    pub last_line: u32,
    pub leading: Option<LeadingContext>,
}

/// Отрезок соседних строк остатка и его контекст.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemainderBlock {
    pub first_line: u32,
    pub last_line: u32,
    pub leading: Option<LeadingContext>,
}

/// Владелец строки: индекс метода в порядке объявления либо остаток.
const REMAINDER: u32 = u32::MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlabLayout {
    spans: FxHashMap<MethodKey, SlabSpan>,
    remainder: Vec<RemainderBlock>,
    /// Индекс строк файла той же ревизии: плиты режут текст по нему, и
    /// строить его на каждый метод заново было бы O(файл × методы).
    line_index: LineIndex,
}

impl SlabLayout {
    pub fn compute(parse: &Parse<SyntaxNode>, item_tree: &ItemTree, text: &str) -> SlabLayout {
        Self::compute_with(parse, item_tree, text, Rules::ALL)
    }

    pub fn compute_with(
        parse: &Parse<SyntaxNode>,
        item_tree: &ItemTree,
        text: &str,
        rules: Rules,
    ) -> SlabLayout {
        let _span = tracing::info_span!("slab_layout").entered();
        let root = parse.syntax_node();
        let line_index = LineIndex::new(text);
        let len_lines = line_index.len_lines();
        let mut owner = vec![REMAINDER; len_lines as usize];

        let methods: Vec<(MethodKey, TextRange)> = item_tree
            .methods()
            .map(|item| (item.key(), item.source_range()))
            .filter(|(_, range)| !range.is_empty())
            .collect();

        let mut node_lines: Vec<(u32, u32)> = Vec::with_capacity(methods.len());
        for (index, (_, range)) in methods.iter().enumerate() {
            let first = line_index.line_col(range.start()).line;
            let last = line_index.line_col(range.end() - TextSize::from(1)).line;
            node_lines.push((first, last));
            // Строку, пересекающую два узла (`КонецПроцедуры Процедура Б()`),
            // берёт первый: одному из двух её всё равно не отдать, а исход от
            // выбора не зависит — блок судит строку целиком.
            for line in first..=last {
                let slot = &mut owner[line as usize];
                if *slot == REMAINDER {
                    *slot = index as u32;
                }
            }
        }

        let lines = LineFacts::collect(&root, &line_index);

        // Docstring: серия строк-комментариев прямо над узлом, не лежащих ни
        // в одном узле, — своему методу.
        let mut spans: Vec<Option<(u32, u32)>> = Vec::with_capacity(methods.len());
        for (index, &(first, last)) in node_lines.iter().enumerate() {
            if owner[first as usize] != index as u32 {
                // Первая строка узла занята предыдущим методом; плита
                // начинается ниже неё.
                let mut start = first;
                while start <= last && owner[start as usize] != index as u32 {
                    start += 1;
                }
                spans
                    .push((start <= last).then_some((start, slab_end(&owner, index, start, last))));
                continue;
            }
            let mut top = first;
            while top > 0 && owner[top as usize - 1] == REMAINDER && lines.has_comment(top - 1) {
                top -= 1;
            }
            for line in top..first {
                owner[line as usize] = index as u32;
            }
            spans.push(Some((top, slab_end(&owner, index, first, last))));
        }

        // Швы: крайняя строка плиты уходит в остаток, пока граница режет то,
        // что блок обязан видеть целиком, — серию комментариев (крайняя
        // строка-комментарий рядом с чужой строкой-комментарием) или открытый
        // многострочный литерал (литерал не закрыт к концу строки над
        // границей). Одного прохода по порядку объявления хватает: условия
        // зависят от текста и от того, чужая ли соседняя строка, а чужой она
        // стать может, своей — уже нет.
        let cuts_run = |lines: &LineFacts, above: u32, below: u32| {
            rules.has(Rules::PEEL_RUNS) && lines.has_comment(above) && lines.has_comment(below)
        };
        let cuts_literal = |lines: &LineFacts, above: u32| {
            rules.has(Rules::OPEN_LITERALS) && lines.open_after(above)
        };
        for (index, span) in spans.iter_mut().enumerate() {
            let Some((mut first, mut last)) = *span else { continue };
            let me = index as u32;
            while first <= last
                && first > 0
                && owner[first as usize - 1] != me
                && (cuts_run(&lines, first - 1, first) || cuts_literal(&lines, first - 1))
            {
                owner[first as usize] = REMAINDER;
                first += 1;
            }
            while first <= last
                && last + 1 < len_lines
                && owner[last as usize + 1] != me
                && (cuts_run(&lines, last, last + 1) || cuts_literal(&lines, last))
            {
                owner[last as usize] = REMAINDER;
                last -= 1;
            }
            *span = (first <= last).then_some((first, last));
        }

        let mut slab_spans = FxHashMap::default();
        for (index, span) in spans.iter().enumerate() {
            let Some((first_line, last_line)) = *span else { continue };
            slab_spans.insert(
                methods[index].0,
                SlabSpan { first_line, last_line, leading: lines.leading_context(first_line) },
            );
        }
        let mut remainder = Vec::new();
        let mut line = 0u32;
        while line < len_lines {
            if owner[line as usize] != REMAINDER {
                line += 1;
                continue;
            }
            let first_line = line;
            while line + 1 < len_lines && owner[line as usize + 1] == REMAINDER {
                line += 1;
            }
            remainder.push(RemainderBlock {
                first_line,
                last_line: line,
                leading: lines.leading_context(first_line),
            });
            line += 1;
        }
        SlabLayout { spans: slab_spans, remainder, line_index }
    }

    pub fn span(&self, key: MethodKey) -> Option<SlabSpan> {
        self.spans.get(&key).copied()
    }

    pub fn spans(&self) -> impl Iterator<Item = (MethodKey, SlabSpan)> + '_ {
        self.spans.iter().map(|(key, span)| (*key, *span))
    }

    pub fn remainder(&self) -> &[RemainderBlock] {
        &self.remainder
    }

    pub fn line_index(&self) -> &LineIndex {
        &self.line_index
    }
}

/// Последняя строка плиты: конец узла, укороченный сверху, если хвост узла
/// отдан шву.
fn slab_end(owner: &[u32], index: usize, first: u32, last: u32) -> u32 {
    let mut end = last;
    while end > first && owner[end as usize] != index as u32 {
        end -= 1;
    }
    end
}

/// Факт о строке, снятый обходом остатка дерева; строки внутри узлов методов
/// обход не видит, и про них спрашивают дерево по месту.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fact {
    No,
    Yes,
    Unknown,
}

/// Факты о строках файла, нужные разметке, за один обход остатка дерева —
/// токенов вне узлов методов; узел метода в этом обходе один элемент со
/// своим первым и последним токеном. Разметка спрашивает только строки у
/// границ плит, и всё, что там снаружи узлов, обход уже видел; строку внутри
/// узла (после отщипывания заголовка) достаёт дерево по месту.
struct LineFacts<'a> {
    root: &'a SyntaxNode,
    line_index: &'a LineIndex,
    /// На строке есть токен `COMMENT`.
    comment: Vec<Fact>,
    /// К концу строки открыт многострочный литерал: последний значимый токен
    /// до её конца — `STRING_START` или `STRING_PART`.
    open_after: Vec<Fact>,
    /// Первый токен, начинающийся на строке, — с него читают первый значимый
    /// токен блока и его соседа слева.
    first_token: Vec<Option<SyntaxToken>>,
}

impl<'a> LineFacts<'a> {
    fn collect(root: &'a SyntaxNode, line_index: &'a LineIndex) -> LineFacts<'a> {
        let len = line_index.len_lines() as usize;
        let mut facts = LineFacts {
            root,
            line_index,
            comment: vec![Fact::No; len],
            open_after: vec![Fact::Unknown; len],
            first_token: vec![None; len],
        };
        let line_of = |offset: TextSize| line_index.line_col(offset).line as usize;
        // Последний значимый токен, увиденный обходом; `None` — ещё ни одного.
        let mut last_significant: Option<SyntaxKind> = None;
        let mut preorder = root.preorder_with_tokens();
        while let Some(event) = preorder.next() {
            let syntax::WalkEvent::Enter(element) = event else { continue };
            match element {
                NodeOrToken::Node(node) if is_method_node(&node) => {
                    preorder.skip_subtree();
                    let (Some(first), Some(last)) = (node.first_token(), node.last_token()) else {
                        continue;
                    };
                    let first_line = line_of(first.text_range().start());
                    facts.first_token[first_line].get_or_insert(first.clone());
                    // Комментарий в заголовке — внутри узла; обходу он не
                    // виден, а отщипыванию нужен.
                    if std::iter::successors(Some(first), |t| t.next_token())
                        .take_while(|t| t.kind() != SyntaxKind::NEWLINE)
                        .any(|t| t.kind() == SyntaxKind::COMMENT)
                    {
                        facts.comment[first_line] = Fact::Yes;
                    }
                    // Последняя строка узла начинается внутри него: её первый
                    // токен и комментарий обходу тоже не видны, а блок
                    // остатка может начаться с неё после отщипывания.
                    let last_line = line_of(last.text_range().start());
                    if last_line != first_line {
                        let line_start = line_index.line_start(last_line as u32);
                        let mut first_on_line = last.clone();
                        for token in std::iter::successors(Some(last.clone()), |t| t.prev_token())
                            .take_while(|t| t.text_range().start() >= line_start)
                        {
                            if token.kind() == SyntaxKind::COMMENT {
                                facts.comment[last_line] = Fact::Yes;
                            }
                            first_on_line = token;
                        }
                        facts.first_token[last_line].get_or_insert(first_on_line);
                    }
                    last_significant = std::iter::successors(Some(last), |t| t.prev_token())
                        .find(|t| !t.kind().is_trivia())
                        .map(|t| t.kind());
                }
                NodeOrToken::Node(_) => {}
                NodeOrToken::Token(token) => {
                    let line = line_of(token.text_range().start());
                    facts.first_token[line].get_or_insert(token.clone());
                    match token.kind() {
                        SyntaxKind::COMMENT => facts.comment[line] = Fact::Yes,
                        SyntaxKind::NEWLINE => {
                            facts.open_after[line] = open_literal(last_significant);
                        }
                        kind if !kind.is_trivia() => last_significant = Some(kind),
                        _ => {}
                    }
                }
            }
        }
        if let Some(last) = facts.open_after.last_mut() {
            if *last == Fact::Unknown {
                *last = open_literal(last_significant);
            }
        }
        facts
    }

    fn has_comment(&self, line: u32) -> bool {
        match self.comment[line as usize] {
            Fact::Yes => true,
            Fact::No if self.first_token[line as usize].is_some() => false,
            _ => self.tokens_of_line(line).any(|token| token.kind() == SyntaxKind::COMMENT),
        }
    }

    /// К концу `line` открыт многострочный литерал.
    fn open_after(&self, line: u32) -> bool {
        match self.open_after[line as usize] {
            Fact::Yes => true,
            Fact::No => false,
            Fact::Unknown => {
                let end = self.line_index.line_range(line).map_or(TextSize::new(0), |r| r.end());
                let last = match self.root.token_at_offset(end) {
                    TokenAtOffset::Single(token) => Some(token),
                    TokenAtOffset::Between(left, _) => Some(left),
                    TokenAtOffset::None => None,
                };
                let significant = std::iter::successors(last, |t| t.prev_token())
                    .find(|t| !t.kind().is_trivia() && t.text_range().start() < end);
                open_literal(significant.map(|t| t.kind())) == Fact::Yes
            }
        }
    }

    /// Первый токен, начинающийся не раньше `offset`; за концом текста
    /// токенов нет.
    fn token_from(&self, offset: TextSize) -> Option<SyntaxToken> {
        if offset >= self.root.text_range().end() {
            return None;
        }
        match self.root.token_at_offset(offset) {
            TokenAtOffset::Single(token) => Some(token),
            TokenAtOffset::Between(_, right) => Some(right),
            TokenAtOffset::None => None,
        }
    }

    fn first_token_of_line(&self, line: u32) -> Option<SyntaxToken> {
        self.first_token[line as usize]
            .clone()
            .or_else(|| self.token_from(self.line_index.line_start(line)))
    }

    fn tokens_of_line(&self, line: u32) -> impl Iterator<Item = SyntaxToken> + '_ {
        let range = self.line_index.line_range(line).unwrap_or(TextRange::empty(0.into()));
        std::iter::successors(self.first_token_of_line(line), |token| token.next_token())
            .take_while(move |token| token.text_range().start() < range.end())
    }

    fn leading_context(&self, first_line: u32) -> Option<LeadingContext> {
        let first_significant =
            std::iter::successors(self.first_token_of_line(first_line), |token| token.next_token())
                .find(|t| !t.kind().is_trivia())?;
        if !matches!(
            first_significant.kind(),
            SyntaxKind::PLUS | SyntaxKind::MINUS | SyntaxKind::SEMICOLON
        ) {
            return None;
        }
        let prev =
            std::iter::successors(first_significant.prev_token(), |token| token.prev_token())
                .find(|t| !t.kind().is_trivia())
                .map(|t| t.kind());
        Some(LeadingContext { prev })
    }
}

fn is_method_node(node: &SyntaxNode) -> bool {
    matches!(node.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF)
}

fn open_literal(last_significant: Option<SyntaxKind>) -> Fact {
    match last_significant {
        Some(SyntaxKind::STRING_START | SyntaxKind::STRING_PART) => Fact::Yes,
        _ => Fact::No,
    }
}

/// Значение плиты: текст её строк с переводами строк и только тот контекст
/// соседей, без которого исход проверок не определён. Строки под плитой в
/// нём нет: единственный взгляд вниз — за продолжением строкового литерала
/// (`"`/`|` первым символом), а край плиты, после которого литерал открыт,
/// разметка отщипывает в остаток вместе с продолжением
/// (`Rules::OPEN_LITERALS`), так что на последней строке плиты литерал
/// всегда закрыт.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSlab {
    pub text: Arc<str>,
    pub leading: Option<LeadingContext>,
}

fn slab_layout_heap(v: &Arc<SlabLayout>) -> usize {
    v.spans.len() * std::mem::size_of::<(MethodKey, SlabSpan)>()
        + v.remainder.len() * std::mem::size_of::<RemainderBlock>()
        + v.line_index.estimated_heap()
}

#[salsa::tracked(lru = 128, heap_size = slab_layout_heap, returns(ref))]
pub fn module_slab_layout_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<SlabLayout> {
    let file_id = file_id_input.file_id(db);
    let parse = db.parse_ref(file_id);
    let item_tree = db.item_tree_ref(file_id);
    let text = db.file_text_ref(file_id);
    Arc::new(SlabLayout::compute(parse, item_tree, text))
}

fn method_slab_heap(v: &Option<Arc<MethodSlab>>) -> usize {
    v.as_ref().map_or(0, |slab| slab.text.len())
}

// Удерживается на метод, как `method_syntax`: мемо строчных диагностик,
// вытесненное ниже плиты, не должно тянуть разметку файла обратно.
#[salsa::tracked(lru = 8192, heap_size = method_slab_heap, returns(ref))]
pub fn method_slab_query<'db>(
    db: &'db dyn DefDatabase,
    method: MethodIdInput<'db>,
) -> Option<Arc<MethodSlab>> {
    let mid = method.method_id(db);
    let file_id = mid.module.file_id;
    let _span =
        tracing::info_span!("method_slab", file_id = file_id.0, local_id = ?mid.local_id).entered();
    let layout = module_slab_layout_query(db, FileIdInput::new(db, file_id));
    let span = layout.span(mid.local_id)?;
    let text = db.file_text_ref(file_id);
    Some(Arc::new(slab_of(text, layout.line_index(), span)))
}

/// Текст плиты — строки `first_line..=last_line` вместе с их переводами
/// строк.
pub fn slab_of(text: &str, line_index: &LineIndex, span: SlabSpan) -> MethodSlab {
    let start = line_index.line_start(span.first_line);
    let end = line_index.try_line_start(span.last_line + 1).unwrap_or(line_index.text_len());
    MethodSlab { text: Arc::from(&text[TextRange::new(start, end)]), leading: span.leading }
}

/// Retention cap of `method_slab_query`; see `set_lowering_lru_sweep_mode`.
pub(crate) fn set_lru_capacity(db: &mut dyn DefDatabase, cap: usize) {
    method_slab_query::set_lru_capacity(db, cap);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_with(code: &str, rules: Rules) -> SlabLayout {
        let parse = parser::parse_with_shared_cache(code);
        let item_tree = ItemTree::from_parse(&parse);
        SlabLayout::compute_with(&parse, &item_tree, code, rules)
    }

    fn layout(code: &str) -> SlabLayout {
        layout_with(code, Rules::ALL)
    }

    fn span(layout: &SlabLayout, name: &str) -> Option<(u32, u32)> {
        layout.span(MethodKey::first(name)).map(|s| (s.first_line, s.last_line))
    }

    fn remainder(layout: &SlabLayout) -> Vec<(u32, u32)> {
        layout.remainder().iter().map(|b| (b.first_line, b.last_line)).collect()
    }

    #[test]
    fn docstring_and_closer_belong_to_the_slab_and_gaps_to_the_remainder() {
        let code = "Перем В;\n\n// Описание А\n// Параметры:\n&НаСервере\nПроцедура А()\n\tХ = 1;\nКонецПроцедуры // хвост\n\nПроцедура Б()\nКонецПроцедуры\n";
        let layout = layout(code);
        assert_eq!(span(&layout, "А"), Some((2, 7)));
        assert_eq!(span(&layout, "Б"), Some((9, 10)));
        assert_eq!(remainder(&layout), vec![(0, 1), (8, 8), (11, 11)]);
    }

    #[test]
    fn a_line_shared_by_two_nodes_goes_to_the_first() {
        let code = "Процедура А()\nКонецПроцедуры Процедура Б()\nКонецПроцедуры\n";
        let layout = layout(code);
        assert_eq!(span(&layout, "А"), Some((0, 1)));
        assert_eq!(span(&layout, "Б"), Some((2, 2)));
        assert_eq!(remainder(&layout), vec![(3, 3)]);
    }

    #[test]
    fn a_run_across_the_border_is_peeled_to_the_remainder_whole() {
        let code =
            "Процедура А()\nКонецПроцедуры // x\n// Описание Б\nПроцедура Б()\nКонецПроцедуры\n";
        let layout = layout(code);
        assert_eq!(span(&layout, "А"), Some((0, 0)), "хвостовая строка А отщипнута к серии");
        assert_eq!(span(&layout, "Б"), Some((3, 4)));
        assert_eq!(remainder(&layout), vec![(1, 2), (5, 5)]);

        let without = layout_with(code, Rules::ALL.without(Rules::PEEL_RUNS));
        assert_eq!(span(&without, "Б"), Some((2, 4)));
        assert_eq!(span(&without, "А"), Some((0, 1)));
    }

    #[test]
    fn a_comment_on_the_header_next_to_a_foreign_comment_is_peeled() {
        let code = "Процедура А()\nКонецПроцедуры // x\n// между\nПроцедура Б() // y\n\tЗ = 1;\nКонецПроцедуры\n";
        let layout = layout(code);
        assert_eq!(span(&layout, "Б"), Some((4, 5)));
        assert_eq!(remainder(&layout), vec![(1, 3), (6, 6)]);
        let leading = layout.span(MethodKey::first("Б")).unwrap().leading;
        assert_eq!(leading, None, "первый значимый токен плиты — не знак");
    }

    #[test]
    fn a_leading_sign_records_its_left_neighbour() {
        let code = "Процедура А()\nКонецПроцедуры // x\n// между\nПроцедура Б() // y\n\t-1;\nКонецПроцедуры\n";
        let layout = layout(code);
        let leading = layout.span(MethodKey::first("Б")).unwrap().leading;
        assert_eq!(leading, Some(LeadingContext { prev: Some(SyntaxKind::R_PAREN) }));
    }

    #[test]
    fn a_literal_open_across_the_border_is_peeled_to_the_remainder() {
        let code = "Процедура А()\n\tХ = 1;\nКонецПроцедуры Т = \"а\n// Х = 2;\n|в\";\n";
        let layout = layout(code);
        assert_eq!(span(&layout, "А"), Some((0, 1)));
        assert_eq!(remainder(&layout), vec![(2, 5)]);
        let without = layout_with(code, Rules::ALL.without(Rules::OPEN_LITERALS));
        assert_eq!(span(&without, "А"), Some((0, 2)));
    }

    #[test]
    fn the_slab_value_carries_whole_lines_and_no_positions() {
        let code = "Перем В;\n// Описание\nПроцедура А()\n\tХ = 1;\nКонецПроцедуры\n\nПроцедура Б()\nКонецПроцедуры\n";
        let moved = format!("Перем В;\nПерем Г;\n{code}");
        let slab = |text: &str| {
            let layout = layout(text);
            slab_of(text, &LineIndex::new(text), layout.span(MethodKey::first("А")).unwrap())
        };
        let (a, b) = (slab(code), slab(&moved));
        assert_eq!(a, b);
        assert_eq!(&*a.text, "// Описание\nПроцедура А()\n\tХ = 1;\nКонецПроцедуры\n");
    }

    #[test]
    fn methods_inside_preprocessor_blocks_are_laid_out_like_any_other() {
        let code = "#Если Сервер Тогда\n// Описание\nПроцедура А()\nКонецПроцедуры\n#КонецЕсли\n";
        let layout = layout(code);
        assert_eq!(span(&layout, "А"), Some((1, 3)));
        assert_eq!(remainder(&layout), vec![(0, 0), (4, 5)]);
    }

    #[test]
    fn crlf_and_missing_final_newline() {
        let code = "// Описание\r\nПроцедура А()\r\nКонецПроцедуры";
        let layout = layout(code);
        assert_eq!(span(&layout, "А"), Some((0, 2)));
        assert_eq!(remainder(&layout), vec![]);
        let slab =
            slab_of(code, &LineIndex::new(code), layout.span(MethodKey::first("А")).unwrap());
        assert_eq!(&*slab.text, code);
    }
}
