use super::types::{Category, EntryKind, ParamRole, Role, SecurityEntry, Severity};

const PATH_ARG0: &[ParamRole] = &[ParamRole { index: 0, role: Role::Path }];
const URL_ARG0: &[ParamRole] = &[ParamRole { index: 0, role: Role::Url }];
const CMD_ARG0: &[ParamRole] = &[ParamRole { index: 0, role: Role::Cmd }];

const MODE_OPENS_TRUE: &[ParamRole] =
    &[ParamRole { index: 0, role: Role::ModeBool { opens_unsafe_when: true } }];
const MODE_OPENS_FALSE: &[ParamRole] =
    &[ParamRole { index: 0, role: Role::ModeBool { opens_unsafe_when: false } }];

const NO_PARAMS: &[ParamRole] = &[];

pub const ENTRIES: &[SecurityEntry] = &[
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
    fs_method_path("ЗначениеВФайл", "ValueToFile"),
    fs_method_path("КопироватьФайл", "CopyFile"),
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
    net_ctor("FTPСоединение", "FTPConnection"),
    net_ctor("HTTPСоединение", "HTTPConnection"),
    net_ctor("WSОпределения", "WSDefinitions"),
    net_ctor("WSПрокси", "WSProxy"),
    net_ctor("ИнтернетПочтовыйПрофиль", "InternetMailProfile"),
    net_ctor("ИнтернетПочта", "InternetMail"),
    net_ctor("Почта", "Mail"),
    net_ctor("HTTPЗапрос", "HTTPRequest"),
    net_ctor("ИнтернетПрокси", "InternetProxy"),
    ext_app_method("КомандаСистемы", "System"),
    ext_app_method("ЗапуститьСистему", "RunSystem"),
    ext_app_method("ЗапуститьПриложение", "RunApp"),
    ext_app_method("НачатьЗапускПриложения", "BeginRunningApplication"),
    ext_app_method("ЗапуститьПриложениеАсинх", "RunAppAsync"),
    ext_app_module_method("ЗапуститьПрограмму"),
    ext_app_module_method("ОткрытьПроводник"),
    ext_app_module_method("ОткрытьФайл"),
    SecurityEntry {
        ru: "ПользователиОС",
        en: "OSUsers",
        kind: EntryKind::GlobalMethod,
        category: Category::OsUsers,
        severity: Severity::Critical,
        params: NO_PARAMS,
        lifetime: None,
    },
    SecurityEntry {
        ru: "Вычислить",
        en: "Eval",
        kind: EntryKind::GlobalMethod,
        category: Category::ExecuteExternalCode,
        severity: Severity::Critical,
        params: NO_PARAMS,
        lifetime: None,
    },
    SecurityEntry {
        ru: "УстановитьПривилегированныйРежим",
        en: "SetPrivilegedMode",
        kind: EntryKind::GlobalMethod,
        category: Category::PrivilegedMode,
        severity: Severity::Major,
        params: MODE_OPENS_TRUE,
        lifetime: None,
    },
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
    SecurityEntry {
        ru: "БезопасныйРежим",
        en: "SafeMode",
        kind: EntryKind::GlobalMethod,
        category: Category::SafeModeQuery,
        severity: Severity::Minor,
        params: NO_PARAMS,
        lifetime: None,
    },
    SecurityEntry {
        ru: "ПривилегированныйРежим",
        en: "PrivilegedMode",
        kind: EntryKind::GlobalMethod,
        category: Category::PrivilegedModeQuery,
        severity: Severity::Minor,
        params: NO_PARAMS,
        lifetime: None,
    },
    SecurityEntry {
        ru: "ОтменитьТранзакцию",
        en: "RollbackTransaction",
        kind: EntryKind::GlobalMethod,
        category: Category::Transaction,
        severity: Severity::Minor,
        params: NO_PARAMS,
        lifetime: None,
    },
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

/// Common modules that hand a path to the operating system. `РаботаСФайламиКлиент`
/// predates the split into client and server modules and is still called.
const FILE_SYSTEM_MODULES: &[&str] =
    &["ФайловаяСистема", "ФайловаяСистемаКлиент", "РаботаСФайламиКлиент"];

const fn ext_app_module_method(ru: &'static str) -> SecurityEntry {
    SecurityEntry {
        ru,
        en: "",
        kind: EntryKind::ModuleMethod { owners: FILE_SYSTEM_MODULES },
        category: Category::ExternalApp,
        severity: Severity::Major,
        params: CMD_ARG0,
        lifetime: None,
    }
}
