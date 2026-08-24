use bsl_metadata::{AttributeType, Form, MdoType, PlatformValueType};
use bsl_platform::PlatformDataInner;
use hir_def::resolver::Resolver;
use hir_def::Name;

use crate::db::HirDatabase;
use crate::platform_property_lookup::{
    lookup_platform_property_by_type, PlatformPropertyResolution,
};

pub const FORM_TYPE_NAME: &str = "ФормаКлиентскогоПриложения";

pub fn managed_form_platform_type_names(form: &Form) -> impl Iterator<Item = &'static str> {
    std::iter::once(FORM_TYPE_NAME).chain(
        form.main_attribute()
            .and_then(|attribute| managed_form_extension_type_name(&attribute.attr_type)),
    )
}

fn managed_form_extension_type_name(attr_type: &AttributeType) -> Option<&'static str> {
    match attr_type {
        AttributeType::AnyObjectRef { mdo_type } | AttributeType::Ref { mdo_type, .. } => {
            Some(match mdo_type {
                MdoType::Catalog => "Расширение формы клиентского приложения для справочника",
                MdoType::Document => "Расширение формы клиентского приложения для документа",
                MdoType::ChartOfCharacteristicTypes => {
                    "Расширение формы клиентского приложения для плана видов характеристик"
                }
                MdoType::BusinessProcess => {
                    "Расширение формы клиентского приложения для бизнес-процесса"
                }
                MdoType::Task => "Расширение формы клиентского приложения для задачи",
                MdoType::DataProcessor => "Расширение формы клиентского приложения для обработки",
                MdoType::Report => "Расширение формы клиентского приложения для отчета",
                MdoType::Constant => "Расширение формы клиентского приложения для констант",
                mdo_type if mdo_type.is_register() => {
                    "Расширение формы клиентского приложения для набора записей"
                }
                _ => "Расширение формы клиентского приложения для объектов",
            })
        }
        AttributeType::Platform(PlatformValueType::ConstantsSet) => {
            Some("Расширение формы клиентского приложения для констант")
        }
        AttributeType::Platform(PlatformValueType::DynamicList) => {
            Some("Расширение формы клиентского приложения для динамического списка")
        }
        AttributeType::Platform(PlatformValueType::SettingsComposer) => {
            Some("Расширение формы клиентского приложения для компоновщика настроек")
        }
        _ => None,
    }
}

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
    use bsl_metadata::{FormAttribute, FormType};
    use uuid::Uuid;

    fn form_with_main_type(attr_type: AttributeType) -> Form {
        let mut form = Form::new("Форма".to_string(), FormType::Managed, Uuid::nil());
        form.attributes.push(FormAttribute {
            name: "Объект".to_string(),
            attr_type,
            is_main: true,
            columns: vec![],
        });
        form
    }

    #[test]
    fn managed_form_platform_types_follow_main_attribute() {
        let document =
            form_with_main_type(AttributeType::AnyObjectRef { mdo_type: MdoType::Document });
        assert_eq!(
            managed_form_platform_type_names(&document).collect::<Vec<_>>(),
            [FORM_TYPE_NAME, "Расширение формы клиентского приложения для документа"]
        );

        let report = form_with_main_type(AttributeType::AnyObjectRef { mdo_type: MdoType::Report });
        assert_eq!(
            managed_form_platform_type_names(&report).collect::<Vec<_>>(),
            [FORM_TYPE_NAME, "Расширение формы клиентского приложения для отчета"]
        );

        let list = form_with_main_type(AttributeType::Platform(PlatformValueType::DynamicList));
        assert_eq!(
            managed_form_platform_type_names(&list).collect::<Vec<_>>(),
            [FORM_TYPE_NAME, "Расширение формы клиентского приложения для динамического списка"]
        );
    }

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
