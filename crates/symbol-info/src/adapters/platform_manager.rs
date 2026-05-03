//! Adapter for platform-defined manager methods
//! (`Справочники.Склады.НайтиПоКоду`, etc.).
//!
//! Only used after `ManagerModuleMethod` resolution has failed — user-defined
//! methods shadow platform ones.

use bsl_metadata::MdoType;
use bsl_platform::{find_prefixed_method, PlatformDataInner};
use hir::Name;

use crate::adapters::mdo_naming::russian_plural;
use crate::adapters::platform_method::from_platform_method;
use crate::domain::{SignatureSource, SymbolSignature};

pub(super) fn build(mdo_type: MdoType, method: &Name) -> Option<SymbolSignature> {
    let manager_prefix = mdo_type.manager_type_prefix()?;
    // Lookup is delegated to `bsl_platform::find_prefixed_method` — the
    // single canonical implementation of composite-prefix matching
    // (placeholder `name = "<Имя"` + bilingual `docs.syntax` /
    // `english_name` resolution). Presentation overrides below stay
    // here because they're symbol-info-specific surface decisions.
    let found = find_prefixed_method(manager_prefix, method.as_str())?;

    let docs = PlatformDataInner::instance().get_method_docs(found.id);
    let mut sig = from_platform_method(&found, docs.as_ref());
    sig.source = SignatureSource::PlatformManager;
    // Qualifier from platform_method uses `{type_name}.` (e.g. "CatalogManager.");
    // override with the MDO plural form so the user sees "Справочники.<Obj>." style.
    sig.qualifier = Some(smol_str::SmolStr::from(format!("{}.", russian_plural(mdo_type))));
    Some(sig)
}
