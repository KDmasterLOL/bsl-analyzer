use super::types::{CapabilityEntry, Category, EntryKind, Replacement};
use Category::{
    AsyncCall, FormDataToValue, GetForm, ModalWindow, SynchronousCall, SystemInformation,
    TemporaryFilesDirectory, UnixUnavailableObject,
};
use EntryKind::{AnyReceiverMethod, GlobalMethod, Type};

macro_rules! replacement {
    ($ru:literal, $en:literal) => {
        Some(Replacement { ru: $ru, en: $en })
    };
}

macro_rules! global_method {
    ($ru:literal, $en:literal, $category:expr, $replacement:expr) => {
        CapabilityEntry {
            ru: $ru,
            en: $en,
            kind: GlobalMethod,
            category: $category,
            replacement: $replacement,
        }
    };
}

macro_rules! ui_modal_method {
    ($ru:literal, $en:literal, $replacement_ru:literal, $replacement_en:literal) => {
        global_method!($ru, $en, ModalWindow, replacement!($replacement_ru, $replacement_en))
    };
}

macro_rules! ui_sync_method {
    ($ru:literal, $en:literal, $replacement_ru:literal, $replacement_en:literal) => {
        global_method!($ru, $en, SynchronousCall, replacement!($replacement_ru, $replacement_en))
    };
}

macro_rules! ui_async_method {
    ($ru:literal, $en:literal) => {
        global_method!($ru, $en, AsyncCall, None)
    };
}

pub const ENTRIES: &[CapabilityEntry] = &[
    ui_modal_method!("Вопрос", "DoQueryBox", "ПоказатьВопрос", "ShowQueryBox"),
    ui_modal_method!("ОткрытьФормуМодально", "OpenFormModal", "ОткрытьФорму", "OpenForm"),
    ui_modal_method!("ОткрытьЗначение", "OpenValue", "ПоказатьЗначение", "ShowValue"),
    ui_modal_method!("Предупреждение", "DoMessageBox", "ПоказатьПредупреждение", "ShowMessageBox"),
    ui_modal_method!("ВвестиДату", "InputDate", "ПоказатьВводДаты", "ShowInputDate"),
    ui_modal_method!("ВвестиЗначение", "InputValue", "ПоказатьВводЗначения", "ShowInputValue"),
    ui_modal_method!("ВвестиСтроку", "InputString", "ПоказатьВводСтроки", "ShowInputString"),
    ui_modal_method!("ВвестиЧисло", "InputNumber", "ПоказатьВводЧисла", "ShowInputNumber"),
    ui_modal_method!(
        "УстановитьВнешнююКомпоненту",
        "InstallAddIn",
        "НачатьУстановкуВнешнейКомпоненты",
        "BeginInstallAddIn"
    ),
    ui_modal_method!(
        "УстановитьРасширениеРаботыСФайлами",
        "InstallFileSystemExtension",
        "НачатьУстановкуРасширенияРаботыСФайлами",
        "BeginInstallFileSystemExtension"
    ),
    ui_modal_method!(
        "УстановитьРасширениеРаботыСКриптографией",
        "InstallCryptoExtension",
        "НачатьУстановкуРасширенияРаботыСКриптографией",
        "BeginInstallCryptoExtension"
    ),
    ui_modal_method!("ПоместитьФайл", "PutFile", "НачатьПомещениеФайла", "BeginPutFile"),
    ui_sync_method!("Вопрос", "DoQueryBox", "ПоказатьВопрос", "ShowQueryBox"),
    ui_sync_method!("ОткрытьФормуМодально", "OpenFormModal", "ОткрытьФорму", "OpenForm"),
    ui_sync_method!("ОткрытьЗначение", "OpenValue", "ПоказатьЗначение", "ShowValue"),
    ui_sync_method!("Предупреждение", "DoMessageBox", "ПоказатьПредупреждение", "ShowMessageBox"),
    ui_sync_method!("ВвестиДату", "InputDate", "ПоказатьВводДаты", "ShowInputDate"),
    ui_sync_method!("ВвестиЗначение", "InputValue", "ПоказатьВводЗначения", "ShowInputValue"),
    ui_sync_method!("ВвестиСтроку", "InputString", "ПоказатьВводСтроки", "ShowInputString"),
    ui_sync_method!("ВвестиЧисло", "InputNumber", "ПоказатьВводЧисла", "ShowInputNumber"),
    ui_sync_method!(
        "УстановитьВнешнююКомпоненту",
        "InstallAddIn",
        "НачатьУстановкуВнешнейКомпоненты",
        "BeginInstallAddIn"
    ),
    ui_sync_method!(
        "УстановитьРасширениеРаботыСФайлами",
        "InstallFileSystemExtension",
        "НачатьУстановкуРасширенияРаботыСФайлами",
        "BeginInstallFileSystemExtension"
    ),
    ui_sync_method!(
        "УстановитьРасширениеРаботыСКриптографией",
        "InstallCryptoExtension",
        "НачатьУстановкуРасширенияРаботыСКриптографией",
        "BeginInstallCryptoExtension"
    ),
    ui_sync_method!(
        "ПодключитьРасширениеРаботыСКриптографией",
        "AttachCryptoExtension",
        "НачатьПодключениеРасширенияРаботыСКриптографией",
        "BeginAttachingCryptoExtension"
    ),
    ui_sync_method!(
        "ПодключитьРасширениеРаботыСФайлами",
        "AttachFileSystemExtension",
        "НачатьПодключениеРасширенияРаботыСФайлами",
        "BeginAttachingFileSystemExtension"
    ),
    ui_sync_method!("ПоместитьФайл", "PutFile", "НачатьПомещениеФайла", "BeginPutFile"),
    ui_sync_method!("КопироватьФайл", "FileCopy", "НачатьКопированиеФайла", "BeginCopyingFile"),
    ui_sync_method!("ПереместитьФайл", "MoveFile", "НачатьПеремещениеФайла", "BeginMovingFile"),
    ui_sync_method!("НайтиФайлы", "FindFiles", "НачатьПоискФайлов", "BeginFindingFiles"),
    ui_sync_method!("УдалитьФайлы", "DeleteFiles", "НачатьУдалениеФайлов", "BeginDeletingFiles"),
    ui_sync_method!(
        "СоздатьКаталог",
        "CreateDirectory",
        "НачатьСозданиеКаталога",
        "BeginCreatingDirectory"
    ),
    ui_sync_method!(
        "КаталогВременныхФайлов",
        "TempFilesDir",
        "НачатьПолучениеКаталогаВременныхФайлов",
        "BeginGettingTempFilesDir"
    ),
    ui_sync_method!(
        "КаталогДокументов",
        "DocumentsDir",
        "НачатьПолучениеКаталогаДокументов",
        "BeginGettingDocumentsDir"
    ),
    ui_sync_method!(
        "РабочийКаталогДанныхПользователя",
        "UserDataWorkDir",
        "НачатьПолучениеРабочегоКаталогаДанныхПользователя",
        "BeginGettingUserDataWorkDir"
    ),
    ui_sync_method!("ПолучитьФайлы", "GetFiles", "НачатьПолучениеФайлов", "BeginGettingFiles"),
    ui_sync_method!("ПоместитьФайлы", "PutFiles", "НачатьПомещениеФайлов", "BeginPuttingFiles"),
    ui_sync_method!(
        "ЗапроситьРазрешениеПользователя",
        "RequestUserPermission",
        "НачатьЗапросРазрешенияПользователя",
        "BeginRequestingUserPermission"
    ),
    ui_sync_method!(
        "ЗапуститьПриложение",
        "RunApp",
        "НачатьЗапускПриложения",
        "BeginRunningApplication"
    ),
    ui_async_method!("ПоказатьВопрос", "ShowQueryBox"),
    ui_async_method!("ПоказатьЗначение", "ShowValue"),
    ui_async_method!("ПоказатьПредупреждение", "ShowMessageBox"),
    ui_async_method!("ПоказатьВводДаты", "ShowInputDate"),
    ui_async_method!("ПоказатьВводЗначения", "ShowInputValue"),
    ui_async_method!("ПоказатьВводСтроки", "ShowInputString"),
    ui_async_method!("ПоказатьВводЧисла", "ShowInputNumber"),
    ui_async_method!("НачатьУстановкуВнешнейКомпоненты", "BeginInstallAddIn"),
    ui_async_method!("НачатьУстановкуРасширенияРаботыСФайлами", "BeginInstallFileSystemExtension"),
    ui_async_method!(
        "НачатьУстановкуРасширенияРаботыСКриптографией",
        "BeginInstallCryptoExtension"
    ),
    ui_async_method!(
        "НачатьПодключениеРасширенияРаботыСКриптографией",
        "BeginAttachingCryptoExtension"
    ),
    ui_async_method!(
        "НачатьПодключениеРасширенияРаботыСФайлами",
        "BeginAttachingFileSystemExtension"
    ),
    ui_async_method!("НачатьПомещениеФайла", "BeginPutFile"),
    ui_async_method!("НачатьКопированиеФайла", "BeginCopyingFile"),
    ui_async_method!("НачатьПеремещениеФайла", "BeginMovingFile"),
    ui_async_method!("НачатьПоискФайлов", "BeginFindingFiles"),
    ui_async_method!("НачатьУдалениеФайлов", "BeginDeletingFiles"),
    ui_async_method!("НачатьСозданиеКаталога", "BeginCreatingDirectory"),
    ui_async_method!("НачатьПолучениеКаталогаВременныхФайлов", "BeginGettingTempFilesDir"),
    ui_async_method!("НачатьПолучениеКаталогаДокументов", "BeginGettingDocumentsDir"),
    ui_async_method!(
        "НачатьПолучениеРабочегоКаталогаДанныхПользователя",
        "BeginGettingUserDataWorkDir"
    ),
    ui_async_method!("НачатьПолучениеФайлов", "BeginGettingFiles"),
    ui_async_method!("НачатьПомещениеФайлов", "BeginPuttingFiles"),
    ui_async_method!("НачатьЗапросРазрешенияПользователя", "BeginRequestingUserPermission"),
    ui_async_method!("НачатьЗапускПриложения", "BeginRunningApplication"),
    type_entry("СистемнаяИнформация", "SystemInfo", SystemInformation),
    type_entry("COMОбъект", "COMObject", UnixUnavailableObject),
    type_entry("Почта", "Mail", UnixUnavailableObject),
    global_method!("КаталогВременныхФайлов", "TempFilesDir", TemporaryFilesDirectory, None),
    global_method!("ДанныеФормыВЗначение", "FormDataToValue", FormDataToValue, None),
    global_method!("ПолучитьФорму", "GetForm", GetForm, None),
    any_receiver_method("ПолучитьФорму", "GetForm", GetForm),
];

const fn type_entry(ru: &'static str, en: &'static str, category: Category) -> CapabilityEntry {
    CapabilityEntry { ru, en, kind: Type, category, replacement: None }
}

const fn any_receiver_method(
    ru: &'static str,
    en: &'static str,
    category: Category,
) -> CapabilityEntry {
    CapabilityEntry { ru, en, kind: AnyReceiverMethod, category, replacement: None }
}
