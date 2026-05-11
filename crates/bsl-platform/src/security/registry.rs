//! Curated catalogue of security-relevant platform APIs.
//!
//! Maintenance contract:
//! - Every entry is a deliberate design decision. Adding or changing one
//!   is a code change reviewed alongside its handler.
//! - Names mirror the existing recognizers in
//!   `crates/hir-def/src/body/lower/expr.rs` and the security handlers
//!   in `crates/ide-diagnostics/src/handlers/`. After Track 2 §1.6 those
//!   recognizers are deleted and this catalogue is the sole source.
//! - When `en` is `""`, the API has no English alias in the existing
//!   matcher. Future audits may fill it in.
//!
//! # Canonical spellings supersede legacy bugs
//!
//! `is_file_system_method` in `crates/hir-def/src/body/lower/expr.rs`
//! contains 14 entries with morphologically wrong Russian spellings:
//! `асинч` (typo for `асинх`) and genitive verbal-noun forms like
//! `НачатьКопированияФайла` (correct accusative is
//! `НачатьКопированиеФайла`, see SYNCHRONOUS_METHODS in `diagnostics.rs`).
//! These were never matched by real BSL code — the platform exports
//! canonical spellings only — but staying faithful to the buggy strings
//! would enshrine dead code. The registry stores canonical names only;
//! `tests/security_registry.rs::legacy_recognizer_parity` enumerates the
//! canonical superset, and §1.6 deletes the legacy `is_file_system_method`
//! function entirely.

use super::types::{Category, EntryKind, ParamRole, Role, SecurityEntry, Severity};

const PATH_ARG0: &[ParamRole] = &[ParamRole { index: 0, role: Role::Path }];
const URL_ARG0: &[ParamRole] = &[ParamRole { index: 0, role: Role::Url }];
const CMD_ARG0: &[ParamRole] = &[ParamRole { index: 0, role: Role::Cmd }];

/// Polarity for `SetPrivilegedMode` and `SetSafeModeDisabled`: passing
/// `Истина` opens the privileged / unsafe frame.
const MODE_OPENS_TRUE: &[ParamRole] =
    &[ParamRole { index: 0, role: Role::ModeBool { opens_unsafe_when: true } }];
/// Polarity for `SetSafeMode`: passing `Ложь` opens the unsafe frame
/// (i.e. disables safe mode).
const MODE_OPENS_FALSE: &[ParamRole] =
    &[ParamRole { index: 0, role: Role::ModeBool { opens_unsafe_when: false } }];

const NO_PARAMS: &[ParamRole] = &[];

/// Curated security catalogue. ~70 entries spanning nine categories.
pub const ENTRIES: &[SecurityEntry] = &[
    // -----------------------------------------------------------------
    // Category::FileSystem — constructors
    // -----------------------------------------------------------------
    fs_ctor("Файл", "File"),
    fs_ctor("xBase", "xBase"),
    fs_ctor("ЗаписьHTML", "HTMLWriter"),
    fs_ctor("ЧтениеHTML", "HTMLReader"),
    fs_ctor("ЧтениеFastInfoset", "FastInfosetReader"),
    fs_ctor("ЗаписьFastInfoset", "FastInfosetWriter"),
    fs_ctor("ПреобразованиеXSL", "XSLTransform"),
    fs_ctor("ЗаписьZipФайла", "ZipFileWriter"),
    fs_ctor("ЧтениеZipФайла", "ZipFileReader"),
    fs_ctor("ЧтениеТекста", "TextReader"),
    fs_ctor("ЗаписьТекста", "TextWriter"),
    fs_ctor("ИзвлечениеТекста", "TextExtraction"),
    fs_ctor("ДвоичныеДанные", "BinaryData"),
    fs_ctor("ФайловыйПоток", "FileStream"),
    fs_ctor("МенеджерФайловыхПотоков", "FileStreamsManager"),
    fs_ctor("ЗаписьДанных", "DataWriter"),
    fs_ctor("ЧтениеДанных", "DataReader"),
    // -----------------------------------------------------------------
    // Category::FileSystem — global methods (path-taking)
    // -----------------------------------------------------------------
    fs_method_path("ЗначениеВФайл", "ValueToFile"),
    fs_method_path("КопироватьФайл", "FileCopy"),
    fs_method_path("ОбъединитьФайлы", "MergeFiles"),
    fs_method_path("ПереместитьФайл", "MoveFile"),
    fs_method_path("РазделитьФайл", "SplitFile"),
    fs_method_path("СоздатьКаталог", "CreateDirectory"),
    fs_method_path("УдалитьФайлы", "DeleteFiles"),
    fs_method_no_arg("КаталогПрограммы", "BinDir"),
    fs_method_no_arg("КаталогВременныхФайлов", "TempFilesDir"),
    fs_method_no_arg("КаталогДокументов", "DocumentsDir"),
    fs_method_no_arg("РабочийКаталогДанныхПользователя", "UserDataWorkDir"),
    fs_method_no_arg(
        "НачатьПодключениеРасширенияРаботыСФайлами",
        "BeginAttachingFileSystemExtension",
    ),
    fs_method_no_arg("НачатьУстановкуРасширенияРаботыСФайлами", "BeginInstallFileSystemExtension"),
    fs_method_no_arg("УстановитьРасширениеРаботыСФайлами", "InstallFileSystemExtension"),
    fs_method_no_arg("УстановитьРасширениеРаботыСФайламиАсинх", "InstallFileSystemExtensionAsync"),
    fs_method_no_arg("ПодключитьРасширениеРаботыСФайламиАсинх", "AttachFileSystemExtensionAsync"),
    fs_method_no_arg("КаталогВременныхФайловАсинх", "TempFilesDirAsync"),
    fs_method_no_arg("КаталогДокументовАсинх", "DocumentsDirAsync"),
    fs_method_no_arg("РабочийКаталогДанныхПользователяАсинх", "UserDataWorkDirAsync"),
    fs_method_no_arg("НачатьПолучениеКаталогаВременныхФайлов", "BeginGettingTempFilesDir"),
    fs_method_no_arg("НачатьПолучениеКаталогаДокументов", "BeginGettingDocumentsDir"),
    fs_method_no_arg(
        "НачатьПолучениеРабочегоКаталогаДанныхПользователя",
        "BeginGettingUserDataWorkDir",
    ),
    fs_method_path("КопироватьФайлАсинх", "CopyFileAsync"),
    fs_method_path("НайтиФайлыАсинх", "FindFilesAsync"),
    fs_method_path("НачатьКопированиеФайла", "BeginCopyingFile"),
    fs_method_path("НачатьПеремещениеФайла", "BeginMovingFile"),
    fs_method_path("НачатьПоискФайлов", "BeginFindingFiles"),
    fs_method_path("НачатьСозданиеДвоичныхДанныхИзФайла", "BeginCreateBinaryDataFromFile"),
    fs_method_path("НачатьСозданиеКаталога", "BeginCreatingDirectory"),
    fs_method_path("НачатьУдалениеФайлов", "BeginDeletingFiles"),
    fs_method_path("ПереместитьФайлАсинх", "MoveFileAsync"),
    fs_method_path("СоздатьДвоичныеДанныеИзФайлаАсинх", "CreateBinaryDataFromFileAsync"),
    fs_method_path("СоздатьКаталогАсинх", "CreateDirectoryAsync"),
    fs_method_path("УдалитьФайлыАсинх", "DeleteFilesAsync"),
    // -----------------------------------------------------------------
    // Category::Internet — constructors only (no global functions in
    // existing matcher).
    // -----------------------------------------------------------------
    net_ctor("FTPСоединение", "FTPConnection"),
    net_ctor("HTTPСоединение", "HTTPConnection"),
    net_ctor("WSОпределения", "WSDefinitions"),
    net_ctor("WSПрокси", "WSProxy"),
    net_ctor("ИнтернетПочтовыйПрофиль", "InternetMailProfile"),
    net_ctor("ИнтернетПочта", "InternetMail"),
    net_ctor("Почта", "Mail"),
    net_ctor("HTTPЗапрос", "HTTPRequest"),
    net_ctor("ИнтернетПрокси", "InternetProxy"),
    // -----------------------------------------------------------------
    // Category::ExternalApp — global methods. `ЗапуститьПрограмму`,
    // `ОткрытьПроводник` and `ОткрытьФайл` are RU-only in the existing
    // matcher; their `en` is left `""` to preserve current behaviour
    // (bilingual symmetry can be added once the EN names are confirmed
    // against HBK).
    // -----------------------------------------------------------------
    ext_app_method("КомандаСистемы", "System"),
    ext_app_method("ЗапуститьСистему", "RunSystem"),
    ext_app_method("ЗапуститьПриложение", "RunApp"),
    ext_app_method("НачатьЗапускПриложения", "BeginRunningApplication"),
    ext_app_method("ЗапуститьПриложениеАсинх", "RunAppAsync"),
    ext_app_method_ru_only("ЗапуститьПрограмму"),
    ext_app_method_ru_only("ОткрытьПроводник"),
    ext_app_method_ru_only("ОткрытьФайл"),
    // -----------------------------------------------------------------
    // Category::OsUsers — single global.
    // -----------------------------------------------------------------
    SecurityEntry {
        ru: "ПользователиОС",
        en: "OSUsers",
        kind: EntryKind::GlobalMethod,
        category: Category::OsUsers,
        severity: Severity::Critical,
        params: NO_PARAMS,
        lifetime: None,
    },
    // -----------------------------------------------------------------
    // Category::ExecuteExternalCode — only `Вычислить` / `Eval` is here.
    //
    // `Выполнить` / `Execute` is a *statement* parsed as
    // `syntax::SyntaxKind::EXECUTE_STMT` (lowered at
    // `crates/hir-def/src/body/lower/stmt.rs:554` via `lower_execute_stmt`,
    // which emits `BodyDiagnostic::ExecuteExternalCode` at the same file
    // line 1256; the common-module handler dispatches the same syntax
    // kind at
    // `crates/ide-diagnostics/src/handlers/execute_external_code_in_common_module.rs:72`).
    // It does not appear at any IDENT call-site, so the §1.6 handler
    // migration MUST keep its existing `SyntaxKind::EXECUTE_STMT` arm
    // alongside the registry-driven `Eval` lookup. Modelling `Выполнить`
    // here as `EntryKind::GlobalMethod` would invite handlers to call
    // `lookup_global("Выполнить")` at sites where there is no IDENT
    // token to feed in.
    // -----------------------------------------------------------------
    SecurityEntry {
        ru: "Вычислить",
        en: "Eval",
        kind: EntryKind::GlobalMethod,
        category: Category::ExecuteExternalCode,
        severity: Severity::Critical,
        params: NO_PARAMS,
        lifetime: None,
    },
    // -----------------------------------------------------------------
    // Category::PrivilegedMode — counter-based: `Истина` opens a frame,
    // `Ложь` closes one. The lattice in §1.2 reads `Role::ModeBool`.
    // -----------------------------------------------------------------
    SecurityEntry {
        ru: "УстановитьПривилегированныйРежим",
        en: "SetPrivilegedMode",
        kind: EntryKind::GlobalMethod,
        category: Category::PrivilegedMode,
        severity: Severity::Major,
        params: MODE_OPENS_TRUE,
        lifetime: None,
    },
    // -----------------------------------------------------------------
    // Category::SafeMode — two distinct toggles that point in opposite
    // directions. `SetSafeMode(False)` and `SetSafeModeDisabled(True)`
    // both *weaken* safe mode; the existing handler distinguishes them
    // by message text only.
    // -----------------------------------------------------------------
    SecurityEntry {
        ru: "УстановитьБезопасныйРежим",
        en: "SetSafeMode",
        kind: EntryKind::GlobalMethod,
        category: Category::SafeMode,
        severity: Severity::Major,
        params: MODE_OPENS_FALSE,
        lifetime: None,
    },
    SecurityEntry {
        ru: "УстановитьОтключениеБезопасногоРежима",
        en: "SetSafeModeDisabled",
        kind: EntryKind::GlobalMethod,
        category: Category::SafeMode,
        severity: Severity::Major,
        params: MODE_OPENS_TRUE,
        lifetime: None,
    },
    // -----------------------------------------------------------------
    // Category::SafeModeQuery — `БезопасныйРежим()` getter. Used by
    // `UnsafeSafeModeMethodCall` to detect bare boolean usage.
    // -----------------------------------------------------------------
    SecurityEntry {
        ru: "БезопасныйРежим",
        en: "SafeMode",
        kind: EntryKind::GlobalMethod,
        category: Category::SafeModeQuery,
        severity: Severity::Minor,
        params: NO_PARAMS,
        lifetime: None,
    },
    // -----------------------------------------------------------------
    // Category::PrivilegedModeQuery — `ПривилегированныйРежим()` getter.
    // Symmetric with the safe-mode getter above; consumers (e.g. the
    // §1.5 guard-predicate detector) treat a `Если ПривилегированныйРежим()`
    // branch as a guard.
    // -----------------------------------------------------------------
    SecurityEntry {
        ru: "ПривилегированныйРежим",
        en: "PrivilegedMode",
        kind: EntryKind::GlobalMethod,
        category: Category::PrivilegedModeQuery,
        severity: Severity::Minor,
        params: NO_PARAMS,
        lifetime: None,
    },
    // -----------------------------------------------------------------
    // Category::Transaction — used by §2 catch-body classifier as a
    // recovery action (rollback before propagating / handling).
    // -----------------------------------------------------------------
    SecurityEntry {
        ru: "ОтменитьТранзакцию",
        en: "RollbackTransaction",
        kind: EntryKind::GlobalMethod,
        category: Category::Transaction,
        severity: Severity::Minor,
        params: NO_PARAMS,
        lifetime: None,
    },
    // -----------------------------------------------------------------
    // Category::Logging — used by §2 catch-body classifier (`LogsOnly`).
    // -----------------------------------------------------------------
    SecurityEntry {
        ru: "ЗаписьЖурналаРегистрации",
        en: "WriteLogEvent",
        kind: EntryKind::GlobalMethod,
        category: Category::Logging,
        severity: Severity::Minor,
        params: NO_PARAMS,
        lifetime: None,
    },
    SecurityEntry {
        ru: "Сообщить",
        en: "Message",
        kind: EntryKind::GlobalMethod,
        category: Category::Logging,
        severity: Severity::Minor,
        params: NO_PARAMS,
        lifetime: None,
    },
    // BSL stdlib `ОбщегоНазначения.СообщитьПользователю(...)` —
    // typically used inside `Исключение` to surface the error to the
    // end user. Registered as a Logging entry so the §2 catch-body
    // classifier doesn't false-positive on the very common qualified
    // call shape (`Module.method(...)`, lowered as
    // `Expr::Call { callee: Expr::Field }` — see `recovery_kind`).
    SecurityEntry {
        ru: "СообщитьПользователю",
        en: "MessageToUser",
        kind: EntryKind::GlobalMethod,
        category: Category::Logging,
        severity: Severity::Minor,
        params: NO_PARAMS,
        lifetime: None,
    },
];

// =====================================================================
// Builder helpers — keep the const block above readable.
// =====================================================================

const fn fs_ctor(ru: &'static str, en: &'static str) -> SecurityEntry {
    SecurityEntry {
        ru,
        en,
        kind: EntryKind::Constructor,
        category: Category::FileSystem,
        severity: Severity::Major,
        params: NO_PARAMS,
        lifetime: None,
    }
}

const fn fs_method_path(ru: &'static str, en: &'static str) -> SecurityEntry {
    SecurityEntry {
        ru,
        en,
        kind: EntryKind::GlobalMethod,
        category: Category::FileSystem,
        severity: Severity::Major,
        params: PATH_ARG0,
        lifetime: None,
    }
}

const fn fs_method_no_arg(ru: &'static str, en: &'static str) -> SecurityEntry {
    SecurityEntry {
        ru,
        en,
        kind: EntryKind::GlobalMethod,
        category: Category::FileSystem,
        severity: Severity::Major,
        params: NO_PARAMS,
        lifetime: None,
    }
}

const fn net_ctor(ru: &'static str, en: &'static str) -> SecurityEntry {
    SecurityEntry {
        ru,
        en,
        kind: EntryKind::Constructor,
        category: Category::Internet,
        severity: Severity::Major,
        params: URL_ARG0,
        lifetime: None,
    }
}

const fn ext_app_method(ru: &'static str, en: &'static str) -> SecurityEntry {
    SecurityEntry {
        ru,
        en,
        kind: EntryKind::GlobalMethod,
        category: Category::ExternalApp,
        severity: Severity::Major,
        params: CMD_ARG0,
        lifetime: None,
    }
}

const fn ext_app_method_ru_only(ru: &'static str) -> SecurityEntry {
    ext_app_method(ru, "")
}
