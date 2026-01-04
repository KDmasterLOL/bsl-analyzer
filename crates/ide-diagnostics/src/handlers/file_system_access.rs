//! FileSystemAccess diagnostic.
//!
//! Detects file system access operations for security review.
//!
//! ## Why?
//! File system access creates security vulnerabilities:
//! - Potential for unauthorized file operations
//! - May leak confidential information
//! - Creates attack vectors for data exfiltration
//! - Destructive operations (delete, move, modify)
//!
//! This diagnostic is a **security audit tool** - disabled by default.
//! Enable it for code review, especially when auditing third-party or contractor code.
//!
//! ## What is detected
//!
//! ### Constructor patterns (NEW_EXPRESSION):
//! - File/Файл - file operations
//! - xBase - database file access
//! - HTMLWriter/ЗаписьHTML, HTMLReader/ЧтениеHTML - HTML file operations
//! - FastInfosetWriter/Reader - Fast Infoset file operations
//! - XSLTransform - XSLT file processing
//! - ZipFileWriter/Reader - archive operations
//! - TextWriter/Reader - text file operations
//! - TextExtraction - text extraction from files
//! - BinaryData - binary file operations
//! - FileStream - file stream operations
//! - FileStreamsManager - file stream management
//! - DataWriter/Reader - data file operations
//!
//! ### Global method patterns (GLOBAL_METHODS):
//! - File operations: ЗначениеВФайл, КопироватьФайл, ПереместитьФайл, etc.
//! - Directory operations: СоздатьКаталог, КаталогВременныхФайлов, etc.
//! - Extension operations: УстановитьРасширениеРаботыСФайлами, etc.
//! - Async operations: КопироватьФайлАсинх, СоздатьКаталогАсинх, etc.
//!
//! ## Bad practice
//! ```bsl
//! Процедура ВыгрузитьДанные()
//!     // File system access without authorization check
//!     ЗаписьТекста = Новый ЗаписьТекста("C:\Temp\PersonalData.txt");
//!     ЗаписьТекста.Записать(ЛичныеДанные);
//!
//!     КопироватьФайл("C:\Temp\Order.htm", "C:\My Documents\Order.htm");
//!     УдалитьФайлы("C:\temp\Works");
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Review and verify file system access is authorized
//! // Use 1C:Enterprise storage mechanisms instead (ValueStorage, temp storage)
//! // Implement proper access control and validation
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** No (security audit tool)
//! - **Severity:** Warning (MAJOR VULNERABILITY)
//! - **Type:** VULNERABILITY
//! - **Tags:** SUSPICIOUS
//! - **Minutes to fix:** 3
//!
//! ## Implementation
//! Ported from:
//! - FileSystemAccessDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - file_system_access.rs (bsl-language-server-rust) - Rust reference (regex-based)
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter or regex.
//! Follows the pattern from external_app_starting.rs.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};

/// Constructor types that indicate file system access.
///
/// Case-insensitive patterns (stored in lowercase).
/// Supports both Russian and English keywords.
const NEW_EXPRESSION_PATTERNS: &[&str] = &[
    "file",
    "файл",
    "xbase",
    "htmlwriter",
    "записьhtml",
    "htmlreader",
    "чтениеhtml",
    "fastinfosetreader",
    "чтениеfastinfoset",
    "fastinfosetwriter",
    "записьfastinfoset",
    "xsltransform",
    "преобразованиеxsl",
    "zipfilewriter",
    "записьzipфайла",
    "zipfilereader",
    "чтениеzipфайла",
    "textreader",
    "чтениетекста",
    "textwriter",
    "записьтекста",
    "textextraction",
    "извлечениетекста",
    "binarydata",
    "двоичныеданные",
    "filestream",
    "файловыйпоток",
    "filestreamsmanager",
    "менеджерфайловыхпотоков",
    "datawriter",
    "записьданных",
    "datareader",
    "чтениеданных",
];

/// Global methods that indicate file system access.
///
/// Case-insensitive patterns (stored in lowercase).
/// Supports both Russian and English keywords.
const GLOBAL_METHODS_PATTERNS: &[&str] = &[
    // File operations
    "значениевфайл",
    "valuetofile",
    "копироватьфайл",
    "filecopy",
    "объединитьфайлы",
    "mergefiles",
    "переместитьфайл",
    "movefile",
    "разделитьфайл",
    "splitfile",
    "создатькаталог",
    "createdirectory",
    "удалитьфайлы",
    "deletefiles",
    // Directory operations
    "каталогпрограммы",
    "bindir",
    "каталогвременныхфайлов",
    "tempfilesdir",
    "каталогдокументов",
    "documentsdir",
    "рабочийкаталогданныхпользователя",
    "userdataworkdir",
    // Extension operations
    "начатьподключениерасширенияработысфайлами",
    "beginattachingfilesystemextension",
    "начатьустановкурасширенияработысфайлами",
    "begininstallfilesystemextension",
    "установитьрасширениеработысфайлами",
    "installfilesystemextension",
    "установитьрасширениеработысфайламиасинх",
    "installfilesystemextensionasync",
    "подключитьрасширениеработысфайламиасинх",
    "attachfilesystemextensionasync",
    // Async directory operations
    "каталогвременныхфайловасинх",
    "tempfilesdirasync",
    "каталогдокументовасинх",
    "documentsdirasync",
    "рабочийкаталогданныхпользователяасинч",
    "userdataworkdirasync",
    "начатьполучениякаталогавременныхфайлов",
    "begingettingtempfilesdir",
    "начатьполучениякаталогадокументов",
    "begingettingdocumentsdir",
    "начатьполучениярабочегокаталогаданныхпользователя",
    "begingettinguserdataworkdir",
    // Async file operations
    "копироватьфайласинх",
    "copyfileasync",
    "найтифайлыасинч",
    "findfilesasync",
    "начатькопированияфайла",
    "begincopyingfile",
    "начатьперемещенияфайла",
    "beginmovingfile",
    "начатьпоискфайлов",
    "beginfindingfiles",
    "начатьсозданиядвоичныхданныхизфайла",
    "begincreatebinarydatafromfile",
    "начатьсозданиякаталога",
    "begincreatingdirectory",
    "начатьудаленияфайлов",
    "begindeletingfiles",
    "переместитьфайласинч",
    "movefileasync",
    "создатьдвоичныеданныеизфайласинч",
    "createbinarydatafromfileasync",
    "создатькаталогасинч",
    "createdirectoryasync",
    "удалитьфайлыасинч",
    "deletefilesasync",
];

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::FileSystemAccess) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    let mut seen_ranges = std::collections::HashSet::new();

    // ✅ OPTIMIZATION: Collect tokens ONCE instead of O(N²) nested tree traversal
    let all_elements: Vec<_> = root.descendants_with_tokens().collect();

    // Check NEW_EXPR nodes for file system types
    for element in all_elements.iter() {
        if let Some(node) = element.as_node() {
            if node.kind() == SyntaxKind::NEW_EXPR {
                if let Some(range) = extract_new_expr_range_optimized(node) {
                    if seen_ranges.insert(range) {
                        diagnostics.push(create_diagnostic(range));
                    }
                }
            }
        }
    }

    // Check method calls for file system operations (IDENT + LPAREN pattern)
    let tokens: Vec<_> = all_elements.iter().filter_map(|el| el.as_token()).collect();
    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT {
            let next_is_lparen =
                tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

            if next_is_lparen {
                let method_name = token.text().to_lowercase();

                if GLOBAL_METHODS_PATTERNS.contains(&method_name.as_str()) {
                    let range = token.text_range();
                    if seen_ranges.insert(range) {
                        diagnostics.push(create_diagnostic(range));
                    }
                }
            }
        }
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

fn create_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::FileSystemAccess,
        message: "File system access detected (security review required)".to_string(),
        range,
        severity: Severity::Warning,
        tags: vec![],
        fixes: vec![],
    }
}

/// Extract range of file system type from NEW_EXPR node (optimized).
///
/// Returns the range of the ENTIRE NEW_EXPR node if it matches file system patterns.
/// This matches Java bsl-language-server behavior (entire "Новый File(...)" expression).
///
/// Examples:
/// - `Новый File(...)` → range of entire "Новый File(...)"
/// - `Новый ЗаписьТекста` → range of entire "Новый ЗаписьТекста"
fn extract_new_expr_range_optimized(node: &SyntaxNode) -> Option<TextRange> {
    // NEW_EXPR pattern: KW_NEW IDENT [LPAREN ...]
    // We only need to check immediate children tokens, not descendants
    let mut found_new_kw = false;

    for element in node.children_with_tokens() {
        if let Some(token) = element.as_token() {
            if token.kind() == SyntaxKind::KW_NEW {
                found_new_kw = true;
                continue;
            }

            if found_new_kw && token.kind() == SyntaxKind::IDENT {
                let type_name = token.text().to_lowercase();

                if NEW_EXPRESSION_PATTERNS.contains(&type_name.as_str()) {
                    return Some(node.text_range());
                }

                break;
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/FileSystemAccessDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 23, "Expected 23 diagnostics");

        // NEW_EXPR diagnostics (lines are 0-indexed, ranges match Java test)
        assert_diagnostic_range(code, &diagnostics[0], 1, 15, 35); // Новый File(ИмяФайла)
        assert_diagnostic_range(code, &diagnostics[1], 2, 15, 41); // Новый xBase("C:\temp.dbf")
        assert_diagnostic_range(code, &diagnostics[2], 3, 15, 31); // Новый HTMLWriter
        assert_diagnostic_range(code, &diagnostics[3], 4, 15, 31); // Новый HTMLReader
        assert_diagnostic_range(code, &diagnostics[4], 5, 15, 38); // Новый FastInfosetReader
        assert_diagnostic_range(code, &diagnostics[5], 6, 15, 38); // Новый FastInfosetWriter
        assert_diagnostic_range(code, &diagnostics[6], 7, 15, 33); // Новый XSLTransform
        assert_diagnostic_range(code, &diagnostics[7], 8, 15, 44); // Новый ZipFileWriter(ИмяФайла)
        assert_diagnostic_range(code, &diagnostics[8], 9, 15, 44); // Новый ZipFileReader(ИмяФайла)
        assert_diagnostic_range(code, &diagnostics[9], 10, 15, 41); // Новый TextReader(ИмяФайла)
        assert_diagnostic_range(code, &diagnostics[10], 11, 15, 41); // Новый TextWriter(ИмяФайла)
        assert_diagnostic_range(code, &diagnostics[11], 12, 15, 45); // Новый TextExtraction(ИмяФайла)
        assert_diagnostic_range(code, &diagnostics[12], 13, 15, 41); // Новый BinaryData(ИмяФайла)
        assert_diagnostic_range(code, &diagnostics[13], 14, 15, 56); // Новый FileStream(ИмяФайла, РежимОткрытия)
        assert_diagnostic_range(code, &diagnostics[14], 19, 15, 41); // Новый xBase("C:\temp.dbf") - Метод2
        assert_diagnostic_range(code, &diagnostics[15], 24, 15, 26); // Новый xBase - Метод3

        // GLOBAL_METHODS diagnostics (method name only)
        assert_diagnostic_range(code, &diagnostics[16], 29, 4, 17); // ЗначениеВФайл
        assert_diagnostic_range(code, &diagnostics[17], 30, 4, 18); // КопироватьФайл
        assert_diagnostic_range(code, &diagnostics[18], 34, 4, 19); // ОбъединитьФайлы
        assert_diagnostic_range(code, &diagnostics[19], 36, 4, 19); // ПереместитьФайл
        assert_diagnostic_range(code, &diagnostics[20], 37, 4, 17); // РазделитьФайл
        assert_diagnostic_range(code, &diagnostics[21], 38, 4, 18); // СоздатьКаталог
        assert_diagnostic_range(code, &diagnostics[22], 39, 4, 16); // УдалитьФайлы
    }

    #[test]
    fn test_new_expression_russian() {
        let code = r#"
Процедура Тест()
    Ф = Новый Файл("test.txt");
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::FileSystemAccess);
    }

    #[test]
    fn test_new_expression_english() {
        let code = r#"
Procedure Test()
    F = New File("test.txt");
EndProcedure
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::FileSystemAccess);
    }

    #[test]
    fn test_global_method_russian() {
        let code = r#"
Процедура Тест()
    КопироватьФайл("src", "dest");
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::FileSystemAccess);
    }

    #[test]
    fn test_global_method_english() {
        let code = r#"
Procedure Test()
    FileCopy("src", "dest");
EndProcedure
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::FileSystemAccess);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Ф1 = Новый файл("test.txt");      // lowercase
    Ф2 = Новый ФАЙЛ("test.txt");      // uppercase
    КОПИРОВАТЬФАЙЛ("src", "dest");    // uppercase method
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn test_standard_types_ignored() {
        let code = r#"
Процедура Тест()
    М = Новый Массив();
    С = Новый Структура();
    Т = Новый ТаблицаЗначений();
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Standard types should be ignored");
    }

    #[test]
    fn test_qualified_call_detected() {
        let code = r#"
Процедура Тест()
    // Object method calls are also detected (unlike ExternalAppStarting)
    ФайловаяСистема.КопироватьФайл("src", "dest");
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Qualified calls should also be detected");
    }

    #[test]
    fn test_all_constructor_types() {
        let code = r#"
Процедура Тест()
    Ф1 = Новый Файл();
    Ф2 = Новый File();
    Х = Новый xBase();
    Зап1 = Новый ЗаписьHTML();
    Зап2 = Новый HTMLWriter();
    Чт1 = Новый ЧтениеHTML();
    Чт2 = Новый HTMLReader();
    Зап3 = Новый ЗаписьТекста();
    Зап4 = Новый TextWriter();
    Чт3 = Новый ЧтениеТекста();
    Чт4 = Новый TextReader();
    Дв1 = Новый ДвоичныеДанные();
    Дв2 = Новый BinaryData();
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 13, "All constructor types detected");
    }

    #[test]
    fn test_mixed_russian_english() {
        let code = r#"
Процедура Тест()
    // Mix of Russian and English
    Ф = Новый File("test.txt");
    FileCopy("a", "b");
    К = Новый TextWriter();
    СоздатьКаталог("dir");
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 4);
    }
}
