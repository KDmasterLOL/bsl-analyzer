use ide_db::RootDatabase;
use symbol_info::{
    build_signature, render_signature_help, resolve_callee_at, ParameterInfoView, SignatureHelpView,
};
use syntax::TextSize;
use vfs::FileId;

#[derive(Debug, Clone)]
pub struct SignatureHelp {
    pub signature: String,
    pub doc: Option<String>,
    pub active_parameter: Option<usize>,
    pub parameters: Vec<ParameterInfo>,
}

#[derive(Debug, Clone)]
pub struct ParameterInfo {
    pub label: String,
    pub documentation: Option<String>,
}

pub fn signature_help<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Option<SignatureHelp> {
    let _span = tracing::info_span!("signature_help", ?file_id, ?offset).entered();

    let (callee, active) = resolve_callee_at(db, file_id, offset)?;
    let sig = build_signature(db, file_id, &callee)?;
    Some(from_view(render_signature_help(&sig, active.index)))
}

fn from_view(view: SignatureHelpView) -> SignatureHelp {
    SignatureHelp {
        signature: view.signature,
        doc: view.doc,
        active_parameter: view.active_parameter,
        parameters: view.parameters.into_iter().map(from_param_view).collect(),
    }
}

fn from_param_view(p: ParameterInfoView) -> ParameterInfo {
    ParameterInfo { label: p.label, documentation: p.documentation }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::vfs::{file_set::FileSet, VfsPath};
    use ide_db::RootDatabaseImpl;

    fn setup_db(code: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, code);
        (db, file_id)
    }

    fn find_cursor(code: &str) -> (String, TextSize) {
        let cursor_pos = code.find("$0").expect("No cursor marker $0 found");
        let code_without_cursor = code.replace("$0", "");
        (code_without_cursor, TextSize::from(cursor_pos as u32))
    }

    #[test]
    fn test_global_function_signature() {
        let code = "Процедура Тест()
    НачатьТранзакцию($0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);

        if let Some(sig) = result {
            assert!(sig.signature.contains("НачатьТранзакцию"));
        }
    }

    #[test]
    fn test_type_conversion_function() {
        let code = "Процедура Тест()
    Строка($0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);

        if let Some(sig) = result {
            assert!(sig.signature.contains("Строка"));
            assert_eq!(sig.active_parameter, Some(0));
        }
    }

    #[test]
    fn test_user_function_signature() {
        let code = "Функция МояФункция(Параметр1, Знач Параметр2)
    Возврат 1;
КонецФункции

Процедура Тест()
    МояФункция($0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);

        if let Some(sig) = result {
            assert!(sig.signature.contains("МояФункция"));
            assert!(sig.signature.contains("Параметр1"));
            assert!(sig.signature.contains("Параметр2"));
            assert!(
                !sig.signature.contains("Знач"),
                "Signature help must not surface the `Знач` modifier, got: {}",
                sig.signature
            );
            assert_eq!(sig.active_parameter, Some(0));
        }
    }

    #[test]
    fn test_second_parameter_active() {
        let code = "Функция МояФункция(Параметр1, Параметр2)
    Возврат 1;
КонецФункции

Процедура Тест()
    МояФункция(1, $0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);

        if let Some(sig) = result {
            assert_eq!(sig.active_parameter, Some(1));
        }
    }

    #[test]
    fn test_outside_call_no_signature() {
        let code = "Процедура Тест()
    Функция()$0
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);
        assert!(result.is_none());
    }

    #[test]
    fn test_nested_call() {
        let code = "Функция Внешняя(А)
    Возврат А;
КонецФункции

Функция Внутренняя(Б)
    Возврат Б;
КонецФункции

Процедура Тест()
    Внешняя(Внутренняя($0))
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);

        if let Some(sig) = result {
            assert!(sig.signature.contains("Внутренняя"));
        }
    }

    #[test]
    fn test_common_module_method_signature() {
        let mut db = RootDatabaseImpl::new();

        let module_file_id = FileId(1);
        let module_code = "// Проверяет, является ли символ разделителем слов.
//
// Параметры:
//  КодСимвола - Число - код проверяемого символа
//  РазделителиСлов - Строка - допустимые разделители
//
// Возвращаемое значение:
//  Булево - Истина, если символ является разделителем
//
Функция ЭтоРазделительСлов(КодСимвола, РазделителиСлов = \" \") Экспорт
    Возврат Истина;
КонецФункции";

        let caller_file_id = FileId(0);
        let caller_code = "Процедура Тест()
    СтроковыеФункцииКлиентСервер.ЭтоРазделительСлов($0)
КонецПроцедуры";

        let (caller_code, offset) = find_cursor(caller_code);

        let mut file_set = FileSet::new();
        file_set.insert(
            module_file_id,
            VfsPath::new("/cf/CommonModules/СтроковыеФункцииКлиентСервер/Ext/Module.bsl"),
        );
        file_set.insert(caller_file_id, VfsPath::new("/cf/HTTPServices/lk/Ext/Module.bsl"));

        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(module_file_id, SourceRootId(0));
        db.set_file_source_root(caller_file_id, SourceRootId(0));
        db.set_file_text(module_file_id, module_code);
        db.set_file_text(caller_file_id, &caller_code);

        let result = signature_help(&db, caller_file_id, offset);

        assert!(result.is_some(), "Expected signature help for CommonModule method");
        let sig = result.unwrap();
        assert!(
            sig.signature.contains("ЭтоРазделительСлов"),
            "Signature should contain method name, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("КодСимвола"),
            "Signature should contain first param, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("КодСимвола: Число"),
            "Param should be enriched with its type from docs, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("Булево"),
            "Function signature should include return type from docs, got: {}",
            sig.signature
        );
        assert_eq!(sig.active_parameter, Some(0));

        let first = &sig.parameters[0];
        let doc = first.documentation.as_deref().unwrap_or("");
        assert!(
            doc.contains("код проверяемого символа"),
            "Param documentation should contain the description from docs, got: {:?}",
            first.documentation
        );
    }

    #[test]
    fn test_common_module_method_signature_union_types() {
        let mut db = RootDatabaseImpl::new();

        let module_file_id = FileId(1);
        let module_code = "// Возвращает значения реквизита.
//
// Параметры:
//  Ссылка       - ЛюбаяСсылка - объект, значения реквизитов которого получить.
//               - Строка      - полное имя предопределенного элемента.
//  ИмяРеквизита - Строка      - имя получаемого реквизита.
//
// Возвращаемое значение:
//  Произвольный - значение реквизита.
//
Функция ЗначениеРеквизитаОбъекта(Ссылка, ИмяРеквизита) Экспорт
    Возврат Неопределено;
КонецФункции";

        let caller_file_id = FileId(0);
        let caller_code = "Процедура Тест()
    ОбщегоНазначения.ЗначениеРеквизитаОбъекта($0)
КонецПроцедуры";

        let (caller_code, offset) = find_cursor(caller_code);

        let mut file_set = FileSet::new();
        file_set.insert(
            module_file_id,
            VfsPath::new("/cf/CommonModules/ОбщегоНазначения/Ext/Module.bsl"),
        );
        file_set.insert(caller_file_id, VfsPath::new("/cf/HTTPServices/lk/Ext/Module.bsl"));

        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(module_file_id, SourceRootId(0));
        db.set_file_source_root(caller_file_id, SourceRootId(0));
        db.set_file_text(module_file_id, module_code);
        db.set_file_text(caller_file_id, &caller_code);

        let result = signature_help(&db, caller_file_id, offset);

        let sig = result.expect("Expected signature help for the common-module method");
        assert!(
            sig.signature.contains("Ссылка: ЛюбаяСсылка | Строка"),
            "Both alternative types must appear next to the parameter name, joined with ' | ', got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("ИмяРеквизита: Строка"),
            "Single-typed parameter should still get its type, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("Произвольный"),
            "Function return type from docs should appear, got: {}",
            sig.signature
        );
        assert_eq!(sig.parameters.len(), 2, "Expected exactly 2 declared parameters");

        let ssylka_doc = sig.parameters[0].documentation.as_deref().unwrap_or("");
        assert!(
            !ssylka_doc.contains("**"),
            "Union-typed parameter doc must not duplicate types in bold, got: {:?}",
            sig.parameters[0].documentation
        );
        let pos_any = ssylka_doc.find("объект, значения").expect("ЛюбаяСсылка description");
        let pos_str = ssylka_doc.find("полное имя").expect("Строка description");
        assert!(
            pos_any < pos_str,
            "Union descriptions must keep declaration order, got: {:?}",
            sig.parameters[0].documentation
        );

        let name_doc = sig.parameters[1].documentation.as_deref().unwrap_or("");
        assert!(
            !name_doc.contains("**"),
            "Single-typed parameter doc must not duplicate the type, got: {:?}",
            sig.parameters[1].documentation
        );
        assert!(
            name_doc.contains("имя получаемого реквизита"),
            "Single-typed parameter doc should still carry the description, got: {:?}",
            sig.parameters[1].documentation
        );
    }

    #[test]
    fn test_manager_module_method_signature() {
        let mut db = RootDatabaseImpl::new();

        let module_file_id = FileId(1);
        let module_code = "// Возвращает варианты выбора группы складов.
//
// Параметры:
//  Параметры - Структура - параметры выбора.
//
// Возвращаемое значение:
//  Массив - варианты выбора.
//
Функция ВариантыВыбораГруппыСкладов(Параметры) Экспорт
    Возврат Новый Массив;
КонецФункции";

        let caller_file_id = FileId(0);
        let caller_code = "Процедура Тест()
    Справочники.Склады.ВариантыВыбораГруппыСкладов($0)
КонецПроцедуры";

        let (caller_code, offset) = find_cursor(caller_code);

        let mut file_set = FileSet::new();
        file_set.insert(module_file_id, VfsPath::new("/cf/Catalogs/Склады/Ext/ManagerModule.bsl"));
        file_set.insert(caller_file_id, VfsPath::new("/cf/HTTPServices/lk/Ext/Module.bsl"));

        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(module_file_id, SourceRootId(0));
        db.set_file_source_root(caller_file_id, SourceRootId(0));
        db.set_file_text(module_file_id, module_code);
        db.set_file_text(caller_file_id, &caller_code);

        let sig = signature_help(&db, caller_file_id, offset)
            .expect("Manager-module method should resolve via module_index.resolve_manager");
        assert!(
            sig.signature.contains("ВариантыВыбораГруппыСкладов"),
            "Signature must include the method name, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("Параметры: Структура"),
            "Parameter must be enriched with its declared type, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("Массив"),
            "Function return type from docs should appear, got: {}",
            sig.signature
        );
    }

    #[test]
    fn test_platform_method_variable_shadows_platform_type_name() {
        let code = "Процедура Тест()
    Массив = Новый СписокЗначений;
    Массив.Добавить($0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let sig = signature_help(&db, file_id, offset)
            .expect("signature help must resolve through inferred Ty, not literal name");
        assert!(
            sig.signature.contains("Добавить"),
            "signature must include the method name, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("Представление"),
            "signature must come from СписокЗначений, not Массив (missing `Представление` parameter), got: {}",
            sig.signature
        );
    }

    #[test]
    fn test_platform_method_signature_on_keyword_method_name() {
        let code = "Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Выполнить($0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let sig = signature_help(&db, file_id, offset)
            .expect("signature help must resolve Запрос.Выполнить despite KW_EXECUTE token kind");
        assert!(
            sig.signature.contains("Выполнить"),
            "signature must include the method name, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("РезультатЗапроса"),
            "signature must surface Query.Execute's return type — proves the platform-method overload was selected. got: {}",
            sig.signature
        );
    }

    #[test]
    fn test_platform_method_on_instance_variable() {
        let code = "Процедура Тест()
    МойМассив = Новый Массив;
    МойМассив.Добавить($0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let sig = signature_help(&db, file_id, offset).expect(
            "signature help must resolve platform methods through the receiver's inferred Ty",
        );
        assert!(
            sig.signature.contains("Добавить"),
            "signature must include the platform method name, got: {}",
            sig.signature
        );
        assert_eq!(sig.active_parameter, Some(0));
    }

    #[test]
    fn test_platform_method_signature_on_chained_call_return_type() {
        let code = "Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = \"Выбрать 0\";
    Результат = Запрос.Выполнить().Выгрузить($0);
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let sig = signature_help(&db, file_id, offset).expect(
            "signature help must resolve РезультатЗапроса.Выгрузить through the chained call's inferred return type",
        );
        assert!(
            sig.signature.contains("Выгрузить"),
            "signature must include the method name, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("ТипОбхода"),
            "signature must include the optional parameter `ТипОбхода`, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("ОбходРезультатаЗапроса"),
            "signature must surface the parameter's platform type `ОбходРезультатаЗапроса`, got: {}",
            sig.signature
        );
        assert_eq!(
            sig.parameters.len(),
            1,
            "QueryResult.Выгрузить has exactly one declared parameter, got: {:?}",
            sig.parameters
        );
        assert_eq!(sig.active_parameter, Some(0));
    }

    #[test]
    fn test_platform_method_signature_on_chained_union_first_arm_wins() {
        let code = "Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Выполнить().Выгрузить().Скопировать($0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let sig = signature_help(&db, file_id, offset).expect(
            "signature help must dispatch through a multi-arm Union (no sentinels) and pick the first arm",
        );
        assert!(
            sig.signature.contains("Скопировать"),
            "signature must include the method name, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("Строки") || sig.signature.contains("Колонки"),
            "signature must come from ТаблицаЗначений::Скопировать (not the zero-param ДеревоЗначений overload), got: {}",
            sig.signature
        );
        assert!(
            !sig.parameters.is_empty(),
            "ТаблицаЗначений::Скопировать has parameters; an empty list would mean ДеревоЗначений won — got: {:?}",
            sig.parameters
        );
    }

    #[test]
    fn test_constructor_signature_on_array() {
        let code = "Процедура Тест()
    Х = Новый Массив($0);
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let sig =
            signature_help(&db, file_id, offset).expect("constructor signature help must fire");
        assert!(
            sig.signature.contains("Новый"),
            "signature must carry the `Новый ` prefix, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("Массив"),
            "signature must carry the type name, got: {}",
            sig.signature
        );
        assert_eq!(sig.active_parameter, Some(0));
    }

    #[test]
    fn test_constructor_signature_unknown_type() {
        let code = "Процедура Тест()
    Х = Новый ЗаведомоНесуществующийТип($0);
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);
        assert!(signature_help(&db, file_id, offset).is_none());
    }

    #[test]
    fn test_constructor_signature_on_structure() {
        let code = "Процедура Тест()
    С = Новый Структура(\"a\", $0);
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        if let Some(sig) = signature_help(&db, file_id, offset) {
            assert!(
                sig.signature.contains("Структура"),
                "expected `Структура` in signature, got: {}",
                sig.signature
            );
            if let Some(idx) = sig.active_parameter {
                assert!(idx <= 1, "cursor past first comma must be 0 or 1, got: {idx}");
            }
        }
    }
}
