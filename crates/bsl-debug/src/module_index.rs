use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::Reader;
use tracing::debug;

use crate::constants::property_id_for_module;
use crate::error::DebugError;
use crate::types::base::ModuleId;

const METADATA_TYPES: &[(&str, &str)] = &[
    ("Languages", "Язык"),
    ("Subsystems", "Подсистема"),
    ("StyleItems", "ЭлементСтиля"),
    ("Styles", "Стиль"),
    ("CommonPictures", "ОбщаяКартинка"),
    ("SessionParameters", "ПараметрСеанса"),
    ("Roles", "Роль"),
    ("CommonTemplates", "ОбщийМакет"),
    ("FilterCriteria", "КритерийОтбора"),
    ("CommonModules", "ОбщийМодуль"),
    ("CommonAttributes", "ОбщийРеквизит"),
    ("ExchangePlans", "ПланОбмена"),
    ("XDTOPackages", "ПакетXDTO"),
    ("WebServices", "WebСервис"),
    ("HTTPServices", "HTTPСервис"),
    ("EventSubscriptions", "ПодпискаНаСобытие"),
    ("ScheduledJobs", "РегламентноеЗадание"),
    ("SettingsStorages", "ХранилищеНастроек"),
    ("FunctionalOptions", "ФункциональнаяОпция"),
    ("FunctionalOptionsParameters", "ПараметрФункциональныхОпций"),
    ("DefinedTypes", "ОпределяемыйТип"),
    ("Bots", "Бот"),
    ("CommonCommands", "ОбщаяКоманда"),
    ("CommandGroups", "ГруппаКоманд"),
    ("Constants", "Константа"),
    ("CommonForms", "ОбщаяФорма"),
    ("Catalogs", "Справочник"),
    ("Documents", "Документ"),
    ("DocumentNumerators", "НумераторДокументов"),
    ("Sequences", "Последовательность"),
    ("DocumentJournals", "ЖурналДокументов"),
    ("Enums", "Перечисление"),
    ("Reports", "Отчет"),
    ("DataProcessors", "Обработка"),
    ("InformationRegisters", "РегистрСведений"),
    ("AccumulationRegisters", "РегистрНакопления"),
    ("ChartsOfCharacteristicTypes", "ПланВидовХарактеристик"),
    ("ChartsOfAccounts", "ПланСчетов"),
    ("ChartsOfCalculationTypes", "ПланВидовРасчета"),
    ("AccountingRegisters", "РегистрБухгалтерии"),
    ("CalculationRegisters", "РегистрРасчета"),
    ("BusinessProcesses", "БизнесПроцесс"),
    ("Tasks", "Задача"),
    ("ExternalDataSources", "ВнешнийИсточникДанных"),
];

pub struct ModuleIndex {
    by_path: HashMap<PathBuf, ModuleId>,
    by_id: HashMap<ModuleId, PathBuf>,
    by_name: HashMap<String, (ModuleId, PathBuf)>,
}

impl ModuleIndex {
    pub fn scan(config_root: &Path, extensions: &[(&str, &Path)]) -> Result<Self, DebugError> {
        let mut index =
            Self { by_path: HashMap::new(), by_id: HashMap::new(), by_name: HashMap::new() };

        let config_present = bsl_conventions::find_child_ci(
            config_root,
            bsl_conventions::ConventionalName::ConfigurationXml.canonical(),
        );
        if config_present.is_none() {
            return Err(DebugError::ConfigRootNotFound(config_root.to_path_buf()));
        }

        index.scan_root("", config_root)?;

        for (ext_name, ext_path) in extensions {
            index.scan_root(ext_name, ext_path)?;
        }

        debug!(modules = index.by_path.len(), "module index built");

        Ok(index)
    }

    pub fn module_by_path(&self, path: &Path) -> Option<&ModuleId> {
        self.by_path.get(path)
    }

    pub fn path_by_module(&self, id: &ModuleId) -> Option<&Path> {
        self.by_id.get(id).map(|p| p.as_path())
    }

    pub fn resolve_name(&self, name: &str) -> Option<&(ModuleId, PathBuf)> {
        if let Some(result) = self.by_name.get(name) {
            return Some(result);
        }

        const SUFFIXES: &[&str] = &[
            "Модуль",
            "МодульОбъекта",
            "МодульМенеджера",
            "МодульНабораЗаписей",
            "МодульМенеджераЗначения",
            "МодульФормы",
        ];
        for suffix in SUFFIXES {
            let expanded = format!("{name}.{suffix}");
            if let Some(result) = self.by_name.get(&expanded) {
                return Some(result);
            }
        }

        None
    }

    pub fn all_modules(&self) -> impl Iterator<Item = (&PathBuf, &ModuleId)> {
        self.by_path.iter()
    }

    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    pub fn all_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.by_name.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    fn scan_root(&mut self, extension: &str, root: &Path) -> Result<(), DebugError> {
        let config_xml = bsl_conventions::find_child_ci(
            root,
            bsl_conventions::ConventionalName::ConfigurationXml.canonical(),
        )
        .unwrap_or_else(|| root.join("Configuration.xml"));
        if let Some(config_object_id) = read_object_uuid(&config_xml)? {
            let ext_dir = bsl_conventions::find_child_ci(
                root,
                bsl_conventions::ConventionalName::Ext.canonical(),
            );
            if let Some(ext_dir) = ext_dir.filter(|d| d.is_dir()) {
                self.index_bsl_files(extension, &config_object_id, "", &ext_dir)?;
            }
        }

        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let dir_path = entry.path();
            let is_root_ext = entry.file_name().to_str().and_then(bsl_conventions::conventional_of)
                == Some(bsl_conventions::ConventionalName::Ext);
            if !dir_path.is_dir() || is_root_ext {
                continue;
            }

            let dir_name = entry.file_name().to_string_lossy().to_string();
            self.scan_metadata_dir(extension, &dir_name, &dir_path)?;
        }

        Ok(())
    }

    fn scan_metadata_dir(
        &mut self,
        extension: &str,
        dir_name: &str,
        dir_path: &Path,
    ) -> Result<(), DebugError> {
        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();

            let is_xml = bsl_conventions::has_extension(&path, bsl_conventions::XML_EXTENSION);
            if !is_xml {
                continue;
            }

            let object_name =
                path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

            let object_id = match read_object_uuid(&path)? {
                Some(id) => id,
                None => continue,
            };

            let object_dir = dir_path.join(&object_name);
            if !object_dir.is_dir() {
                continue;
            }

            let ext_dir = bsl_conventions::find_child_ci(
                &object_dir,
                bsl_conventions::ConventionalName::Ext.canonical(),
            )
            .unwrap_or_else(|| object_dir.join("Ext"));
            if ext_dir.is_dir() {
                self.index_bsl_files(extension, &object_id, dir_name, &ext_dir)?;

                self.register_names(extension, dir_name, &object_name, &object_id, &ext_dir)?;
            }

            let forms_dir = bsl_conventions::find_child_ci(
                &object_dir,
                bsl_conventions::ConventionalName::Forms.canonical(),
            )
            .unwrap_or_else(|| object_dir.join("Forms"));
            if forms_dir.is_dir() {
                self.scan_forms(extension, dir_name, &object_name, &forms_dir)?;
            }

            let commands_dir = bsl_conventions::find_child_ci(
                &object_dir,
                bsl_conventions::ConventionalName::Commands.canonical(),
            )
            .unwrap_or_else(|| object_dir.join("Commands"));
            if commands_dir.is_dir() {
                self.scan_commands(extension, dir_name, &object_name, &path, &commands_dir)?;
            }
        }

        Ok(())
    }

    fn scan_forms(
        &mut self,
        extension: &str,
        dir_name: &str,
        object_name: &str,
        forms_dir: &Path,
    ) -> Result<(), DebugError> {
        for entry in fs::read_dir(forms_dir)? {
            let entry = entry?;
            let path = entry.path();

            let is_xml = bsl_conventions::has_extension(&path, bsl_conventions::XML_EXTENSION);
            if !is_xml {
                continue;
            }

            let form_name =
                path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

            let form_object_id = match read_object_uuid(&path)? {
                Some(id) => id,
                None => continue,
            };

            let form_dir = forms_dir.join(&form_name);
            let module_file = bsl_conventions::resolve_chain_ci(
                &form_dir,
                &[
                    bsl_conventions::ConventionalName::Ext.canonical(),
                    bsl_conventions::ConventionalName::Form.canonical(),
                    bsl_conventions::ConventionalName::Module.canonical(),
                ],
            );
            if let Some(module_file) = module_file {
                let property_id =
                    property_id_for_module(dir_name, bsl_conventions::ConventionalName::Module)
                        .unwrap_or(crate::constants::PROPERTY_FORM_MODULE);

                let module_id = ModuleId {
                    extension: extension.to_string(),
                    object_id: form_object_id,
                    property_id: property_id.to_string(),
                };

                self.cache_module(module_file.clone(), module_id.clone());

                let ru_type = ru_type_name(dir_name);
                let name = format!("{ru_type}.{object_name}.Форма.{form_name}");
                self.by_name.insert(name, (module_id, module_file));
            }
        }

        Ok(())
    }

    fn scan_commands(
        &mut self,
        extension: &str,
        dir_name: &str,
        object_name: &str,
        object_xml_path: &Path,
        commands_dir: &Path,
    ) -> Result<(), DebugError> {
        let command_uuids = read_command_uuids(object_xml_path)?;

        for entry in fs::read_dir(commands_dir)? {
            let entry = entry?;
            let cmd_dir = entry.path();
            if !cmd_dir.is_dir() {
                continue;
            }

            let cmd_name = entry.file_name().to_string_lossy().to_string();
            let cmd_object_id = match command_uuids.get(&cmd_name) {
                Some(id) => id,
                None => continue,
            };

            let module_file = bsl_conventions::resolve_chain_ci(
                &cmd_dir,
                &[
                    bsl_conventions::ConventionalName::Ext.canonical(),
                    bsl_conventions::ConventionalName::CommandModule.canonical(),
                ],
            );
            if let Some(module_file) = module_file {
                let module_id = ModuleId {
                    extension: extension.to_string(),
                    object_id: cmd_object_id.clone(),
                    property_id: crate::constants::PROPERTY_COMMAND_MODULE.to_string(),
                };

                self.cache_module(module_file.clone(), module_id.clone());

                let ru_type = ru_type_name(dir_name);
                let name = format!("{ru_type}.{object_name}.Команда.{cmd_name}");
                self.by_name.insert(name, (module_id, module_file));
            }
        }

        Ok(())
    }

    fn index_bsl_files(
        &mut self,
        extension: &str,
        object_id: &str,
        dir_name: &str,
        ext_dir: &Path,
    ) -> Result<(), DebugError> {
        for entry in fs::read_dir(ext_dir)? {
            let entry = entry?;
            let path = entry.path();

            let is_bsl = bsl_conventions::has_extension(&path, bsl_conventions::BSL_EXTENSION);
            if !is_bsl {
                continue;
            }

            let Some(kind) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(bsl_conventions::conventional_of)
            else {
                continue;
            };

            let property_id = match property_id_for_module(dir_name, kind) {
                Some(id) => id,
                None => {
                    debug!(dir_name, ?kind, "skipping unknown module type");
                    continue;
                }
            };

            let module_id = ModuleId {
                extension: extension.to_string(),
                object_id: object_id.to_string(),
                property_id: property_id.to_string(),
            };

            self.cache_module(path, module_id);
        }

        Ok(())
    }

    fn register_names(
        &mut self,
        extension: &str,
        dir_name: &str,
        object_name: &str,
        object_id: &str,
        ext_dir: &Path,
    ) -> Result<(), DebugError> {
        for entry in fs::read_dir(ext_dir)? {
            let entry = entry?;
            let path = entry.path();

            let is_bsl = bsl_conventions::has_extension(&path, bsl_conventions::BSL_EXTENSION);
            if !is_bsl {
                continue;
            }

            let Some(kind) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(bsl_conventions::conventional_of)
            else {
                continue;
            };

            let property_id = match property_id_for_module(dir_name, kind) {
                Some(id) => id,
                None => continue,
            };

            let module_id = ModuleId {
                extension: extension.to_string(),
                object_id: object_id.to_string(),
                property_id: property_id.to_string(),
            };

            let ru_type = ru_type_name(dir_name);
            let ru_module = crate::constants::module_kind_label(property_id);
            let name = format!("{ru_type}.{object_name}.{ru_module}");
            self.by_name.insert(name, (module_id, path));
        }

        Ok(())
    }

    fn cache_module(&mut self, path: PathBuf, module_id: ModuleId) {
        self.by_id.insert(module_id.clone(), path.clone());
        self.by_path.insert(path, module_id);
    }
}

fn read_object_uuid(xml_path: &Path) -> Result<Option<String>, DebugError> {
    let content = fs::read(xml_path)?;
    let mut reader = Reader::from_reader(content.as_slice());

    let mut depth = 0u32;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                depth += 1;
                if depth == 2 {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"uuid" {
                            let value = attr.unescape_value().map_err(|e| {
                                DebugError::XmlParse { path: xml_path.to_path_buf(), source: e }
                            })?;
                            return Ok(Some(value.to_string()));
                        }
                    }
                    return Ok(None);
                }
            }
            Ok(Event::Eof) => return Ok(None),
            Err(e) => return Err(DebugError::XmlParse { path: xml_path.to_path_buf(), source: e }),
            _ => {}
        }
    }
}

fn read_command_uuids(xml_path: &Path) -> Result<HashMap<String, String>, DebugError> {
    let content = fs::read(xml_path)?;
    let mut reader = Reader::from_reader(content.as_slice());

    let mut result = HashMap::new();
    let mut in_command = false;
    let mut current_uuid = String::new();
    let mut in_name = false;
    let mut depth = 0u32;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let name_bytes = e.name();
                let local = local_name(name_bytes.as_ref());

                if local == b"Command" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"uuid" {
                            current_uuid = attr
                                .unescape_value()
                                .map_err(|e| DebugError::XmlParse {
                                    path: xml_path.to_path_buf(),
                                    source: e,
                                })?
                                .to_string();
                            in_command = true;
                        }
                    }
                } else if in_command && local == b"Name" {
                    in_name = true;
                }
            }
            Ok(Event::Text(e)) if in_name => {
                let name = String::from_utf8_lossy(e.as_ref()).to_string();
                result.insert(name, current_uuid.clone());
                in_name = false;
                in_command = false;
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(DebugError::XmlParse { path: xml_path.to_path_buf(), source: e }),
            _ => {}
        }
    }

    Ok(result)
}

fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().position(|&b| b == b':') {
        Some(pos) => &name[pos + 1..],
        None => name,
    }
}

fn ru_type_name(dir_name: &str) -> &'static str {
    METADATA_TYPES.iter().find(|(d, _)| *d == dir_name).map(|(_, ru)| *ru).unwrap_or("")
}

#[cfg(test)]
mod case_parity_tests {
    //! По контролю на каждую независимую ветвь индекса; в фикстуре каждой
    //! ветви регистр меняется у ВСЕХ конвенционных сегментов её цепочки.

    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn object_xml(uuid: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?><MetaDataObject><Catalog uuid=\"{uuid}\"></Catalog></MetaDataObject>"
        )
    }

    fn root_with_config(dir: &Path, config_name: &str) {
        write(
            &dir.join(config_name),
            "<?xml version=\"1.0\"?><MetaDataObject><Configuration \
             uuid=\"00000000-0000-0000-0000-0000000000c0\"></Configuration></MetaDataObject>",
        );
    }

    #[test]
    fn every_branch_of_the_index_takes_case_variant_conventional_segments() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Корень: CONFIGURATION.XML целиком в другом регистре + корневой модуль.
        root_with_config(root, "CONFIGURATION.XML");
        write(&root.join("EXT/SESSIONMODULE.BSL"), "//");
        // Объектная ветвь: XML-фильтр обхода на C.XML + объектный модуль.
        write(&root.join("Catalogs/C.XML"), &object_xml("00000000-0000-0000-0000-000000000001"));
        write(&root.join("Catalogs/C/EXT/OBJECTMODULE.BSL"), "//");
        // Форменная цепочка.
        write(
            &root.join("Catalogs/C/FORMS/F.XML"),
            &object_xml("00000000-0000-0000-0000-000000000002"),
        );
        write(&root.join("Catalogs/C/FORMS/F/EXT/FORM/MODULE.BSL"), "//");

        let index = ModuleIndex::scan(root, &[]).expect("CONFIGURATION.XML — тот же корень");

        assert!(
            index.module_by_path(&root.join("EXT/SESSIONMODULE.BSL")).is_some(),
            "корневой модуль сеанса в индексе"
        );
        assert!(
            index.module_by_path(&root.join("Catalogs/C/EXT/OBJECTMODULE.BSL")).is_some(),
            "объектный модуль в индексе"
        );
        assert!(
            index.module_by_path(&root.join("Catalogs/C/FORMS/F/EXT/FORM/MODULE.BSL")).is_some(),
            "форменная цепочка в индексе"
        );
    }

    #[test]
    fn the_command_branch_takes_case_variant_segments() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        root_with_config(root, "Configuration.xml");
        write(
            &root.join("Catalogs/C.xml"),
            "<?xml version=\"1.0\"?><MetaDataObject><Catalog \
             uuid=\"00000000-0000-0000-0000-000000000001\"><ChildObjects><Command \
             uuid=\"00000000-0000-0000-0000-000000000003\"><Properties><Name>К</Name>\
             </Properties></Command></ChildObjects></Catalog></MetaDataObject>",
        );
        std::fs::create_dir_all(root.join("Catalogs/C/Ext")).unwrap();
        write(&root.join("Catalogs/C/COMMANDS/К/EXT/COMMANDMODULE.BSL"), "//");

        let index = ModuleIndex::scan(root, &[]).unwrap();
        assert!(
            index
                .module_by_path(&root.join("Catalogs/C/COMMANDS/К/EXT/COMMANDMODULE.BSL"))
                .is_some(),
            "командная цепочка в индексе"
        );
    }
}
