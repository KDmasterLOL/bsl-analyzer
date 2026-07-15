use bsl_metadata::MdoType;
use bsl_platform::{find_prefixed_method, PlatformDataInner};
use hir::Name;

use crate::adapters::mdo_naming::russian_plural;
use crate::adapters::platform_method::from_platform_method;
use crate::domain::{SignatureSource, SymbolSignature};

pub(super) fn build(mdo_type: MdoType, method: &Name) -> Option<Vec<SymbolSignature>> {
    let manager_prefix = mdo_type.manager_type_prefix()?;
    let found = find_prefixed_method(manager_prefix, method.as_str())?;

    let docs = PlatformDataInner::instance().get_method_docs(found.id);
    let mut sigs = from_platform_method(&found, docs.as_ref());
    for sig in &mut sigs {
        sig.source = SignatureSource::PlatformManager;
        sig.qualifier = Some(smol_str::SmolStr::from(format!("{}.", russian_plural(mdo_type))));
        sig.platform_id = Some(found.id);
    }
    Some(sigs)
}
