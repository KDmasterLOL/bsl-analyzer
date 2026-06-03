use bsl_platform::PlatformDataInner;
use hir_def::resolver::Resolver;
use hir_def::Name;

use crate::db::HirDatabase;
use crate::platform_property_lookup::{
    lookup_platform_property_by_type, PlatformPropertyResolution,
};

pub const FORM_TYPE_NAME: &str = "ФормаКлиентскогоПриложения";

pub(crate) fn resolve_form_self_property(
    db: &dyn HirDatabase,
    resolver: &Resolver,
    name: &Name,
) -> Option<PlatformPropertyResolution> {
    let resolution = lookup_platform_property_by_type(db, FORM_TYPE_NAME, name)?;
    if !crate::this_object::is_managed_form_module(db, resolver) {
        return None;
    }
    Some(resolution)
}

pub fn is_form_self_property_name(name: &str) -> bool {
    PlatformDataInner::instance().get_property(FORM_TYPE_NAME, name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_form_self_property_name_recognizes_known_russian_props() {
        for name in &["Элементы", "Команды", "Параметры", "ТекущийЭлемент", "Заголовок"]
        {
            assert!(
                is_form_self_property_name(name),
                "expected {name:?} to be a form-self property"
            );
        }
    }

    #[test]
    fn is_form_self_property_name_is_bilingual() {
        for name in &["Items", "Commands", "Title"] {
            assert!(is_form_self_property_name(name), "expected English alias {name:?} to resolve");
        }
    }

    #[test]
    fn is_form_self_property_name_is_case_insensitive() {
        assert!(is_form_self_property_name("элементы"));
        assert!(is_form_self_property_name("ЭЛЕМЕНТЫ"));
    }

    #[test]
    fn is_form_self_property_name_rejects_non_members() {
        assert!(!is_form_self_property_name("ЭтоТочноНеСвойствоФормы12345"));
    }

    #[test]
    fn no_form_property_collides_with_mdo_plural() {
        let data = PlatformDataInner::instance();
        for prop in data.get_type_properties(FORM_TYPE_NAME) {
            assert!(
                bsl_metadata::MdoType::from_plural(&prop.name).is_none(),
                "form property {:?} collides with an MdoType plural — cascade order \
                 in infer_path_name must be revisited",
                prop.name
            );
            assert!(
                bsl_metadata::MdoType::from_plural(&prop.english_name).is_none(),
                "English form property {:?} collides with an MdoType plural — cascade \
                 order in infer_path_name must be revisited",
                prop.english_name
            );
        }
    }
}
