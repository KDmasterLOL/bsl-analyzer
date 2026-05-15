//! Module index for fast module resolution without parsing.
//!
//! This module provides a lightweight index that maps module names to file IDs
//! based solely on file paths (Designer format). No parsing is required.
//!
//! ## Performance
//!
//! Building the index from VFS paths is very fast (~10ms for 6,540 files)
//! because it only analyzes path strings, not file contents.

use bsl_metadata::MdoType;
use rustc_hash::FxHashMap;
use vfs::FileId;

use crate::body::{ExternalRef, ManagerType};
use crate::Name;

/// Index for resolving module references without parsing.
///
/// Built from VFS file paths using Designer format conventions:
/// - `CommonModules/<Name>/Ext/Module.bsl` → CommonModule
/// - `<Folder>/<Name>/Ext/ManagerModule.bsl` → Manager module
/// - `<Folder>/<Name>/Ext/ObjectModule.bsl` → Object module (Phase B)
/// - `<Folder>/<Name>/Ext/RecordSetModule.bsl` → Record-set module (Phase C)
///
/// Other module flavours (`FormModule.bsl`, `CommandModule.bsl`) are
/// deliberately not indexed — they have no type-system call surface
/// today.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleIndex {
    /// CommonModules: lowercase name → FileId
    common_modules: FxHashMap<String, FileId>,

    /// CommonModules: lowercase name → display-cased name (as extracted from
    /// the file path). Mirrors [`Self::common_modules`] keys; used by IDE
    /// completion / hover to render labels with their canonical casing.
    common_modules_display: FxHashMap<String, String>,

    /// Manager objects: (ManagerType, lowercase object name) → FileId (manager module)
    managers: FxHashMap<(ManagerType, String), FileId>,

    /// Object modules: (MdoType, lowercase object name) → FileId
    /// (`<MDO>/Ext/ObjectModule.bsl`).
    ///
    /// Keyed on [`MdoType`] (not [`ManagerType`]) to match
    /// [`crate::ty::Ty::ObjectManager.kind`] and the strict
    /// `MetadataKind → MdoType` filter on the call site (Phase B). Not
    /// every `ManagerType` has a paired `*Object` `MetadataKind`
    /// (registers, charts of calculation types, business processes,
    /// tasks today), but the index itself is permissive — the strict
    /// filter at lookup time gates which kinds are eligible.
    object_modules: FxHashMap<(MdoType, String), FileId>,

    /// Record-set modules: (MdoType, lowercase register name) → FileId
    /// (`<RegisterFolder>/<Name>/Ext/RecordSetModule.bsl`).
    ///
    /// Phase C: workspace call surface for register-set receivers.
    /// The strict filter at lookup time (`record_set_kind_to_mdo` in
    /// `hir-ty/method_resolution.rs`) gates which `MetadataKind`
    /// variants actually consult this map — today only
    /// `AccumulationRegisterRecordSet`. The index itself is permissive
    /// (any register MDO whose path emits a `RecordSetModule.bsl`
    /// file is recorded), mirroring the `object_modules` contract.
    record_set_modules: FxHashMap<(MdoType, String), FileId>,
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

    /// Resolve an object module (`<MDO>/Ext/ObjectModule.bsl`) by MDO
    /// type and object name. Phase B counterpart to
    /// [`Self::resolve_manager`].
    pub fn resolve_object_module(&self, mdo_type: MdoType, name: &Name) -> Option<FileId> {
        self.object_modules.get(&(mdo_type, name.as_str().to_lowercase())).copied()
    }

    /// Resolve a record-set module
    /// (`<RegisterFolder>/<Name>/Ext/RecordSetModule.bsl`) by register
    /// MDO type and name. Phase C counterpart to
    /// [`Self::resolve_object_module`] for register receivers.
    pub fn resolve_record_set_module(&self, mdo_type: MdoType, name: &Name) -> Option<FileId> {
        self.record_set_modules.get(&(mdo_type, name.as_str().to_lowercase())).copied()
    }

    /// Get number of indexed CommonModules.
    pub fn common_module_count(&self) -> usize {
        self.common_modules.len()
    }

    /// Get number of indexed manager objects.
    pub fn manager_count(&self) -> usize {
        self.managers.len()
    }

    /// Get number of indexed object modules.
    pub fn object_module_count(&self) -> usize {
        self.object_modules.len()
    }

    /// Get number of indexed record-set modules.
    pub fn record_set_module_count(&self) -> usize {
        self.record_set_modules.len()
    }

    /// Iterate over all CommonModule display-cased names (preserving the
    /// casing extracted from the file path). When the indexer fell back to
    /// lowercase storage (legacy paths missing display info), yields the
    /// lowercase form.
    pub fn common_module_display_names(&self) -> impl Iterator<Item = &str> {
        self.common_modules.keys().map(|lower| {
            self.common_modules_display.get(lower).map(|s| s.as_str()).unwrap_or(lower)
        })
    }

    /// Iterate over all CommonModule names (lowercase, as stored). Prefer
    /// [`Self::common_module_display_names`] when rendering for the user.
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

impl ModulePathType {
    /// Map a path-extracted [`ModulePathType`] to a [`ManagerType`] for
    /// the [`ModuleIndex::managers`] keying scheme. `None` for
    /// `CommonModule` (it lives in its own table).
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

    /// Map a path-extracted [`ModulePathType`] to an [`MdoType`] for
    /// the [`ModuleIndex::object_modules`] keying scheme. `None` for
    /// path types that have no `MdoType` counterpart (`CommonModule`).
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

/// Which BSL module-file flavour a designer-format path identifies.
///
/// Drives the dispatch in [`ModuleIndex::build_from_paths`]:
/// `Common` files go to `common_modules`, `Manager` files go to
/// `managers`, `Object` files go to `object_modules`, `RecordSet`
/// files go to `record_set_modules`. Other module flavours
/// (`FormModule.bsl`, `CommandModule.bsl`) are filtered out at the
/// parse step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModuleFileKind {
    /// `CommonModules/<Name>/Ext/Module.bsl`
    Common,
    /// `<Folder>/<Name>/Ext/ManagerModule.bsl`
    Manager,
    /// `<Folder>/<Name>/Ext/ObjectModule.bsl` (Phase B)
    Object,
    /// `<RegisterFolder>/<Name>/Ext/RecordSetModule.bsl` (Phase C)
    RecordSet,
}

/// Parse module path to extract type, name, and file flavour.
///
/// Designer-format paths recognised:
/// - `CommonModules/<Name>/Ext/Module.bsl` → ([`CommonModule`], `Common`)
/// - `<Folder>/<Name>/Ext/ManagerModule.bsl` → (folder MDO, `Manager`)
/// - `<Folder>/<Name>/Ext/ObjectModule.bsl` → (folder MDO, `Object`)
/// - `<RegisterFolder>/<Name>/Ext/RecordSetModule.bsl` → (folder MDO, `RecordSet`)
///
/// Other module flavours (`FormModule.bsl`, `CommandModule.bsl`)
/// return `None` — they have no type-system call surface today.
///
/// [`CommonModule`]: ModulePathType::CommonModule
fn parse_module_path(path: &str) -> Option<(ModulePathType, String, ModuleFileKind)> {
    let parts: Vec<&str> = path.split('/').collect();

    // Need at least: <Type>/<Name>/Ext/<Module>.bsl
    if parts.len() < 4 {
        return None;
    }

    let path_lower = path.to_lowercase();

    // Find the nearest type folder to the module file.
    //
    // Absolute paths can contain ordinary directories named like metadata
    // plural folders, for example `/Users/.../Documents/git/.../Catalogs/...`.
    // The metadata folder is the one closest to `Ext/<module>.bsl`.
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
            // Next part should be the module/object name
            if i + 1 < parts.len() {
                let name = parts[i + 1].to_string();

                if mod_type == ModulePathType::CommonModule {
                    // CommonModules: accept Module.bsl only.
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
                // FormModule.bsl / CommandModule.bsl intentionally
                // fall through — no type-system call surface today.
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
        // ManagerModule.bsl is indexed for manager resolution
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

        // ObjectModule.bsl is now also indexed (Phase B).
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
        // ManagerModule.bsl is indexed
        let manager_path = "InformationRegisters/Test/Ext/ManagerModule.bsl";
        assert_eq!(
            parse_module_path(manager_path),
            Some((
                ModulePathType::InformationRegister,
                "Test".to_string(),
                ModuleFileKind::Manager,
            )),
        );

        // RecordSetModule.bsl is now indexed too (Phase C).
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
        // Phase B: ObjectModule.bsl paths are now indexed for catalog,
        // document, exchange-plan, chart-of-accounts, etc.
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
        // Build the index from a path tuple to verify both
        // `parse_module_path` and `resolve_object_module` agree on the
        // `(MdoType, name)` keying.
        let file_id = FileId::from_raw(99);
        let path = "Catalogs/Справочник1/Ext/ObjectModule.bsl";
        let index = ModuleIndex::build_from_paths([(file_id, path)].into_iter());
        assert_eq!(index.object_module_count(), 1);
        assert_eq!(
            index.resolve_object_module(MdoType::Catalog, &Name::new("Справочник1")),
            Some(file_id),
        );
        // Wrong MDO type misses (strict keying).
        assert_eq!(index.resolve_object_module(MdoType::Document, &Name::new("Справочник1")), None,);
    }

    #[test]
    fn test_parse_record_set_module_path() {
        // Phase C: register-flavour `RecordSetModule.bsl` paths are
        // now indexed. Pins both the path-type extraction and the
        // file-kind discrimination.
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
        // Manager / object indexes must NOT pick this up — pins the
        // file-kind dispatch.
        assert_eq!(index.manager_count(), 0);
        assert_eq!(index.object_module_count(), 0);
        assert_eq!(
            index.resolve_record_set_module(
                MdoType::InformationRegister,
                &Name::new("РегистрСведений1")
            ),
            Some(file_id),
        );
        // Wrong register flavour misses (strict keying).
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
        // FormModule.bsl / CommandModule.bsl have no type-system call
        // surface today — pin the negative case so a future
        // "index everything" drift is caught.
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
