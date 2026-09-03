use crate::define_metadata;
use crate::metadata::*;
use crate::slab::{self, Block};
use crate::{AnalysisContext, Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use hir::LocalRange;
use ide_db::TextRange;
use lexer::{tokenize, Token, TokenKind};
use syntax::{CommentLine, CommentRun};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

#[derive(Debug, Clone)]
struct Config {
    exclusion_prefixes: Vec<String>,
}

impl Config {
    fn from_context(ctx: &AnalysisContext) -> Self {
        let exclusion_prefixes_str =
            ctx.config_string(DiagnosticCode::CommentedCode, "exclusionPrefixes", "");

        let exclusion_prefixes: Vec<String> = exclusion_prefixes_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Self { exclusion_prefixes }
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    slab::check_file_by_blocks(ctx, check_block)
}

pub fn check_block(ctx: &AnalysisContext, block: &Block) -> Vec<Diagnostic<LocalRange>> {
    let _span = tracing::debug_span!("CommentedCode::check").entered();
    let code = DiagnosticCode::CommentedCode;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let config = Config::from_context(ctx);
    let text = block.text;

    // Владения строкой диагностика не требует: комментарий за кодом — такой же
    // кандидат в закомментированный код, как стоящий на своей строке.
    for run in syntax::comment_runs_of(block.tokens) {
        if is_commented_code(&run, text, &config) {
            // Deleting commented-out code is destructive and easy to regret, so it is an
            // opt-in quick fix, never part of an unattended `source.fixAll` sweep. The whole
            // commented line(s) are removed, but only when they hold nothing but the
            // comments (no real code shares them — see `deletable_lines_range`).
            let fixes = deletable_lines_range(text, &run)
                .map(|delete_range| {
                    Fix::manual(
                        "Удалить закомментированный код",
                        vec![TextEdit {
                            range: LocalRange::of_detached_node(delete_range),
                            new_text: String::new(),
                        }],
                    )
                })
                .into_iter()
                .collect();
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::CommentedCode,
                message: message_ru(),
                range: LocalRange::of_detached_node(run.range()),
                severity: ctx.severity(code),
                tags: ctx.tags(code),
                fixes,
            });
        }
    }

    diagnostics
}

/// The full lines a comment group occupies — indentation and trailing newline included —
/// but only when the group is the sole content of those lines. Returns `None` if any real
/// code shares a line (before, between, or after the comments), so deleting never drops it.
fn deletable_lines_range(text: &str, run: &CommentRun) -> Option<TextRange> {
    let start: usize = run.range().start().into();
    let end: usize = run.range().end().into();
    let line_start = text[..start].rfind('\n').map_or(0, |nl| nl + 1);
    let line_end = text[end..].find('\n').map_or(text.len(), |nl| end + nl + 1);

    // Everything in the region that is not a comment token must be whitespace.
    let mut cursor = line_start;
    for line in run.lines() {
        let token_start: usize = line.range.start().into();
        if !text[cursor..token_start].trim().is_empty() {
            return None;
        }
        cursor = line.range.end().into();
    }
    if !text[cursor..line_end].trim().is_empty() {
        return None;
    }

    Some(TextRange::new((line_start as u32).into(), (line_end as u32).into()))
}

/// Strips the leading `//` markers from a single comment line's text.
fn comment_body<'a>(text: &'a str, line: &CommentLine) -> &'a str {
    text[line.range].trim_start_matches('/')
}

fn message_ru() -> String {
    "Программные модули не должны иметь закомментированных фрагментов кода".to_string()
}

/// Decides whether a group of consecutive `//` comments is commented-out BSL
/// code (as opposed to prose, structured documentation, or data).
///
/// A group is flagged when at least one of its lines has the syntactic shape of
/// a BSL statement and the group does not carry a recognised documentation
/// marker. Reporting the whole group range keeps a single finding aligned with
/// the block a developer would delete.
fn is_commented_code(run: &CommentRun, text: &str, config: &Config) -> bool {
    if run.lines().is_empty() {
        return false;
    }

    // Documentation blocks (`Параметры:`, `Возвращаемое значение:`, …) describe
    // an API; the structured parameter lines below such markers frequently end
    // in `;` and read like code without being executable.
    if is_documentation_block(run, text) {
        return false;
    }

    // A group whose first non-empty line is structured data (XML/HTML/JSON) and
    // where data lines form the majority is a commented-out data block, not
    // commented-out code.  The first-line guard prevents a group that opens with
    // a real assignment and continues with HTML fragments from being suppressed.
    if group_is_commented_data(run, text) {
        return false;
    }

    // The SQL text-block continuation prefix (`|`) only marks code when the same
    // group also opens a real query (`ВЫБРАТЬ`, `SELECT`, …); a lone `| прозаичный
    // хвост` is just a boxed comment, not a commented-out query.
    let group_has_query = run.lines().iter().any(|line| line_opens_query(comment_body(text, line)));

    run.lines().iter().any(|line| line_is_code(comment_body(text, line), config, group_has_query))
}

const DOC_MARKERS: &[&str] = &[
    "параметры:",
    "возвращаемое значение:",
    "возвращаемоезначение:",
    "пример:",
    "описание:",
    "parameters:",
    "returns:",
    "return value:",
    "example:",
    "description:",
];

fn is_documentation_block(run: &CommentRun, text: &str) -> bool {
    run.lines().iter().any(|line| {
        let lowered = comment_body(text, line).trim().to_lowercase();
        DOC_MARKERS.iter().any(|marker| lowered.starts_with(marker))
    })
}

/// True when a group is a commented-out data block (XML, HTML, JSON) rather
/// than commented-out code.  Two conditions must both hold:
///   1. The first non-empty comment line is itself data (so a group that opens
///      with a real statement and continues with HTML fragments is not skipped).
///   2. Data lines form a strict majority (> half) of all non-empty lines.
fn group_is_commented_data(run: &CommentRun, text: &str) -> bool {
    let bodies: Vec<&str> = run
        .lines()
        .iter()
        .map(|line| comment_body(text, line).trim())
        .filter(|s| !s.is_empty())
        .collect();

    if bodies.is_empty() {
        return false;
    }

    if !looks_like_commented_data(bodies[0]) {
        return false;
    }

    let data_count = bodies.iter().filter(|s| looks_like_commented_data(s)).count();
    if data_count * 2 <= bodies.len() {
        return false;
    }

    // A group that contains any line ending with `;` has at least one genuine
    // BSL statement in it, so it must not be suppressed wholesale.  Data
    // blocks (XML, HTML, JSON, BNF) never end lines with a semicolon.
    if bodies.iter().any(|s| s.ends_with(';')) {
        return false;
    }

    true
}

/// Classifies a single comment line (body already stripped of `//`) as having
/// the syntactic shape of a BSL statement.
fn line_is_code(body: &str, config: &Config, group_has_query: bool) -> bool {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return false;
    }

    for prefix in &config.exclusion_prefixes {
        if trimmed.starts_with(prefix) {
            return false;
        }
    }

    let lowered = trimmed.to_lowercase();
    if DOC_MARKERS.iter().any(|marker| lowered.starts_with(marker)) {
        return false;
    }

    // High-precision prose veto: natural-language text routinely places two bare
    // nouns next to each other ("тип свойства", "выбора группы"), which real BSL
    // never does — there is always a `.`, `(`, `,`, `=`, or operator between two
    // names. This runs before any keyword signal so a sentence that merely starts
    // with `Если`/`Попытка`/`Возврат` cannot be mistaken for a statement.
    //
    // Tokenize once here and thread the slice into the downstream helpers so the
    // lexer is not invoked multiple times for the same text.
    let toks = code_tokens(trimmed);
    if has_two_adjacent_idents(&toks) {
        return false;
    }

    // Commented-out data (JSON objects, HTML/markup) is not commented-out code.
    // Keyed on markup/JSON shape, not on the bare `<`/`>` characters, so a real
    // comparison such as `ТипЗнч(...) <> Тип(...)` is still recognised as code.
    if looks_like_commented_data(trimmed) {
        return false;
    }

    strong_statement(trimmed, &toks, group_has_query)
}

/// True when two consecutive non-trivia tokens are both bare identifiers (each is
/// `Ident`, i.e. neither a keyword nor punctuation/operator/number/string). This
/// is the natural-language prose signature.
fn has_two_adjacent_idents(toks: &[Token]) -> bool {
    toks.windows(2).any(|w| w[0].kind == TokenKind::Ident && w[1].kind == TokenKind::Ident)
}

/// True when the (uncommented) line is data rather than BSL: a JSON object/array
/// fragment or an HTML/XML/markup tag. Conservative on purpose — it keys on
/// markup shape so BSL using `<`/`>` as comparison operators is never suppressed.
fn looks_like_commented_data(trimmed: &str) -> bool {
    let body = trimmed.trim_start_matches('|').trim_start();
    let first = body.chars().next();

    // JSON object/array braces, or a `"key": value` pair.
    if matches!(first, Some('{') | Some('}')) {
        return true;
    }
    if is_json_pair(body) {
        return true;
    }

    // Opening/closing markup tag: `<tag`, `</tag`, `<?xml`, `<!DOCTYPE`,
    // `<Формула>` (Cyrillic).  A BSL statement never begins with `<`, so any
    // `<` followed by `/`, `?`, `!`, or any Unicode letter is data.
    if first == Some('<') {
        let after = body[1..].trim_start();
        let name = after.trim_start_matches('/');
        if let Some(c) = name.chars().next() {
            if c == '?' || c == '!' || c.is_alphabetic() {
                return true;
            }
        }
    }

    false
}

/// Recognises a JSON member line of the form `"name": value`.
fn is_json_pair(body: &str) -> bool {
    let bytes = body.as_bytes();
    if bytes.first() != Some(&b'"') {
        return false;
    }
    let mut idx = 1;
    while idx < bytes.len() && bytes[idx] != b'"' {
        idx += 1;
    }
    if idx >= bytes.len() {
        return false;
    }
    body[idx + 1..].trim_start().starts_with(':')
}

const QUERY_KW: &[&str] = &[
    "выбрать",
    "select",
    "из",
    "from",
    "где",
    "where",
    "сгруппировать",
    "group",
    "упорядочить",
    "order",
    "объединить",
    "union",
    "имеющие",
    "having",
];

/// True when a `|`-prefixed text-block line opens a BSL query (`ВЫБРАТЬ`, …),
/// marking the surrounding block as a commented-out query rather than a boxed
/// prose comment.
fn line_opens_query(body: &str) -> bool {
    let trimmed = body.trim();
    let Some(rest) = trimmed.strip_prefix('|') else {
        return false;
    };
    let lowered = rest.trim_start().to_lowercase();
    QUERY_KW.iter().any(|kw| {
        lowered
            .strip_prefix(kw)
            .is_some_and(|tail| tail.is_empty() || tail.starts_with(|c: char| c.is_whitespace()))
    })
}

/// Returns the BSL tokens of a comment line, minus whitespace.
///
/// The set is whitespace alone, deliberately, and not `TokenKind::is_trivia`:
/// what is being tokenized is the body of a comment, so a second `//` in it
/// is code commented twice — `// //Модуль.Метод(Данные);` — and it swallows
/// the rest of the line into one `Comment` token. Dropping that token leaves
/// the line with nothing to judge, and the check stops seeing exactly the
/// thing it looks for. Measured on real modules: of 96 comment lines carrying
/// a second `//`, widening the set loses that one verdict and changes no
/// other.
fn code_tokens(text: &str) -> Vec<Token> {
    tokenize(text).into_iter().filter(|t| t.kind != TokenKind::Whitespace).collect()
}

fn ends_with(text: &str, ch: char) -> bool {
    text.trim_end().ends_with(ch)
}

/// A bare member reference (`Имя`, `Модуль.Метод`, `Объект.Метод()`) is a name
/// mention in prose, not an executable statement.
fn is_bare_reference(toks: &[Token]) -> bool {
    if toks.is_empty() {
        return false;
    }
    let mut i = 0;
    if toks[i].kind != TokenKind::Ident {
        return false;
    }
    i += 1;
    while i + 1 < toks.len()
        && toks[i].kind == TokenKind::Dot
        && toks[i + 1].kind == TokenKind::Ident
    {
        i += 2;
    }
    if i + 1 < toks.len()
        && toks[i].kind == TokenKind::LParen
        && toks[i + 1].kind == TokenKind::RParen
    {
        i += 2;
    }
    if i < toks.len() && toks[i].kind == TokenKind::Dot {
        i += 1;
    }
    i == toks.len()
}

/// Detects `Идент … = …` assignment, returning the index of the `=` token.
/// Rejects comparison (`<=`, `>=`, `<>`, `==`) and arrow (`=>`) shapes.
/// The token before `=` must be an identifier, `)`, or `]` — a numeric literal
/// cannot be an assignment target in BSL.
fn assignment_index(toks: &[Token]) -> Option<usize> {
    for idx in 1..toks.len().saturating_sub(1) {
        if toks[idx].kind != TokenKind::Eq {
            continue;
        }
        let prev = toks[idx - 1].kind;
        let next = toks[idx + 1].kind;
        if matches!(prev, TokenKind::Lt | TokenKind::Gt | TokenKind::Eq) {
            continue;
        }
        if matches!(next, TokenKind::Eq | TokenKind::Gt) {
            continue;
        }
        if matches!(prev, TokenKind::Ident | TokenKind::RParen | TokenKind::RBracket) {
            return Some(idx);
        }
    }
    None
}

/// `Идент . Идент (` — a qualified method call.
fn has_member_call(toks: &[Token]) -> bool {
    toks.windows(4).any(|w| {
        w[0].kind == TokenKind::Ident
            && w[1].kind == TokenKind::Dot
            && w[2].kind == TokenKind::Ident
            && w[3].kind == TokenKind::LParen
    })
}

const DECL_KW: &[TokenKind] = &[TokenKind::KwProcedure, TokenKind::KwFunction];
const END_KW: &[TokenKind] = &[
    TokenKind::KwEndProcedure,
    TokenKind::KwEndFunction,
    TokenKind::KwEndIf,
    TokenKind::KwEndDo,
    TokenKind::KwEndTry,
];
const SIMPLE_STMT_KW: &[TokenKind] = &[
    TokenKind::KwReturn,
    TokenKind::KwContinue,
    TokenKind::KwBreak,
    TokenKind::KwGoto,
    TokenKind::KwTry,
    TokenKind::KwExcept,
    TokenKind::KwVar,
    TokenKind::KwRaise,
];
const COND_KW: &[TokenKind] = &[TokenKind::KwIf, TokenKind::KwFor, TokenKind::KwWhile];
const PAIR_KW: &[TokenKind] = &[TokenKind::KwThen, TokenKind::KwDo];

fn has_code_operator(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '=' | '+' | '*' | '/'))
}

/// True when a line carries an unambiguous statement structure: an assignment,
/// a declaration with parameters, a block keyword, a paired conditional/loop, a
/// terminated call, or a query-language continuation.
///
/// `toks` is the pre-computed non-whitespace token slice for `trimmed`,
/// produced once by `line_is_code` to avoid redundant lexer invocations.
fn strong_statement(trimmed: &str, toks: &[Token], group_has_query: bool) -> bool {
    // SQL/query string continuation lines begin with the `|` text-block prefix,
    // but only count when the surrounding block actually opens a query — a lone
    // `| прозаичный хвост` is a boxed comment, not commented-out SQL.
    if trimmed.starts_with('|') {
        return group_has_query;
    }

    if toks.is_empty() {
        return false;
    }

    // A trailing `Dot` token is sentence punctuation, not member-access syntax:
    // real member access always has an identifier after the dot, so a dot that
    // is the last token cannot be part of an expression.  Strip it before the
    // structural tests so `М = Менеджер.` is not treated as an assignment whose
    // RHS contains a Dot (which would otherwise trigger `rhs_is_code`).
    let toks = if toks.last().map(|t| t.kind) == Some(TokenKind::Dot) {
        &toks[..toks.len() - 1]
    } else {
        toks
    };

    // The Dot strip above can leave toks empty (e.g. a comment body of just
    // `.` or `..`).  Nothing to classify — not code.
    if toks.is_empty() {
        return false;
    }

    if is_bare_reference(toks) {
        return false;
    }

    let ends_period = ends_with(trimmed, '.');
    let ends_semi = ends_with(trimmed, ';');
    let opens = trimmed.matches('(').count();
    let closes = trimmed.matches(')').count();
    let has_string = toks.iter().any(|t| t.kind == TokenKind::String);

    if let Some(idx) = assignment_index(toks) {
        let rhs = &toks[idx + 1..];
        let rhs_is_code = rhs.iter().any(|t| {
            matches!(
                t.kind,
                TokenKind::Decimal
                    | TokenKind::Float
                    | TokenKind::String
                    | TokenKind::LParen
                    | TokenKind::Dot
                    | TokenKind::Plus
                    | TokenKind::Star
                    | TokenKind::Slash
                    | TokenKind::Semicolon
                    | TokenKind::RParen
            ) || t.kind.is_keyword()
        });
        if ends_semi || rhs_is_code || opens > 0 {
            return true;
        }
        // A bare `X = Y` where the RHS is a lone identifier is ambiguous: it
        // could be genuine commented code or a prose equation ("Параметр =
        // Прочтена"). Reject it unless it ends with `;`, has code in the RHS,
        // or has open parens — all of which are handled above. A period-
        // terminated line is already excluded by `ends_period`; non-period bare
        // assignments also lack enough signal to distinguish code from prose.
        return false;
    }

    let first = toks[0].kind;
    if DECL_KW.contains(&first) {
        return opens > 0;
    }
    if END_KW.contains(&first) {
        return true;
    }
    if SIMPLE_STMT_KW.contains(&first) {
        // `Попытка`/`Исключение` as standalone keywords (`toks.len() == 1`) are
        // unambiguous block delimiters; with following words they could be prose
        // ("Попытка №1.", "Исключение для пользователя.") so they fall through
        // to the shared gate below like all other simple statement keywords.
        if ends_semi || opens > 0 || has_code_operator(trimmed) {
            return true;
        }
        if toks.len() == 1 {
            return true;
        }
    }

    if ends_semi && opens > 0 {
        return true;
    }

    // `Идент.Метод(` is a call statement, but `Если Х Тогда`-style conditionals
    // also appear verbatim in prose ("Если …, тогда …"); guard those soft
    // signals so a full sentence ending in a period is never treated as code.
    let prose = ends_period || is_prose(trimmed, toks);

    if has_member_call(toks) && !prose {
        return true;
    }

    let has_pair = toks.iter().any(|t| PAIR_KW.contains(&t.kind));
    let has_cond = toks.iter().any(|t| COND_KW.contains(&t.kind));
    if has_pair && has_cond && !prose {
        return true;
    }

    if opens > 0 && !prose {
        let comma_in_parens = comma_inside_parens(trimmed);
        if (comma_in_parens || has_string) && (ends_semi || opens > closes) {
            return true;
        }
    }

    false
}

fn comma_inside_parens(text: &str) -> bool {
    let mut depth = 0i32;
    for ch in text.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth > 0 => return true,
            _ => {}
        }
    }
    false
}

/// True when a line reads like natural-language prose: a run of mostly Cyrillic
/// words with almost no code operators. A member call or a real operator-bearing
/// expression disqualifies prose, but an isolated quoted word inside a sentence
/// (`… реквизит "Партнер" …`) does not.
///
/// `toks` is the pre-computed non-whitespace token slice for `trimmed`, reused
/// from `line_is_code` to avoid a redundant lexer call.
fn is_prose(trimmed: &str, toks: &[Token]) -> bool {
    let body = trimmed.trim_start_matches(['+', '-']).trim_start();
    // When the leading markers were stripped the token slice may cover more
    // text than `body`; re-tokenize only the stripped body for the word counts
    // so that a leading `+` operator is not mistaken for a code operator.
    let body_toks = if body.len() == trimmed.len() {
        // no leading markers — reuse the slice already computed
        std::borrow::Cow::Borrowed(toks)
    } else {
        std::borrow::Cow::Owned(code_tokens(body))
    };
    if has_member_call(&body_toks) {
        return false;
    }
    let words =
        body_toks.iter().filter(|t| t.kind == TokenKind::Ident || t.kind.is_keyword()).count();
    let cyrillic_words = body_toks
        .iter()
        .filter(|t| {
            (t.kind == TokenKind::Ident || t.kind.is_keyword())
                && t.text.chars().next().is_some_and(is_cyrillic)
        })
        .count();
    let code_ops = body
        .chars()
        .filter(|c| matches!(c, '=' | '(' | ')' | '[' | ']' | ';' | '+' | '*' | '/' | '%'))
        .count();
    words >= 4 && code_ops < 2 && cyrillic_words >= 3
}

fn is_cyrillic(c: char) -> bool {
    ('\u{0400}'..='\u{04FF}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, check_fix_snapshot_for, format_diags};
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_fix_deletes_commented_code_manual() {
        let code = "Функция Тест()\n    //А = 1;\n    Возврат Б;\nКонецФункции";
        check_fix_snapshot_for(
            code,
            DiagnosticCode::CommentedCode,
            expect![[r#"
            CommentedCode @ 2:5..2:13 — Удалить закомментированный код [fix_all=false]
            Функция Тест()
                Возврат Б;
            КонецФункции"#]],
        );
    }

    #[test]
    fn commented_code_inside_multiline_literal_is_not_reported() {
        // Строки многострочного литерала, начинающиеся с `//`, лексер отдаёт
        // токенами комментария — текстом запроса они от этого быть не перестают.
        let inside_literal = "Процедура П()\n    Т = \"ВЫБРАТЬ *\n    // Сообщить(1);\n    // Возврат;\n    |ИЗ Т\";\nКонецПроцедуры";
        assert!(check_ast_diagnostic(inside_literal, check).is_empty());

        // Тот же текст вне литерала: без него пустой ответ выше означал бы и
        // «литерал распознан», и «диагностика перестала срабатывать вовсе».
        let outside_literal = "Процедура П()\n    // Сообщить(1);\n    // Возврат;\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(outside_literal, check);
        expect![[r#"
            CommentedCode @ 2:5..3:16
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(outside_literal, &diagnostics));
    }

    #[test]
    fn inline_code_comment_is_still_reported() {
        // Комментарий за кодом остаётся кандидатом в закомментированный код:
        // отсутствие быстрой правки не означает отсутствия находки.
        let code = "Функция Тест()\n    Х = ВызовФункции();    // Возврат Старое;\n    Возврат Х;\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 2:28..2:46
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_fix_not_offered_for_inline_code_comment() {
        // The comment body looks like code, but real code precedes it on the line, so
        // deleting the line would drop the assignment — no fix is offered.
        let code = "Функция Тест()\n    Х = ВызовФункции();    // Возврат Старое;\n    Возврат Х;\nКонецФункции";
        check_fix_snapshot_for(code, DiagnosticCode::CommentedCode, expect![]);
    }

    #[test]
    fn test_no_diagnostic_for_regular_comments() {
        let code = r#"Функция Тест()
    // Это обычный комментарий
    // Описание функции
    А = 1;
    Возврат А;
КонецФункции"#;

        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    /// Код, закомментированный дважды, остаётся находкой.
    ///
    /// Это и есть вход, на котором `code_tokens` обязан оставаться при своём
    /// наборе: тело комментария здесь само начинается с `//`, и общий предикат
    /// тривии унёс бы его целиком вместе с находкой. Вход взят из реального
    /// модуля — он единственный из 96 строк с двумя `//`, на котором вердикт
    /// расходится.
    #[test]
    fn code_commented_twice_is_still_a_finding() {
        let code = "Функция Тест()\n    // //Модуль.Метод(Данные, Идентификатор);\n    Возврат А;\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "дважды закомментированный код перестал быть находкой");
    }

    /// BOM в начале файла не меняет вердикт.
    #[test]
    fn a_byte_order_mark_does_not_change_the_verdict() {
        let with_bom = "\u{feff}Функция Тест()\n    // Б = 2;\n    Возврат А;\nКонецФункции";
        let without = "Функция Тест()\n    // Б = 2;\n    Возврат А;\nКонецФункции";
        assert_eq!(
            check_ast_diagnostic(with_bom, check).len(),
            check_ast_diagnostic(without, check).len(),
            "BOM изменил число находок"
        );
        assert_eq!(check_ast_diagnostic(without, check).len(), 1, "вход обязан давать находку");
    }

    #[test]
    fn test_commented_assignment() {
        let code = r#"Функция Тест()
    А = 1;
    // Б = 2;
    Возврат А;
КонецФункции"#;

        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 3:5..3:14
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_multiline_commented_block() {
        let code = r#"//НужноПересчитать = Ложь;
//Если Документ.Проведен Тогда
//    НужноПересчитать = Истина;
//КонецЕсли;"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 1:1..4:13
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_commented_out_procedure() {
        let code = r#"//// Процедура ВыполнитьСервис()
////
////    ПодготовитьДанные();
////
////КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 1:1..5:19
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_two_consecutive_commented_lines() {
        let code = r#"//Параметры.Вставить("ДатаНачала", ТекущаяДата());
//Параметры.Вставить("ДатаОкончания", ТекущаяДата());"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 1:1..2:54
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_range_covers_whole_group_with_header() {
        let code = r#"Процедура Тест()
    // ++ Проверяем одинаковые значения
    //Таблица = Источник;
    //Таблица.Свернуть("Код");
    //Если Таблица.Количество() > 1 Тогда
    //    Возврат Ложь;
    //КонецЕсли;
    //Возврат Истина;
    // -- Конец проверки
КонецПроцедуры"#;

        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 2:5..9:25
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_method_documentation_not_flagged() {
        let code = r#"// Получает данные из хранилища.
//
// Параметры:
//  Ключ - Строка - ключ значения;
//
// Возвращаемое значение:
//  Произвольный - сохранённое значение.
Процедура Тест(Ключ)
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_header_attached_to_method_is_flagged() {
        // A commented-out assignment directly above a method declaration is
        // genuine commented code and must be flagged regardless of proximity.
        let code = r#"// Записать = Истина;
&НаСервере
Процедура Тест()
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 1:1..1:22
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_prose_not_flagged() {
        let code = r#"Процедура Тест()
    // Если количество напоминаний больше максимального, создаём одно общее напоминание.
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_marker_comment_not_flagged() {
        let code = r#"Процедура Тест()
    // +CRM
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_method_reference_not_flagged() {
        let code = r#"Процедура Тест()
    // ИнициализироватьЭлементУсловногоОформления()
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_floating_commented_procedure_flagged() {
        let code = r#"Процедура Реальная()
    //Процедура Старая()
    //    ПодготовитьДанные();
    //КонецПроцедуры
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 2:5..4:21
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_prose_with_leading_keyword_not_flagged() {
        let code = r#"Процедура Тест()
    // Попытка определить тип свойства приемника.
    // Исключение выбора группы Все внешние пользователи в качестве родителя.
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_member_assignment_flagged() {
        let code = r#"Процедура Тест()
    // Действие.Ширина = 3;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 2:5..2:28
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_real_conditional_still_flagged() {
        let code = r#"Процедура Тест()
    // Если ПолучитьФункциональнуюОпцию("CRM_Опция") Тогда
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 2:5..2:59
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_json_data_not_flagged() {
        let code = r#"Процедура Тест()
    // { "login": "User" }
    // "password": "secret",
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_html_markup_not_flagged() {
        let code = r#"Процедура Тест()
    // <p style="color: red">Текст</p>
    // <div id="main">
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_comparison_operator_not_treated_as_markup() {
        let code = r#"Процедура Тест()
    // Если ТипЗнч(Х) <> Тип("Строка") Тогда
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 2:5..2:45
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_lone_bar_comment_not_flagged() {
        let code = r#"Процедура Тест()
    // | это просто оформление в рамке
    // | ещё одна строка примечания
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_bar_query_block_flagged() {
        let code = r#"Процедура Тест()
    // |ВЫБРАТЬ
    // |	Таблица.Ссылка КАК Ссылка
    // |ИЗ
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 2:5..4:11
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_trailing_plus_marker_not_flagged() {
        let code = r#"Процедура Тест()
    // + 1 неделя
    // ++ PVS Внедрение CRM
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    // ── FIX A: numeric literal cannot be an assignment target ──────────────
    #[test]
    fn test_number_lhs_not_flagged() {
        let code = r#"Процедура Тест()
    // 7776000 = 60 * 60 * 24 * 90.
    // 50*1024*1024 = 50 Мб
    // 3%3=0, 4%3=1, 5%3=2
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    // ── FIX B: Попытка/Исключение with following words is prose ────────────
    #[test]
    fn test_try_except_prose_not_flagged() {
        let code = r#"Процедура Тест()
    // Попытка №1.
    // Исключение для пользователя.
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_bare_try_still_flagged() {
        let code = r#"Процедура Тест()
    // Попытка
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 2:5..2:15
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    // ── FIX C: XML/SOAP/BNF blocks are commented data, not code ───────────
    #[test]
    fn test_xml_block_not_flagged() {
        let code = r#"Процедура Тест()
    // <?xml version="1.0" encoding="utf-8"?>
    // <soap:Envelope xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
    //   <soap:Body>
    //   </soap:Body>
    // </soap:Envelope>
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_cyrillic_tag_not_flagged() {
        let code = r#"Процедура Тест()
    // <Формула> ::= "(" <Формула> ")" <Остаток>
    // <Терм> ::= число
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_code_before_html_still_flagged() {
        // Group opens with a real assignment — must not be suppressed by the
        // data-majority guard even though continuation lines are HTML.
        let code = r#"Процедура Тест()
    //ИсторияВыполнения = ИсторияВыполнения + ?(ИсторияВыполнения = "","","
    // |<P>
    // |<HR>
    //|<P></P>");
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 2:5..5:18
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    // ── FIX D: trailing sentence dot is not member-access ─────────────────
    #[test]
    fn test_trailing_dot_legend_not_flagged() {
        let code = r#"Процедура Тест()
    // М = Менеджер.
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_real_member_assign_still_flagged() {
        // The dot here is NOT final — it separates Объект and Реквизит — so the
        // trailing-dot strip must not remove it.
        let code = r#"Процедура Тест()
    // Сумма = Объект.Реквизит;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            CommentedCode @ 2:5..2:32
              message: Программные модули не должны иметь закомментированных фрагментов кода
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_exclusion_prefix() {
        let code = r#"Процедура ШаблонМетода(Параметр)
    //<code>Если Истина Тогда
    //<code>Возврат;
    //<code>КонецЕсли;
КонецПроцедуры"#;
        let mut config = crate::DiagnosticsConfig::all_enabled();
        config.parameters.insert(
            DiagnosticCode::CommentedCode,
            serde_json::json!({"exclusionPrefixes": "<code>"}),
        );
        let diagnostics = crate::test_utils::check_ast_diagnostic_with_config(code, config, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_dot_only_comment_not_flagged() {
        // A comment body of just `.` or `..` must not panic and must not be flagged.
        let code = "Процедура Тест()\n    // .\n    // ..\nКонецПроцедуры";
        let diagnostics = crate::test_utils::check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    /// `!=` в теле комментария — сравнение, а не присваивание.
    ///
    /// Пара входов обязательна: на одном лишь `!=` проверка зелена при любой
    /// реализации `assignment_index`, включая ту, что не находит присваиваний
    /// вовсе. `а = 1;` рядом показывает, что мерка вообще способна их видеть.
    #[test]
    fn bang_equals_is_not_an_assignment() {
        let index = |text: &str| super::assignment_index(&super::code_tokens(text));

        assert_eq!(index("а != 1"), None);
        assert_eq!(index("Массив[0] != 1"), None);
        assert_eq!(index("Функция() != 1"), None);

        assert!(index("а = 1;").is_some(), "мерка не видит присваивания и потому ничего не пиннит");
        assert!(index("Массив[0] = 1;").is_some());
        assert!(index("Функция() = 1;").is_some());
    }

    #[test]
    fn test_data_group_with_statement_still_flagged() {
        // A group that looks like data (XML tags) but contains a genuine BSL
        // statement ending in `;` must not be suppressed by group_is_commented_data.
        let code = "Процедура Тест()\n    // <Root>\n    // Сообщить(\"Привет\");\n    // </Root>\nКонецПроцедуры";
        let diagnostics = crate::test_utils::check_ast_diagnostic(code, check);
        // The group contains a `;`-terminated statement so it must be flagged.
        assert!(
            !diagnostics.is_empty(),
            "expected CommentedCode to fire when a group contains a BSL statement"
        );
    }
}
