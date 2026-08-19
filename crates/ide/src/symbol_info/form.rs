use std::sync::Arc;

mod card;

use bsl_metadata::MdoType;
use hir::{DefDatabase, Name, Semantics};
use ide_db::RootDatabaseImpl;
use vfs::FileId;

use super::{
    file_path, name_eq, parse_mdo_type, SymbolInfoCard, SymbolInfoRequest, CONFIG_SOURCE_ROOT,
};

#[derive(Clone, Copy)]
struct FormLookup<'a> {
    owner: Option<(MdoType, &'a str)>,
    form_name: &'a str,
    member: Option<&'a str>,
}

pub(super) struct ResolvedForm {
    file_id: FileId,
    owner: Option<(MdoType, String)>,
    form_name: String,
    form: Arc<bsl_metadata::Form>,
}

impl ResolvedForm {
    fn container_name(&self) -> String {
        match &self.owner {
            Some((_mdo_type, object)) => format!("{object}.{}", self.form_name),
            None => self.form_name.clone(),
        }
    }

    fn qualified_form_name(&self) -> String {
        match &self.owner {
            Some((mdo_type, object)) => {
                format!("{}.{}.Форма.{}", mdo_type.russian_name(), object, self.form_name)
            }
            None => format!("ОбщаяФорма.{}", self.form_name),
        }
    }
}

pub(super) fn resolve_form(
    db: &RootDatabaseImpl,
    symbol: &str,
    segments: &[&str],
    req: &SymbolInfoRequest,
) -> Option<SymbolInfoCard> {
    let lookup = parse_form_lookup(segments)?;
    let resolved = resolve_form_lookup(db, lookup)?;
    match lookup.member {
        Some(member) => card::form_member_card(db, symbol, &resolved, member, req),
        None => Some(card::form_card(db, symbol, &resolved, req)),
    }
}

fn parse_form_lookup<'a>(segments: &[&'a str]) -> Option<FormLookup<'a>> {
    match segments {
        [common, form_name] if is_common_form_keyword(common) => {
            Some(FormLookup { owner: None, form_name, member: None })
        }
        [common, form_name, member] if is_common_form_keyword(common) => {
            Some(FormLookup { owner: None, form_name, member: Some(member) })
        }
        [mdo, object, marker, form_name] if is_form_marker(marker) => Some(FormLookup {
            owner: Some((parse_mdo_type(mdo)?, object)),
            form_name,
            member: None,
        }),
        [mdo, object, marker, form_name, member] if is_form_marker(marker) => Some(FormLookup {
            owner: Some((parse_mdo_type(mdo)?, object)),
            form_name,
            member: Some(member),
        }),
        _ => None,
    }
}

fn resolve_form_lookup(db: &RootDatabaseImpl, lookup: FormLookup<'_>) -> Option<ResolvedForm> {
    let module_index = db.module_index(CONFIG_SOURCE_ROOT);
    let form_name = Name::new(lookup.form_name);
    let form_file = match lookup.owner {
        Some((mdo_type, object)) => {
            let object_name = Name::new(object);
            module_index.resolve_form_module(Some((mdo_type, &object_name)), &form_name)
        }
        None => module_index.resolve_form_module(None, &form_name),
    }?;
    let sema = Semantics::new(db);
    let form = sema.form(form_file)?;
    let path_key = file_path(db, form_file).and_then(|path| hir::parse_form_module_path(&path));
    let owner = path_key
        .as_ref()
        .and_then(|key| key.owner.clone())
        .or_else(|| lookup.owner.map(|(mdo_type, object)| (mdo_type, object.to_string())));
    let form_name =
        path_key.map(|key| key.form_name).unwrap_or_else(|| lookup.form_name.to_string());
    Some(ResolvedForm { file_id: form_file, owner, form_name, form })
}

/// `Форма` / `Form` as the marker segment of an owned form's qualified name.
/// Shared with the reference surface, which has to read the SAME strings as a
/// form name — two spellings of one keyword would make one surface resolve a
/// name the other calls missing.
pub(crate) fn is_form_marker(s: &str) -> bool {
    name_eq(s, "Форма") || name_eq(s, "Form")
}

/// `ОбщаяФорма` / `CommonForm`. Shared for the same reason as
/// [`is_form_marker`].
pub(crate) fn is_common_form_keyword(s: &str) -> bool {
    name_eq(s, "ОбщаяФорма") || name_eq(s, "CommonForm")
}

#[cfg(test)]
mod form_path_mirror_tests {
    /// Зеркальность двух форменных классификаторов: hir-def и ide-db обязаны
    /// принимать и отвергать ОДНИ И ТЕ ЖЕ пути — иначе форма распознаётся там,
    /// где её метаданные не загрузятся (или наоборот).
    #[test]
    fn the_two_form_classifiers_agree_on_every_spelling() {
        let paths = [
            "Catalogs/C/Forms/F/Ext/Form/Module.bsl",
            "Catalogs/C/Forms/F/EXT/FORM/MODULE.BSL",
            "CATALOGS/C/FORMS/F/EXT/FORM/MODULE.BSL",
            "CommonForms/Ф/Ext/Form/Module.bsl",
            "Catalogs/C/Ext/ObjectModule.bsl",
            "CommonModules/X/Ext/Module.bsl",
            "Catalogs/C/Forms/F/Ext/Form/Другой.bsl",
            "tmp/forms/X/Ext/Form/Module.bsl",
        ];
        for path in paths {
            let hir_says = hir::parse_form_module_path(path).is_some();
            let ide_db_says = ide_db::metadata::get_module_type_from_uri(path)
                == Some(bsl_metadata::ModuleType::FormModule);
            assert_eq!(hir_says, ide_db_says, "зеркала разошлись на {path}");
        }
        assert!(hir::parse_form_module_path("Catalogs/C/Forms/F/EXT/FORM/MODULE.BSL").is_some());
    }
}
