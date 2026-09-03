//! External data processors and reports: the EPF/ERF designer exports.
//!
//! An export is the `DataProcessors/<Name>` (or `Reports/<Name>`) subtree of a
//! configuration dump with the collection directory cut off: `<Name>.xml` beside
//! `<Name>/`, and no `Configuration.xml` anywhere. Its object element is
//! `ExternalDataProcessor` or `ExternalReport`, and its object type — the one a
//! form's main attribute carries — is `ExternalDataProcessorObject.<Name>`.
//!
//! Which of the two an export is gets decided ONCE, when the project validates
//! the declared root, and travels with the root from then on: the structure
//! listing takes it from the workspace snapshot instead of reading the file, so
//! discovery keeps to the [`DirTree`] it is handed. The whole-config load reads
//! the file anyway and decides by the element it finds; the parser checks that
//! the element matches the kind it was asked for, so a root declared as one kind
//! whose export is the other yields an error, not an object of the wrong kind.

use std::path::{Path, PathBuf};

use bsl_conventions::DirTree;

use crate::loader::DiscoveredMdo;
use crate::metadata_object::{MdoType, MetadataObject};

/// What an external object export contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExternalObjectKind {
    /// `<ExternalDataProcessor>` — an EPF export.
    DataProcessor,
    /// `<ExternalReport>` — an ERF export.
    Report,
}

impl ExternalObjectKind {
    /// The element name under `MetaDataObject`, as the designer spells it.
    pub fn element_name(self) -> &'static str {
        match self {
            Self::DataProcessor => "ExternalDataProcessor",
            Self::Report => "ExternalReport",
        }
    }

    pub fn from_element(tag: &str) -> Option<Self> {
        match tag {
            "ExternalDataProcessor" => Some(Self::DataProcessor),
            "ExternalReport" => Some(Self::Report),
            _ => None,
        }
    }

    pub fn mdo_type(self) -> MdoType {
        match self {
            Self::DataProcessor => MdoType::ExternalDataProcessor,
            Self::Report => MdoType::ExternalReport,
        }
    }

    pub fn of_mdo_type(mdo_type: MdoType) -> Option<Self> {
        match mdo_type {
            MdoType::ExternalDataProcessor => Some(Self::DataProcessor),
            MdoType::ExternalReport => Some(Self::Report),
            _ => None,
        }
    }
}

/// The one object XML directly under an export root: the single `.xml` file
/// there, provided no `Configuration.xml` sits beside it. `None` for anything
/// else — a configuration root, an empty directory, several exports in one.
pub fn external_object_xml(root: &Path, tree: &dyn DirTree) -> Option<PathBuf> {
    let mut xmls: Vec<PathBuf> = tree
        .entries(root)
        .into_iter()
        .filter(|entry| entry.is_file())
        .map(|entry| entry.path)
        .filter(|path| bsl_conventions::has_extension(path, bsl_conventions::XML_EXTENSION))
        .collect();
    if xmls.iter().any(|path| {
        path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
            bsl_conventions::conventional_of(name)
                == Some(bsl_conventions::ConventionalName::ConfigurationXml)
        })
    }) {
        return None;
    }
    xmls.sort();
    match xmls.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
}

/// The structure of an export root of a known kind: one metadata object whose
/// main file is the root's object XML. The external counterpart of
/// [`crate::discover_metadata_structure`], keyed on the kind the project
/// established rather than on the file's content.
pub fn discover_external_object_structure(
    root: &Path,
    tree: &dyn DirTree,
    kind: ExternalObjectKind,
) -> Option<DiscoveredMdo> {
    let main = external_object_xml(root, tree)?;
    let name = main.file_stem()?.to_str()?.to_string();
    Some(DiscoveredMdo { mdo_type: kind.mdo_type(), name, main, predefined: None })
}

/// Whole-config loading of an export root: the object XML read and parsed, its
/// kind decided by the element it carries. `None` when the root is not an
/// export or its element is neither external kind; a malformed export logs and
/// yields `None` too, the way a malformed object in a configuration dump does.
pub fn load_external_object(root: &Path) -> Option<MetadataObject> {
    let main = external_object_xml(root, &bsl_conventions::RealFs)?;
    let text = match std::fs::read_to_string(&main) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(path = %main.display(), %error, "cannot read external object");
            return None;
        }
    };
    let doc = match crate::xml_parser::helpers::parse_xml(&text) {
        Ok(doc) => doc,
        Err(error) => {
            tracing::warn!(path = %main.display(), %error, "cannot parse external object");
            return None;
        }
    };
    let Some(element) = crate::xml_parser::helpers::find_mdo_element(&doc) else {
        tracing::warn!(path = %main.display(), "external object XML carries no object element");
        return None;
    };
    let kind = ExternalObjectKind::from_element(element.tag_name().name())?;
    let parsed = match kind {
        ExternalObjectKind::DataProcessor => {
            crate::xml_parser::parse_external_data_processor_xml(&text)
        }
        ExternalObjectKind::Report => crate::xml_parser::parse_external_report_xml(&text),
    };
    match parsed {
        Ok(object) => Some(object),
        Err(error) => {
            tracing::warn!(path = %main.display(), %error, "cannot parse external object");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata_object::AttributeType;
    use bsl_conventions::{PathSetTree, RealFs};

    /// The shape of `~/share/АРМПроизводство.7z`, trimmed to what the loader reads.
    const ARM_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" version="2.20">
	<ExternalDataProcessor uuid="3696c164-ad14-4a0d-b659-10e3bf6d6ad2">
		<Properties>
			<Name>АРМПроизводство</Name>
			<Synonym/>
			<DefaultForm>ExternalDataProcessor.АРМПроизводство.Form.Форма</DefaultForm>
		</Properties>
		<ChildObjects>
			<Attribute uuid="d010948a-27f1-4b21-80a2-361efec05def">
				<Properties>
					<Name>АвторизованныйПользователь</Name>
					<Type><v8:Type>cfg:CatalogRef.Пользователи</v8:Type></Type>
				</Properties>
			</Attribute>
			<TabularSection uuid="e3b4f1c2-0000-4000-8000-000000000001">
				<Properties><Name>Этапы</Name></Properties>
				<ChildObjects>
					<Attribute uuid="e3b4f1c2-0000-4000-8000-000000000002">
						<Properties><Name>Номер</Name><Type><v8:Type>xs:decimal</v8:Type></Type></Properties>
					</Attribute>
				</ChildObjects>
			</TabularSection>
			<Form>Форма</Form>
		</ChildObjects>
	</ExternalDataProcessor>
</MetaDataObject>"#;

    fn write_export(root: &Path, name: &str, xml: &str) {
        std::fs::create_dir_all(root.join(name).join("Forms")).unwrap();
        std::fs::write(root.join(format!("{name}.xml")), xml).unwrap();
    }

    #[test]
    fn an_export_parses_into_an_external_object_with_its_own_attributes() {
        let object = crate::xml_parser::parse_external_data_processor_xml(ARM_XML).unwrap();
        assert_eq!(object.mdo_type, MdoType::ExternalDataProcessor);
        assert_eq!(object.name, "АРМПроизводство");
        let attribute = object.find_attribute("АвторизованныйПользователь").expect("attribute");
        assert!(matches!(
            &attribute.attr_type,
            AttributeType::Ref { mdo_type: MdoType::Catalog, name } if name.as_str() == "Пользователи"
        ));
        assert!(object.find_tabular_section("Этапы").is_some());
    }

    #[test]
    fn the_parser_refuses_an_element_of_another_kind() {
        let as_report = crate::xml_parser::parse_external_report_xml(ARM_XML);
        assert!(as_report.is_err(), "an EPF read as an ERF must not yield a report");
        let internal = ARM_XML.replace("ExternalDataProcessor", "DataProcessor");
        assert!(
            crate::xml_parser::parse_external_data_processor_xml(&internal).is_err(),
            "an internal object's export is not an external one"
        );
        // The control: the same element read as its own kind still parses.
        assert!(crate::xml_parser::parse_data_processor_xml(&internal).is_ok());
    }

    #[test]
    fn a_form_main_attribute_types_the_external_object() {
        let form = |type_name: &str| {
            crate::xml_parser::parse_form_xml(&format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.20">
	<Attributes>
		<Attribute name="Объект" id="1">
			<Type><v8:Type>{type_name}</v8:Type></Type>
			<MainAttribute>true</MainAttribute>
		</Attribute>
	</Attributes>
</Form>"#
            ))
            .unwrap()
        };
        let epf = form("cfg:ExternalDataProcessorObject.АРМПроизводство");
        let main = epf.main_attribute().expect("main attribute");
        assert!(
            matches!(
                &main.attr_type,
                AttributeType::Ref { mdo_type: MdoType::ExternalDataProcessor, name }
                    if name.as_str() == "АРМПроизводство"
            ),
            "got {:?}",
            main.attr_type
        );
        let erf = form("cfg:ExternalReportObject.Отчет");
        assert!(matches!(
            &erf.main_attribute().unwrap().attr_type,
            AttributeType::Ref { mdo_type: MdoType::ExternalReport, .. }
        ));
    }

    #[test]
    fn discovery_takes_the_kind_it_is_given_and_reads_no_content() {
        let tree = PathSetTree::from_files(
            [
                "/epf/АРМ.xml",
                "/epf/АРМ/Forms/Форма.xml",
                "/epf/АРМ/Forms/Форма/Ext/Form/Module.bsl",
            ]
            .map(PathBuf::from),
        );
        let found = discover_external_object_structure(
            Path::new("/epf"),
            &tree,
            ExternalObjectKind::Report,
        )
        .expect("one export");
        assert_eq!(found.mdo_type, MdoType::ExternalReport, "the kind is the caller's");
        assert_eq!(found.name, "АРМ");
        assert_eq!(found.main, PathBuf::from("/epf/АРМ.xml"));
        assert!(found.predefined.is_none());

        let configuration = PathSetTree::from_files(
            ["/cf/Configuration.xml", "/cf/Catalogs/Товары.xml"].map(PathBuf::from),
        );
        assert!(
            discover_external_object_structure(
                Path::new("/cf"),
                &configuration,
                ExternalObjectKind::DataProcessor
            )
            .is_none(),
            "a configuration root is never an export"
        );
        let two = PathSetTree::from_files(["/x/A.xml", "/x/B.xml"].map(PathBuf::from));
        assert!(external_object_xml(Path::new("/x"), &two).is_none(), "one export per root");
    }

    #[test]
    fn a_whole_config_load_of_an_export_root_holds_exactly_the_object() {
        let dir = tempfile::tempdir().unwrap();
        write_export(dir.path(), "АРМПроизводство", ARM_XML);

        let object = load_external_object(dir.path()).expect("the export loads");
        assert_eq!(object.mdo_type, MdoType::ExternalDataProcessor);

        let configuration = crate::load_from_directory(dir.path()).unwrap();
        assert_eq!(configuration.metadata_objects().len(), 1);
        assert!(configuration
            .find_metadata_object(MdoType::ExternalDataProcessor, "АРМПроизводство")
            .is_some());
        assert!(
            configuration.find_metadata_object(MdoType::DataProcessor, "АРМПроизводство").is_none(),
            "the export is not an internal data processor"
        );

        assert!(
            external_object_xml(dir.path(), &RealFs).is_some(),
            "the real tree agrees with the listed one"
        );
    }

    #[test]
    fn external_kinds_are_never_manager_collections() {
        for kind in [MdoType::ExternalDataProcessor, MdoType::ExternalReport] {
            assert!(kind.manager_type_prefix().is_none(), "{kind:?}: no manager");
            assert!(kind.russian_plural().is_none(), "{kind:?}: no collection");
            assert!(!MdoType::all().contains(&kind), "{kind:?}: not enumerated");
        }
        // `ВнешниеОбработки` is the platform's own manager (`Создать`, `Подключить`),
        // not a collection of named objects: the plural must stay unmapped.
        assert_eq!(MdoType::from_plural("ВнешниеОбработки"), None);
        assert_eq!(MdoType::from_plural("ExternalDataProcessors"), None);
        assert_eq!("ВнешняяОбработка".parse::<MdoType>(), Ok(MdoType::ExternalDataProcessor));
        assert_eq!("ExternalReport".parse::<MdoType>(), Ok(MdoType::ExternalReport));
    }
}
