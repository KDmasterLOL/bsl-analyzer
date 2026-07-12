use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::execution_env::EnvFlags;
use hir::{EnvMemberKind, Name};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "warning",
};

pub fn from_hir(
    name: &Name,
    member_kind: EnvMemberKind,
    missing: EnvFlags,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let (kind_ru, suffix_ru) = match member_kind {
        EnvMemberKind::Method => ("Метод", "недоступен"),
        EnvMemberKind::Property => ("Свойство", "недоступно"),
        EnvMemberKind::GlobalFunction => ("Глобальная функция", "недоступна"),
        EnvMemberKind::GlobalProperty => ("Глобальное свойство", "недоступно"),
    };
    let envs: Vec<&str> = missing.iter().map(|flag| flag.name_ru()).collect();
    let message = format!("{} '{}' {} [{}]", kind_ru, name.as_str(), suffix_ru, envs.join(", "));
    crate::simple_hir_diagnostic(DiagnosticCode::UnavailableInEnvironment, message, range, ctx)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic_with_fixtures;
    use crate::DiagnosticCode;

    fn env_diags(fixture: &str) -> Vec<(String, String)> {
        check_hir_diagnostic_with_fixtures(fixture)
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::UnavailableInEnvironment)
            .map(|d| (d.message, String::new()))
            .collect()
    }

    #[test]
    fn server_only_type_method_flagged_in_client_form_method() {
        // ЧтениеТекста is unavailable in the web client; a form method behind
        // &НаКлиенте runs in every configured client environment.
        let fixture = r#"
//- /Catalogs/Товары/Forms/ФормаЭлемента/Ext/Form/Module.bsl
&НаСервере
Функция ПрочитатьНаСервере()
    Чтение = Новый ЧтениеТекста;
    Возврат Чтение.ПрочитатьСтроку();
КонецФункции
"#;
        let diags = env_diags(fixture);
        assert!(diags.is_empty(), "server context admits ЧтениеТекста, got: {diags:?}");

        let fixture_client = r#"
//- /Catalogs/Товары/Forms/ФормаЭлемента/Ext/Form/Module.bsl
&НаКлиенте
Функция ПрочитатьНаКлиенте()
    Чтение = Новый ЧтениеТекста;
    Возврат Чтение.ПрочитатьСтроку();
КонецФункции
"#;
        let diags = env_diags(fixture_client);
        assert_eq!(diags.len(), 1, "web client lacks ЧтениеТекста methods, got: {diags:?}");
        assert!(
            diags[0].0.contains("Веб-клиент"),
            "qualifier must name the missing environment: {}",
            diags[0].0
        );
        assert!(
            !diags[0].0.contains("Тонкий клиент"),
            "thin client has ЧтениеТекста — must not be reported: {}",
            diags[0].0
        );
    }

    #[test]
    fn preprocessor_branch_suppresses_check() {
        let fixture = r#"
//- /Catalogs/Товары/Forms/ФормаЭлемента/Ext/Form/Module.bsl
&НаКлиенте
Процедура Прочитать()
    #Если НЕ ВебКлиент Тогда
    Чтение = Новый ЧтениеТекста;
    Стр = Чтение.ПрочитатьСтроку();
    #КонецЕсли
КонецПроцедуры
"#;
        let diags = env_diags(fixture);
        assert!(diags.is_empty(), "code inside #Если must not be flagged, got: {diags:?}");
    }

    #[test]
    fn server_only_global_function_flagged_on_client() {
        let fixture = r#"
//- /Catalogs/Товары/Forms/ФормаЭлемента/Ext/Form/Module.bsl
&НаКлиенте
Процедура Прочитать()
    ЗаписьЖурналаРегистрации("Событие");
КонецПроцедуры
"#;
        let diags = env_diags(fixture);
        assert_eq!(
            diags.len(),
            1,
            "ЗаписьЖурналаРегистрации is not available on thin/web clients, got: {diags:?}"
        );
        assert!(
            diags[0].0.contains("Тонкий клиент") && diags[0].0.contains("Веб-клиент"),
            "qualifier must list both missing client environments: {}",
            diags[0].0
        );
        assert!(
            !diags[0].0.contains("управляемое приложение"),
            "thick client is available — must not be reported: {}",
            diags[0].0
        );
    }

    #[test]
    fn common_module_without_client_flag_not_checked_against_client() {
        // A server common module calling server API — no diagnostics.
        let fixture = r#"
//- /CommonModules/Серверный/Ext/Module.bsl
Процедура Прочитать() Экспорт
    Чтение = Новый ЧтениеТекста;
    Стр = Чтение.ПрочитатьСтроку();
КонецПроцедуры
"#;
        let diags = env_diags(fixture);
        assert!(diags.is_empty(), "server module may use server API, got: {diags:?}");
    }
}
