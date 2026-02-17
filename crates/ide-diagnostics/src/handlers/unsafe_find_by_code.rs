//! UnsafeFindByCode diagnostic.
//!
//! Detects calls to FindByCode() / НайтиПоКоду() on metadata objects
//! where uniqueness is not guaranteed.
//!
//! ## Why?
//! When code uniqueness control is disabled (CheckUnique=false) or
//! code series is applied within subordination/owner (CodeSeries!=WholeCatalog),
//! FindByCode may return unexpected results because multiple objects
//! can have the same code.
//!
//! ## Affected metadata types
//! - Catalogs (Справочники)
//! - Charts of Characteristic Types (ПланыВидовХарактеристик)
//! - Charts of Accounts (ПланыСчетов)
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Major
//! - **Type:** CODE_SMELL
//! - **Tags:** DESIGN, SUSPICIOUS
//! - **Minutes to fix:** 5

use bsl_metadata::{CodeSeries, MdoType};
use ide_db::TextRange;
use syntax::SyntaxKind;

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    manager_name: &str,
    object_name: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::UnsafeFindByCode;
    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let config = ctx.load_configuration()?;

    let manager_lower = manager_name.to_lowercase();
    let mdo_type = if matches!(manager_lower.as_str(), "справочники" | "catalogs") {
        MdoType::Catalog
    } else if matches!(
        manager_lower.as_str(),
        "планывидовхарактеристик" | "chartsofcharacteristictypes"
    ) {
        MdoType::ChartOfCharacteristicTypes
    } else if matches!(manager_lower.as_str(), "планысчетов" | "chartsofaccounts") {
        MdoType::ChartOfAccounts
    } else {
        return None;
    };

    let mdo = config.find_metadata_object(mdo_type, object_name)?;

    if mdo.is_find_by_code_safe() {
        return None;
    }

    let message = build_message(mdo_type, object_name, mdo.check_unique, mdo.code_series);

    Some(Diagnostic {
        code,
        message,
        range,
        severity: ctx.severity(code),
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

const FIND_BY_CODE_METHODS: &[&str] = &["найтипокоду", "findbycode"];

const CATALOG_MANAGERS: &[&str] = &["справочники", "catalogs"];
const CHART_OF_CHARACTERISTIC_TYPES_MANAGERS: &[&str] =
    &["планывидовхарактеристик", "chartsofcharacteristictypes"];
const CHART_OF_ACCOUNTS_MANAGERS: &[&str] = &["планысчетов", "chartsofaccounts"];

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UnsafeFindByCode;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = match ctx.load_configuration() {
        Some(c) => c,
        None => return Vec::new(),
    };

    let root = ctx.parse().syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() != SyntaxKind::CALL_EXPR {
            continue;
        }

        // CALL_EXPR structure: <receiver>.Method(args)
        // First child is the callee expression (FIELD_EXPR for method calls)
        let Some(callee) = node.first_child() else {
            continue;
        };

        if callee.kind() != SyntaxKind::FIELD_EXPR {
            continue;
        }

        // FIELD_EXPR structure: receiver . field
        // Get the method name (last IDENT token)
        let Some(method_name_token) = callee
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
            .last()
        else {
            continue;
        };

        let method_name_lower = method_name_token.text().to_lowercase();
        if !FIND_BY_CODE_METHODS.contains(&method_name_lower.as_str()) {
            continue;
        }

        // Get the receiver part (first child of FIELD_EXPR)
        let Some(receiver) = callee.first_child() else {
            continue;
        };

        // Receiver should be another FIELD_EXPR: Manager.Object
        if receiver.kind() != SyntaxKind::FIELD_EXPR {
            continue;
        }

        // Extract object name (last IDENT in receiver FIELD_EXPR)
        let Some(object_name_token) = receiver
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
            .last()
        else {
            continue;
        };
        let object_name = object_name_token.text().to_string();

        // Get manager expression (first child of receiver FIELD_EXPR)
        let Some(manager_node) = receiver.first_child() else {
            continue;
        };

        // Extract manager name (should be an IDENT-like expression)
        let manager_name = extract_ident_text(&manager_node);
        let Some(manager_name) = manager_name else {
            continue;
        };
        let manager_lower = manager_name.to_lowercase();

        let mdo_type = if CATALOG_MANAGERS.contains(&manager_lower.as_str()) {
            MdoType::Catalog
        } else if CHART_OF_CHARACTERISTIC_TYPES_MANAGERS.contains(&manager_lower.as_str()) {
            MdoType::ChartOfCharacteristicTypes
        } else if CHART_OF_ACCOUNTS_MANAGERS.contains(&manager_lower.as_str()) {
            MdoType::ChartOfAccounts
        } else {
            continue;
        };

        let Some(mdo) = config.find_metadata_object(mdo_type, &object_name) else {
            continue;
        };

        if !mdo.is_find_by_code_safe() {
            let range = method_name_token.text_range();
            let message = build_message(mdo_type, &object_name, mdo.check_unique, mdo.code_series);

            diagnostics.push(Diagnostic {
                code,
                message,
                range,
                severity: ctx.severity(code),
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics
}

fn extract_ident_text(node: &syntax::SyntaxNode) -> Option<String> {
    // For simple identifiers, the node itself contains the token
    if let Some(token) = node.first_token() {
        if token.kind() == SyntaxKind::IDENT {
            return Some(token.text().to_string());
        }
    }
    None
}

fn build_message(
    mdo_type: MdoType,
    name: &str,
    check_unique: bool,
    code_series: CodeSeries,
) -> String {
    let type_name = match mdo_type {
        MdoType::Catalog => "справочника",
        MdoType::ChartOfCharacteristicTypes => "плана видов характеристик",
        MdoType::ChartOfAccounts => "плана счетов",
        _ => "объекта",
    };

    let reason = if !check_unique {
        "отключен контроль уникальности кода"
    } else if !code_series.is_whole() {
        "коды уникальны только в пределах подчинения/владельца"
    } else {
        "коды не уникальны"
    };

    format!(
        "Небезопасный вызов НайтиПоКоду() для {} \"{}\": {}. \
         Используйте другие методы поиска или измените настройки объекта метаданных",
        type_name, name, reason
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{FileId, FileSet, VfsPath};
    #[test]
    fn test_disabled_returns_empty() {
        let code = r#"
Процедура Тест()
    Элемент = Справочники.Справочник1.НайтиПоКоду("001");
КонецПроцедуры
"#;
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let vfs_path = VfsPath::new("/test.bsl");
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, code);

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::UnsafeFindByCode);

        let ctx = crate::DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        assert!(diagnostics.is_empty(), "Disabled diagnostic should return empty");
    }

    #[test]
    fn test_no_configuration_returns_empty() {
        let code = r#"
Процедура Тест()
    Элемент = Справочники.Справочник1.НайтиПоКоду("001");
КонецПроцедуры
"#;
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let vfs_path = VfsPath::new("/test.bsl");
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, code);

        let config = DiagnosticsConfig::default();

        let ctx = crate::DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        assert!(diagnostics.is_empty(), "No configuration should return empty diagnostics");
    }

    #[test]
    fn test_build_message_check_unique_false() {
        let message = build_message(MdoType::Catalog, "Товары", false, CodeSeries::WholeCatalog);
        assert!(message.contains("отключен контроль уникальности кода"));
        assert!(message.contains("справочника"));
        assert!(message.contains("Товары"));
    }

    #[test]
    fn test_build_message_subordination_series() {
        let message =
            build_message(MdoType::Catalog, "Номенклатура", true, CodeSeries::WithinSubordination);
        assert!(message.contains("коды уникальны только в пределах подчинения/владельца"));
        assert!(message.contains("Номенклатура"));
    }

    #[test]
    fn test_build_message_chart_of_characteristic_types() {
        let message = build_message(
            MdoType::ChartOfCharacteristicTypes,
            "ДополнительныеРеквизиты",
            false,
            CodeSeries::WholeCatalog,
        );
        assert!(message.contains("плана видов характеристик"));
        assert!(message.contains("ДополнительныеРеквизиты"));
    }

    #[test]
    fn test_build_message_chart_of_accounts() {
        let message = build_message(
            MdoType::ChartOfAccounts,
            "Хозрасчетный",
            false,
            CodeSeries::WholeCatalog,
        );
        assert!(message.contains("плана счетов"));
        assert!(message.contains("Хозрасчетный"));
    }
}
