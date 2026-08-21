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

use crate::{NodeOrToken, SyntaxKind, SyntaxNode, SyntaxToken, TextRange};

/// Одна строка серии.
#[derive(Debug, Clone)]
pub struct CommentLine {
    pub token: SyntaxToken,
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
pub fn comment_runs(root: &SyntaxNode) -> Vec<CommentRun> {
    let mut runs = Vec::new();
    let mut current: Vec<CommentLine> = Vec::new();
    let mut at_line_start = true;
    let mut newlines = 0usize;

    for element in root.descendants_with_tokens() {
        let NodeOrToken::Token(token) = element else { continue };
        match token.kind() {
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
                if token.parent().map(|parent| parent.kind()) == Some(SyntaxKind::LITERAL) {
                    at_line_start = false;
                    continue;
                }
                if !current.is_empty() && newlines != 1 {
                    push_run(&mut runs, std::mem::take(&mut current));
                }
                current.push(CommentLine { owns_line: at_line_start, token });
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

fn push_run(runs: &mut Vec<CommentRun>, lines: Vec<CommentLine>) {
    let range = match (lines.first(), lines.last()) {
        (Some(first), Some(last)) => {
            TextRange::new(first.token.text_range().start(), last.token.text_range().end())
        }
        _ => return,
    };
    runs.push(CommentRun { lines, range });
}
