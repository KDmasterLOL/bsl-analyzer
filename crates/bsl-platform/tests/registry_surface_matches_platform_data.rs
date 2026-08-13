//! The security and capability registries are hand-written while the platform
//! database is generated from the HBK dumps, so a name can drift out from under
//! an entry without any registry test noticing: those tests restate the
//! registry's own literals and stay green on a ghost entry. These checks compare
//! each entry against the generated data, by the surface the entry declares.

use bsl_platform::capability;
use bsl_platform::security;
use bsl_platform::PlatformData;
use stdx::case::CaseExt;

/// Any platform type declaring a method spelled this way.
fn some_type_declares(name: &str) -> bool {
    let folded = name.fold_lower();
    PlatformData::instance()
        .all_methods()
        .iter()
        .any(|m| m.name.fold_lower() == folded || m.english_name.fold_lower() == folded)
}

fn is_global_function(name: &str) -> bool {
    PlatformData::instance().get_global_function(name).is_some()
}

fn is_known_type(name: &str) -> bool {
    PlatformData::instance().get_type(name).is_some()
}

/// Entries the platform data contradicts and that are knowingly left alone.
/// Each costs a wrong attribution, so the list must not grow silently.
///
/// `СообщитьПользователю` is a BSP library method — the deprecation registry
/// itself recommends it as `ОбщегоНазначения.СообщитьПользователю` — yet it is
/// registered as a platform global. Narrowing it to its owners changes how
/// `catch_class` classifies recovery in `Попытка`, which is a separate decision
/// with its own measure.
const KNOWN_DEVIATIONS: &[&str] = &["СообщитьПользователю"];

/// A guarded name is worth guarding only where the platform actually has it.
#[test]
fn security_entries_match_platform_surface() {
    let mut wrong = Vec::new();

    for entry in security::registry().entries() {
        if KNOWN_DEVIATIONS.contains(&entry.ru) {
            continue;
        }
        match entry.kind {
            security::EntryKind::GlobalMethod => {
                if !is_global_function(entry.ru) && !is_global_function(entry.en) {
                    wrong.push(format!("{}: global method absent from platform data", entry.ru));
                }
            }
            security::EntryKind::Constructor => {
                if !is_known_type(entry.ru) && !is_known_type(entry.en) {
                    wrong.push(format!("{}: constructor type absent from platform data", entry.ru));
                }
            }
            // The owners are library common modules — BSP ships them, the
            // platform does not. A method name colliding with a platform one is
            // expected and is the very reason this kind carries owners:
            // `ОткрытьФайл` is also spelled by nine serializers.
            security::EntryKind::ModuleMethod { owners } => {
                for owner in owners {
                    if is_known_type(owner) {
                        wrong.push(format!(
                            "{}: owner {owner} is a platform type, not a library module",
                            entry.ru
                        ));
                    }
                }
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "security registry disagrees with platform data:\n{}",
        wrong.join("\n")
    );
}

/// A deviation that stopped deviating is a stale excuse: it would hide the next
/// drift of the same name.
#[test]
fn known_deviations_are_still_deviating() {
    for name in KNOWN_DEVIATIONS {
        let entry = security::registry()
            .lookup_global(name)
            .unwrap_or_else(|| panic!("{name}: no longer a registry global, drop the deviation"));
        assert!(
            !is_global_function(entry.ru) && !is_global_function(entry.en),
            "{name}: platform data now has it, drop the deviation"
        );
    }
}

#[test]
fn capability_entries_match_platform_surface() {
    let mut wrong = Vec::new();

    for entry in capability::registry().entries() {
        match entry.kind {
            capability::EntryKind::GlobalMethod => {
                if !is_global_function(entry.ru) && !is_global_function(entry.en) {
                    wrong.push(format!("{}: global method absent from platform data", entry.ru));
                }
            }
            // Spelling alone matches this kind, so at least one type must own
            // the name — otherwise every match is provably something else. A
            // same-named global function does not save the entry: the global
            // surface is declared by a separate `GlobalMethod` entry.
            capability::EntryKind::AnyReceiverMethod => {
                if !some_type_declares(entry.ru) && !some_type_declares(entry.en) {
                    wrong.push(format!("{}: no platform type declares this method", entry.ru));
                }
            }
            capability::EntryKind::Constructor | capability::EntryKind::Type => {
                if !is_known_type(entry.ru) && !is_known_type(entry.en) {
                    wrong.push(format!("{}: type absent from platform data", entry.ru));
                }
            }
            capability::EntryKind::GlobalProperty => {}
        }
    }

    assert!(
        wrong.is_empty(),
        "capability registry disagrees with platform data:\n{}",
        wrong.join("\n")
    );
}
