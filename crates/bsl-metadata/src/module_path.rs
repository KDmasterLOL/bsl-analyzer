//! Structural parsing of a Designer-dump module path.
//!
//! Both the workspace module index and the per-file metadata builder need to
//! read a collection and an object name out of a path, and both used to do it
//! with their own scan. Every scan variant broke on a different shape, so the
//! structure lives here once — and so does the question of which spellings name
//! a collection ([`collection_directory`]). Two tables for one question drifted
//! apart in practice: one layer gave a module its metadata while the other left
//! it out of the index.
//!
//! # The shape
//!
//! ```text
//! <any prefix>/<Collection>/<ObjectName>[/Ext]/<ModuleFile>.bsl
//! ```
//!
//! # Invariants
//!
//! 1. The object name is the segment immediately preceding the service tail;
//!    the collection is the segment immediately preceding the object name.
//! 2. Segments to the LEFT of the collection never take part. An ancestor
//!    directory named like a collection — a Windows profile's `Documents`, a
//!    checkout under `Catalogs/` — must not be mistaken for one.
//! 3. The object name may equal a COLLECTION name: `Catalogs/Constants/…` is
//!    the catalog `Constants`, not the constant collection.
//! 4. A segment named `Ext` is ALWAYS the service level, never an object name.
//!    This is a declared limitation, not an oversight: without it the path is
//!    genuinely ambiguous, because
//!    `…/Documents/Catalogs/Ext/ManagerModule.bsl` reads equally well as the
//!    document `Catalogs` under a service level and as the catalog `Ext` with
//!    none. Objects are not named `Ext` in practice — the service directory
//!    would collide with them in the dump itself — so the service reading wins
//!    and the shape stays decidable.
//! 5. The presence of the service level therefore decides which distance
//!    applies. Deciding instead by which segment merely LOOKS like a collection
//!    is what let an ancestor directory take the type.
//! 6. Form modules are NOT this shape (`…/Forms/<Form>/Ext/Form/Module.bsl`);
//!    they carry their own parser and must be matched before this one.

use crate::MdoType;
use stdx::case::CaseExt;

/// The metadata collection a dump DIRECTORY names, or `None`.
///
/// A directory name is not a name written in code, and the difference is the whole
/// reason this function exists apart from [`MdoType::from_plural`]:
///
/// * `Отчёты` and `Отчеты` are the same directory of the same dump — the exporter
///   that wrote one could have written the other — while the two spellings in BSL
///   source are two different identifiers, only one of which resolves.
/// * A filesystem may hand back `ё` decomposed (`е` + U+0308); HFS+ does. The same
///   directory must still be recognised.
///
/// What this must NOT do is fold `ё` blindly: `РегистрыСвёдений` is a misspelling,
/// not a variant, and accepting it would attach a real register's metadata to a
/// stray file. Only the words that genuinely carry `ё` have a second spelling, and
/// they are listed below.
pub fn collection_directory(segment: &str) -> Option<MdoType> {
    /// Legitimate `ё` spellings and their `е` counterparts. `счетов` is absent on
    /// purpose: «счёт» loses its `ё` in the genitive plural, so `ПланыСчётов` is a
    /// misspelling the way `РегистрыСвёдений` is.
    const YO_SPELLINGS: &[(&str, &str)] = &[
        ("отчёты", "отчеты"),
        ("регистрырасчёта", "регистрырасчета"),
        ("планывидоврасчёта", "планывидоврасчета"),
    ];

    // Case first, composition second: a decomposed CAPITAL `Ё` only looks like the
    // lowercase sequence after folding, and composing before that misses it.
    // Russian has exactly two composed letters — `ё` and `й` — so handling both
    // makes the normalization complete for every name this table can hold.
    let folded = segment.fold_lower();
    let composed = if folded.contains('\u{0308}') || folded.contains('\u{0306}') {
        folded.replace("е\u{0308}", "ё").replace("и\u{0306}", "й")
    } else {
        folded
    };
    let canonical = YO_SPELLINGS
        .iter()
        .find(|(yo, _)| *yo == composed)
        .map_or(composed.as_str(), |(_, plain)| *plain);

    MdoType::from_plural(canonical)
}

/// A module path split into its meaningful parts. Borrows from the normalized
/// path the caller passes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModulePathParts<'a> {
    /// The collection segment, spelled as the path spells it.
    pub collection: &'a str,
    pub object_name: &'a str,
    /// The final segment, e.g. `ManagerModule.bsl`.
    pub module_file: &'a str,
}

/// Split a normalized (`/`-separated) module path per the invariants above.
///
/// `is_collection` decides whether a segment names a collection; the caller
/// supplies it so each layer keeps its own accepted spellings.
pub fn split_module_path<'a>(
    normalized_path: &'a str,
    is_collection: impl Fn(&str) -> bool,
) -> Option<ModulePathParts<'a>> {
    let parts: Vec<&str> = normalized_path.split('/').collect();
    let module_file = *parts.last()?;

    // The service level decides the distance (invariants 4-5). Trying both
    // distances and keeping whichever lands on a collection-looking segment is
    // what made an ancestor directory win.
    let has_service_level =
        parts.len().checked_sub(2).is_some_and(|i| parts[i].eq_ignore_ascii_case("Ext"));
    let distance_from_end = if has_service_level { 4 } else { 3 };
    let collection_idx = parts.len().checked_sub(distance_from_end)?;
    if !is_collection(parts[collection_idx]) {
        return None;
    }

    Some(ModulePathParts {
        collection: parts[collection_idx],
        object_name: parts[collection_idx + 1],
        module_file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spellings these tests treat as collections — deliberately a small
    /// fixed set, so a test failure means the STRUCTURE changed, not a table.
    fn is_collection(segment: &str) -> bool {
        matches!(segment, "Catalogs" | "Documents" | "Constants" | "CommonModules" | "Справочники")
    }

    fn split(path: &str) -> Option<(&str, &str, &str)> {
        split_module_path(path, is_collection).map(|p| (p.collection, p.object_name, p.module_file))
    }

    #[test]
    fn the_canonical_shapes_split() {
        assert_eq!(
            split("Catalogs/Товары/Ext/ManagerModule.bsl"),
            Some(("Catalogs", "Товары", "ManagerModule.bsl"))
        );
        assert_eq!(
            split("/CommonModules/Общий/Module.bsl"),
            Some(("CommonModules", "Общий", "Module.bsl"))
        );
        assert_eq!(
            split("src/cf/Documents/ПКО/Ext/ObjectModule.bsl"),
            Some(("Documents", "ПКО", "ObjectModule.bsl"))
        );
    }

    /// Invariant 2: an ancestor directory named like a collection is not one.
    #[test]
    fn an_ancestor_directory_never_wins() {
        assert_eq!(
            split("/home/Documents/Catalogs/Товары/ManagerModule.bsl"),
            Some(("Catalogs", "Товары", "ManagerModule.bsl"))
        );
        assert_eq!(
            split("C:/Users/Alice/Documents/Catalogs/Товары/Ext/ManagerModule.bsl"),
            Some(("Catalogs", "Товары", "ManagerModule.bsl"))
        );
        assert_eq!(
            split("/Documents/Catalogs/Товары/ManagerModule.bsl"),
            Some(("Catalogs", "Товары", "ManagerModule.bsl"))
        );
    }

    /// Invariant 3: the object name may collide with a collection name.
    #[test]
    fn an_object_may_be_named_like_a_collection() {
        assert_eq!(
            split("/Catalogs/Constants/Ext/ManagerModule.bsl"),
            Some(("Catalogs", "Constants", "ManagerModule.bsl"))
        );
        assert_eq!(
            split("/Catalogs/Documents/ManagerModule.bsl"),
            Some(("Catalogs", "Documents", "ManagerModule.bsl"))
        );
    }

    /// Invariant 4, the declared limitation. `Ext` reads as the service level
    /// even where an object could in principle carry that name, because the two
    /// readings are otherwise indistinguishable — the second case below is the
    /// same path shape as the first with one more prefix segment.
    #[test]
    fn ext_always_reads_as_the_service_level() {
        assert_eq!(split("/Catalogs/Ext/ManagerModule.bsl"), None);
        assert_eq!(
            split("/Documents/Catalogs/Ext/ManagerModule.bsl"),
            Some(("Documents", "Catalogs", "ManagerModule.bsl"))
        );
        // An object genuinely named `Ext` still parses WITH the service level,
        // which is how a dump actually stores it.
        assert_eq!(
            split("/Catalogs/Ext/Ext/ManagerModule.bsl"),
            Some(("Catalogs", "Ext", "ManagerModule.bsl"))
        );
    }

    /// Every shape × every awkward object name, so a future scan cannot pass by
    /// covering one representative.
    #[test]
    fn every_shape_survives_every_awkward_name() {
        for prefix in ["", "/", "src/cf/", "/home/Documents/", "C:/Users/Documents/"] {
            for name in ["Товары", "Catalogs", "Documents", "Constants"] {
                for tail in ["Ext/ManagerModule.bsl", "ManagerModule.bsl"] {
                    let path = format!("{prefix}Catalogs/{name}/{tail}");
                    assert_eq!(
                        split(&path),
                        Some(("Catalogs", name, "ManagerModule.bsl")),
                        "{path}"
                    );
                }
            }
        }
    }

    /// Написание каталога: обе орфографии одного слова — одна коллекция, а
    /// подстановка `ё` вместо обычной `е` коллекцией не становится.
    #[test]
    fn a_directory_spelling_names_the_same_collection() {
        for (segment, expected) in [
            ("Отчеты", MdoType::Report),
            ("Отчёты", MdoType::Report),
            ("ОТЧЁТЫ", MdoType::Report),
            ("Reports", MdoType::Report),
            ("РегистрыРасчёта", MdoType::CalculationRegister),
            ("РегистрыРасчета", MdoType::CalculationRegister),
            ("ПланыВидовРасчёта", MdoType::ChartOfCalculationTypes),
            // `ё`, разложенная файловой системой на `е` и надстрочный знак, —
            // в том числе заглавная, которая до приведения регистра выглядит иначе.
            ("РегистрыРасче\u{0308}та", MdoType::CalculationRegister),
            ("Отче\u{0308}ты", MdoType::Report),
            ("ОТЧЕ\u{0308}ТЫ", MdoType::Report),
            ("Планывидоврасче\u{0308}та", MdoType::ChartOfCalculationTypes),
            // `й` — вторая и последняя составная буква русского алфавита.
            ("РегистрыСведении\u{0306}", MdoType::InformationRegister),
            ("РЕГИСТРЫСВЕДЕНИИ\u{0306}", MdoType::InformationRegister),
        ] {
            assert_eq!(collection_directory(segment), Some(expected), "{segment}");
        }

        for segment in [
            "РегистрыСвёдений",
            "Пёречисления",
            "БизнёсПроцессы",
            "ПланыСчётов",
            "СовсемНеКоллекция",
        ] {
            assert_eq!(collection_directory(segment), None, "{segment} is a misspelling");
        }

        // Контроль: правильные написания тех же коллекций принимаются.
        assert_eq!(collection_directory("РегистрыСведений"), Some(MdoType::InformationRegister));
        assert_eq!(collection_directory("ПланыСчетов"), Some(MdoType::ChartOfAccounts));
    }

    #[test]
    fn a_path_without_a_collection_does_not_split() {
        assert_eq!(split("/home/user/notes.bsl"), None);
        assert_eq!(split("Module.bsl"), None);
        assert_eq!(split("/Unknown/Товары/Ext/ManagerModule.bsl"), None);
    }
}
