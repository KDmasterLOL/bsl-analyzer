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
