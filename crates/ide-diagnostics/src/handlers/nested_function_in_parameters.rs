use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Body, BodySourceMap, Expr, ExprId, ExprIdx, IdConversion, Name};
use line_index::LineIndex;
use stdx::case::CaseExt;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextRange};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Brainoverload, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_ALLOW_ONELINER: bool = true;
const DEFAULT_ALLOWED_METHOD_NAMES: &str = "НСтр,NStr,ПредопределенноеЗначение,PredefinedValue";

#[derive(Debug, Clone)]
struct Config {
    allow_oneliner: bool,
    allowed_method_names: Vec<String>,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let code = DiagnosticCode::NestedFunctionInParameters;

        let allow_oneliner =
            ctx.config.get_bool(code, "allowOneliner").unwrap_or(DEFAULT_ALLOW_ONELINER);

        let allowed_method_names_str = ctx
            .config
            .get_string(code, "allowedMethodNames")
            .unwrap_or(DEFAULT_ALLOWED_METHOD_NAMES);

        let allowed_method_names: Vec<String> = allowed_method_names_str
            .split(',')
            .map(|s| s.trim().fold_lower())
            .filter(|s| !s.is_empty())
            .collect();

        tracing::debug!(
            allow_oneliner = allow_oneliner,
            allowed_method_names = ?allowed_method_names,
            "NestedFunctionInParameters config loaded"
        );

        Self { allow_oneliner, allowed_method_names }
    }

    fn is_allowed_method(&self, name: &str) -> bool {
        let lower = name.fold_lower();
        self.allowed_method_names.iter().any(|allowed| allowed == &lower)
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::NestedFunctionInParameters;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);

    let line_index = ctx.line_index();

    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = crate::utils::for_each_body(ctx, |body, source_map, diags| {
        check_body(body, source_map, diags, &config, &line_index, &root, code, ctx);
    });

    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));

    diagnostics
}

#[allow(clippy::too_many_arguments)]
fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    config: &Config,
    line_index: &LineIndex,
    root: &SyntaxNode,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    for (expr_id, expr) in body.exprs_iter() {
        match expr {
            Expr::Call { callee, args } => {
                check_call_expr(
                    expr_id,
                    ExprId::from_idx(*callee),
                    args,
                    body,
                    source_map,
                    diagnostics,
                    config,
                    line_index,
                    root,
                    code,
                    ctx,
                );
            }
            Expr::MethodCall { receiver: _, method, args } => {
                check_method_call_expr(
                    expr_id,
                    method,
                    args,
                    body,
                    source_map,
                    diagnostics,
                    config,
                    line_index,
                    root,
                    code,
                    ctx,
                );
            }
            Expr::New { type_name, args } => {
                check_new_expr(
                    expr_id,
                    type_name,
                    args,
                    body,
                    source_map,
                    diagnostics,
                    config,
                    line_index,
                    root,
                    code,
                    ctx,
                );
            }
            _ => {}
        }
    }
}

fn find_name_token_range(expr_range: TextRange, root: &SyntaxNode) -> TextRange {
    let covering = root.covering_element(expr_range);

    let start_node = match covering {
        syntax::NodeOrToken::Node(node) => node,
        syntax::NodeOrToken::Token(token) => match token.parent() {
            Some(parent) => parent,
            None => return expr_range,
        },
    };

    for ancestor in start_node.ancestors() {
        let ancestor_range = ancestor.text_range();

        if ancestor_range != expr_range {
            if ancestor_range.contains_range(expr_range) && ancestor_range != expr_range {
                if let Some(arg_list) = ancestor.children().find(|c| {
                    c.kind() == SyntaxKind::ARG_LIST && c.text_range().end() == expr_range.end()
                }) {
                    match ancestor.kind() {
                        SyntaxKind::CALL_EXPR | SyntaxKind::CALL_STMT => {
                            if let Some(name_token) =
                                find_last_ident_before_node(&ancestor, &arg_list)
                            {
                                return name_token.text_range();
                            }
                        }
                        SyntaxKind::NEW_EXPR => {
                            if let Some(name_token) = find_type_name_or_new_keyword(&ancestor) {
                                return name_token.text_range();
                            }
                        }
                        _ => {}
                    }
                }
            }
            continue;
        }

        match ancestor.kind() {
            SyntaxKind::CALL_EXPR | SyntaxKind::CALL_STMT => {
                if let Some(name_token) = find_last_ident_before_arg_list(&ancestor) {
                    return name_token.text_range();
                }
            }
            SyntaxKind::NEW_EXPR => {
                if let Some(name_token) = find_type_name_or_new_keyword(&ancestor) {
                    return name_token.text_range();
                }
            }
            _ => {}
        }
    }

    let start_offset = expr_range.start();
    for token in root.token_at_offset(start_offset) {
        if token.kind() == SyntaxKind::IDENT {
            return token.text_range();
        }
    }

    expr_range
}

fn find_last_ident_before_node(parent: &SyntaxNode, target: &SyntaxNode) -> Option<SyntaxToken> {
    let target_start = target.text_range().start();

    let mut last_ident: Option<SyntaxToken> = None;
    for child in parent.children_with_tokens() {
        match child {
            syntax::NodeOrToken::Token(token) => {
                if token.text_range().start() >= target_start {
                    break;
                }
                if token.kind() == SyntaxKind::IDENT {
                    last_ident = Some(token);
                }
            }
            syntax::NodeOrToken::Node(node) => {
                if node.text_range().start() >= target_start {
                    break;
                }
                for descendant in node.descendants_with_tokens() {
                    if let syntax::NodeOrToken::Token(token) = descendant {
                        if token.text_range().start() >= target_start {
                            break;
                        }
                        if token.kind() == SyntaxKind::IDENT {
                            last_ident = Some(token);
                        }
                    }
                }
            }
        }
    }
    last_ident
}

fn find_last_ident_before_arg_list(node: &SyntaxNode) -> Option<SyntaxToken> {
    let arg_list = node.children().find(|c| c.kind() == SyntaxKind::ARG_LIST)?;
    let arg_list_start = arg_list.text_range().start();

    let mut last_ident: Option<SyntaxToken> = None;
    for child in node.descendants_with_tokens() {
        if let syntax::NodeOrToken::Token(token) = child {
            if token.text_range().start() >= arg_list_start {
                break;
            }
            if token.kind() == SyntaxKind::IDENT {
                last_ident = Some(token);
            }
        }
    }
    last_ident
}

fn find_type_name_or_new_keyword(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut found_new = false;
    let mut new_keyword: Option<SyntaxToken> = None;

    for child in node.children_with_tokens() {
        if let Some(token) = child.into_token() {
            if token.kind() == SyntaxKind::KW_NEW {
                found_new = true;
                new_keyword = Some(token.clone());
                continue;
            }
            if found_new && token.kind() == SyntaxKind::IDENT {
                return Some(token);
            }
        }
    }

    new_keyword
}

#[allow(clippy::too_many_arguments)]
fn check_call_expr(
    expr_id: ExprId,
    callee: ExprId,
    args: &[ExprIdx],
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    config: &Config,
    line_index: &LineIndex,
    root: &SyntaxNode,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    if args.is_empty() {
        return;
    }

    let Some(range) = source_map.expr_range(expr_id) else {
        return;
    };

    let start_line = line_index.line_col(range.start()).line;
    let end_line = line_index.line_col(range.end()).line;
    if start_line == end_line {
        return;
    }

    if !has_forbidden_nested_call(args, body, config) {
        return;
    }

    if config.allow_oneliner && !has_multiline_param_hir(args, body, source_map, line_index) {
        return;
    }

    let method_name = match body.expr(callee) {
        Expr::Path(name) => name.as_str(),
        Expr::Field { field, .. } => field.as_str(),
        Expr::QualifiedPath(path) => path.last().as_str(),
        _ => "метод",
    };

    let name_range = find_name_token_range(range, root);

    diagnostics.push(Diagnostic {
        code,
        message: format!(
            "Убрать инициализацию параметров метода '{}' вложенными методами",
            method_name
        ),
        severity: ctx.severity(code),
        range: name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

#[allow(clippy::too_many_arguments)]
fn check_method_call_expr(
    expr_id: ExprId,
    method: &Name,
    args: &[ExprIdx],
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    config: &Config,
    line_index: &LineIndex,
    root: &SyntaxNode,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    if args.is_empty() {
        return;
    }

    let Some(range) = source_map.expr_range(expr_id) else {
        return;
    };

    let start_line = line_index.line_col(range.start()).line;
    let end_line = line_index.line_col(range.end()).line;
    if start_line == end_line {
        return;
    }

    if !has_forbidden_nested_call(args, body, config) {
        return;
    }

    if config.allow_oneliner && !has_multiline_param_hir(args, body, source_map, line_index) {
        return;
    }

    let name_range = find_name_token_range(range, root);

    diagnostics.push(Diagnostic {
        code,
        message: format!(
            "Убрать инициализацию параметров метода '{}' вложенными методами",
            method.as_str()
        ),
        severity: ctx.severity(code),
        range: name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

#[allow(clippy::too_many_arguments)]
fn check_new_expr(
    expr_id: ExprId,
    type_name: &Option<Name>,
    args: &[ExprIdx],
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    config: &Config,
    line_index: &LineIndex,
    root: &SyntaxNode,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    if args.is_empty() {
        return;
    }

    let Some(range) = source_map.expr_range(expr_id) else {
        return;
    };

    let start_line = line_index.line_col(range.start()).line;
    let end_line = line_index.line_col(range.end()).line;
    if start_line == end_line {
        return;
    }

    if !has_forbidden_nested_call(args, body, config) {
        return;
    }

    if config.allow_oneliner && !has_multiline_param_hir(args, body, source_map, line_index) {
        return;
    }

    let display_name = type_name.as_ref().map(|n| n.as_str()).unwrap_or("Новый");

    let name_range = find_name_token_range(range, root);

    diagnostics.push(Diagnostic {
        code,
        message: format!(
            "Убрать инициализацию параметров конструктора '{}' вложенными методами",
            display_name
        ),
        severity: ctx.severity(code),
        range: name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

fn has_forbidden_nested_call(args: &[ExprIdx], body: &Body, config: &Config) -> bool {
    args.iter().any(|&arg_idx| is_forbidden_call(ExprId::from_idx(arg_idx), body, config))
}

fn is_forbidden_call(expr_id: ExprId, body: &Body, config: &Config) -> bool {
    match body.expr(expr_id) {
        Expr::Call { callee, .. } => {
            if let Expr::Path(name) = body.expr(ExprId::from_idx(*callee)) {
                if config.is_allowed_method(name.as_str()) {
                    return false;
                }
            }
            true
        }
        Expr::MethodCall { .. } => true,
        Expr::New { args, .. } => !args.is_empty(),
        Expr::BinaryOp { lhs, rhs, .. } => {
            is_forbidden_call(ExprId::from_idx(*lhs), body, config)
                || is_forbidden_call(ExprId::from_idx(*rhs), body, config)
        }
        Expr::UnaryOp { expr, .. } => is_forbidden_call(ExprId::from_idx(*expr), body, config),
        Expr::Ternary { condition, then_expr, else_expr } => {
            is_forbidden_call(ExprId::from_idx(*condition), body, config)
                || is_forbidden_call(ExprId::from_idx(*then_expr), body, config)
                || is_forbidden_call(ExprId::from_idx(*else_expr), body, config)
        }
        Expr::Index { base, index } => {
            is_forbidden_call(ExprId::from_idx(*base), body, config)
                || is_forbidden_call(ExprId::from_idx(*index), body, config)
        }
        Expr::Field { base, .. } => is_forbidden_call(ExprId::from_idx(*base), body, config),
        _ => false,
    }
}

fn has_multiline_param_hir(
    args: &[ExprIdx],
    _body: &Body,
    source_map: &BodySourceMap,
    line_index: &LineIndex,
) -> bool {
    for &arg_idx in args {
        let arg_id = ExprId::from_idx(arg_idx);
        let Some(range) = source_map.expr_range(arg_id) else {
            continue;
        };

        let start_line = line_index.line_col(range.start()).line;
        let end_line = line_index.line_col(range.end()).line;
        if end_line > start_line {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, check_ast_diagnostic_with_config, format_diags};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    #[test]
    fn test_no_diagnostic_single_line() {
        let code = r#"Сообщить(СуммаСтрокой("7"), СуммаСтрокой(СуммаНДС(Перечисление.Сумма)));"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_multiline_but_single_line_params() {
        let code = r#"Сообщить(СуммаСтрокой("77"),
    СуммаСтрокой(СуммаНДС(Перечисление.ВтораяСумма)));"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_diagnostic_multiline_param() {
        let code = r#"Метод(
    ВложенныйМетод(
        Параметр
    ));"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            NestedFunctionInParameters @ 1:1..1:6
              message: Убрать инициализацию параметров метода 'Метод' вложенными методами
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_allowed_method() {
        let code = r#"ЗаписьЖурналаРегистрации(
    НСтр("ru = 'WMS.InDeliverySetEndReceiving'", ОбщегоНазначения.КодОсновногоЯзыка()),
    УровеньЖурналаРегистрации.Ошибка,
    ,
    ,
    ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())
);"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_diagnostic_with_allow_oneliner_false() {
        let code = r#"Сообщить(СуммаСтрокой("7"), СуммаСтрокой(СуммаНДС(Перечисление.Сумма)));"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::NestedFunctionInParameters,
            serde_json::json!({"allowOneliner": false}),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_empty_params() {
        let code = r#"А = Новый Массив;
Сообщить();"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_new_expr_single_line_params() {
        let code = r#"Структура = Новый Структура("Параметр1, Параметр2",
            Новый Структура(), Новый Структура("Параметр3", Новый Массив()));"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_diagnostic_new_expr_with_multiline_param() {
        let code = r#"Структура = Новый Структура("Параметр1, Параметр2",
            Новый Структура(
                "ВложенныйПараметр"
            ));"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            NestedFunctionInParameters @ 1:19..1:28
              message: Убрать инициализацию параметров конструктора 'Структура' вложенными методами
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_new_expr_without_init() {
        let code = r#"Структура = Новый Структура("Параметр1, Параметр2",
            Новый Структура, Новый Массив);"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    const FIXTURE: &str = r#"Процедура Тест() // Все комментарии про allowOneliner=false
    СтруктураВложений.Вставить(                     // <-- Ошибка, т.к. есть вложенный конструктов и метод
     ПрисоединенныйФайл.Наименование,
     Новый Картинка(ПолучитьИзВременногоХранилища(  // <-- 2 ошибки, т.к. есть вложенный метод м в конструкторе и в методе
      ПрисоединенныеФайлы.ПолучитьДанныеФайла(ПрисоединенныйФайл.Ссылка).СсылкаНаДвоичныеДанныеФайла)));

    Сообщить(СуммаСтрокой("7"), СуммаСтрокой(СуммаНДС(Перечисление.Сумма)));  // <-- не ошибка, все в одной строке

    Сообщить(СуммаСтрокой("77"),                            // <-- ошибка для Сообщить, т.к есть вложенный метод в параметрах
        СуммаСтрокой(СуммаНДС(Перечисление.ВтораяСумма)));  // <-- не ошибка для СуммаСтрокой, т.к. в одной строке
    А = Новый Массив;
    Сообщить();
    Объект.Метод().Метод2(Объект2.Метод2()); // <-- не ошибка, все в одной строке
    Объект.Метод(Объект2.Метод2()).Метод21( // <-- ошибка для Метод21, т.к. есть вложенный метод
        Объект2.Метод22());

    Структура = Новый Структура("Параметр1, Параметр2", Новый Массив(), Новый СписокЗначений()); // <-- не ошибка, все в одной строке
    Структура = Новый Структура("Параметр1, Параметр2",                             // <-- ошибка, есть вложенный конструктор с инициализацией
                Новый Структура(), Новый Структура("Параметр3", Новый Массив()));   // <-- ошибок нет

    Структура = Новый Структура("Параметр1, Параметр2",                             // <-- ошибок нет, т.к. конструктор без инициализации
                Новый Структура, Новый Массив);

КонецПроцедуры

&НаКлиенте
Процедура ПолучателиВыбор(Элемент, ВыбраннаяСтрока, Поле, СтандартнаяОбработка)

	Если Поле <> Элементы.ПолучателиСостояниеСообщенияSMS Тогда
		Возврат;
	КонецЕсли;

	Отбор = Новый Структура;
	Отбор.Вставить("МассоваяРассылка", Объект.Ссылка);
	Отбор.Вставить("КакСвязаться", Элемент.ТекущиеДанные.КакСвязаться);

	КлючЗаписи = Новый(
	Тип("РегистрСведенийКлючЗаписи.ОчередьРассылок"),
	ОбщегоНазначенияКлиентСервер.ЗначениеВМассиве(Отбор));

	ПараметрыФормыЗаписи = Новый Структура;
	ПараметрыФормыЗаписи.Вставить("Ключ", КлючЗаписи);

	ОткрытьФорму("РегистрСведений.ОчередьРассылок.ФормаЗаписи", ПараметрыФормыЗаписи);

КонецПроцедуры
Процедура Тест2()
    СсылкаНаОбъект = РегистрыСведений.СоответствиеСсылокИдентификаторам.ПолучитьСсылкуНаОбъект(
        Новый УникальныйИдентификатор(ИдентификаторОбъекта)
	);

    СсылкаНаОбъект = РегистрыСведений.СоответствиеСсылокИдентификаторам.ПолучитьСсылкуНаОбъект(
    	    Новый УникальныйИдентификатор(ИдентификаторОбъекта,
    	    Истина)
    );

    ЗаписьЖурналаРегистрации(
        НСтр("ru = 'WMS.InDeliverySetEndReceiving'", ОбщегоНазначения.КодОсновногоЯзыка()),
        УровеньЖурналаРегистрации.Ошибка,
        ,
        ,
        ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())
    );
КонецПроцедуры

Процедура Тест3()
    Значение = Метод(ПредопределенноеЗначение("Справочник.Контрагенты.ПустаяСсылка"), // нет ошибки
        ПредопределенноеЗначение("Справочник.Организации.ПустаяСсылка"));

    Значение2 = Метод(ПредопределенноеЗначение("Справочник.Контрагенты.ПустаяСсылка"),
            ДругаяФункция("Справочник.Организации.ПустаяСсылка"));                    // ошибка

    Значение = Метод(ПредопределенноеЗначение("Справочник.Контрагенты.ПустаяСсылка"), // Нет ошибки
            NStr("ru = 'Сообщение'"));

    PerformCalculations.RecalculateAccruals(                                          // Нет ошибки
        PredefinedValue("ChartOfCalculationTypes.MainAccruals.Salary"),
        PredefinedValue("ChartOfCalculationTypes.MainAccruals.Bonus"));

    PerformCalculations.RecalculateAccruals(Метод2(Параметр),                         // ошибка
        PredefinedValue("ChartOfCalculationTypes.MainAccruals.Salary"));

    Метод2(PredefinedValue(                                                           // Нет ошибки
        NStr("ru = 'ChartOfCalculationTypes.MainAccruals.Salary'"));
КонецПроцедуры
"#;

    #[test]
    fn test_comprehensive() {
        let diagnostics = check_ast_diagnostic(FIXTURE, check);

        expect![[r#"
            NestedFunctionInParameters @ 2:23..2:31
              message: Убрать инициализацию параметров метода 'Вставить' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 4:12..4:20
              message: Убрать инициализацию параметров конструктора 'Картинка' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 52:73..52:95
              message: Убрать инициализацию параметров метода 'ПолучитьСсылкуНаОбъект' вложенными методами
              severity: Information"#]]
        .assert_eq(&format_diags(FIXTURE, &diagnostics));
    }

    #[test]
    fn test_comprehensive_allow_oneliner_false() {
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::NestedFunctionInParameters,
            serde_json::json!({"allowOneliner": false}),
        );
        let diagnostics = check_ast_diagnostic_with_config(FIXTURE, config, check);

        expect![[r#"
            NestedFunctionInParameters @ 2:23..2:31
              message: Убрать инициализацию параметров метода 'Вставить' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 4:12..4:20
              message: Убрать инициализацию параметров конструктора 'Картинка' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 4:21..4:50
              message: Убрать инициализацию параметров метода 'ПолучитьИзВременногоХранилища' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 9:5..9:13
              message: Убрать инициализацию параметров метода 'Сообщить' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 14:36..14:43
              message: Убрать инициализацию параметров метода 'Метод21' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 18:23..18:32
              message: Убрать инициализацию параметров конструктора 'Структура' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 37:15..37:20
              message: Убрать инициализацию параметров конструктора 'Новый' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 48:73..48:95
              message: Убрать инициализацию параметров метода 'ПолучитьСсылкуНаОбъект' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 52:73..52:95
              message: Убрать инициализацию параметров метода 'ПолучитьСсылкуНаОбъект' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 57:5..57:29
              message: Убрать инициализацию параметров метода 'ЗаписьЖурналаРегистрации' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 70:17..70:22
              message: Убрать инициализацию параметров метода 'Метод' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 80:25..80:44
              message: Убрать инициализацию параметров метода 'RecalculateAccruals' вложенными методами
              severity: Information"#]].assert_eq(&format_diags(FIXTURE, &diagnostics));
    }

    #[test]
    fn test_comprehensive_custom_allowed_methods() {
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::NestedFunctionInParameters,
            serde_json::json!({
                "allowedMethodNames": "НСтр, ПредопределенноеЗначение, PredefinedValue, ДругаяФункция",
                "allowOneliner": false
            }),
        );
        let diagnostics = check_ast_diagnostic_with_config(FIXTURE, config, check);

        expect![[r#"
            NestedFunctionInParameters @ 2:23..2:31
              message: Убрать инициализацию параметров метода 'Вставить' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 4:12..4:20
              message: Убрать инициализацию параметров конструктора 'Картинка' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 4:21..4:50
              message: Убрать инициализацию параметров метода 'ПолучитьИзВременногоХранилища' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 9:5..9:13
              message: Убрать инициализацию параметров метода 'Сообщить' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 14:36..14:43
              message: Убрать инициализацию параметров метода 'Метод21' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 18:23..18:32
              message: Убрать инициализацию параметров конструктора 'Структура' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 37:15..37:20
              message: Убрать инициализацию параметров конструктора 'Новый' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 48:73..48:95
              message: Убрать инициализацию параметров метода 'ПолучитьСсылкуНаОбъект' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 52:73..52:95
              message: Убрать инициализацию параметров метода 'ПолучитьСсылкуНаОбъект' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 57:5..57:29
              message: Убрать инициализацию параметров метода 'ЗаписьЖурналаРегистрации' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 73:16..73:21
              message: Убрать инициализацию параметров метода 'Метод' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 80:25..80:44
              message: Убрать инициализацию параметров метода 'RecalculateAccruals' вложенными методами
              severity: Information
            NestedFunctionInParameters @ 83:12..83:27
              message: Убрать инициализацию параметров метода 'PredefinedValue' вложенными методами
              severity: Information"#]].assert_eq(&format_diags(FIXTURE, &diagnostics));
    }
}
