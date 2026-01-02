//! DeprecatedMethods8310 diagnostic.
//!
//! Detects usage of deprecated client application methods introduced in 8.3.10.
//!
//! ## Why?
//! Since 1C:Enterprise 8.3.10, several global methods related to client application
//! were deprecated and replaced with methods of the `КлиентскоеПриложение` / `ClientApplication` object:
//! - Better organization (methods grouped in a single object)
//! - Clearer API design
//! - Future-proof architecture
//!
//! ## Deprecated methods (RU → EN):
//! 1. `УстановитьКраткийЗаголовокПриложения` → `КлиентскоеПриложение.УстановитьКраткийЗаголовок`
//! 2. `ПолучитьКраткийЗаголовокПриложения` → `КлиентскоеПриложение.ПолучитьКраткийЗаголовок`
//! 3. `УстановитьЗаголовокКлиентскогоПриложения` → `КлиентскоеПриложение.УстановитьЗаголовок`
//! 4. `ПолучитьЗаголовокКлиентскогоПриложения` → `КлиентскоеПриложение.ПолучитьЗаголовок`
//! 5. `ТекущийВариантОсновногоШрифтаКлиентскогоПриложения` → `КлиентскоеПриложение.ТекущийВариантОсновногоШрифта`
//! 6. `ТекущийВариантИнтерфейсаКлиентскогоПриложения` → `КлиентскоеПриложение.ТекущийВариантИнтерфейса`
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест()
//!     Заголовок = ПолучитьКраткийЗаголовокПриложения(); // ❌ Deprecated
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест()
//!     Заголовок = КлиентскоеПриложение.ПолучитьКраткийЗаголовок(); // ✅
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (INFO)
//! - **Tags:** DEPRECATED
//! - **Compatibility mode:** 8.3.10+
//! - **Minutes to fix:** 1
//!
//! ## Implementation
//! Ported from:
//! - DeprecatedMethods8310Diagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use std::collections::HashMap;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DeprecatedMethods8310) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Optimized: single traversal O(n) instead of O(n²)
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() != SyntaxKind::IDENT {
            continue;
        }

        // Check pattern: IDENT ( but not .IDENT(
        let next_is_lparen =
            tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

        if !next_is_lparen {
            continue;
        }

        let prev_is_dot = i
            .checked_sub(1)
            .and_then(|idx| tokens.get(idx))
            .map(|t| t.kind() == SyntaxKind::DOT)
            .unwrap_or(false);

        if prev_is_dot {
            continue;
        }

        // Found global method call - check if deprecated
        let method_name = token.text().to_string();
        if let Some(replacement) = get_replacement(&method_name) {
            diagnostics.push(create_diagnostic(token, &method_name, replacement));
        }
    }

    diagnostics
}

fn find_arg_list_after_token(start_token: &SyntaxToken) -> Option<SyntaxNode> {
    let mut current = start_token.parent()?;

    for _ in 0..10 {
        for child in current.descendants() {
            if child.kind() == SyntaxKind::ARG_LIST {
                let arg_list_start = child.text_range().start();
                let token_end = start_token.text_range().end();

                let arg_list_offset: usize = arg_list_start.into();
                let token_offset: usize = token_end.into();

                if arg_list_start >= token_end && arg_list_offset - token_offset <= 10 {
                    return Some(child);
                }
            }
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }

    None
}

fn get_replacement(method_name: &str) -> Option<&'static str> {
    let map = get_replacement_map();
    let lower = method_name.to_lowercase();
    map.get(lower.as_str()).copied()
}

fn get_replacement_map() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();

    map.insert(
        "установитькраткийзаголовокприложения",
        "КлиентскоеПриложение.УстановитьКраткийЗаголовок",
    );
    map.insert(
        "получитькраткийзаголовокприложения",
        "КлиентскоеПриложение.ПолучитьКраткийЗаголовок",
    );
    map.insert(
        "установитьзаголовокклиентскогоприложения",
        "КлиентскоеПриложение.УстановитьЗаголовок",
    );
    map.insert("получитьзаголовокклиентскогоприложения", "КлиентскоеПриложение.ПолучитьЗаголовок");
    map.insert(
        "текущийвариантосновногошрифтаклиентскогоприложения",
        "КлиентскоеПриложение.ТекущийВариантОсновногоШрифта",
    );
    map.insert(
        "текущийвариантинтерфейсаклиентскогоприложения",
        "КлиентскоеПриложение.ТекущийВариантИнтерфейса",
    );

    map.insert("setshortapplicationcaption", "ClientApplication.SetShortCaption");
    map.insert("getshortapplicationcaption", "ClientApplication.GetShortCaption");
    map.insert("setclientapplicationcaption", "ClientApplication.SetCaption");
    map.insert("getclientapplicationcaption", "ClientApplication.GetCaption");
    map.insert(
        "clientapplicationbasefontcurrentvariant",
        "ClientApplication.CurrentBaseFontVariant",
    );
    map.insert(
        "clientapplicationinterfacecurrentvariant",
        "ClientApplication.CurrentInterfaceVariant",
    );

    map
}

fn create_diagnostic(token: &SyntaxToken, method_name: &str, replacement: &str) -> Diagnostic {
    let message = get_message(method_name, replacement);

    let range = if let Some(arg_list) = find_arg_list_after_token(token) {
        let start = token.text_range().start();
        let end = arg_list.text_range().end();
        ide_db::TextRange::new(start, end)
    } else {
        token.text_range()
    };

    Diagnostic {
        code: DiagnosticCode::DeprecatedMethods8310,
        message,
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

fn get_message(method_name: &str, replacement: &str) -> String {
    let lower = method_name.to_lowercase();
    let is_russian = lower.chars().any(|c| c as u32 > 127);

    if is_russian {
        format!("Метод \"{}\" устарел. Следует использовать \"{}\".", method_name, replacement)
    } else {
        format!("Method \"{}\" is deprecated. You should use \"{}\".", method_name, replacement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_deprecated_russian_set_short_caption() {
        let code = r#"
Процедура Тест()
    УстановитьКраткийЗаголовокПриложения("Заголовок");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedMethods8310);
        assert_eq!(diagnostics[0].severity, Severity::Information);
        assert!(diagnostics[0].message.contains("КлиентскоеПриложение.УстановитьКраткийЗаголовок"));
    }

    #[test]
    fn test_deprecated_english_get_short_caption() {
        let code = r#"
Procedure Test()
    Caption = GetShortApplicationCaption();
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedMethods8310);
        assert!(diagnostics[0].message.contains("ClientApplication.GetShortCaption"));
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Модуль.УстановитьКраткийЗаголовокПриложения("Заголовок");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    УСТАНОВИТЬКРАТКИЙЗАГОЛОВОКПРИЛОЖЕНИЯ("A");
    установитькраткийзаголовокприложения("B");
    УстановитьКраткийЗаголовокПриложения("C");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn test_all_russian_methods() {
        let code = r#"
Процедура Тест()
    УстановитьКраткийЗаголовокПриложения();
    ПолучитьКраткийЗаголовокПриложения();
    УстановитьЗаголовокКлиентскогоПриложения();
    ПолучитьЗаголовокКлиентскогоПриложения();
    ТекущийВариантОсновногоШрифтаКлиентскогоПриложения();
    ТекущийВариантИнтерфейсаКлиентскогоПриложения();
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 6);
    }

    #[test]
    fn test_all_english_methods() {
        let code = r#"
Procedure Test()
    SetShortApplicationCaption();
    GetShortApplicationCaption();
    SetClientApplicationCaption();
    GetClientApplicationCaption();
    ClientApplicationBaseFontCurrentVariant();
    ClientApplicationInterfaceCurrentVariant();
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 6);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedMethods8310Diagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(input);

        assert_eq!(diagnostics.len(), 12, "Expected 12 diagnostics");

        assert_diagnostic_range(&file_content, &diagnostics[0], 1, 31, 78);
        assert_diagnostic_range(&file_content, &diagnostics[1], 5, 31, 67);
        assert_diagnostic_range(&file_content, &diagnostics[2], 9, 31, 73);
        assert_diagnostic_range(&file_content, &diagnostics[3], 13, 31, 71);
        assert_diagnostic_range(&file_content, &diagnostics[4], 17, 31, 83);
        assert_diagnostic_range(&file_content, &diagnostics[5], 21, 31, 78);
        assert_diagnostic_range(&file_content, &diagnostics[6], 25, 11, 39);
        assert_diagnostic_range(&file_content, &diagnostics[7], 30, 11, 39);
        assert_diagnostic_range(&file_content, &diagnostics[8], 35, 11, 40);
        assert_diagnostic_range(&file_content, &diagnostics[9], 40, 11, 40);
        assert_diagnostic_range(&file_content, &diagnostics[10], 45, 11, 52);
        assert_diagnostic_range(&file_content, &diagnostics[11], 50, 11, 53);
    }
}
