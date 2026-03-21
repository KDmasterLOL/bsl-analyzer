use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::Reader;
use tracing::debug;

use crate::constants::property_id_for_module;
use crate::error::DebugError;
use crate::types::base::ModuleId;

/// Mapping of configuration directory names to Russian type labels.
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

/// Bidirectional index: file path <-> debug module ID.
///
/// Built by scanning a 1C configuration source tree on disk.
/// Used to translate between human-readable module names / file paths
/// and the (objectId, propertyId) pairs required by the debug protocol.
pub struct ModuleIndex {
    by_path: HashMap<PathBuf, ModuleId>,
    by_id: HashMap<ModuleId, PathBuf>,
    by_name: HashMap<String, (ModuleId, PathBuf)>,
}

impl ModuleIndex {
    /// Scans configuration root directory and builds the index.
    ///
    /// `config_root` — path to the `Configuration/` directory containing `Configuration.xml`.
    /// `extensions` — list of (extension_name, extension_root_path) pairs.
    pub fn scan(config_root: &Path, extensions: &[(&str, &Path)]) -> Result<Self, DebugError> {
        let mut index =
            Self { by_path: HashMap::new(), by_id: HashMap::new(), by_name: HashMap::new() };

        let config_xml = config_root.join("Configuration.xml");
        if !config_xml.exists() {
            return Err(DebugError::ConfigRootNotFound(config_root.to_path_buf()));
        }

        index.scan_root("", config_root)?;

        for (ext_name, ext_path) in extensions {
            index.scan_root(ext_name, ext_path)?;
        }

        debug!(modules = index.by_path.len(), "module index built");

        Ok(index)
    }

    /// Resolves file path to module ID.
    pub fn module_by_path(&self, path: &Path) -> Option<&ModuleId> {
        self.by_path.get(path)
    }

    /// Resolves module ID to file path.
    pub fn path_by_module(&self, id: &ModuleId) -> Option<&Path> {
        self.by_id.get(id).map(|p| p.as_path())
    }

    /// Resolves human-readable name like "Справочник.Товары.МодульОбъекта"
    /// or "Catalog.Товары.ObjectModule" to module ID and path.
    ///
    /// If exact match fails, tries appending common module kind suffixes:
    /// "ОбщийМодуль.Foo" → "ОбщийМодуль.Foo.Модуль",
    /// "Справочник.Foo" → "Справочник.Foo.МодульОбъекта", etc.
    pub fn resolve_name(&self, name: &str) -> Option<&(ModuleId, PathBuf)> {
        if let Some(result) = self.by_name.get(name) {
            return Some(result);
        }

        // Fallback: try appending common module kind suffixes
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

    /// Returns all indexed modules.
    pub fn all_modules(&self) -> impl Iterator<Item = (&PathBuf, &ModuleId)> {
        self.by_path.iter()
    }

    /// Total number of indexed modules.
    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    /// Returns all registered human-readable module names.
    pub fn all_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.by_name.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    fn scan_root(&mut self, extension: &str, root: &Path) -> Result<(), DebugError> {
        // Configuration-level modules (Ext/*.bsl)
        let config_xml = root.join("Configuration.xml");
        if let Some(config_object_id) = read_object_uuid(&config_xml)? {
            let ext_dir = root.join("Ext");
            if ext_dir.is_dir() {
                self.index_bsl_files(extension, &config_object_id, "", &ext_dir)?;
            }
        }

        // Scan all metadata type directories
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let dir_path = entry.path();
            if !dir_path.is_dir() || entry.file_name() == "Ext" {
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
        // Each .xml file in the directory is a metadata object
        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();

            let is_xml = path.extension().is_some_and(|e| e == "xml");
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

            // Ext/*.bsl — object-level modules
            let ext_dir = object_dir.join("Ext");
            if ext_dir.is_dir() {
                self.index_bsl_files(extension, &object_id, dir_name, &ext_dir)?;

                // Build human-readable names for object-level modules
                self.register_names(extension, dir_name, &object_name, &object_id, &ext_dir)?;
            }

            // Forms/{FormName}/Ext/Form/Module.bsl
            let forms_dir = object_dir.join("Forms");
            if forms_dir.is_dir() {
                self.scan_forms(extension, dir_name, &object_name, &forms_dir)?;
            }

            // Commands/{CmdName}/Ext/CommandModule.bsl
            let commands_dir = object_dir.join("Commands");
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

            let is_xml = path.extension().is_some_and(|e| e == "xml");
            if !is_xml {
                continue;
            }

            let form_name =
                path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

            let form_object_id = match read_object_uuid(&path)? {
                Some(id) => id,
                None => continue,
            };

            // Forms have their own objectId, propertyId is PROPERTY_FORM_MODULE
            let form_dir = forms_dir.join(&form_name);
            let module_file = form_dir.join("Ext").join("Form").join("Module.bsl");
            if module_file.exists() {
                let property_id = property_id_for_module(dir_name, "Module")
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
        // Command UUIDs are stored inside the parent object's XML
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

            let module_file = cmd_dir.join("Ext").join("CommandModule.bsl");
            if module_file.exists() {
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

            let is_bsl = path.extension().is_some_and(|e| e == "bsl");
            if !is_bsl {
                continue;
            }

            let stem =
                path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

            let property_id = match property_id_for_module(dir_name, &stem) {
                Some(id) => id,
                None => {
                    debug!(dir_name, stem, "skipping unknown module type");
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

            let is_bsl = path.extension().is_some_and(|e| e == "bsl");
            if !is_bsl {
                continue;
            }

            let stem =
                path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

            let property_id = match property_id_for_module(dir_name, &stem) {
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

/// Reads the `uuid` attribute from the root metadata element.
///
/// Expected XML structure:
/// ```xml
/// <MetaDataObject>
///   <SomeType uuid="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx">
///     ...
///   </SomeType>
/// </MetaDataObject>
/// ```
fn read_object_uuid(xml_path: &Path) -> Result<Option<String>, DebugError> {
    let content = fs::read(xml_path)?;
    let mut reader = Reader::from_reader(content.as_slice());

    let mut depth = 0u32;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                depth += 1;
                // depth 2 = child of <MetaDataObject>
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

/// Reads command UUIDs from a metadata object XML.
///
/// Commands are nested inside `<ChildObjects><Command uuid="..."><Properties><Name>...</Name>`.
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

/// Maps directory name to Russian singular type name for human-readable labels.
fn ru_type_name(dir_name: &str) -> &'static str {
    METADATA_TYPES.iter().find(|(d, _)| *d == dir_name).map(|(_, ru)| *ru).unwrap_or("")
}
