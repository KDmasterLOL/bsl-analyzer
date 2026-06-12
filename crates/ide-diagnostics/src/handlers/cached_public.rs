use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::ReturnValueReuse;
use stdx::case::CaseExt;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CachedPublic;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let region_tree = ctx.region_tree();

    let public_regions: Vec<_> = region_tree
        .regions()
        .filter(|(_, region)| is_public_region(region.name.as_str()))
        .collect();

    if public_regions.is_empty() {
        return Vec::new();
    }

    let metadata = ctx.module_metadata();

    let common_module = match &metadata.common_module {
        Some(cm) => cm,
        None => return Vec::new(),
    };

    if !is_cached_reuse(common_module.return_values_reuse()) {
        return Vec::new();
    }

    let item_tree = ctx.item_tree();

    public_regions
        .into_iter()
        .filter_map(|(_, region)| {
            let has_methods = item_tree
                .procedures()
                .any(|(_, proc)| region.range.contains_range(proc.source_range))
                || item_tree
                    .functions()
                    .any(|(_, func)| region.range.contains_range(func.source_range));

            if has_methods {
                Some(Diagnostic {
                    code: DiagnosticCode::CachedPublic,
                    message: "Кэшируемый модуль не должен содержать методы в публичных областях"
                        .to_string(),
                    severity: ctx.severity(code),
                    range: region.range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                })
            } else {
                None
            }
        })
        .collect()
}

fn is_cached_reuse(reuse: ReturnValueReuse) -> bool {
    matches!(reuse, ReturnValueReuse::DuringRequest | ReturnValueReuse::DuringSession)
}

fn is_public_region(region_name: &str) -> bool {
    let name_lower = region_name.fold_lower();
    name_lower == "public" || name_lower == "программныйинтерфейс"
}

#[cfg(test)]
fn check_with_reuse(ctx: &DiagnosticsContext, reuse: ReturnValueReuse) -> Vec<Diagnostic> {
    if !is_cached_reuse(reuse) {
        return Vec::new();
    }

    let region_tree = ctx.region_tree();

    let public_regions: Vec<_> = region_tree
        .regions()
        .filter(|(_, region)| is_public_region(region.name.as_str()))
        .collect();

    if public_regions.is_empty() {
        return Vec::new();
    }

    let item_tree = ctx.item_tree();

    public_regions
        .into_iter()
        .filter_map(|(_, region)| {
            let has_methods = item_tree
                .procedures()
                .any(|(_, proc)| region.range.contains_range(proc.source_range))
                || item_tree
                    .functions()
                    .any(|(_, func)| region.range.contains_range(func.source_range));

            if has_methods {
                let code = DiagnosticCode::CachedPublic;
                Some(Diagnostic {
                    code,
                    message: "Кэшируемый модуль не должен содержать методы в публичных областях"
                        .to_string(),
                    severity: ctx.severity(code),
                    range: region.range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;
    use vfs::file_set::FileSet;
    use vfs::VfsPath;
    fn create_test_ctx(code: &str) -> (Rc<dyn RootDatabase>, vfs::FileId, DiagnosticsConfig) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, code);
        let db = Rc::new(db) as Rc<dyn RootDatabase>;

        (db, file_id, DiagnosticsConfig::default())
    }

    #[test]
    fn test_no_common_module_metadata() {
        let code = r#"
#Область ПрограммныйИнтерфейс
Процедура Метод1()
КонецПроцедуры
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 0, "Should skip when no CommonModule metadata");
    }

    #[test]
    fn test_non_public_region_ignored() {
        let code = r#"
#Область СлужебныйПрограммныйИнтерфейс
Процедура Метод1()
КонецПроцедуры
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_empty_public_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_during_request_finds_public_regions() {
        let code = r#"#Область ПрограммныйИнтерфейс
Процедура ПолучитьНастройки()
КонецПроцедуры
#КонецОбласти

#Область ВнутренниеПроцедуры
Процедура ПодготовитьКэш()
КонецПроцедуры
#КонецОбласти

#Область Public
Функция ПолучитьВерсию()
    Возврат "1.0";
КонецФункции
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringRequest);

        assert_eq!(diagnostics.len(), 2, "Should find 2 public regions with methods");

        let (first_line, _, _, _) =
            crate::test_utils::range_to_line_col(code, diagnostics[0].range);
        assert_eq!(first_line, 0, "First diagnostic at line 0");

        let (second_line, _, _, _) =
            crate::test_utils::range_to_line_col(code, diagnostics[1].range);
        assert_eq!(second_line, 10, "Second diagnostic at line 10");
    }

    #[test]
    fn test_during_session_finds_public_regions() {
        let code = r#"#Область ПрограммныйИнтерфейс
Процедура ПолучитьНастройки()
КонецПроцедуры
#КонецОбласти

#Область ВнутренниеПроцедуры
Процедура ПодготовитьКэш()
КонецПроцедуры
#КонецОбласти

#Область Public
Функция ПолучитьВерсию()
    Возврат "1.0";
КонецФункции
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringSession);
        assert_eq!(diagnostics.len(), 2, "DuringSession is also cached");
    }

    #[test]
    fn test_dont_use_skips_check() {
        let code = r#"#Область ПрограммныйИнтерфейс
Процедура ПолучитьНастройки()
КонецПроцедуры
#КонецОбласти

#Область ВнутренниеПроцедуры
Процедура ПодготовитьКэш()
КонецПроцедуры
#КонецОбласти

#Область Public
Функция ПолучитьВерсию()
    Возврат "1.0";
КонецФункции
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DontUse);
        assert_eq!(diagnostics.len(), 0, "DontUse means not cached");
    }

    #[test]
    fn test_is_cached_reuse() {
        assert!(is_cached_reuse(ReturnValueReuse::DuringRequest));
        assert!(is_cached_reuse(ReturnValueReuse::DuringSession));
        assert!(!is_cached_reuse(ReturnValueReuse::DontUse));
    }

    #[test]
    fn test_is_public_region_russian() {
        assert!(is_public_region("ПрограммныйИнтерфейс"));
        assert!(is_public_region("программныйинтерфейс"));
        assert!(is_public_region("ПРОГРАММНЫЙИНТЕРФЕЙС"));
    }

    #[test]
    fn test_is_public_region_english() {
        assert!(is_public_region("Public"));
        assert!(is_public_region("public"));
        assert!(is_public_region("PUBLIC"));
    }

    #[test]
    fn test_is_not_public_region() {
        assert!(!is_public_region("СлужебныйПрограммныйИнтерфейс"));
        assert!(!is_public_region("Private"));
        assert!(!is_public_region("Internal"));
        assert!(!is_public_region(""));
    }

    #[test]
    fn test_public_region_with_function() {
        let code = r#"#Область ПрограммныйИнтерфейс
Функция ПолучитьДанные()
    Возврат 1;
КонецФункции
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringRequest);
        assert_eq!(diagnostics.len(), 1, "Function should trigger diagnostic");
    }

    #[test]
    fn test_multiple_methods_in_public_region() {
        let code = r#"#Область ПрограммныйИнтерфейс
Процедура Первая()
КонецПроцедуры
Функция Вторая()
    Возврат 1;
КонецФункции
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringRequest);
        assert_eq!(diagnostics.len(), 1, "One region = one diagnostic");
    }

    #[test]
    fn test_nested_regions() {
        let code = r#"#Область ПрограммныйИнтерфейс
Процедура Метод1()
КонецПроцедуры
#КонецОбласти

#Область СлужебныйПрограммныйИнтерфейс
Процедура Метод2()
КонецПроцедуры
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringRequest);
        assert_eq!(diagnostics.len(), 1, "Only public region triggers diagnostic");
    }
}
