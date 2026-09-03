//! Серии комментариев: строки `//`, стоящие на соседних строках.
//!
//! Правила серии нужны и складке `foldingRange`, и диагностике
//! закомментированного кода, но требования у них разные: складке нужен
//! комментарий, владеющий своей строкой, диагностике — все подряд идущие,
//! включая хвостовой на строке с кодом. Поэтому примитив не решает за
//! потребителя, а отдаёт факты о тексте — соседство строк и владение строкой.
//!
//! Соседство выражается через токены `NEWLINE`, а не через индекс строк: обход
//! дерева и так идёт по тексту, и номера строк тут ничего не добавляют.

use crate::{LineToken, NodeOrToken, SyntaxKind, SyntaxNode, TextRange};

/// Одна строка серии: диапазон токена `COMMENT` в тексте, откуда серия взята.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentLine {
    pub range: TextRange,
    /// `//` — первый непробельный символ своей строки.
    pub owns_line: bool,
}

/// Комментарии, стоящие на соседних строках.
#[derive(Debug, Clone)]
pub struct CommentRun {
    lines: Vec<CommentLine>,
    range: TextRange,
}

impl CommentRun {
    pub fn lines(&self) -> &[CommentLine] {
        &self.lines
    }

    /// От начала первого комментария до конца последнего; строка ниже серии не
    /// захватывается, иначе диапазон пересёкся бы с объявлением под шапкой.
    pub fn range(&self) -> TextRange {
        self.range
    }
}

/// Все серии комментариев дерева в порядке текста.
///
/// Считается по токенам дерева тем же правилом, что и [`comment_runs_of`] по
/// токенам текста: у серии нет свойства, которого не было бы у потока
/// токенов, и два способа получить поток обязаны давать одну серию.
pub fn comment_runs(root: &SyntaxNode) -> Vec<CommentRun> {
    let tokens: Vec<LineToken> = root
        .descendants_with_tokens()
        .filter_map(|element| match element {
            NodeOrToken::Token(token) => {
                Some(LineToken { kind: token.kind(), range: token.text_range() })
            }
            NodeOrToken::Node(_) => None,
        })
        .collect();
    comment_runs_of(&tokens)
}

/// Серии комментариев по токенам текста без дерева: для блока строк,
/// разобранного лексером отдельно от файла.
pub fn comment_runs_of(tokens: &[LineToken]) -> Vec<CommentRun> {
    collect_runs(
        tokens
            .iter()
            .enumerate()
            .map(|(index, token)| (token.kind, token.range, is_string_text_at(tokens, index))),
    )
}

fn collect_runs(tokens: impl Iterator<Item = (SyntaxKind, TextRange, bool)>) -> Vec<CommentRun> {
    let mut runs = Vec::new();
    let mut current: Vec<CommentLine> = Vec::new();
    let mut at_line_start = true;
    let mut newlines = 0usize;

    for (kind, range, string_text) in tokens {
        match kind {
            SyntaxKind::NEWLINE => {
                newlines += 1;
                at_line_start = true;
            }
            // Отступ строку не начинает и не кончает; `\r` в CRLF приходит
            // отдельным пробельным токеном ровно на пустой строке.
            SyntaxKind::WHITESPACE | SyntaxKind::BOM => {}
            SyntaxKind::COMMENT => {
                // Строку многострочного литерала, начинающуюся с `//`, лексер
                // отдаёт токеном комментария; комментарием она от этого не
                // становится. Счётчик строк не сбрасываем — иначе серия
                // перепрыгнула бы через такую строку.
                if string_text {
                    at_line_start = false;
                    continue;
                }
                if !current.is_empty() && newlines != 1 {
                    push_run(&mut runs, std::mem::take(&mut current));
                }
                current.push(CommentLine { owns_line: at_line_start, range });
                at_line_start = false;
                newlines = 0;
            }
            // Код серию не закрывает: комментарий кончается вместе со строкой,
            // поэтому между комментариями соседних строк может стоять только
            // код той же строки, а строка кода между сериями и так даёт два
            // `NEWLINE`.
            _ => at_line_start = false,
        }
    }

    push_run(&mut runs, current);
    runs
}

/// Комментарий между частями одного многострочного литерала — текст строки,
/// а не комментарий: слева его открывает `STRING_START` или продолжает
/// `STRING_PART`, справа продолжает `STRING_PART` или закрывает
/// `STRING_TAIL`. Комментарий за незакрытой строкой, за которой литерал не
/// продолжен, — настоящий комментарий: строка уже оборвана, и дальше идёт
/// не её текст. Правило лексическое, потому что судить его приходится и там,
/// где дерева нет, — по токенам одного блока строк.
fn is_string_text_at(tokens: &[LineToken], index: usize) -> bool {
    let left = tokens[..index].iter().rev().map(|t| t.kind).find(|k| !k.is_trivia());
    let right = tokens[index + 1..].iter().map(|t| t.kind).find(|k| !k.is_trivia());
    matches!(left, Some(SyntaxKind::STRING_START | SyntaxKind::STRING_PART))
        && matches!(right, Some(SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL))
}

fn push_run(runs: &mut Vec<CommentRun>, lines: Vec<CommentLine>) {
    let range = match (lines.first(), lines.last()) {
        (Some(first), Some(last)) => TextRange::new(first.range.start(), last.range.end()),
        _ => return,
    };
    runs.push(CommentRun { lines, range });
}
