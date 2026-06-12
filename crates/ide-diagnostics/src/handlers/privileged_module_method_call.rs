use bsl_metadata::traits::MdObject;
use stdx::case::CaseExt;

use crate::define_metadata;
use crate::metadata::*;
use crate::{common_module_helpers, Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::call_graph::{CallEdge, CallTarget, CallerId, EdgeKind};
use hir::dataflow::guard_predicates::{default_registry, is_stmt_guarded, GuardRegistry};
use hir::PathResolution;
use ide_db::TextRange;
use rustc_hash::FxHashSet;
use std::sync::Arc;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 60,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_VALIDATE_NESTED_CALLS: bool = true;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::PrivilegedModuleMethodCall;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let validate_nested_calls = ctx
        .config
        .get_bool(DiagnosticCode::PrivilegedModuleMethodCall, "validateNestedCalls")
        .unwrap_or(DEFAULT_VALIDATE_NESTED_CALLS);

    let privileged_modules: FxHashSet<String> = ctx
        .visible_configurations()
        .iter()
        .flat_map(|vc| vc.config.configuration.common_modules().to_vec())
        .filter(|m| m.is_privileged())
        .map(|m| m.name().fold_lower())
        .collect();

    if privileged_modules.is_empty() {
        return Vec::new();
    }

    let current_module_name = common_module_helpers::find_common_module_for_file_anywhere(ctx)
        .map(|m| m.name().to_string());

    let is_current_privileged = current_module_name
        .as_ref()
        .is_some_and(|name| privileged_modules.contains(&name.fold_lower()));

    if !validate_nested_calls && is_current_privileged {
        return Vec::new();
    }

    let summary = ctx.call_summary(hir::ModuleId::new(ctx.file_id));

    let guard_registry = default_registry();
    let mut module_bodies: Option<Arc<hir::ModuleBodies>> = None;
    let mut module_cfgs: Option<Arc<hir::cfg::ModuleCfgs>> = None;

    let mut diagnostics = Vec::new();

    for edge in &summary.call_edges {
        if edge.kind != EdgeKind::DirectQualifiedModule {
            continue;
        }

        if let CallTarget::QualifiedModule { module_name, method_name } = &edge.target {
            let module_lower = module_name.as_str().fold_lower();

            if !privileged_modules.contains(&module_lower) {
                continue;
            }

            if !validate_nested_calls {
                if let Some(ref current) = current_module_name {
                    if current.fold_lower() == module_lower {
                        continue;
                    }
                }
            }

            let resolution = ctx.resolve_qualified_path(module_name, method_name);
            if matches!(resolution, PathResolution::Method(_)) {
                if is_call_guarded(ctx, edge, &guard_registry, &mut module_bodies, &mut module_cfgs)
                {
                    continue;
                }

                diagnostics.push(Diagnostic {
                    code,
                    message: format!(
                        "Проверьте обращение к методу {} привилегированного модуля",
                        method_name.as_str()
                    ),
                    severity: ctx.severity(code),
                    range: edge.range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    diagnostics
}

fn is_call_guarded(
    ctx: &DiagnosticsContext,
    edge: &CallEdge,
    registry: &GuardRegistry,
    bodies_cache: &mut Option<Arc<hir::ModuleBodies>>,
    cfgs_cache: &mut Option<Arc<hir::cfg::ModuleCfgs>>,
) -> bool {
    let CallerId::Method(caller_local_id) = edge.caller else {
        return false;
    };

    let bodies = bodies_cache.get_or_insert_with(|| ctx.module_bodies()).clone();
    let cfgs = cfgs_cache.get_or_insert_with(|| ctx.module_cfgs()).clone();

    let body = match bodies.body(caller_local_id) {
        Some(b) => b,
        None => return false,
    };
    let source_map = match bodies.source_map(caller_local_id) {
        Some(s) => s,
        None => return false,
    };
    let cfg = match cfgs.get(caller_local_id) {
        Some(c) => c,
        None => return false,
    };

    let stmt_id = match find_stmt_containing(body, source_map, edge.range) {
        Some(s) => s,
        None => return false,
    };

    is_stmt_guarded(cfg, body, stmt_id, registry)
}

fn find_stmt_containing(
    body: &hir::Body,
    source_map: &hir::BodySourceMap,
    target: TextRange,
) -> Option<hir::StmtId> {
    let mut best: Option<(hir::StmtId, TextRange)> = None;
    for (stmt_id, _) in body.stmts_iter() {
        let Some(stmt_range) = source_map.stmt_range(stmt_id) else { continue };
        if !stmt_range.contains_range(target) {
            continue;
        }
        match best {
            Some((_, best_range)) if best_range.len() <= stmt_range.len() => {}
            _ => best = Some((stmt_id, stmt_range)),
        }
    }
    best.map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{check_snapshot_with_cfe, format_diags};
    use crate::DiagnosticsConfig;
    use expect_test::expect;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use test_fixture::CfeFixtureBuilder;
    use vfs::{FileId, FileSet, VfsPath};
    #[test]
    fn test_no_metadata_returns_empty() {
        let code = r#"
Процедура Тест()
    ПривилегированныйМодуль.Метод();
КонецПроцедуры
"#;
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let vfs_path = VfsPath::new("/test/Module.bsl");
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, code);

        let config = DiagnosticsConfig::default();

        let provider = ide_db::SalsaProvider::new(&db, None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    const PRIVILEGED_MODULE_BODY: &str = r#"
Процедура Метод() Экспорт
КонецПроцедуры
"#;

    #[test]
    fn test_direct_unguarded_call_emits() {
        let code = r#"
Процедура Тест()
    ПривилегМодуль.Метод();
КонецПроцедуры
"#;
        let diagnostics = crate::test_utils::check_diagnostic_with_privileged_modules(
            code,
            &[("ПривилегМодуль", PRIVILEGED_MODULE_BODY)],
            check,
        );
        expect![[r#"
            PrivilegedModuleMethodCall @ 3:5..3:27
              message: Проверьте обращение к методу Метод привилегированного модуля
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_role_guarded_call_suppressed() {
        let code = r#"
Процедура Тест()
    Если РольДоступна("Чтение") Тогда
        ПривилегМодуль.Метод();
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = crate::test_utils::check_diagnostic_with_privileged_modules(
            code,
            &[("ПривилегМодуль", PRIVILEGED_MODULE_BODY)],
            check,
        );
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_cfe_privileged_module_call_emits() {
        let code = r#"
#Область ПрограммныйИнтерфейс
// Описание
Процедура Тест() Экспорт
    РасширениеПривилегированный.Выполнить();
КонецПроцедуры
#КонецОбласти
"#;
        let mut builder = CfeFixtureBuilder::new("");
        builder
            .add_extension(
                "SecurityExt",
                r#"<MetaDataObject>
    <Privileged>true</Privileged>
</MetaDataObject>"#,
            )
            .add_extension_module(
                "SecurityExt",
                "РасширениеПривилегированный",
                r#"
Процедура Выполнить() Экспорт
КонецПроцедуры
"#,
            );

        check_snapshot_with_cfe(
            code,
            builder.build(),
            expect![[r#"
                PrivilegedModuleMethodCall @ 5:5..5:44
                  message: Проверьте обращение к методу Выполнить привилегированного модуля
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_disabled_config() {
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let vfs_path = VfsPath::new("/test/Module.bsl");
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "");

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::PrivilegedModuleMethodCall);

        let provider = ide_db::SalsaProvider::new(&db, None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        expect![[r#""#]].assert_eq(&format_diags("", &diagnostics));
    }
}
