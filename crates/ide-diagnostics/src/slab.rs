//! Блок строк — единица строчных проверок.
//!
//! Строчные проверки (`LineLength`, `MissingSpace`, `IncorrectLineBreak`,
//! `CommentedCode`) судят строку и её соседей по тексту. Файл для них делится
//! на блоки целых строк: плита каждого метода (`hir::SlabLayout`) и отрезки
//! остатка между плитами. У блока свои токены от лексера, свой индекс строк и
//! ровно тот контекст соседей, от которого зависит исход. Один и тот же
//! `check_block` считает и плиту из мемо, и остаток, и — для тестов и
//! провайдеров без salsa — файл, разрезанный на блоки прямо здесь.

use hir::{LeadingContext, LocalRange, MethodOffset, Rules, SlabLayout};
use line_index::LineIndex;
use syntax::{LineToken, SyntaxNode, TextRange, TextSize};

use crate::{handlers, AnalysisContext, Diagnostic, DiagnosticCode, DiagnosticsContext};

/// Целые строки текста и их контекст; все координаты — от начала блока.
pub struct Block<'a> {
    pub text: &'a str,
    pub tokens: &'a [LineToken],
    pub line_index: &'a LineIndex,
    /// Строки описания методов (`LineLength`, `checkMethodDescription=false`)
    /// внутри блока, отсортированы.
    pub description_lines: &'a [u32],
    pub leading: Option<LeadingContext>,
}

impl Block<'_> {
    /// Ближайший значимый токен перед блоком, когда первый значимый токен
    /// блока — знак или `;`; иначе исход от него не зависит и он не известен.
    pub fn prev_significant(&self) -> Option<syntax::SyntaxKind> {
        self.leading.and_then(|leading| leading.prev)
    }

    pub fn line_text(&self, line: u32) -> Option<&str> {
        self.line_index.line_range(line).map(|range| &self.text[range])
    }
}

/// Коды, которые считаются по блокам.
pub(crate) const SLAB_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::LineLength,
    DiagnosticCode::MissingSpace,
    DiagnosticCode::IncorrectLineBreak,
    DiagnosticCode::CommentedCode,
];

pub(crate) type BlockCheck = fn(&AnalysisContext, &Block) -> Vec<Diagnostic<LocalRange>>;

/// Все четыре строчные проверки одного блока.
pub(crate) fn check_block_all(ctx: &AnalysisContext, block: &Block) -> Vec<Diagnostic<LocalRange>> {
    let mut result = handlers::line_length::check_block(ctx, block);
    result.extend(handlers::missing_space::check_block(ctx, block));
    result.extend(handlers::incorrect_line_break::check_block(ctx, block));
    result.extend(handlers::commented_code::check_block(ctx, block));
    result
}

/// Токены и индекс строк одного блока — то, что мемо плиты и остаток
/// строят одинаково.
pub(crate) struct OwnedBlock<'t> {
    pub text: &'t str,
    pub tokens: Vec<LineToken>,
    pub line_index: LineIndex,
}

impl<'t> OwnedBlock<'t> {
    pub fn new(text: &'t str) -> OwnedBlock<'t> {
        OwnedBlock { tokens: parser::line_tokens(text), line_index: LineIndex::new(text), text }
    }

    pub fn block<'a>(
        &'a self,
        description_lines: &'a [u32],
        leading: Option<LeadingContext>,
    ) -> Block<'a> {
        Block {
            text: self.text,
            tokens: &self.tokens,
            line_index: &self.line_index,
            description_lines,
            leading,
        }
    }
}

/// Строки описания методов по эталону `LineLength` — по всему файлу, слепо к
/// узлам: ближайший комментарий над методом на любом расстоянии и смежные с
/// ним выше. Считается только когда конфиг выключил проверку описаний.
pub(crate) fn description_lines(
    ctx: &AnalysisContext,
    root: &SyntaxNode,
    line_index: &LineIndex,
) -> Vec<u32> {
    if ctx.config_bool(DiagnosticCode::LineLength, "checkMethodDescription", true) {
        return Vec::new();
    }
    handlers::line_length::find_method_description_lines(root, line_index)
}

/// Строки множества внутри `first..=last`, в координатах блока.
pub(crate) fn project_lines(sorted: &[u32], first: u32, last: u32) -> Vec<u32> {
    let from = sorted.partition_point(|&line| line < first);
    let to = sorted.partition_point(|&line| line <= last);
    sorted[from..to].iter().map(|line| line - first).collect()
}

/// Текст строк `first..=last` с их переводами строк.
pub(crate) fn lines_text<'a>(
    text: &'a str,
    line_index: &LineIndex,
    first: u32,
    last: u32,
) -> &'a str {
    let start = line_index.line_start(first);
    let end = line_index.try_line_start(last + 1).unwrap_or(line_index.text_len());
    &text[TextRange::new(start, end)]
}

/// Контексты блока, каждый из которых можно выключить в тесте, чтобы показать
/// вход, на котором без него блок судит иначе, чем файл.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fidelity(u8);

impl Fidelity {
    pub const LEADING: Fidelity = Fidelity(1);
    pub const DESCRIPTIONS: Fidelity = Fidelity(2);
    pub const ALL: Fidelity = Fidelity(3);

    pub fn without(self, part: Fidelity) -> Fidelity {
        Fidelity(self.0 & !part.0)
    }

    fn has(self, part: Fidelity) -> bool {
        self.0 & part.0 != 0
    }
}

/// Файл, разрезанный на блоки плит и остатка тем же правилом, что и мемо:
/// путь без salsa для тестов и провайдеров без базы.
pub(crate) fn check_file_by_blocks(ctx: &DiagnosticsContext, check: BlockCheck) -> Vec<Diagnostic> {
    check_file_by_blocks_with(ctx, Rules::ALL, Fidelity::ALL, check)
}

pub(crate) fn check_file_by_blocks_with(
    ctx: &DiagnosticsContext,
    rules: Rules,
    fidelity: Fidelity,
    check: BlockCheck,
) -> Vec<Diagnostic> {
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let item_tree = ctx.item_tree();
    let file_text = ctx.file_text();
    let text: &str = &file_text;
    let layout = SlabLayout::compute_with(&parse, &item_tree, text, rules);
    let line_index = layout.line_index();
    let descriptions = description_lines(ctx, &root, line_index);

    let mut result = Vec::new();
    let mut run = |first: u32, last: u32, leading: Option<LeadingContext>| {
        let owned = OwnedBlock::new(lines_text(text, line_index, first, last));
        let described = if fidelity.has(Fidelity::DESCRIPTIONS) {
            project_lines(&descriptions, first, last)
        } else {
            Vec::new()
        };
        let leading = leading.filter(|_| fidelity.has(Fidelity::LEADING));
        let block = owned.block(&described, leading);
        let base = MethodOffset::new(line_index.line_start(first));
        result.extend(check(ctx, &block).into_iter().map(|d| d.lift(base)));
    };
    for (_, span) in layout.spans() {
        run(span.first_line, span.last_line, span.leading);
    }
    for block in layout.remainder() {
        run(block.first_line, block.last_line, block.leading);
    }
    result
}

/// Четыре строчные проверки файла по блокам: вход без salsa
/// (`diagnostics()`), рядом с телами через провайдер.
pub(crate) fn collect_slab_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if !ctx.config.any_enabled(SLAB_DIAGNOSTICS) {
        return Vec::new();
    }
    check_file_by_blocks(ctx, check_block_all)
}

/// Блоки остатка файла — всё, что не вошло ни в одну плиту, — теми же
/// проверками; вход salsa, рядом с мемо плит.
pub(crate) fn collect_remainder(ctx: &DiagnosticsContext, layout: &SlabLayout) -> Vec<Diagnostic> {
    let _span = tracing::info_span!("slab_remainder").entered();
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let file_text = ctx.file_text();
    let text: &str = &file_text;
    let line_index = layout.line_index();
    let descriptions = description_lines(ctx, &root, line_index);
    let mut result = Vec::new();
    for block in layout.remainder() {
        let owned =
            OwnedBlock::new(lines_text(text, line_index, block.first_line, block.last_line));
        let described = project_lines(&descriptions, block.first_line, block.last_line);
        let base = MethodOffset::new(line_index.line_start(block.first_line));
        result.extend(
            check_block_all(ctx, &owned.block(&described, block.leading))
                .into_iter()
                .map(|d| d.lift(base)),
        );
    }
    result
}

/// Проверка тождества под `BSL_SLAB_VERIFY=1`: собранные из мемо плит и
/// остатка находки четырёх строчных кодов против файла одним блоком, и
/// `DuplicateStringLiteral` из мемо тел против файлового обхода. Расхождение
/// — ошибка в журнале и счётчик процесса; значение запроса оно не меняет.
pub(crate) fn verify_assembled(ctx: &DiagnosticsContext, assembled: &[Diagnostic]) {
    const CHECKS: &[(DiagnosticCode, BlockCheck)] = &[
        (DiagnosticCode::LineLength, handlers::line_length::check_block),
        (DiagnosticCode::MissingSpace, handlers::missing_space::check_block),
        (DiagnosticCode::IncorrectLineBreak, handlers::incorrect_line_break::check_block),
        (DiagnosticCode::CommentedCode, handlers::commented_code::check_block),
    ];
    let by_file = CHECKS
        .iter()
        .map(|(code, check)| (*code, check_file_as_one_block(ctx, *check)))
        .chain(std::iter::once((
            DiagnosticCode::DuplicateStringLiteral,
            handlers::duplicate_string_literal::check_file(ctx),
        )));
    for (code, mut expected) in by_file {
        let mut actual: Vec<Diagnostic> =
            assembled.iter().filter(|d| d.code == code).cloned().collect();
        crate::normalize_diagnostics(&mut expected);
        crate::normalize_diagnostics(&mut actual);
        if expected != actual {
            SLAB_VERIFY_MISMATCHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let first = expected
                .iter()
                .zip(actual.iter())
                .find(|(e, a)| e != a)
                .map(|(e, a)| format!("ожидалось {e:?}, собрано {a:?}"))
                .unwrap_or_else(|| {
                    format!("ожидалось {}, собрано {}", expected.len(), actual.len())
                });
            tracing::error!(
                file_id = ctx.file_id.0,
                code = code.as_str(),
                "slab verify: сборка по плитам разошлась с файлом: {first}"
            );
        }
    }
}

static SLAB_VERIFY_MISMATCHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Сколько раз с начала процесса сборка по плитам разошлась с файлом под
/// `BSL_SLAB_VERIFY=1`.
pub fn slab_verify_mismatches() -> u64 {
    SLAB_VERIFY_MISMATCHES.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn slab_verify_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("BSL_SLAB_VERIFY").is_ok_and(|v| v == "1"))
}

/// Файл одним блоком без контекста соседей: прежний файловый алгоритм с
/// точностью до источника токенов — эталон для разрезанного пути.
pub(crate) fn check_file_as_one_block(
    ctx: &DiagnosticsContext,
    check: BlockCheck,
) -> Vec<Diagnostic> {
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let file_text = ctx.file_text();
    let owned = OwnedBlock::new(&file_text);
    let descriptions = description_lines(ctx, &root, &owned.line_index);
    let block = owned.block(&descriptions, None);
    let base = MethodOffset::new(TextSize::new(0));
    check(ctx, &block).into_iter().map(|d| d.lift(base)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_db;
    use crate::DiagnosticsConfig;
    use hir::MethodKey;

    const MODULE: &str = include_str!("../../parser/tests/fixtures/Module.bsl");

    /// Раскладки, на которых строка, серия или сосед пересекают границу
    /// владельцев: каждая — целый модуль.
    const SEAMS: &[&str] = &[
        // docstring, аннотации, хвостовой комментарий на закрывающем слове
        "Перем В;\n\n// Описание А, длинное описание, которое не влезает в сорок символов\n// Параметры:\n&НаСервере\nПроцедура А()\n\tХ = 1;\nКонецПроцедуры // хвост хвост хвост хвост хвост хвост\n\nПроцедура Б()\nКонецПроцедуры\n",
        // два узла на одной строке
        "Процедура А()\n\tХ=1;\nКонецПроцедуры Процедура Б(Пар1,Пар2)\n\tУ=2;\nКонецПроцедуры\n",
        // закрывающее слово с комментарием прямо над docstring; длинный
        // комментарий в теле А — «описание Б» для эталона
        "Процедура А()\n\t// комментарий в теле А, очень длинный комментарий, длиннее сорока символов\n\tХ = 1;\nКонецПроцедуры // x\n// Описание Б, ещё одно длинное описание длиннее сорока символов\nПроцедура Б()\nКонецПроцедуры\n",
        // заголовок с комментарием под открытой серией и знак первым токеном тела
        "Процедура А()\nКонецПроцедуры // x\n// между\nПроцедура Б() // y\n\t-1;\n\t+2;\nКонецПроцедуры\n",
        // незакрытый метод: строка кончается знаком, ниже — продолжение литерала
        "Процедура А()\n\tХ = \"а\" +\n\"б\";\n",
        "Процедура А()\n\tХ = \"а\" +\n|б\";\n",
        "Процедура А()\n\tХ = 1 +\n\nПерем Г;\n",
        // модульный код после методов: знак и `;` первыми токенами строк
        "Процедура А()\nКонецПроцедуры\n-1;\n;\n+Х;\nХ=Х-1;\n",
        // литерал, открытый на закрывающей строке; со строкой-текстом `//`
        // внутри и без неё
        "Процедура А()\n\tХ = 1;\nКонецПроцедуры Х = \"abc\n|def\";\n;\n",
        "Процедура А()\n\tХ = 1;\nКонецПроцедуры Т = \"а\n// Х = 2;\n|в\";\n",
        "Процедура А()\nКонецПроцедуры Т = \"а\n// Х = 2;\n// У = 3;\n|в\";\nПроцедура Б()\nКонецПроцедуры\n",
        // узел метода кончается посреди строки, начатой знаком в теле; строка
        // уходит в остаток по литералу либо по серии комментариев
        "Процедура А()\n\tХ = Сумма\n\t+1;КонецПроцедуры Т = \"а\n|в\";\n",
        "Процедура А()\n\tХ = Сумма\n\t-1;КонецПроцедуры // x\n// y\nПроцедура Б()\nКонецПроцедуры\n",
        // литерал, оборванный над объявлением: его `//`-строка выглядит docstring
        "Т = \"а\n// Х = 2;\nПроцедура Б()\n\tЗ = 1;\nКонецПроцедуры\n",
        // методы внутри директив и областей, docstring через пустую строку
        "#Область Публичные\n#Если Сервер Тогда\n// Описание А, длинное описание, которое не влезает в сорок символов\n\nПроцедура А()\n\tХ=1;\nКонецПроцедуры\n#КонецЕсли\n#КонецОбласти\n",
        // комментарий между аннотацией и объявлением, docstring над аннотацией
        "// Описание\n&НаСервере // на сервере, длинный хвост длиннее сорока символов\n// между аннотацией и объявлением\nПроцедура А() Экспорт\nКонецПроцедуры\n",
        // закомментированный код по обе стороны шва
        "Процедура А()\n\tХ = 1;\nКонецПроцедуры // Х = 1;\n// У = 2;\n// Z = 3;\nПроцедура Б()\n\t// Сообщить(1);\n\t// Возврат;\nКонецПроцедуры\n// Ещё = 4;\n",
        // CRLF, BOM, без завершающего перевода строки
        "\u{feff}// Описание\r\nПроцедура А()\r\n\tХ=1; // хвост длиннее сорока символов, честное слово, длиннее\r\nКонецПроцедуры // x\r\n// y\r\nПроцедура Б()\r\nКонецПроцедуры",
        // пустые строки и хвостовые комментарии вокруг единственного метода
        "\n\n// a\n// b\n\nПроцедура А()\n\n\tХ = 1;\n\nКонецПроцедуры\n\n// c\n\n",
    ];

    fn configs() -> Vec<(&'static str, DiagnosticsConfig)> {
        let mut out = vec![("default", DiagnosticsConfig::all_enabled())];
        let mut c = DiagnosticsConfig::all_enabled();
        c.parameters.insert(
            DiagnosticCode::LineLength,
            serde_json::json!({"checkMethodDescription": false, "maxLineLength": 40}),
        );
        out.push(("descriptions", c));
        let mut c = DiagnosticsConfig::all_enabled();
        c.parameters.insert(
            DiagnosticCode::LineLength,
            serde_json::json!({"excludeTrailingComments": true, "maxLineLength": 40}),
        );
        out.push(("trailing", c));
        let mut c = DiagnosticsConfig::all_enabled();
        c.parameters.insert(
            DiagnosticCode::MissingSpace,
            serde_json::json!({"listForCheckLeft": ", ;", "checkSpaceToRightOfUnary": true}),
        );
        out.push(("unary", c));
        let mut c = DiagnosticsConfig::all_enabled();
        c.parameters.insert(
            DiagnosticCode::MissingSpace,
            serde_json::json!({"listForCheckRight": ", ;", "allowMultipleCommas": true}),
        );
        out.push(("commas", c));
        out
    }

    const CHECKS: &[(DiagnosticCode, BlockCheck)] = &[
        (DiagnosticCode::LineLength, handlers::line_length::check_block),
        (DiagnosticCode::MissingSpace, handlers::missing_space::check_block),
        (DiagnosticCode::IncorrectLineBreak, handlers::incorrect_line_break::check_block),
        (DiagnosticCode::CommentedCode, handlers::commented_code::check_block),
    ];

    type Key<'a> = (u32, u32, &'a str, &'a str, Vec<(u32, u32, &'a str)>);

    fn key(d: &Diagnostic) -> Key<'_> {
        (
            d.range.start().into(),
            d.range.end().into(),
            d.code.as_str(),
            &d.message,
            d.fixes
                .iter()
                .flat_map(|f| f.edits.iter())
                .map(|e| (e.range.start().into(), e.range.end().into(), e.new_text.as_str()))
                .collect(),
        )
    }

    fn sorted(mut diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
        diags.sort_by(|a, b| key(a).cmp(&key(b)));
        diags
    }

    fn with_ctx<T>(
        code: &str,
        config: &DiagnosticsConfig,
        f: impl FnOnce(&DiagnosticsContext) -> T,
    ) -> T {
        let (db, file_id) = create_test_db(code);
        let provider = ide_db::SalsaProvider::new(&db, None);
        let ctx = DiagnosticsContext::new(config, file_id, &provider);
        f(&ctx)
    }

    fn corpus() -> Vec<String> {
        let mut texts: Vec<String> = SEAMS.iter().map(|s| s.to_string()).collect();
        texts.push(SEAMS.concat());
        texts.push(MODULE.to_string());
        texts
    }

    /// Разрезанный путь равен файлу одним блоком по каждому коду и конфигу,
    /// и корпус не пуст ни для одного кода ни в плитах, ни в остатке.
    #[test]
    fn blocks_equal_the_whole_file() {
        let mut in_slabs = std::collections::BTreeMap::<&str, usize>::new();
        let mut in_remainder = std::collections::BTreeMap::<&str, usize>::new();
        for text in corpus() {
            for (name, config) in configs() {
                for (code, check) in CHECKS {
                    with_ctx(&text, &config, |ctx| {
                        let oracle = sorted(check_file_as_one_block(ctx, *check));
                        let by_blocks = sorted(check_file_by_blocks(ctx, *check));
                        assert_eq!(
                            by_blocks.iter().map(key).collect::<Vec<_>>(),
                            oracle.iter().map(key).collect::<Vec<_>>(),
                            "{code:?} / {name}:\n{text}"
                        );
                        let parse = ctx.parse();
                        let layout = SlabLayout::compute(&parse, &ctx.item_tree(), &text);
                        let li = layout.line_index();
                        for d in &by_blocks {
                            let line = li.line_col(d.range.start()).line;
                            let owned = layout.spans().any(|(_, span)| {
                                (span.first_line..=span.last_line).contains(&line)
                            });
                            *if owned { &mut in_slabs } else { &mut in_remainder }
                                .entry(code.as_str())
                                .or_insert(0usize) += 1;
                        }
                    });
                }
            }
        }
        for (code, _) in CHECKS {
            let code = code.as_str();
            assert!(in_slabs.get(code).copied().unwrap_or(0) > 0, "{code}: нет находок в плитах");
            assert!(
                in_remainder.get(code).copied().unwrap_or(0) > 0,
                "{code}: нет находок в остатке"
            );
        }
    }

    /// Каждое правило владения и каждый контекст блока имеют вход, на котором
    /// без них разрезанный путь расходится с файлом.
    #[test]
    fn each_rule_and_context_has_a_red_control() {
        let controls: [(&str, Rules, Fidelity); 4] = [
            ("PEEL_RUNS", Rules::ALL.without(Rules::PEEL_RUNS), Fidelity::ALL),
            ("OPEN_LITERALS", Rules::ALL.without(Rules::OPEN_LITERALS), Fidelity::ALL),
            ("LEADING", Rules::ALL, Fidelity::ALL.without(Fidelity::LEADING)),
            ("DESCRIPTIONS", Rules::ALL, Fidelity::ALL.without(Fidelity::DESCRIPTIONS)),
        ];
        let mut vacuous = Vec::new();
        for (name, rules, fidelity) in controls {
            let mut differs = false;
            'search: for text in SEAMS {
                for (_, config) in configs() {
                    for (_, check) in CHECKS {
                        let d = with_ctx(text, &config, |ctx| {
                            let oracle = sorted(check_file_as_one_block(ctx, *check));
                            let weakened =
                                sorted(check_file_by_blocks_with(ctx, rules, fidelity, *check));
                            weakened.iter().map(key).collect::<Vec<_>>()
                                != oracle.iter().map(key).collect::<Vec<_>>()
                        });
                        if d {
                            differs = true;
                            break 'search;
                        }
                    }
                }
            }
            if !differs {
                vacuous.push(name);
            }
        }
        assert!(
            vacuous.is_empty(),
            "без этих правил ни один вход корпуса не краснеет — они лишние: {vacuous:?}"
        );
    }

    #[test]
    fn project_lines_shifts_into_the_block() {
        assert_eq!(project_lines(&[1, 4, 5, 9], 4, 8), vec![0, 1]);
        assert_eq!(project_lines(&[1, 4, 5, 9], 0, 0), Vec::<u32>::new());
        let _ = MethodKey::first("А");
    }
}
