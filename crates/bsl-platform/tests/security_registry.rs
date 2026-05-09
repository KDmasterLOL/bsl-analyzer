//! Audit tests for the curated security registry.
//!
//! These tests guard the `bsl-platform/src/security/` catalogue against
//! the kinds of mistakes that const-data is prone to: typos that break
//! bilingual lookup, duplicate `(name, kind)` keys that would silently
//! mask one entry with another, and category usages out of step with
//! what handlers expect.

use bsl_platform::security::{registry, Category, EntryKind, SecurityEntry};
use std::collections::HashSet;

/// Every entry must have a non-empty `ru` field. Empty `en` is allowed
/// (RU-only APIs); empty `ru` is not.
#[test]
fn every_entry_has_russian_name() {
    for entry in registry().entries() {
        assert!(!entry.ru.is_empty(), "entry {entry:?} has empty `ru`",);
    }
}

/// `(lower_ru, kind)` and `(lower_en, kind)` must each be unique across
/// the catalogue. A duplicate would mean two entries collide on lookup
/// and `SecurityRegistry::build` would silently drop one.
#[test]
fn no_duplicate_lookup_keys() {
    let mut seen: HashSet<(String, EntryKind)> = HashSet::new();
    for entry in registry().entries() {
        let ru_key = entry.ru.to_lowercase();
        assert!(seen.insert((ru_key.clone(), entry.kind)), "duplicate RU key: {entry:?}",);
        if !entry.en.is_empty() {
            let en_key = entry.en.to_lowercase();
            // Some types share the same lexeme on both sides (`xBase`);
            // that is not a duplicate, just a bilingual coincidence.
            if en_key != ru_key {
                assert!(seen.insert((en_key, entry.kind)), "duplicate EN key: {entry:?}",);
            }
        }
    }
}

/// Round-trip: looking up by RU name returns the same entry as looking
/// up by EN name (when EN exists), regardless of case.
#[test]
fn bilingual_lookup_round_trip() {
    let reg = registry();
    for entry in reg.entries() {
        let by_ru = lookup_by_kind(reg, entry.ru, entry.kind)
            .unwrap_or_else(|| panic!("RU lookup failed for {entry:?}"));
        assert_eq!(by_ru.ru, entry.ru, "RU lookup returned wrong entry");

        // Case insensitivity on the RU side.
        let by_ru_upper = lookup_by_kind(reg, &entry.ru.to_uppercase(), entry.kind);
        assert!(by_ru_upper.is_some(), "uppercase RU lookup failed for {entry:?}");

        if !entry.en.is_empty() {
            let by_en = lookup_by_kind(reg, entry.en, entry.kind)
                .unwrap_or_else(|| panic!("EN lookup failed for {entry:?}"));
            assert_eq!(by_en.ru, entry.ru, "EN lookup returned wrong entry");
            // Case insensitivity on the EN side.
            assert!(
                lookup_by_kind(reg, &entry.en.to_lowercase(), entry.kind).is_some(),
                "lowercase EN lookup failed for {entry:?}",
            );
        }
    }
}

/// Empty / unknown lookups return `None` rather than panicking.
#[test]
fn empty_and_unknown_lookups_return_none() {
    let reg = registry();
    assert!(reg.lookup_global("").is_none());
    assert!(reg.lookup_constructor("").is_none());
    assert!(reg.lookup_global("__definitely_not_a_method__").is_none());
    assert!(reg.lookup_constructor("__definitely_not_a_type__").is_none());
}

/// Categories that the security handlers in §1.6 will switch over to
/// must each have at least one entry — otherwise the migration will
/// trip the first time the handler queries the registry.
#[test]
fn each_handler_category_has_entries() {
    let reg = registry();
    for category in [
        Category::FileSystem,
        Category::Internet,
        Category::ExternalApp,
        Category::OsUsers,
        Category::ExecuteExternalCode,
        Category::PrivilegedMode,
        Category::PrivilegedModeQuery,
        Category::SafeMode,
        Category::SafeModeQuery,
        Category::Logging,
        Category::Transaction,
    ] {
        assert!(!reg.entries_by_category(category).is_empty(), "no entries for {category:?}",);
    }
}

/// Every constructor entry must have RU and EN spellings. Constructors
/// with no English alias would silently break `Новый("FileStream", ...)`
/// detection in some BSL fixtures. Global methods can stay RU-only
/// (existing behaviour for `ЗапуститьПрограмму` etc.).
#[test]
fn constructors_are_fully_bilingual() {
    for entry in registry().entries() {
        if entry.kind == EntryKind::Constructor {
            assert!(!entry.en.is_empty(), "constructor entry {entry:?} is missing English alias",);
        }
    }
}

/// `Role::ModeBool` only makes sense on lifetime-controlling APIs. The
/// §1.2 saturating-counter lattice in `dataflow` will route exactly
/// these entries through the privilege/safe-mode transfer functions —
/// other categories must not silently leak in. Polarity is also pinned
/// here: `SetSafeMode` is the only API where `False` opens the unsafe
/// frame; the other two open when `True`.
#[test]
fn mode_bool_role_only_on_mode_categories() {
    use bsl_platform::security::Role;
    for entry in registry().entries() {
        let mode_bool_polarity = entry.params.iter().find_map(|p| match p.role {
            Role::ModeBool { opens_unsafe_when } => Some(opens_unsafe_when),
            _ => None,
        });
        if let Some(opens_unsafe_when) = mode_bool_polarity {
            assert!(
                matches!(entry.category, Category::PrivilegedMode | Category::SafeMode),
                "ModeBool param on non-mode category: {entry:?}",
            );
            // Pin the three polarity assignments so a future edit can't
            // silently flip one and corrupt §1.2 dataflow semantics.
            let expected_open_on_true = matches!(
                entry.ru,
                "УстановитьПривилегированныйРежим" | "УстановитьОтключениеБезопасногоРежима",
            );
            let expected_open_on_false = matches!(entry.ru, "УстановитьБезопасныйРежим");
            assert!(
                expected_open_on_true || expected_open_on_false,
                "ModeBool entry not in expected polarity table: {entry:?}",
            );
            if expected_open_on_true {
                assert!(opens_unsafe_when, "expected opens_unsafe_when=true for {entry:?}",);
            }
            if expected_open_on_false {
                assert!(!opens_unsafe_when, "expected opens_unsafe_when=false for {entry:?}",);
            }
        }
    }
}

/// Cross-check: every name accepted by the legacy hardcoded recognizers
/// in `crates/hir-def/src/body/lower/expr.rs` is reachable through the
/// registry. The list is the post-cleanup canonical superset; legacy
/// expr.rs entries that were morphologically wrong (`…Асинч` instead of
/// `…Асинх`, genitive forms like `НачатьКопированияФайла` instead of
/// `НачатьКопированиеФайла`) are intentionally NOT replicated — they
/// were latent bugs that never matched real platform method names. The
/// §1.6 migration deletes those legacy spellings.
#[test]
fn legacy_recognizer_parity() {
    let reg = registry();

    // is_external_app_method (RU + EN) — expr.rs:1679
    for &name in &[
        "командасистемы",
        "system",
        "запуститьсистему",
        "runsystem",
        "запуститьприложение",
        "runapp",
        "начатьзапускприложения",
        "beginrunningapplication",
        "запуститьприложениеасинх",
        "runappasync",
        // RU-only in legacy; registry stores `en: ""` so EN lookup is
        // not asserted here.
        "запуститьпрограмму",
        "открытьпроводник",
        "открытьфайл",
    ] {
        assert!(
            reg.lookup_global(name).is_some(),
            "legacy is_external_app_method name not in registry: {name}",
        );
    }

    // is_os_users_method (RU + EN) — expr.rs:1663
    for &name in &["пользователиос", "osusers"] {
        assert!(reg.lookup_global(name).is_some(), "missing OS-users name: {name}");
    }

    // is_file_system_type — expr.rs:1714 (NEW expression types)
    for &name in &[
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
    ] {
        assert!(
            reg.lookup_constructor(name).is_some(),
            "legacy is_file_system_type ctor not in registry: {name}",
        );
    }

    // is_file_system_method — expr.rs:1949 (canonical-only superset).
    // The 14 legacy buggy spellings (`…Асинч` typos and genitive verbal
    // nouns) are intentionally NOT covered — see test doc-comment and
    // `registry.rs` module-doc. Each canonical-equivalent IS covered
    // below to ensure §1.6 handlers can reach them.
    for &name in &[
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
        "каталогпрограммы",
        "bindir",
        "каталогвременныхфайлов",
        "tempfilesdir",
        "каталогдокументов",
        "documentsdir",
        "рабочийкаталогданныхпользователя",
        "userdataworkdir",
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
        "каталогвременныхфайловасинх",
        "tempfilesdirasync",
        "каталогдокументовасинх",
        "documentsdirasync",
        "рабочийкаталогданныхпользователяасинх",
        "userdataworkdirasync",
        "копироватьфайласинх",
        "copyfileasync",
        "найтифайлыасинх",
        "findfilesasync",
        "начатькопированиефайла",
        "begincopyingfile",
        "начатьперемещениефайла",
        "beginmovingfile",
        "начатьпоискфайлов",
        "beginfindingfiles",
        "начатьсозданиедвоичныхданныхизфайла",
        "begincreatebinarydatafromfile",
        "начатьсозданиекаталога",
        "begincreatingdirectory",
        "начатьудалениефайлов",
        "begindeletingfiles",
        "переместитьфайласинх",
        "movefileasync",
        "создатьдвоичныеданныеизфайлаасинх",
        "createbinarydatafromfileasync",
        "создатькаталогасинх",
        "createdirectoryasync",
        "удалитьфайлыасинх",
        "deletefilesasync",
        "начатьполучениекаталогавременныхфайлов",
        "begingettingtempfilesdir",
        "начатьполучениекаталогадокументов",
        "begingettingdocumentsdir",
        "начатьполучениерабочегокаталогаданныхпользователя",
        "begingettinguserdataworkdir",
    ] {
        assert!(
            reg.lookup_global(name).is_some(),
            "legacy is_file_system_method (canonical) name not in registry: {name}",
        );
    }

    // Internet constructors — `internet_access.rs::NEW_EXPRESSION_PATTERNS`
    // (line 77) and `using_hardcode_secret_information.rs:38` both
    // enumerate the same set; cover each name explicitly so §1.6 cannot
    // silently drop one during migration.
    for &name in &[
        "ftpсоединение",
        "ftpconnection",
        "httpсоединение",
        "httpconnection",
        "wsопределения",
        "wsdefinitions",
        "wsпрокси",
        "wsproxy",
        "интернетпочтовыйпрофиль",
        "internetmailprofile",
        "интернетпочта",
        "internetmail",
        "почта",
        "mail",
        "httpзапрос",
        "httprequest",
        "интернетпрокси",
        "internetproxy",
    ] {
        assert!(
            reg.lookup_constructor(name).is_some(),
            "legacy internet ctor not in registry: {name}",
        );
    }

    // is_write_log_event_method — expr.rs:1774. Lives under
    // `Category::Logging` because the §2 catch-body classifier consults
    // the same registry to recognize `LogsOnly` clauses.
    for &name in &["записьжурналарегистрации", "writelogevent"] {
        assert!(reg.lookup_global(name).is_some(), "WriteLogEvent name not in registry: {name}",);
    }

    // is_safe_mode_method, is_safe_mode_query, is_set_privileged_mode —
    // diagnostics.rs:191/197/209. `ПривилегированныйРежим` getter is
    // listed separately (it has no legacy `is_*` predicate today, but
    // is referenced by §1.5 guard-predicate detection and downstream
    // `IsInRoleMethod` handler).
    for &name in &[
        "установитьбезопасныйрежим",
        "setsafemode",
        "установитьотключениебезопасногорежима",
        "setsafemodedisabled",
        "безопасныйрежим",
        "safemode",
        "установитьпривилегированныйрежим",
        "setprivilegedmode",
        "привилегированныйрежим",
        "privilegedmode",
        "вычислить",
        "eval",
    ] {
        assert!(
            reg.lookup_global(name).is_some(),
            "legacy mode/eval method not in registry: {name}",
        );
    }
}

/// Track 2 §1.6 Group A category-parity guard.
///
/// The legacy recognizers in `hir-def/body/lower/expr.rs` filter on a
/// fixed `Category` (e.g. `is_file_system_method` checks
/// `Category::FileSystem` on `EntryKind::GlobalMethod`). After the §1.6
/// swap, an entry whose name appears in the legacy list but whose
/// registry `category` is the wrong one would silently stop being
/// detected — `legacy_recognizer_parity` only asserts `is_some()`,
/// missing this class of regression.
///
/// This test pins the contract: **every** name covered by a migrated
/// predicate (full enumeration matching `legacy_recognizer_parity`,
/// not a representative sample — Codex round-2 MAJOR fix) is reachable
/// AND classified under the predicate's filter category. If a future
/// PR reassigns any name's category in `registry.rs` (e.g. moves
/// `КопироватьФайл` from `FileSystem` to `Internet`), the migrated
/// `is_file_system_method` will silently lose detection — this test
/// fails first and forces an explicit decision.
#[test]
fn legacy_recognizer_category_parity() {
    let reg = registry();

    let global_cases: &[(&str, &str, Category)] = &[
        // is_external_app_method — Category::ExternalApp
        ("командасистемы", "is_external_app_method", Category::ExternalApp),
        ("system", "is_external_app_method", Category::ExternalApp),
        ("запуститьсистему", "is_external_app_method", Category::ExternalApp),
        ("runsystem", "is_external_app_method", Category::ExternalApp),
        ("запуститьприложение", "is_external_app_method", Category::ExternalApp),
        ("runapp", "is_external_app_method", Category::ExternalApp),
        ("начатьзапускприложения", "is_external_app_method", Category::ExternalApp),
        ("beginrunningapplication", "is_external_app_method", Category::ExternalApp),
        ("запуститьприложениеасинх", "is_external_app_method", Category::ExternalApp),
        ("runappasync", "is_external_app_method", Category::ExternalApp),
        ("запуститьпрограмму", "is_external_app_method", Category::ExternalApp),
        ("открытьпроводник", "is_external_app_method", Category::ExternalApp),
        ("открытьфайл", "is_external_app_method", Category::ExternalApp),
        // is_os_users_method — Category::OsUsers
        ("пользователиос", "is_os_users_method", Category::OsUsers),
        ("osusers", "is_os_users_method", Category::OsUsers),
        // is_global_eval_call (`execute_external_code_in_common_module`) —
        // Category::ExecuteExternalCode (Track 2 §1.6 Group B).
        ("вычислить", "is_global_eval_call", Category::ExecuteExternalCode),
        ("eval", "is_global_eval_call", Category::ExecuteExternalCode),
        // is_safe_mode_method (DisableSafeMode handler — `hir-def/body/
        // lower/diagnostics.rs`) — Category::SafeMode covers both
        // setter flavours.
        ("установитьбезопасныйрежим", "is_safe_mode_method", Category::SafeMode),
        ("setsafemode", "is_safe_mode_method", Category::SafeMode),
        ("установитьотключениебезопасногорежима", "is_safe_mode_method", Category::SafeMode),
        ("setsafemodedisabled", "is_safe_mode_method", Category::SafeMode),
        // is_safe_mode_query (UnsafeSafeModeMethodCall handler) —
        // Category::SafeModeQuery distinguishes the getter from the
        // setters above so the predicate stays strict.
        ("безопасныйрежим", "is_safe_mode_query", Category::SafeModeQuery),
        ("safemode", "is_safe_mode_query", Category::SafeModeQuery),
        // is_set_privileged_mode (SetPrivilegedMode handler) —
        // Category::PrivilegedMode is for the setter only; the
        // getter `ПривилегированныйРежим` lives under
        // `Category::PrivilegedModeQuery`, so the category filter
        // alone preserves the legacy strictness.
        ("установитьпривилегированныйрежим", "is_set_privileged_mode", Category::PrivilegedMode),
        ("setprivilegedmode", "is_set_privileged_mode", Category::PrivilegedMode),
        // is_file_system_method — Category::FileSystem (canonical-only
        // superset). Full enumeration matches `legacy_recognizer_parity`
        // — Codex round-2 MAJOR: a representative sample lets a
        // category reassignment outside the sample slip past silently.
        ("значениевфайл", "is_file_system_method", Category::FileSystem),
        ("valuetofile", "is_file_system_method", Category::FileSystem),
        ("копироватьфайл", "is_file_system_method", Category::FileSystem),
        ("filecopy", "is_file_system_method", Category::FileSystem),
        ("объединитьфайлы", "is_file_system_method", Category::FileSystem),
        ("mergefiles", "is_file_system_method", Category::FileSystem),
        ("переместитьфайл", "is_file_system_method", Category::FileSystem),
        ("movefile", "is_file_system_method", Category::FileSystem),
        ("разделитьфайл", "is_file_system_method", Category::FileSystem),
        ("splitfile", "is_file_system_method", Category::FileSystem),
        ("создатькаталог", "is_file_system_method", Category::FileSystem),
        ("createdirectory", "is_file_system_method", Category::FileSystem),
        ("удалитьфайлы", "is_file_system_method", Category::FileSystem),
        ("deletefiles", "is_file_system_method", Category::FileSystem),
        ("каталогпрограммы", "is_file_system_method", Category::FileSystem),
        ("bindir", "is_file_system_method", Category::FileSystem),
        ("каталогвременныхфайлов", "is_file_system_method", Category::FileSystem),
        ("tempfilesdir", "is_file_system_method", Category::FileSystem),
        ("каталогдокументов", "is_file_system_method", Category::FileSystem),
        ("documentsdir", "is_file_system_method", Category::FileSystem),
        ("рабочийкаталогданныхпользователя", "is_file_system_method", Category::FileSystem),
        ("userdataworkdir", "is_file_system_method", Category::FileSystem),
        (
            "начатьподключениерасширенияработысфайлами",
            "is_file_system_method",
            Category::FileSystem,
        ),
        ("beginattachingfilesystemextension", "is_file_system_method", Category::FileSystem),
        ("начатьустановкурасширенияработысфайлами", "is_file_system_method", Category::FileSystem),
        ("begininstallfilesystemextension", "is_file_system_method", Category::FileSystem),
        ("установитьрасширениеработысфайлами", "is_file_system_method", Category::FileSystem),
        ("installfilesystemextension", "is_file_system_method", Category::FileSystem),
        ("установитьрасширениеработысфайламиасинх", "is_file_system_method", Category::FileSystem),
        ("installfilesystemextensionasync", "is_file_system_method", Category::FileSystem),
        ("подключитьрасширениеработысфайламиасинх", "is_file_system_method", Category::FileSystem),
        ("attachfilesystemextensionasync", "is_file_system_method", Category::FileSystem),
        ("каталогвременныхфайловасинх", "is_file_system_method", Category::FileSystem),
        ("tempfilesdirasync", "is_file_system_method", Category::FileSystem),
        ("каталогдокументовасинх", "is_file_system_method", Category::FileSystem),
        ("documentsdirasync", "is_file_system_method", Category::FileSystem),
        ("рабочийкаталогданныхпользователяасинх", "is_file_system_method", Category::FileSystem),
        ("userdataworkdirasync", "is_file_system_method", Category::FileSystem),
        ("копироватьфайласинх", "is_file_system_method", Category::FileSystem),
        ("copyfileasync", "is_file_system_method", Category::FileSystem),
        ("найтифайлыасинх", "is_file_system_method", Category::FileSystem),
        ("findfilesasync", "is_file_system_method", Category::FileSystem),
        ("начатькопированиефайла", "is_file_system_method", Category::FileSystem),
        ("begincopyingfile", "is_file_system_method", Category::FileSystem),
        ("начатьперемещениефайла", "is_file_system_method", Category::FileSystem),
        ("beginmovingfile", "is_file_system_method", Category::FileSystem),
        ("начатьпоискфайлов", "is_file_system_method", Category::FileSystem),
        ("beginfindingfiles", "is_file_system_method", Category::FileSystem),
        ("начатьсозданиедвоичныхданныхизфайла", "is_file_system_method", Category::FileSystem),
        ("begincreatebinarydatafromfile", "is_file_system_method", Category::FileSystem),
        ("начатьсозданиекаталога", "is_file_system_method", Category::FileSystem),
        ("begincreatingdirectory", "is_file_system_method", Category::FileSystem),
        ("начатьудалениефайлов", "is_file_system_method", Category::FileSystem),
        ("begindeletingfiles", "is_file_system_method", Category::FileSystem),
        ("переместитьфайласинх", "is_file_system_method", Category::FileSystem),
        ("movefileasync", "is_file_system_method", Category::FileSystem),
        ("создатьдвоичныеданныеизфайлаасинх", "is_file_system_method", Category::FileSystem),
        ("createbinarydatafromfileasync", "is_file_system_method", Category::FileSystem),
        ("создатькаталогасинх", "is_file_system_method", Category::FileSystem),
        ("createdirectoryasync", "is_file_system_method", Category::FileSystem),
        ("удалитьфайлыасинх", "is_file_system_method", Category::FileSystem),
        ("deletefilesasync", "is_file_system_method", Category::FileSystem),
        ("начатьполучениекаталогавременныхфайлов", "is_file_system_method", Category::FileSystem),
        ("begingettingtempfilesdir", "is_file_system_method", Category::FileSystem),
        ("начатьполучениекаталогадокументов", "is_file_system_method", Category::FileSystem),
        ("begingettingdocumentsdir", "is_file_system_method", Category::FileSystem),
        (
            "начатьполучениерабочегокаталогаданныхпользователя",
            "is_file_system_method",
            Category::FileSystem,
        ),
        ("begingettinguserdataworkdir", "is_file_system_method", Category::FileSystem),
    ];
    for (name, recognizer, expected) in global_cases {
        let entry = reg.lookup_global(name).unwrap_or_else(|| {
            panic!(
                "{recognizer}: registry has no global entry for {name:?} \
                 (run `cargo test legacy_recognizer_parity` first to localise)"
            )
        });
        assert!(
            std::mem::discriminant(&entry.category) == std::mem::discriminant(expected),
            "{recognizer}: name {name:?} has category {:?}, expected {expected:?} — \
             a category reassignment would silently drop detection",
            entry.category,
        );
    }

    // Constructor cases — full enumeration matches the legacy lists
    // covered by `legacy_recognizer_parity` (Codex round-2 MAJOR fix:
    // representative samples leak silent regressions for any name the
    // sample omits).
    let constructor_cases: &[(&str, &str, Category)] = &[
        // is_file_system_type — Category::FileSystem (32 names).
        ("file", "is_file_system_type", Category::FileSystem),
        ("файл", "is_file_system_type", Category::FileSystem),
        ("xbase", "is_file_system_type", Category::FileSystem),
        ("htmlwriter", "is_file_system_type", Category::FileSystem),
        ("записьhtml", "is_file_system_type", Category::FileSystem),
        ("htmlreader", "is_file_system_type", Category::FileSystem),
        ("чтениеhtml", "is_file_system_type", Category::FileSystem),
        ("fastinfosetreader", "is_file_system_type", Category::FileSystem),
        ("чтениеfastinfoset", "is_file_system_type", Category::FileSystem),
        ("fastinfosetwriter", "is_file_system_type", Category::FileSystem),
        ("записьfastinfoset", "is_file_system_type", Category::FileSystem),
        ("xsltransform", "is_file_system_type", Category::FileSystem),
        ("преобразованиеxsl", "is_file_system_type", Category::FileSystem),
        ("zipfilewriter", "is_file_system_type", Category::FileSystem),
        ("записьzipфайла", "is_file_system_type", Category::FileSystem),
        ("zipfilereader", "is_file_system_type", Category::FileSystem),
        ("чтениеzipфайла", "is_file_system_type", Category::FileSystem),
        ("textreader", "is_file_system_type", Category::FileSystem),
        ("чтениетекста", "is_file_system_type", Category::FileSystem),
        ("textwriter", "is_file_system_type", Category::FileSystem),
        ("записьтекста", "is_file_system_type", Category::FileSystem),
        ("textextraction", "is_file_system_type", Category::FileSystem),
        ("извлечениетекста", "is_file_system_type", Category::FileSystem),
        ("binarydata", "is_file_system_type", Category::FileSystem),
        ("двоичныеданные", "is_file_system_type", Category::FileSystem),
        ("filestream", "is_file_system_type", Category::FileSystem),
        ("файловыйпоток", "is_file_system_type", Category::FileSystem),
        ("filestreamsmanager", "is_file_system_type", Category::FileSystem),
        ("менеджерфайловыхпотоков", "is_file_system_type", Category::FileSystem),
        ("datawriter", "is_file_system_type", Category::FileSystem),
        ("записьданных", "is_file_system_type", Category::FileSystem),
        ("datareader", "is_file_system_type", Category::FileSystem),
        ("чтениеданных", "is_file_system_type", Category::FileSystem),
        // internet_access::is_internet_constructor — Category::Internet (18 names).
        ("ftpсоединение", "is_internet_constructor", Category::Internet),
        ("ftpconnection", "is_internet_constructor", Category::Internet),
        ("httpсоединение", "is_internet_constructor", Category::Internet),
        ("httpconnection", "is_internet_constructor", Category::Internet),
        ("wsопределения", "is_internet_constructor", Category::Internet),
        ("wsdefinitions", "is_internet_constructor", Category::Internet),
        ("wsпрокси", "is_internet_constructor", Category::Internet),
        ("wsproxy", "is_internet_constructor", Category::Internet),
        ("интернетпочтовыйпрофиль", "is_internet_constructor", Category::Internet),
        ("internetmailprofile", "is_internet_constructor", Category::Internet),
        ("интернетпочта", "is_internet_constructor", Category::Internet),
        ("internetmail", "is_internet_constructor", Category::Internet),
        ("почта", "is_internet_constructor", Category::Internet),
        ("mail", "is_internet_constructor", Category::Internet),
        ("httpзапрос", "is_internet_constructor", Category::Internet),
        ("httprequest", "is_internet_constructor", Category::Internet),
        ("интернетпрокси", "is_internet_constructor", Category::Internet),
        ("internetproxy", "is_internet_constructor", Category::Internet),
    ];
    for (name, recognizer, expected) in constructor_cases {
        let entry = reg.lookup_constructor(name).unwrap_or_else(|| {
            panic!("{recognizer}: registry has no constructor entry for {name:?}")
        });
        assert!(
            std::mem::discriminant(&entry.category) == std::mem::discriminant(expected),
            "{recognizer}: ctor {name:?} has category {:?}, expected {expected:?}",
            entry.category,
        );
    }
}

fn lookup_by_kind(
    reg: &bsl_platform::security::SecurityRegistry,
    name: &str,
    kind: EntryKind,
) -> Option<&'static SecurityEntry> {
    match kind {
        EntryKind::GlobalMethod => reg.lookup_global(name),
        EntryKind::Constructor => reg.lookup_constructor(name),
    }
}
