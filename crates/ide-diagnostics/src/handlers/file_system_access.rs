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
//! **This is a HIR-based diagnostic** - detects file system access during HIR lowering.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when FileSystemAccess diagnostic is emitted during lowering.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::FileSystemAccess,
        "File system access detected (security review required)",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_all_constructor_types_in_procedure() {
        // All 14 NEW_EXPR types from the fixture (Метод1) trigger diagnostics
        let code = r#"Процедура Метод1()
    Значение = Новый File(ИмяФайла);
    Значение = Новый xBase("C:\temp.dbf");
    Значение = Новый HTMLWriter;
    Значение = Новый HTMLReader;
    Значение = Новый FastInfosetReader;
    Значение = Новый FastInfosetWriter;
    Значение = Новый XSLTransform;
    Значение = Новый ZipFileWriter(ИмяФайла);
    Значение = Новый ZipFileReader(ИмяФайла);
    Значение = Новый TextReader(ИмяФайла);
    Значение = Новый TextWriter(ИмяФайла);
    Значение = Новый TextExtraction(ИмяФайла);
    Значение = Новый BinaryData(ИмяФайла);
    Значение = Новый FileStream(ИмяФайла, РежимОткрытия);
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 2:16..2:36
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 3:16..3:42
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 4:16..4:32
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 5:16..5:32
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 6:16..6:39
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 7:16..7:39
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 8:16..8:34
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 9:16..9:45
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 10:16..10:45
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 11:16..11:42
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 12:16..12:42
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 13:16..13:46
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 14:16..14:42
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 15:16..15:57
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_all_global_methods() {
        // All global method calls from the fixture (Метод4) trigger diagnostics
        let code = r#"Процедура Метод4()
    ЗначениеВФайл("C:\Temp\PersonalData.txt", ЛичныеДанные);
    КопироватьФайл("C:\Temp\Order.htm", "C:\My Documents\Order.htm");
    МассивИмен = Новый Массив();
    МассивИмен.Добавить("C:\Windows\Temp\Presentation.ppt.1");
    ОбъединитьФайлы(МассивИмен, "C:\Windows\Temp\Presentation.ppt");
    ПереместитьФайл("C:\Temp\Order.htm", "C:\My Documents\Order.htm");
    РазделитьФайл("C:\Windows\Temp\Presentation.ppt", 1024 * 1024);
    СоздатьКаталог("C:\Temp");
    УдалитьФайлы("C:\temp\Works");
КонецПроцедуры"#;
        // 7 global methods (Массив and its method are not file system)
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 2:5..2:18
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 3:5..3:19
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 6:5..6:20
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 7:5..7:20
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 8:5..8:18
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 9:5..9:19
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 10:5..10:17
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_annotation_does_not_suppress() {
        // Fixture Метод2 and Метод3: annotations don't suppress FileSystemAccess
        let code = r#"&НаСервере
Процедура Метод2()
    Значение = Новый xBase("C:\temp.dbf");
КонецПроцедуры

&НаСервереБезКонтекста
Процедура Метод3()
    Значение = Новый xBase;
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 3:16..3:42
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 8:16..8:27
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_new_expression_russian() {
        let code = r#"
Процедура Тест()
    Ф = Новый Файл("test.txt");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 3:9..3:31
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_new_expression_english() {
        let code = r#"
Procedure Test()
    F = New File("test.txt");
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 3:9..3:29
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_global_method_russian() {
        let code = r#"
Процедура Тест()
    КопироватьФайл("src", "dest");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 3:5..3:19
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_global_method_english() {
        let code = r#"
Procedure Test()
    FileCopy("src", "dest");
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 3:5..3:13
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 3:10..3:32
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 4:10..4:32
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 5:5..5:19
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
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
        check_diagnostics_snapshot_for(code, DiagnosticCode::FileSystemAccess, expect![[r#""#]]);
    }

    #[test]
    fn test_qualified_call_detected() {
        let code = r#"
Процедура Тест()
    // Object method calls are also detected (unlike ExternalAppStarting)
    ФайловаяСистема.КопироватьФайл("src", "dest");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 4:21..4:35
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 3:10..3:22
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 4:10..4:22
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 5:9..5:22
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 6:12..6:30
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 7:12..7:30
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 8:11..8:29
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 9:11..9:29
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 10:12..10:32
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 11:12..11:30
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 12:11..12:31
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 13:11..13:29
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 14:11..14:33
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 15:11..15:29
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FileSystemAccess,
            expect![[r#"
                FileSystemAccess @ 4:9..4:31
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 5:5..5:13
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 6:9..6:27
                  message: File system access detected (security review required)
                  severity: Major
                FileSystemAccess @ 7:5..7:19
                  message: File system access detected (security review required)
                  severity: Major"#]],
        );
    }
}
