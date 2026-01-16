//! Module index for fast module resolution without parsing.
//!
//! This module provides a lightweight index that maps module names to file IDs
//! based solely on file paths (Designer format). No parsing is required.
//!
//! ## Performance
//!
//! Building the index from VFS paths is very fast (~10ms for 6,540 files)
//! because it only analyzes path strings, not file contents.

use rustc_hash::FxHashMap;
use vfs::FileId;

use crate::body::{ExternalRef, ManagerType};
use crate::Name;

/// Index for resolving module references without parsing.
///
/// Built from VFS file paths using Designer format conventions:
/// - `CommonModules/<Name>/Ext/Module.bsl` → CommonModule
/// - `Documents/<Name>/Ext/*.bsl` → Document
/// - `Catalogs/<Name>/Ext/*.bsl` → Catalog
/// - etc.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleIndex {
    /// CommonModules: lowercase name → FileId
    common_modules: FxHashMap<String, FileId>,

    /// Manager objects: (ManagerType, lowercase object name) → FileId (manager module)
    managers: FxHashMap<(ManagerType, String), FileId>,
}

impl ModuleIndex {
    /// Create a new empty module index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build module index from an iterator of (FileId, path) pairs.
    ///
    /// Analyzes file paths to identify CommonModules and manager objects
    /// without parsing any BSL files.
    pub fn build_from_paths<'a>(paths: impl Iterator<Item = (FileId, &'a str)>) -> Self {
        let mut index = Self::new();

        for (file_id, path) in paths {
            // Normalize path separators
            let normalized = path.replace('\\', "/");

            // Extract module info from path
            if let Some((module_type, name)) = parse_module_path(&normalized) {
                match module_type {
                    ModulePathType::CommonModule => {
                        index.common_modules.insert(name.to_lowercase(), file_id);
                    }
                    ModulePathType::Document => {
                        index
                            .managers
                            .insert((ManagerType::Documents, name.to_lowercase()), file_id);
                    }
                    ModulePathType::Catalog => {
                        index
                            .managers
                            .insert((ManagerType::Catalogs, name.to_lowercase()), file_id);
                    }
                    ModulePathType::DataProcessor => {
                        index
                            .managers
                            .insert((ManagerType::DataProcessors, name.to_lowercase()), file_id);
                    }
                    ModulePathType::Report => {
                        index.managers.insert((ManagerType::Reports, name.to_lowercase()), file_id);
                    }
                    ModulePathType::InformationRegister => {
                        index.managers.insert(
                            (ManagerType::InformationRegisters, name.to_lowercase()),
                            file_id,
                        );
                    }
                    ModulePathType::AccumulationRegister => {
                        index.managers.insert(
                            (ManagerType::AccumulationRegisters, name.to_lowercase()),
                            file_id,
                        );
                    }
                    ModulePathType::AccountingRegister => {
                        index.managers.insert(
                            (ManagerType::AccountingRegisters, name.to_lowercase()),
                            file_id,
                        );
                    }
                    ModulePathType::CalculationRegister => {
                        index.managers.insert(
                            (ManagerType::CalculationRegisters, name.to_lowercase()),
                            file_id,
                        );
                    }
                    ModulePathType::ChartOfCharacteristicTypes => {
                        index.managers.insert(
                            (ManagerType::ChartsOfCharacteristicTypes, name.to_lowercase()),
                            file_id,
                        );
                    }
                    ModulePathType::ChartOfAccounts => {
                        index
                            .managers
                            .insert((ManagerType::ChartsOfAccounts, name.to_lowercase()), file_id);
                    }
                    ModulePathType::ChartOfCalculationTypes => {
                        index.managers.insert(
                            (ManagerType::ChartsOfCalculationTypes, name.to_lowercase()),
                            file_id,
                        );
                    }
                    ModulePathType::BusinessProcess => {
                        index
                            .managers
                            .insert((ManagerType::BusinessProcesses, name.to_lowercase()), file_id);
                    }
                    ModulePathType::Task => {
                        index.managers.insert((ManagerType::Tasks, name.to_lowercase()), file_id);
                    }
                    ModulePathType::Enum => {
                        index.managers.insert((ManagerType::Enums, name.to_lowercase()), file_id);
                    }
                    ModulePathType::ExchangePlan => {
                        index
                            .managers
                            .insert((ManagerType::ExchangePlans, name.to_lowercase()), file_id);
                    }
                    ModulePathType::ExternalDataSource => {
                        index.managers.insert(
                            (ManagerType::ExternalDataSources, name.to_lowercase()),
                            file_id,
                        );
                    }
                    ModulePathType::Constant => {
                        index
                            .managers
                            .insert((ManagerType::Constants, name.to_lowercase()), file_id);
                    }
                }
            }
        }

        index
    }

    /// Resolve an external reference to a FileId.
    ///
    /// Returns None if the module is not found in the index.
    pub fn resolve(&self, external_ref: &ExternalRef) -> Option<FileId> {
        match external_ref {
            ExternalRef::QualifiedCall { receiver, .. } => self.resolve_common_module(receiver),
            ExternalRef::ManagerAccess { manager_type, object_name, .. } => {
                self.resolve_manager(*manager_type, object_name)
            }
        }
    }

    /// Resolve a CommonModule by name.
    pub fn resolve_common_module(&self, name: &Name) -> Option<FileId> {
        self.common_modules.get(&name.as_str().to_lowercase()).copied()
    }

    /// Resolve a manager object (Document, Catalog, etc.) by type and name.
    pub fn resolve_manager(&self, manager_type: ManagerType, name: &Name) -> Option<FileId> {
        self.managers.get(&(manager_type, name.as_str().to_lowercase())).copied()
    }

    /// Get number of indexed CommonModules.
    pub fn common_module_count(&self) -> usize {
        self.common_modules.len()
    }

    /// Get number of indexed manager objects.
    pub fn manager_count(&self) -> usize {
        self.managers.len()
    }

    /// Iterate over all CommonModule names.
    pub fn common_module_names(&self) -> impl Iterator<Item = &str> {
        self.common_modules.keys().map(|s| s.as_str())
    }
}

/// Type of module extracted from path.
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

/// Parse module path to extract type and name.
///
/// Designer format paths:
/// - `CommonModules/<Name>/Ext/Module.bsl`
/// - `Documents/<Name>/Ext/ObjectModule.bsl`
/// - `Catalogs/<Name>/Ext/ObjectModule.bsl`
/// - `DataProcessors/<Name>/Ext/ObjectModule.bsl`
/// - `Reports/<Name>/Ext/ObjectModule.bsl`
/// - `InformationRegisters/<Name>/Ext/RecordSetModule.bsl`
/// - `AccumulationRegisters/<Name>/Ext/RecordSetModule.bsl`
fn parse_module_path(path: &str) -> Option<(ModulePathType, String)> {
    let parts: Vec<&str> = path.split('/').collect();

    // Need at least: <Type>/<Name>/Ext/<Module>.bsl
    if parts.len() < 4 {
        return None;
    }

    // Find the type folder and extract name
    for (i, part) in parts.iter().enumerate() {
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
            // Next part should be the module/object name
            if i + 1 < parts.len() {
                let name = parts[i + 1].to_string();

                // Verify it's a .bsl file (anywhere in the path after name)
                if path.to_lowercase().ends_with(".bsl") {
                    return Some((mod_type, name));
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
        assert_eq!(result, Some((ModulePathType::CommonModule, "ОбщегоНазначения".to_string())));
    }

    #[test]
    fn test_parse_document_path() {
        let path = "Documents/ПриходнаяНакладная/Ext/ObjectModule.bsl";
        let result = parse_module_path(path);
        assert_eq!(result, Some((ModulePathType::Document, "ПриходнаяНакладная".to_string())));
    }

    #[test]
    fn test_parse_catalog_path() {
        let path = "Catalogs/Номенклатура/Ext/ObjectModule.bsl";
        let result = parse_module_path(path);
        assert_eq!(result, Some((ModulePathType::Catalog, "Номенклатура".to_string())));
    }

    #[test]
    fn test_parse_russian_path() {
        let path = "ОбщиеМодули/СтандартныеПодсистемыСервер/Ext/Module.bsl";
        let result = parse_module_path(path);
        assert_eq!(
            result,
            Some((ModulePathType::CommonModule, "СтандартныеПодсистемыСервер".to_string()))
        );
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
            range: text_size::TextRange::empty(0.into()),
        };

        assert_eq!(index.resolve(&external_ref), Some(file_id));
    }
}
