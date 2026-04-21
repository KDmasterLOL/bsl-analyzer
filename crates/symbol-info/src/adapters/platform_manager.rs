//! Adapter for platform-defined manager methods
//! (`Справочники.Склады.НайтиПоКоду`, etc.).
//!
//! Only used after `ManagerModuleMethod` resolution has failed — user-defined
//! methods shadow platform ones.

use bsl_metadata::MdoType;
use bsl_platform::{PlatformData, PlatformDataInner};
use hir::Name;

use crate::adapters::mdo_naming::russian_plural;
use crate::adapters::platform_method::from_platform_method;
use crate::domain::{SignatureSource, SymbolSignature};

pub(super) fn build(mdo_type: MdoType, method: &Name) -> Option<SymbolSignature> {
    let manager_prefix = mdo_type.manager_type_prefix()?;
    let data = PlatformData::instance();
    let docs_db = PlatformDataInner::instance();

    let method_lower = method.as_str().to_lowercase();

    // Manager methods in platform data use name="<Имя" for all entries; the real
    // Russian name lives in `docs.syntax`. Match either via syntax.split('(')
    // or via the English name after the dot.
    let found = data.get_manager_methods(manager_prefix).into_iter().find(|m| {
        let docs = docs_db.get_method_docs(m.id);
        let ru_match = docs
            .as_ref()
            .and_then(|d| d.syntax.split('(').next())
            .is_some_and(|ru| ru.to_lowercase() == method_lower);
        if ru_match {
            return true;
        }
        let en_name = m.english_name.rsplit_once('.').map(|(_, n)| n).unwrap_or(&m.english_name);
        en_name.to_lowercase() == method_lower
    })?;

    let docs = docs_db.get_method_docs(found.id);
    let mut sig = from_platform_method(found, docs.as_ref());
    sig.source = SignatureSource::PlatformManager;
    // Qualifier from platform_method uses `{type_name}.` (e.g. "CatalogManager.");
    // override with the MDO plural form so the user sees "Справочники.<Obj>." style.
    sig.qualifier = Some(smol_str::SmolStr::from(format!("{}.", russian_plural(mdo_type))));
    Some(sig)
}
