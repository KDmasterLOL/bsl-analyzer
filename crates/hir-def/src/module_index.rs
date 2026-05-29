use bsl_metadata::MdoType;
use rustc_hash::FxHashMap;
use vfs::FileId;

use crate::body::{ExternalRef, ManagerType};
use crate::Name;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleIndex {
    common_modules: FxHashMap<String, FileId>,

    common_modules_display: FxHashMap<String, String>,

    managers: FxHashMap<(ManagerType, String), FileId>,

    object_modules: FxHashMap<(MdoType, String), FileId>,

    record_set_modules: FxHashMap<(MdoType, String), FileId>,
}

impl ModuleIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build_from_paths<'a>(paths: impl Iterator<Item = (FileId, &'a str)>) -> Self {
        let mut index = Self::new();

        for (file_id, path) in paths {
            let normalized = path.replace('\\', "/");

            let Some((module_type, name, file_kind)) = parse_module_path(&normalized) else {
                continue;
            };
            let lower = name.to_lowercase();

            match file_kind {
                ModuleFileKind::Common => {
                    index.common_modules_display.insert(lower.clone(), name.to_string());
                    index.common_modules.insert(lower, file_id);
                }
                ModuleFileKind::Manager => {
                    if let Some(manager_type) = module_type.to_manager_type() {
                        index.managers.insert((manager_type, lower), file_id);
                    }
                }
                ModuleFileKind::Object => {
                    if let Some(mdo_type) = module_type.to_mdo_type() {
                        index.object_modules.insert((mdo_type, lower), file_id);
                    }
                }
                ModuleFileKind::RecordSet => {
                    if let Some(mdo_type) = module_type.to_mdo_type() {
                        index.record_set_modules.insert((mdo_type, lower), file_id);
                    }
                }
            }
        }

        index
    }

    pub fn resolve(&self, external_ref: &ExternalRef) -> Option<FileId> {
        match external_ref {
            ExternalRef::QualifiedCall { receiver, .. } => self.resolve_common_module(receiver),
            ExternalRef::ManagerAccess { manager_type, object_name, .. } => {
                self.resolve_manager(*manager_type, object_name)
            }
        }
    }

    pub fn resolve_common_module(&self, name: &Name) -> Option<FileId> {
        self.common_modules.get(&name.as_str().to_lowercase()).copied()
    }

    pub fn resolve_manager(&self, manager_type: ManagerType, name: &Name) -> Option<FileId> {
        self.managers.get(&(manager_type, name.as_str().to_lowercase())).copied()
    }

    pub fn resolve_object_module(&self, mdo_type: MdoType, name: &Name) -> Option<FileId> {
        self.object_modules.get(&(mdo_type, name.as_str().to_lowercase())).copied()
    }

    pub fn resolve_record_set_module(&self, mdo_type: MdoType, name: &Name) -> Option<FileId> {
        self.record_set_modules.get(&(mdo_type, name.as_str().to_lowercase())).copied()
    }

    pub fn common_module_count(&self) -> usize {
        self.common_modules.len()
    }

    pub fn manager_count(&self) -> usize {
        self.managers.len()
    }

    pub fn object_module_count(&self) -> usize {
        self.object_modules.len()
    }

    pub fn record_set_module_count(&self) -> usize {
        self.record_set_modules.len()
    }

    pub fn common_module_display_names(&self) -> impl Iterator<Item = &str> {
        self.common_modules.keys().map(|lower| {
            self.common_modules_display.get(lower).map(|s| s.as_str()).unwrap_or(lower)
        })
    }

    pub fn common_module_names(&self) -> impl Iterator<Item = &str> {
        self.common_modules.keys().map(|s| s.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModulePathType {
    CommonModule,
    Document,
    Catalog,
    DataProcessor,
    Report,
    InformationRegister,
    AccumulationRegister,
    AccountingRegister,
    CalculationRegister,
    ChartOfCharacteristicTypes,
    ChartOfAccounts,
    ChartOfCalculationTypes,
    BusinessProcess,
    Task,
    Enum,
    ExchangePlan,
    ExternalDataSource,
    Constant,
}

impl ModulePathType {
    fn to_manager_type(self) -> Option<ManagerType> {
        Some(match self {
            ModulePathType::CommonModule => return None,
            ModulePathType::Document => ManagerType::Documents,
            ModulePathType::Catalog => ManagerType::Catalogs,
            ModulePathType::DataProcessor => ManagerType::DataProcessors,
            ModulePathType::Report => ManagerType::Reports,
            ModulePathType::InformationRegister => ManagerType::InformationRegisters,
            ModulePathType::AccumulationRegister => ManagerType::AccumulationRegisters,
            ModulePathType::AccountingRegister => ManagerType::AccountingRegisters,
            ModulePathType::CalculationRegister => ManagerType::CalculationRegisters,
            ModulePathType::ChartOfCharacteristicTypes => ManagerType::ChartsOfCharacteristicTypes,
            ModulePathType::ChartOfAccounts => ManagerType::ChartsOfAccounts,
            ModulePathType::ChartOfCalculationTypes => ManagerType::ChartsOfCalculationTypes,
            ModulePathType::BusinessProcess => ManagerType::BusinessProcesses,
            ModulePathType::Task => ManagerType::Tasks,
            ModulePathType::Enum => ManagerType::Enums,
            ModulePathType::ExchangePlan => ManagerType::ExchangePlans,
            ModulePathType::ExternalDataSource => ManagerType::ExternalDataSources,
            ModulePathType::Constant => ManagerType::Constants,
        })
    }

    fn to_mdo_type(self) -> Option<MdoType> {
        Some(match self {
            ModulePathType::CommonModule => return None,
            ModulePathType::Document => MdoType::Document,
            ModulePathType::Catalog => MdoType::Catalog,
            ModulePathType::DataProcessor => MdoType::DataProcessor,
            ModulePathType::Report => MdoType::Report,
            ModulePathType::InformationRegister => MdoType::InformationRegister,
            ModulePathType::AccumulationRegister => MdoType::AccumulationRegister,
            ModulePathType::AccountingRegister => MdoType::AccountingRegister,
            ModulePathType::CalculationRegister => MdoType::CalculationRegister,
            ModulePathType::ChartOfCharacteristicTypes => MdoType::ChartOfCharacteristicTypes,
            ModulePathType::ChartOfAccounts => MdoType::ChartOfAccounts,
            ModulePathType::ChartOfCalculationTypes => MdoType::ChartOfCalculationTypes,
            ModulePathType::BusinessProcess => MdoType::BusinessProcess,
            ModulePathType::Task => MdoType::Task,
            ModulePathType::Enum => MdoType::Enum,
            ModulePathType::ExchangePlan => MdoType::ExchangePlan,
            ModulePathType::ExternalDataSource => MdoType::ExternalDataSource,
            ModulePathType::Constant => MdoType::Constant,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModuleFileKind {
    Common,
    Manager,
    Object,
    RecordSet,
}

fn parse_module_path(path: &str) -> Option<(ModulePathType, String, ModuleFileKind)> {
    let parts: Vec<&str> = path.split('/').collect();

    if parts.len() < 4 {
        return None;
    }

    let path_lower = path.to_lowercase();

    for (i, part) in parts.iter().enumerate().rev() {
        let module_type = match part.to_lowercase().as_str() {
            "commonmodules" | "общиемодули" => Some(ModulePathType::CommonModule),
            "documents" | "документы" => Some(ModulePathType::Document),
            "catalogs" | "справочники" => Some(ModulePathType::Catalog),
            "dataprocessors" | "обработки" => Some(ModulePathType::DataProcessor),
            "reports" | "отчёты" | "отчеты" => Some(ModulePathType::Report),
            "informationregisters" | "регистрысведений" => {
                Some(ModulePathType::InformationRegister)
            }
            "accumulationregisters" | "регистрынакопления" => {
                Some(ModulePathType::AccumulationRegister)
            }
            "accountingregisters" | "регистрыбухгалтерии" => {
                Some(ModulePathType::AccountingRegister)
            }
            "calculationregisters" | "регистрырасчёта" | "регистрырасчета" => {
                Some(ModulePathType::CalculationRegister)
            }
            "chartsofcharacteristictypes" | "планывидовхарактеристик" => {
                Some(ModulePathType::ChartOfCharacteristicTypes)
            }
            "chartsofaccounts" | "планысчетов" => Some(ModulePathType::ChartOfAccounts),
            "chartsofcalculationtypes" | "планывидоврасчёта" | "планывидоврасчета" => {
                Some(ModulePathType::ChartOfCalculationTypes)
            }
            "businessprocesses" | "бизнеспроцессы" => {
                Some(ModulePathType::BusinessProcess)
            }
            "tasks" | "задачи" => Some(ModulePathType::Task),
            "enums" | "перечисления" => Some(ModulePathType::Enum),
            "exchangeplans" | "планыобмена" => Some(ModulePathType::ExchangePlan),
            "externaldatasources" | "внешниеисточникиданных" => {
                Some(ModulePathType::ExternalDataSource)
            }
            "constants" | "константы" => Some(ModulePathType::Constant),
            _ => None,
        };

        if let Some(mod_type) = module_type {
            if i + 1 < parts.len() {
                let name = parts[i + 1].to_string();

                if mod_type == ModulePathType::CommonModule {
                    if path_lower.ends_with("module.bsl")
                        && !path_lower.ends_with("managermodule.bsl")
                        && !path_lower.ends_with("objectmodule.bsl")
                        && !path_lower.ends_with("recordsetmodule.bsl")
                    {
                        return Some((mod_type, name, ModuleFileKind::Common));
                    }
                } else if path_lower.ends_with("managermodule.bsl") {
                    return Some((mod_type, name, ModuleFileKind::Manager));
                } else if path_lower.ends_with("objectmodule.bsl") {
                    return Some((mod_type, name, ModuleFileKind::Object));
                } else if path_lower.ends_with("recordsetmodule.bsl") {
                    return Some((mod_type, name, ModuleFileKind::RecordSet));
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_common_module_path() {
        let path = "src/CommonModules/ОбщегоНазначения/Ext/Module.bsl";
        let result = parse_module_path(path);
        assert_eq!(
            result,
            Some((
                ModulePathType::CommonModule,
                "ОбщегоНазначения".to_string(),
                ModuleFileKind::Common,
            )),
        );
    }

    #[test]
    fn test_parse_document_path() {
        let path = "Documents/ПриходнаяНакладная/Ext/ManagerModule.bsl";
        let result = parse_module_path(path);
        assert_eq!(
            result,
            Some((
                ModulePathType::Document,
                "ПриходнаяНакладная".to_string(),
                ModuleFileKind::Manager,
            )),
        );

        let object_path = "Documents/ПриходнаяНакладная/Ext/ObjectModule.bsl";
        assert_eq!(
            parse_module_path(object_path),
            Some((
                ModulePathType::Document,
                "ПриходнаяНакладная".to_string(),
                ModuleFileKind::Object,
            )),
        );
    }

    #[test]
    fn test_parse_catalog_path() {
        let path = "Catalogs/Номенклатура/Ext/ManagerModule.bsl";
        let result = parse_module_path(path);
        assert_eq!(
            result,
            Some((ModulePathType::Catalog, "Номенклатура".to_string(), ModuleFileKind::Manager,)),
        );
    }

    #[test]
    fn test_parse_module_path_with_absolute_documents_prefix() {
        let path = "/Users/test/Documents/git/project/Catalogs/Справочник1/Ext/ObjectModule.bsl";
        let result = parse_module_path(path);
        assert_eq!(
            result,
            Some((ModulePathType::Catalog, "Справочник1".to_string(), ModuleFileKind::Object,)),
        );
    }

    #[test]
    fn test_parse_information_register_path() {
        let manager_path = "InformationRegisters/Test/Ext/ManagerModule.bsl";
        assert_eq!(
            parse_module_path(manager_path),
            Some((
                ModulePathType::InformationRegister,
                "Test".to_string(),
                ModuleFileKind::Manager,
            )),
        );

        let recordset_path = "InformationRegisters/Test/Ext/RecordSetModule.bsl";
        assert_eq!(
            parse_module_path(recordset_path),
            Some((
                ModulePathType::InformationRegister,
                "Test".to_string(),
                ModuleFileKind::RecordSet,
            )),
        );
    }

    #[test]
    fn test_parse_russian_path() {
        let path = "ОбщиеМодули/СтандартныеПодсистемыСервер/Ext/Module.bsl";
        let result = parse_module_path(path);
        assert_eq!(
            result,
            Some((
                ModulePathType::CommonModule,
                "СтандартныеПодсистемыСервер".to_string(),
                ModuleFileKind::Common,
            )),
        );
    }

    #[test]
    fn test_parse_object_module_path() {
        let cat = "Catalogs/Справочник1/Ext/ObjectModule.bsl";
        assert_eq!(
            parse_module_path(cat),
            Some((ModulePathType::Catalog, "Справочник1".to_string(), ModuleFileKind::Object,)),
        );
        let doc = "Documents/Документ1/Ext/ObjectModule.bsl";
        assert_eq!(
            parse_module_path(doc),
            Some((ModulePathType::Document, "Документ1".to_string(), ModuleFileKind::Object,)),
        );
    }

    #[test]
    fn test_resolve_object_module_returns_file_id() {
        let file_id = FileId::from_raw(99);
        let path = "Catalogs/Справочник1/Ext/ObjectModule.bsl";
        let index = ModuleIndex::build_from_paths([(file_id, path)].into_iter());
        assert_eq!(index.object_module_count(), 1);
        assert_eq!(
            index.resolve_object_module(MdoType::Catalog, &Name::new("Справочник1")),
            Some(file_id),
        );
        assert_eq!(index.resolve_object_module(MdoType::Document, &Name::new("Справочник1")), None,);
    }

    #[test]
    fn test_parse_record_set_module_path() {
        let info = "InformationRegisters/РегистрСведений1/Ext/RecordSetModule.bsl";
        assert_eq!(
            parse_module_path(info),
            Some((
                ModulePathType::InformationRegister,
                "РегистрСведений1".to_string(),
                ModuleFileKind::RecordSet,
            )),
        );
        let acc = "AccumulationRegisters/РегистрНакопления1/Ext/RecordSetModule.bsl";
        assert_eq!(
            parse_module_path(acc),
            Some((
                ModulePathType::AccumulationRegister,
                "РегистрНакопления1".to_string(),
                ModuleFileKind::RecordSet,
            )),
        );
    }

    #[test]
    fn test_resolve_record_set_module_returns_file_id() {
        let file_id = FileId::from_raw(101);
        let path = "InformationRegisters/РегистрСведений1/Ext/RecordSetModule.bsl";
        let index = ModuleIndex::build_from_paths([(file_id, path)].into_iter());
        assert_eq!(index.record_set_module_count(), 1);
        assert_eq!(index.manager_count(), 0);
        assert_eq!(index.object_module_count(), 0);
        assert_eq!(
            index.resolve_record_set_module(
                MdoType::InformationRegister,
                &Name::new("РегистрСведений1")
            ),
            Some(file_id),
        );
        assert_eq!(
            index.resolve_record_set_module(
                MdoType::AccumulationRegister,
                &Name::new("РегистрСведений1")
            ),
            None,
        );
    }

    #[test]
    fn test_form_module_remains_unindexed() {
        let form = FileId::from_raw(77);
        let cmd = FileId::from_raw(78);
        let index = ModuleIndex::build_from_paths(
            [
                (form, "Documents/Документ1/Forms/ФормаДокумента/Ext/FormModule.bsl"),
                (cmd, "Catalogs/Справочник1/Commands/КомандаКаталога/Ext/CommandModule.bsl"),
            ]
            .into_iter(),
        );
        assert_eq!(index.object_module_count(), 0);
        assert_eq!(index.manager_count(), 0);
        assert_eq!(index.common_module_count(), 0);
        assert_eq!(index.record_set_module_count(), 0);
    }

    #[test]
    fn test_resolve_common_module() {
        let mut index = ModuleIndex::new();
        let file_id = FileId::from_raw(42);
        index.common_modules.insert("общегоназначения".to_string(), file_id);

        let name = Name::new("ОбщегоНазначения");
        assert_eq!(index.resolve_common_module(&name), Some(file_id));
    }

    #[test]
    fn test_resolve_external_ref() {
        let mut index = ModuleIndex::new();
        let file_id = FileId::from_raw(42);
        index.common_modules.insert("общегоназначения".to_string(), file_id);

        let external_ref = ExternalRef::QualifiedCall {
            receiver: Name::new("ОбщегоНазначения"),
            method: Name::new("СообщитьПользователю"),
            range: syntax::MODULE_RANGE,
        };

        assert_eq!(index.resolve(&external_ref), Some(file_id));
    }
}
