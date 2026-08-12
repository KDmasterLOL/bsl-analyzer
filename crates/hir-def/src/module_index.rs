use bsl_metadata::MdoType;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use stdx::case::CaseExt;
use vfs::FileId;

use crate::body::{ExternalRef, ManagerType};
use crate::Name;

/// Approximate live heap bytes for Salsa's `memory_usage` report: each map's
/// table plus its owned `String` keys (and, for `common_modules_display`, its
/// owned `String` values too). New heap-owning fields must be added here too.
pub(crate) fn module_index_heap(v: &Arc<ModuleIndex>) -> usize {
    use crate::heap_estimate::map_table_bytes;

    let idx = &**v;
    let mut bytes = std::mem::size_of::<ModuleIndex>();

    bytes += map_table_bytes::<String, FileId>(idx.common_modules.len());
    bytes += idx.common_modules.keys().map(String::capacity).sum::<usize>();

    bytes += map_table_bytes::<String, String>(idx.common_modules_display.len());
    for (k, v) in &idx.common_modules_display {
        bytes += k.capacity() + v.capacity();
    }

    bytes += map_table_bytes::<(ManagerType, String), FileId>(idx.managers.len());
    bytes += idx.managers.keys().map(|(_, s)| s.capacity()).sum::<usize>();

    bytes += map_table_bytes::<(MdoType, String), FileId>(idx.object_modules.len());
    bytes += idx.object_modules.keys().map(|(_, s)| s.capacity()).sum::<usize>();

    bytes += map_table_bytes::<(MdoType, String), FileId>(idx.record_set_modules.len());
    bytes += idx.record_set_modules.keys().map(|(_, s)| s.capacity()).sum::<usize>();

    bytes += map_table_bytes::<(Option<(MdoType, String)>, String), FileId>(idx.forms.len());
    for (owner_key, form_name) in idx.forms.keys() {
        bytes += owner_key.as_ref().map_or(0, |(_, s)| s.capacity()) + form_name.capacity();
    }

    bytes
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleIndex {
    common_modules: FxHashMap<String, FileId>,

    common_modules_display: FxHashMap<String, String>,

    managers: FxHashMap<(ManagerType, String), FileId>,

    object_modules: FxHashMap<(MdoType, String), FileId>,

    record_set_modules: FxHashMap<(MdoType, String), FileId>,

    forms: FxHashMap<(Option<(MdoType, String)>, String), FileId>,
}

impl ModuleIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the path-derived index. The input is sorted by path here, and
    /// same-name collisions (a module adopted by a configuration extension
    /// shares its base module's name) are FIRST-wins — together that makes the
    /// winner independent of the caller's iteration order, and the ascending
    /// order lets the base-config file beat its extension sibling. This index
    /// is the visibility-blind fallback: consumers with a caller file resolve
    /// through the config-scoped
    /// `ConfigsDatabase::resolve_common_module_file_candidates` instead.
    pub fn build_from_paths<'a>(paths: impl Iterator<Item = (FileId, &'a str)>) -> Self {
        let mut paths: Vec<(FileId, &str)> = paths.collect();
        paths.sort_unstable_by_key(|&(_, path)| path);

        let mut index = Self::new();

        for (file_id, path) in paths {
            let normalized = path.replace('\\', "/");

            if let Some(form_key) = parse_form_module_path(&normalized) {
                let owner = form_key
                    .owner
                    .map(|(mdo_type, object)| (mdo_type, object.as_str().fold_lower()));
                index
                    .forms
                    .entry((owner, form_key.form_name.as_str().fold_lower()))
                    .or_insert(file_id);
                continue;
            }

            let Some((module_type, name, file_kind)) = parse_module_path(&normalized) else {
                continue;
            };
            let lower = name.fold_lower();

            match file_kind {
                ModuleFileKind::Common => {
                    index
                        .common_modules_display
                        .entry(lower.clone())
                        .or_insert_with(|| name.to_string());
                    index.common_modules.entry(lower).or_insert(file_id);
                }
                ModuleFileKind::Manager => {
                    if let Some(manager_type) = module_type.to_manager_type() {
                        index.managers.entry((manager_type, lower)).or_insert(file_id);
                    }
                }
                ModuleFileKind::Object => {
                    if let Some(mdo_type) = module_type.to_mdo_type() {
                        index.object_modules.entry((mdo_type, lower)).or_insert(file_id);
                    }
                }
                ModuleFileKind::RecordSet => {
                    if let Some(mdo_type) = module_type.to_mdo_type() {
                        index.record_set_modules.entry((mdo_type, lower)).or_insert(file_id);
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
        self.common_modules.get(&name.as_str().fold_lower()).copied()
    }

    /// Canonical (original-case) name of a common module, or `None` if no such module
    /// is known. Used to give `ОбщийМодуль("имя")` a `TypeKind::CommonModule` whose name
    /// is case-stable, so the same module never interns to two distinct types.
    pub fn canonical_common_module_name(&self, name: &Name) -> Option<&str> {
        self.common_modules_display.get(&name.as_str().fold_lower()).map(String::as_str)
    }

    pub fn resolve_manager(&self, manager_type: ManagerType, name: &Name) -> Option<FileId> {
        self.managers.get(&(manager_type, name.as_str().fold_lower())).copied()
    }

    pub fn resolve_object_module(&self, mdo_type: MdoType, name: &Name) -> Option<FileId> {
        self.object_modules.get(&(mdo_type, name.as_str().fold_lower())).copied()
    }

    pub fn resolve_record_set_module(&self, mdo_type: MdoType, name: &Name) -> Option<FileId> {
        self.record_set_modules.get(&(mdo_type, name.as_str().fold_lower())).copied()
    }

    pub fn resolve_form_module(
        &self,
        owner: Option<(MdoType, &Name)>,
        form_name: &Name,
    ) -> Option<FileId> {
        let owner_key = owner.map(|(mdo_type, name)| (mdo_type, name.as_str().fold_lower()));
        self.forms.get(&(owner_key, form_name.as_str().fold_lower())).copied()
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

    /// Resolve a path-derived [`ModuleKey`] back to its `FileId`, mirroring the
    /// per-kind `resolve_*` routes used during dependency resolution.
    pub fn resolve_module_key(&self, key: &ModuleKey) -> Option<FileId> {
        let name = Name::new(key.name());
        match key {
            ModuleKey::Common { .. } => self.resolve_common_module(&name),
            ModuleKey::Manager { mdo_type, .. } => {
                self.resolve_manager(ManagerType::from_mdo_type(*mdo_type)?, &name)
            }
            ModuleKey::Object { mdo_type, .. } => self.resolve_object_module(*mdo_type, &name),
            ModuleKey::RecordSet { mdo_type, .. } => {
                self.resolve_record_set_module(*mdo_type, &name)
            }
        }
    }
}

/// A durable, path-derived identity for an indexed BSL module. The role plus the
/// metadata-object type and object name fully determine which module file the
/// name belongs to, so it round-trips: [`module_key_for_path`] derives it from a
/// path and [`ModuleIndex::resolve_module_key`] resolves it back to a `FileId`
/// in the current revision. Common modules carry no metadata-object type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleKey {
    Common { name: String },
    Manager { mdo_type: MdoType, name: String },
    Object { mdo_type: MdoType, name: String },
    RecordSet { mdo_type: MdoType, name: String },
}

impl ModuleKey {
    pub fn name(&self) -> &str {
        match self {
            ModuleKey::Common { name }
            | ModuleKey::Manager { name, .. }
            | ModuleKey::Object { name, .. }
            | ModuleKey::RecordSet { name, .. } => name,
        }
    }

    /// The metadata-object type, or `None` for common modules.
    pub fn mdo_type(&self) -> Option<MdoType> {
        match self {
            ModuleKey::Common { .. } => None,
            ModuleKey::Manager { mdo_type, .. }
            | ModuleKey::Object { mdo_type, .. }
            | ModuleKey::RecordSet { mdo_type, .. } => Some(*mdo_type),
        }
    }
}

/// Derive a [`ModuleKey`] from a module file path. Returns `None` for files that
/// are not indexable user modules (forms, commands, non-module files).
pub fn module_key_for_path(path: &str) -> Option<ModuleKey> {
    let normalized = path.replace('\\', "/");
    let (module_type, name, file_kind) = parse_module_path(&normalized)?;
    Some(match file_kind {
        ModuleFileKind::Common => ModuleKey::Common { name },
        ModuleFileKind::Manager => {
            ModuleKey::Manager { mdo_type: module_type.to_mdo_type()?, name }
        }
        ModuleFileKind::Object => ModuleKey::Object { mdo_type: module_type.to_mdo_type()?, name },
        ModuleFileKind::RecordSet => {
            ModuleKey::RecordSet { mdo_type: module_type.to_mdo_type()?, name }
        }
    })
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

    fn from_mdo(mdo: MdoType) -> Option<Self> {
        Some(match mdo {
            MdoType::CommonModule => ModulePathType::CommonModule,
            MdoType::Document => ModulePathType::Document,
            MdoType::Catalog => ModulePathType::Catalog,
            MdoType::DataProcessor => ModulePathType::DataProcessor,
            MdoType::Report => ModulePathType::Report,
            MdoType::InformationRegister => ModulePathType::InformationRegister,
            MdoType::AccumulationRegister => ModulePathType::AccumulationRegister,
            MdoType::AccountingRegister => ModulePathType::AccountingRegister,
            MdoType::CalculationRegister => ModulePathType::CalculationRegister,
            MdoType::ChartOfCharacteristicTypes => ModulePathType::ChartOfCharacteristicTypes,
            MdoType::ChartOfAccounts => ModulePathType::ChartOfAccounts,
            MdoType::ChartOfCalculationTypes => ModulePathType::ChartOfCalculationTypes,
            MdoType::BusinessProcess => ModulePathType::BusinessProcess,
            MdoType::Task => ModulePathType::Task,
            MdoType::Enum => ModulePathType::Enum,
            MdoType::ExchangePlan => ModulePathType::ExchangePlan,
            MdoType::ExternalDataSource => ModulePathType::ExternalDataSource,
            MdoType::Constant => ModulePathType::Constant,
            _ => return None,
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

/// Which directory spellings name a collection is decided once, in
/// `bsl_metadata::module_path::collection_directory`: a dump written with `Ё`
/// names the same collection as one written with `Е`, and the metadata builder in
/// `ide-db` must agree with this index about that, or a module gets metadata here
/// and no index entry there.
fn module_path_type_from_segment(segment: &str) -> Option<ModulePathType> {
    ModulePathType::from_mdo(bsl_metadata::module_path::collection_directory(segment)?)
}

/// A durable, path-derived identity for a form module. A managed form module lives
/// at `…/Ext/Form/Module.bsl`; the owner is the metadata object whose `Forms/`
/// directory contains it, or `None` for a common form (`CommonForms/…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormKey {
    pub owner: Option<(MdoType, String)>,
    pub form_name: String,
}

/// Parse a form module path into its owner + form name, or `None` if the path is
/// not a managed form module. Mirrors the authoritative detection in
/// `ide-db/src/metadata.rs` (managed-form suffix `…/Ext/Form/Module.bsl`),
/// including its service-folder matching policy (`bsl_conventions`, ASCII-ci):
/// a path this parser accepts but `metadata.rs` would not loads no
/// `module_metadata.form`, so sharing one policy keeps the form pass from
/// claiming a form whose metadata never loads.
pub fn parse_form_module_path(path: &str) -> Option<FormKey> {
    use bsl_conventions::{conventional_of, ConventionalName as Conv};
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').collect();
    let n = parts.len();
    if n < 5
        || conventional_of(parts[n - 1]) != Some(Conv::Module)
        || conventional_of(parts[n - 2]) != Some(Conv::Form)
        || conventional_of(parts[n - 3]) != Some(Conv::Ext)
    {
        return None;
    }
    let form_name = parts[n - 4].to_string();
    let container = parts[n - 5];
    if container.eq_ignore_ascii_case("CommonForms") || container.fold_lower() == "общиеформы"
    {
        return Some(FormKey { owner: None, form_name });
    }
    if conventional_of(container) == Some(Conv::Forms) && n >= 7 {
        let object = parts[n - 6].to_string();
        let mdo_type = module_path_type_from_segment(parts[n - 7])?.to_mdo_type()?;
        return Some(FormKey { owner: Some((mdo_type, object)), form_name });
    }
    None
}

fn parse_module_path(path: &str) -> Option<(ModulePathType, String, ModuleFileKind)> {
    use bsl_conventions::ConventionalName as Conv;
    let parts: Vec<&str> = path.split('/').collect();
    let file_kind = parts.last().copied().and_then(bsl_conventions::conventional_of);

    // Structure comes from the shared specification; the spelling table stays
    // here, because this index accepts `Ё` variants the metadata builder does not.
    let split = bsl_metadata::module_path::split_module_path(path, |segment| {
        module_path_type_from_segment(segment).is_some()
    })?;
    let mod_type = module_path_type_from_segment(split.collection)?;
    let name = split.object_name.to_string();

    if mod_type == ModulePathType::CommonModule {
        if file_kind == Some(Conv::Module) {
            return Some((mod_type, name, ModuleFileKind::Common));
        }
    } else {
        match file_kind {
            Some(Conv::ManagerModule) => return Some((mod_type, name, ModuleFileKind::Manager)),
            Some(Conv::ObjectModule) => return Some((mod_type, name, ModuleFileKind::Object)),
            Some(Conv::RecordSetModule) => {
                return Some((mod_type, name, ModuleFileKind::RecordSet))
            }
            _ => {}
        }
    }

    None
}

/// Per-component match modes for a module path RELATIVE to a configuration
/// root, by the dump grammar: conventional positions fold, NAME positions
/// (object, form, command) stay exact — even an object named `Ext` keeps its
/// case. `None` means the layout is not a module path this grammar knows; the
/// caller then resolves exactly, the historical behaviour.
pub fn module_path_segment_modes(rel: &str) -> Option<Vec<bsl_conventions::SegmentMatch>> {
    use bsl_conventions::{conventional_of, ConventionalName as Conv, SegmentMatch as M};
    let normalized = rel.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').collect();
    let at = |i: usize, name: Conv| conventional_of(parts[i]) == Some(name);
    match parts.len() {
        2 if at(0, Conv::Ext) => Some(vec![M::Ci, M::Ci]),
        4 if at(2, Conv::Ext) => Some(vec![M::Ci, M::Exact, M::Ci, M::Ci]),
        5 if at(2, Conv::Ext) && at(3, Conv::Form) => {
            Some(vec![M::Ci, M::Exact, M::Ci, M::Ci, M::Ci])
        }
        6 if at(2, Conv::Commands) && at(4, Conv::Ext) => {
            Some(vec![M::Ci, M::Exact, M::Ci, M::Exact, M::Ci, M::Ci])
        }
        7 if at(2, Conv::Forms) && at(4, Conv::Ext) && at(5, Conv::Form) => {
            Some(vec![M::Ci, M::Exact, M::Ci, M::Exact, M::Ci, M::Ci, M::Ci])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_case_variant_form_module_path_is_still_a_form() {
        let key = super::parse_form_module_path("Catalogs/C/Forms/F/EXT/FORM/MODULE.BSL")
            .expect("регистровый близнец форменного пути распознаётся");
        assert_eq!(key.form_name, "F");
    }

    /// Позиция имени формы и объекта — имена, их регистр значим и сохраняется.
    #[test]
    fn form_and_object_names_keep_their_case() {
        let key = super::parse_form_module_path("Catalogs/ТОВАРЫ/Forms/ФОРМА/Ext/Form/Module.bsl")
            .expect("нижнерегистровые сервисные сегменты с именами в любом регистре");
        assert_eq!(key.form_name, "ФОРМА");
        assert_eq!(key.owner.as_ref().map(|(_, o)| o.as_str()), Some("ТОВАРЫ"));
    }

    /// Имя объекта может совпадать с именем коллекции. Тип берётся по позиции в
    /// жёсткой форме `<Коллекция>/<Имя>/Ext/<Модуль>.bsl`, иначе обход с конца
    /// принимает имя объекта за тип и индексирует модуль под именем `Ext`.
    #[test]
    fn object_named_like_a_collection_is_indexed_under_its_own_name() {
        for (path, expected_type, expected_name) in [
            ("/Documents/Constants/Ext/ManagerModule.bsl", ModulePathType::Document, "Constants"),
            ("/Documents/Documents/Ext/ManagerModule.bsl", ModulePathType::Document, "Documents"),
            (
                "/Catalogs/Перечисления/Ext/ObjectModule.bsl",
                ModulePathType::Catalog,
                "Перечисления",
            ),
            // Каталог-предок может называться как коллекция.
            ("/Documents/Catalogs/Товары/ManagerModule.bsl", ModulePathType::Catalog, "Товары"),
            (
                "/home/Documents/Catalogs/Товары/ManagerModule.bsl",
                ModulePathType::Catalog,
                "Товары",
            ),
            // Сегмент `Ext` не обязателен.
            (
                "/CommonModules/ПервыйОбщийМодуль/Module.bsl",
                ModulePathType::CommonModule,
                "ПервыйОбщийМодуль",
            ),
            ("/Catalogs/Constants/ManagerModule.bsl", ModulePathType::Catalog, "Constants"),
            // Написание каталога через `ё` называет ту же коллекцию, и решает это
            // одна таблица на оба слоя — иначе модуль попадает в индекс здесь и
            // остаётся без метаданных там (или наоборот).
            ("Отчёты/Продажи/Ext/ManagerModule.bsl", ModulePathType::Report, "Продажи"),
            ("Отчеты/Продажи/Ext/ManagerModule.bsl", ModulePathType::Report, "Продажи"),
            (
                "РегистрыРасчёта/Начисления/Ext/RecordSetModule.bsl",
                ModulePathType::CalculationRegister,
                "Начисления",
            ),
            // Кратчайшая форма: относительный путь без служебного уровня.
            ("Documents/ПКО/ManagerModule.bsl", ModulePathType::Document, "ПКО"),
            ("CommonModules/Общий/Module.bsl", ModulePathType::CommonModule, "Общий"),
            // Контроль: обычное имя работало и раньше.
            ("/Documents/ПКО/Ext/ManagerModule.bsl", ModulePathType::Document, "ПКО"),
        ] {
            let (mod_type, name, _kind) =
                super::parse_module_path(path).unwrap_or_else(|| panic!("{path} must parse"));
            assert_eq!(mod_type, expected_type, "{path}");
            assert_eq!(name, expected_name, "{path}");
        }
    }

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
    fn value_manager_module_does_not_replace_constant_manager_module() {
        let manager = FileId::from_raw(1);
        let value_manager = FileId::from_raw(2);
        let index = ModuleIndex::build_from_paths(
            [
                (manager, "Constants/ИспользоватьОсобыеУсловияТруда/Ext/ManagerModule.bsl"),
                (
                    value_manager,
                    "Constants/ИспользоватьОсобыеУсловияТруда/Ext/ValueManagerModule.bsl",
                ),
            ]
            .into_iter(),
        );

        assert_eq!(
            index.resolve_manager(
                ManagerType::Constants,
                &Name::new("ИспользоватьОсобыеУсловияТруда")
            ),
            Some(manager),
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
    fn parse_form_module_path_object_form() {
        let path = "Catalogs/Контрагенты/Forms/ФормаЭлемента/Ext/Form/Module.bsl";
        assert_eq!(
            parse_form_module_path(path),
            Some(FormKey {
                owner: Some((MdoType::Catalog, "Контрагенты".to_string())),
                form_name: "ФормаЭлемента".to_string(),
            }),
        );
    }

    #[test]
    fn parse_form_module_path_common_form() {
        let path = "CommonForms/НастройкиПрограммы/Ext/Form/Module.bsl";
        assert_eq!(
            parse_form_module_path(path),
            Some(FormKey {
                owner: None, form_name: "НастройкиПрограммы".to_string()
            }),
        );
        let localized = "ОбщиеФормы/НастройкиПрограммы/Ext/Form/Module.bsl";
        assert_eq!(
            parse_form_module_path(localized),
            Some(FormKey {
                owner: None, form_name: "НастройкиПрограммы".to_string()
            }),
        );
    }

    #[test]
    fn parse_form_module_path_with_absolute_prefix() {
        let path = "/Users/x/project/Documents/Заказ/Forms/ФормаДокумента/Ext/Form/Module.bsl";
        assert_eq!(
            parse_form_module_path(path),
            Some(FormKey {
                owner: Some((MdoType::Document, "Заказ".to_string())),
                form_name: "ФормаДокумента".to_string(),
            }),
        );
    }

    #[test]
    fn parse_form_module_path_rejects_non_form_modules() {
        // Manager/object/common modules are not forms.
        assert_eq!(parse_form_module_path("Catalogs/Х/Ext/ManagerModule.bsl"), None);
        assert_eq!(parse_form_module_path("CommonModules/Х/Ext/Module.bsl"), None);
        // The legacy ordinary-form `FormModule.bsl` shape (no `Ext/Form/Module.bsl`)
        // does not load form metadata, so it is not a graph form module either.
        assert_eq!(parse_form_module_path("Documents/Х/Forms/Ф/Ext/FormModule.bsl"), None,);
    }

    #[test]
    fn resolve_form_module_returns_object_form_file_id_when_path_is_managed_form_module() {
        // Given: a real designer-style managed form module path under an owning document.
        let file_id = FileId::from_raw(77);
        let index = ModuleIndex::build_from_paths(
            [(file_id, "Documents/Документ1/Forms/ФормаДокумента/Ext/Form/Module.bsl")].into_iter(),
        );

        // When: resolving the same owner/form through the path-derived index.
        let resolved = index.resolve_form_module(
            Some((MdoType::Document, &Name::new("документ1"))),
            &Name::new("формаДокумента"),
        );

        // Then: object and form names match case-insensitively and return the form module FileId.
        assert_eq!(resolved, Some(file_id));
        assert_eq!(
            index.resolve_form_module(
                Some((MdoType::Catalog, &Name::new("Документ1"))),
                &Name::new("ФормаДокумента"),
            ),
            None,
        );
    }

    #[test]
    fn resolve_form_module_returns_common_form_file_id_when_owner_is_absent() {
        // Given: a common managed form module path.
        let file_id = FileId::from_raw(78);
        let index = ModuleIndex::build_from_paths(
            [(file_id, "CommonForms/ТестоваяФорма/Ext/Form/Module.bsl")].into_iter(),
        );

        // When: resolving by common-form name.
        let resolved = index.resolve_form_module(None, &Name::new("тестоваяформа"));

        // Then: the common form resolves and object-owned lookup does not alias it.
        assert_eq!(resolved, Some(file_id));
        assert_eq!(
            index.resolve_form_module(
                Some((MdoType::Document, &Name::new("ТестоваяФорма"))),
                &Name::new("ТестоваяФорма"),
            ),
            None,
        );
    }

    #[test]
    fn adopted_extension_module_never_shadows_base_regardless_of_input_order() {
        let base = FileId::from_raw(1);
        let ext = FileId::from_raw(2);
        let base_path = "src/cf/CommonModules/Модуль/Ext/Module.bsl";
        let ext_path = "src/cfe/X/CommonModules/Модуль/Ext/Module.bsl";

        for pair in [[(base, base_path), (ext, ext_path)], [(ext, ext_path), (base, base_path)]] {
            let index = ModuleIndex::build_from_paths(pair.into_iter());
            assert_eq!(
                index.resolve_common_module(&Name::new("Модуль")),
                Some(base),
                "the base-config file must win the name collision in any input order",
            );
        }
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
