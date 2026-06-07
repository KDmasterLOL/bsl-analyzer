use crate::configuration::Configuration;
use crate::error::Result;
use crate::metadata_object::{MdoType, MetadataObject};
use crate::traits::MdObject;
use crate::xml_parser;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

pub fn load_from_directory(path: impl AsRef<Path>) -> Result<Configuration> {
    let path = path.as_ref();
    let _span = tracing::info_span!("load_from_directory", ?path).entered();

    let loaded = load_all_metadata_parallel(path);
    let config = build_configuration(loaded);

    tracing::info!(
        common_modules = config.common_modules().len(),
        metadata_objects = config.metadata_objects().len(),
        registers = config.registers().len(),
        event_subscriptions = config.event_subscriptions().len(),
        scheduled_jobs = config.scheduled_jobs().len(),
        roles = config.roles().len(),
        defined_types = config.defined_types().len(),
        http_services = config.http_services().len(),
        web_services = config.web_services().len(),
        "configuration loaded"
    );

    Ok(config)
}

struct LoadedMetadata {
    common_modules: Vec<crate::common_module::CommonModule>,
    catalogs: Vec<MetadataObject>,
    documents: Vec<MetadataObject>,
    info_registers: Vec<crate::register::Register>,
    accum_registers: Vec<crate::register::Register>,
    account_registers: Vec<crate::register::Register>,
    calc_registers: Vec<crate::register::Register>,
    event_subscriptions: Vec<crate::event_subscription::EventSubscription>,
    scheduled_jobs: Vec<crate::scheduled_job::ScheduledJob>,
    roles: Vec<crate::role::Role>,
    defined_types: Vec<crate::defined_type::DefinedType>,
    charts_char_types: Vec<MetadataObject>,
    constants: Vec<MetadataObject>,
    exchange_plans: Vec<MetadataObject>,
    business_processes: Vec<MetadataObject>,
    enums: Vec<MetadataObject>,
    tasks: Vec<MetadataObject>,
    charts_accounts: Vec<MetadataObject>,
    charts_calc_types: Vec<MetadataObject>,
    external_data_sources: Vec<MetadataObject>,
    http_services: Vec<crate::http_service::HTTPService>,
    web_services: Vec<crate::web_service::WebService>,
    data_processors: Vec<MetadataObject>,
    reports: Vec<MetadataObject>,
    subsystems: Vec<crate::subsystem::Subsystem>,
}

fn load_all_metadata_parallel(path: &Path) -> LoadedMetadata {
    let start = std::time::Instant::now();
    let common_modules = Mutex::new(Vec::new());
    let catalogs = Mutex::new(Vec::new());
    let documents = Mutex::new(Vec::new());
    let info_registers = Mutex::new(Vec::new());
    let accum_registers = Mutex::new(Vec::new());
    let account_registers = Mutex::new(Vec::new());
    let calc_registers = Mutex::new(Vec::new());
    let event_subscriptions = Mutex::new(Vec::new());
    let scheduled_jobs = Mutex::new(Vec::new());
    let roles = Mutex::new(Vec::new());
    let defined_types = Mutex::new(Vec::new());
    let charts_char_types = Mutex::new(Vec::new());
    let constants = Mutex::new(Vec::new());
    let exchange_plans = Mutex::new(Vec::new());
    let business_processes = Mutex::new(Vec::new());
    let enums = Mutex::new(Vec::new());
    let tasks = Mutex::new(Vec::new());
    let charts_accounts = Mutex::new(Vec::new());
    let charts_calc_types = Mutex::new(Vec::new());
    let external_data_sources = Mutex::new(Vec::new());
    let http_services = Mutex::new(Vec::new());
    let web_services = Mutex::new(Vec::new());
    let data_processors = Mutex::new(Vec::new());
    let reports = Mutex::new(Vec::new());
    let subsystems = Mutex::new(Vec::new());

    rayon::scope(|s| {
        s.spawn(|_| {
            *common_modules.lock().unwrap() =
                load_common_modules_parallel(&path.join("CommonModules"))
        });
        s.spawn(|_| *catalogs.lock().unwrap() = load_catalogs_parallel(&path.join("Catalogs")));
        s.spawn(|_| *documents.lock().unwrap() = load_documents_parallel(&path.join("Documents")));
        s.spawn(|_| {
            *info_registers.lock().unwrap() =
                load_information_registers_parallel(&path.join("InformationRegisters"))
        });
        s.spawn(|_| {
            *accum_registers.lock().unwrap() =
                load_accumulation_registers_parallel(&path.join("AccumulationRegisters"))
        });
        s.spawn(|_| {
            *account_registers.lock().unwrap() =
                load_accounting_registers_parallel(&path.join("AccountingRegisters"))
        });
        s.spawn(|_| {
            *calc_registers.lock().unwrap() =
                load_calculation_registers_parallel(&path.join("CalculationRegisters"))
        });
        s.spawn(|_| {
            *event_subscriptions.lock().unwrap() =
                load_event_subscriptions_parallel(&path.join("EventSubscriptions"))
        });
        s.spawn(|_| {
            *scheduled_jobs.lock().unwrap() =
                load_scheduled_jobs_parallel(&path.join("ScheduledJobs"))
        });
        s.spawn(|_| *roles.lock().unwrap() = load_roles_parallel(&path.join("Roles")));
        s.spawn(|_| {
            *defined_types.lock().unwrap() = load_defined_types_parallel(&path.join("DefinedTypes"))
        });
        s.spawn(|_| {
            *charts_char_types.lock().unwrap() = load_charts_of_characteristic_types_parallel(
                &path.join("ChartsOfCharacteristicTypes"),
            )
        });
        s.spawn(|_| *constants.lock().unwrap() = load_constants_parallel(&path.join("Constants")));
        s.spawn(|_| {
            *exchange_plans.lock().unwrap() =
                load_exchange_plans_parallel(&path.join("ExchangePlans"))
        });
        s.spawn(|_| {
            *business_processes.lock().unwrap() =
                load_business_processes_parallel(&path.join("BusinessProcesses"))
        });
        s.spawn(|_| *enums.lock().unwrap() = load_enums_parallel(&path.join("Enums")));
        s.spawn(|_| *tasks.lock().unwrap() = load_tasks_parallel(&path.join("Tasks")));
        s.spawn(|_| {
            *charts_accounts.lock().unwrap() =
                load_charts_of_accounts_parallel(&path.join("ChartsOfAccounts"))
        });
        s.spawn(|_| {
            *charts_calc_types.lock().unwrap() = load_simple_metadata_objects_parallel(
                &path.join("ChartsOfCalculationTypes"),
                MdoType::ChartOfCalculationTypes,
            )
        });
        s.spawn(|_| {
            *external_data_sources.lock().unwrap() = load_simple_metadata_objects_parallel(
                &path.join("ExternalDataSources"),
                MdoType::ExternalDataSource,
            )
        });
        s.spawn(|_| {
            *http_services.lock().unwrap() = load_http_services_parallel(&path.join("HTTPServices"))
        });
        s.spawn(|_| {
            *web_services.lock().unwrap() = load_web_services_parallel(&path.join("WebServices"))
        });
        s.spawn(|_| {
            *data_processors.lock().unwrap() =
                load_data_processors_parallel(&path.join("DataProcessors"))
        });
        s.spawn(|_| *reports.lock().unwrap() = load_reports_parallel(&path.join("Reports")));
        s.spawn(|_| *subsystems.lock().unwrap() = load_subsystems(&path.join("Subsystems")));
    });

    let result = LoadedMetadata {
        common_modules: common_modules.into_inner().unwrap(),
        catalogs: catalogs.into_inner().unwrap(),
        documents: documents.into_inner().unwrap(),
        info_registers: info_registers.into_inner().unwrap(),
        accum_registers: accum_registers.into_inner().unwrap(),
        account_registers: account_registers.into_inner().unwrap(),
        calc_registers: calc_registers.into_inner().unwrap(),
        event_subscriptions: event_subscriptions.into_inner().unwrap(),
        scheduled_jobs: scheduled_jobs.into_inner().unwrap(),
        roles: roles.into_inner().unwrap(),
        defined_types: defined_types.into_inner().unwrap(),
        charts_char_types: charts_char_types.into_inner().unwrap(),
        constants: constants.into_inner().unwrap(),
        exchange_plans: exchange_plans.into_inner().unwrap(),
        business_processes: business_processes.into_inner().unwrap(),
        enums: enums.into_inner().unwrap(),
        tasks: tasks.into_inner().unwrap(),
        charts_accounts: charts_accounts.into_inner().unwrap(),
        charts_calc_types: charts_calc_types.into_inner().unwrap(),
        external_data_sources: external_data_sources.into_inner().unwrap(),
        http_services: http_services.into_inner().unwrap(),
        web_services: web_services.into_inner().unwrap(),
        data_processors: data_processors.into_inner().unwrap(),
        reports: reports.into_inner().unwrap(),
        subsystems: subsystems.into_inner().unwrap(),
    };

    tracing::info!(
        path = %path.display(),
        elapsed_ms = start.elapsed().as_millis() as u64,
        common_modules = result.common_modules.len(),
        catalogs = result.catalogs.len(),
        documents = result.documents.len(),
        data_processors = result.data_processors.len(),
        reports = result.reports.len(),
        "load_all_metadata_parallel complete",
    );

    result
}

fn build_configuration(loaded: LoadedMetadata) -> Configuration {
    let mut config = Configuration::new("Configuration");

    for module in loaded.common_modules {
        config.add_common_module(module);
    }
    for obj in loaded.catalogs {
        config.add_metadata_object(obj);
    }
    for obj in loaded.documents {
        config.add_metadata_object(obj);
    }
    for reg in loaded.info_registers {
        config.add_register(reg);
    }
    for reg in loaded.accum_registers {
        config.add_register(reg);
    }
    for reg in loaded.account_registers {
        config.add_register(reg);
    }
    for reg in loaded.calc_registers {
        config.add_register(reg);
    }
    for sub in loaded.event_subscriptions {
        config.add_event_subscription(sub);
    }
    for job in loaded.scheduled_jobs {
        config.add_scheduled_job(job);
    }
    for role in loaded.roles {
        config.add_role(role);
    }
    for dt in loaded.defined_types {
        config.add_defined_type(dt);
    }
    for obj in loaded.charts_char_types {
        config.add_metadata_object(obj);
    }
    for obj in loaded.constants {
        config.add_metadata_object(obj);
    }
    for obj in loaded.exchange_plans {
        config.add_metadata_object(obj);
    }
    for obj in loaded.business_processes {
        config.add_metadata_object(obj);
    }
    for obj in loaded.enums {
        config.add_metadata_object(obj);
    }
    for obj in loaded.tasks {
        config.add_metadata_object(obj);
    }
    for obj in loaded.charts_accounts {
        config.add_metadata_object(obj);
    }
    for obj in loaded.charts_calc_types {
        config.add_metadata_object(obj);
    }
    for obj in loaded.external_data_sources {
        config.add_metadata_object(obj);
    }
    for obj in loaded.data_processors {
        config.add_metadata_object(obj);
    }
    for obj in loaded.reports {
        config.add_metadata_object(obj);
    }
    for svc in loaded.http_services {
        config.add_http_service(svc);
    }
    for svc in loaded.web_services {
        config.add_web_service(svc);
    }
    for subsystem in loaded.subsystems {
        config.add_subsystem(subsystem);
    }

    config
}

/// Load every subsystem `.xml` under `Subsystems/`, recursing into nested
/// `<Name>/Subsystems/` directories. Each file directly inside a `Subsystems` directory is
/// one subsystem; the parent/child relationship is carried in each subsystem's
/// `child_subsystems`, not inferred from the directory layout.
fn load_subsystems(dir: &Path) -> Vec<crate::subsystem::Subsystem> {
    let mut out = Vec::new();
    collect_subsystems(dir, &mut out);
    out
}

fn collect_subsystems(dir: &Path, out: &mut Vec<crate::subsystem::Subsystem>) {
    if !dir.exists() {
        return;
    }
    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return,
    };
    for entry in entries {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("xml") {
            if let Ok(xml) = fs::read_to_string(&path) {
                if let Ok(subsystem) = xml_parser::parse_subsystem_xml(&xml) {
                    out.push(subsystem);
                }
            }
        } else if path.is_dir() {
            // Nested subsystems live under `<Name>/Subsystems/`.
            collect_subsystems(&path.join("Subsystems"), out);
        }
    }
}

fn load_common_modules_parallel(dir: &Path) -> Vec<crate::common_module::CommonModule> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let module_dir = entry.path();
            if !module_dir.is_dir() {
                return None;
            }

            let name = module_dir.file_name()?.to_str()?;
            let xml_path = dir.join(format!("{}.xml", name));
            let module_bsl_path = module_dir.join("Ext/Module.bsl");
            let module_bin_path = module_dir.join("Ext/Module.bin");

            if !xml_path.exists() {
                return None;
            }

            let xml = fs::read_to_string(&xml_path).ok()?;
            let mut module = xml_parser::parse_common_module_xml(&xml).ok()?;

            let is_protected = module_bin_path.exists() && !module_bsl_path.exists();

            if module_bsl_path.exists() {
                let uri = format!("CommonModules/{}/Ext/Module.bsl", name);
                module = crate::common_module::CommonModule::builder()
                    .uuid(*module.uuid())
                    .name(module.name())
                    .uri(Some(uri))
                    .server(module.is_server())
                    .global(module.is_global())
                    .client_managed_application(module.is_client_managed_application())
                    .client_ordinary_application(module.is_client_ordinary_application())
                    .external_connection(module.is_external_connection())
                    .server_call(module.is_server_call())
                    .privileged(module.is_privileged())
                    .return_values_reuse(module.return_values_reuse())
                    .protected(false)
                    .build();
            } else if is_protected {
                module = crate::common_module::CommonModule::builder()
                    .uuid(*module.uuid())
                    .name(module.name())
                    .uri(None::<String>)
                    .server(module.is_server())
                    .global(module.is_global())
                    .client_managed_application(module.is_client_managed_application())
                    .client_ordinary_application(module.is_client_ordinary_application())
                    .external_connection(module.is_external_connection())
                    .server_call(module.is_server_call())
                    .privileged(module.is_privileged())
                    .return_values_reuse(module.return_values_reuse())
                    .protected(true)
                    .build();
            }

            Some(module)
        })
        .collect()
}

fn load_catalogs_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_catalog_xml)
}

fn load_documents_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_document_xml)
}

fn load_business_processes_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_business_process_xml)
}

fn load_tasks_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_task_xml)
}

fn load_exchange_plans_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_exchange_plan_xml)
}

fn load_charts_of_characteristic_types_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_chart_of_characteristic_types_xml)
}

fn load_charts_of_accounts_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_chart_of_accounts_xml)
}

fn load_data_processors_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_data_processor_xml)
}

fn load_reports_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_report_xml)
}

fn load_metadata_objects_parallel<F>(dir: &Path, parser: F) -> Vec<MetadataObject>
where
    F: Fn(&str) -> Result<MetadataObject> + Sync,
{
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    let dir_names: std::collections::HashSet<String> = entries
        .iter()
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                path.file_name()?.to_str().map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();

            if path.is_dir() {
                let name = path.file_name()?.to_str()?;
                let xml_path = dir.join(format!("{}.xml", name));

                if !xml_path.exists() {
                    return None;
                }

                let main_xml = fs::read_to_string(&xml_path).ok()?;
                let predefined_path = path.join("Ext").join("Predefined.xml");
                let predefined_xml = predefined_path
                    .exists()
                    .then(|| fs::read_to_string(&predefined_path).ok())
                    .flatten();

                build_metadata_object(&main_xml, predefined_xml.as_deref(), &parser)
            } else if path.extension().and_then(|e| e.to_str()) == Some("xml") {
                let file_stem = path.file_stem()?.to_str()?;

                if dir_names.contains(file_stem) {
                    return None;
                }

                let xml = fs::read_to_string(&path).ok()?;
                build_metadata_object(&xml, None, &parser)
            } else {
                None
            }
        })
        .collect()
}

/// Build one metadata object from its already-read composing XML texts: the main
/// `<name>.xml` plus an optional `Ext/Predefined.xml`. Pure (no filesystem access)
/// so it can back a per-MDO Salsa parse query whose reads go through the versioned
/// VFS, while the directory loaders above supply the texts from disk.
fn build_metadata_object<F>(
    main_xml: &str,
    predefined_xml: Option<&str>,
    parser: &F,
) -> Option<MetadataObject>
where
    F: Fn(&str) -> Result<MetadataObject> + ?Sized,
{
    let mut mdo = parser(main_xml).ok()?;

    if let Some(predefined_xml) = predefined_xml {
        mdo.predefined_items = xml_parser::parse_predefined_xml(predefined_xml);
        tracing::debug!(
            name = %mdo.name,
            count = mdo.predefined_items.len(),
            "Loaded predefined items"
        );
    }

    Some(mdo)
}

fn load_enums_parallel(dir: &Path) -> Vec<MetadataObject> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            xml_parser::parse_enum_xml(&xml).ok()
        })
        .collect()
}

fn load_constants_parallel(dir: &Path) -> Vec<MetadataObject> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            xml_parser::parse_constant_xml(&xml).ok()
        })
        .collect()
}

fn load_registers_parallel<F>(dir: &Path, parser: F) -> Vec<crate::register::Register>
where
    F: Fn(&str) -> Result<crate::register::Register> + Sync,
{
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            parser(&xml).ok()
        })
        .collect()
}

fn load_information_registers_parallel(dir: &Path) -> Vec<crate::register::Register> {
    load_registers_parallel(dir, xml_parser::parse_information_register_xml)
}

fn load_accumulation_registers_parallel(dir: &Path) -> Vec<crate::register::Register> {
    load_registers_parallel(dir, xml_parser::parse_accumulation_register_xml)
}

fn load_accounting_registers_parallel(dir: &Path) -> Vec<crate::register::Register> {
    load_registers_parallel(dir, xml_parser::parse_accounting_register_xml)
}

fn load_calculation_registers_parallel(dir: &Path) -> Vec<crate::register::Register> {
    load_registers_parallel(dir, xml_parser::parse_calculation_register_xml)
}

fn load_event_subscriptions_parallel(
    dir: &Path,
) -> Vec<crate::event_subscription::EventSubscription> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            xml_parser::parse_event_subscription_xml(&xml).ok()
        })
        .collect()
}

fn load_scheduled_jobs_parallel(dir: &Path) -> Vec<crate::scheduled_job::ScheduledJob> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            xml_parser::parse_scheduled_job_xml(&xml).ok()
        })
        .collect()
}

fn load_roles_parallel(dir: &Path) -> Vec<crate::role::Role> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let name = path.file_stem()?.to_str()?;
            let xml = fs::read_to_string(&path).ok()?;
            let mut role = xml_parser::parse_role_xml(&xml).ok()?;

            let rights_path = dir.join(name).join("Ext").join("Rights.xml");
            if rights_path.exists() {
                if let Ok(rights_xml) = fs::read_to_string(&rights_path) {
                    if let Ok(rights_data) = xml_parser::parse_rights_xml(&rights_xml) {
                        role = crate::role::Role::with_data(
                            *role.uuid(),
                            role.name().to_string(),
                            rights_data,
                        );
                    }
                }
            }

            Some(role)
        })
        .collect()
}

fn load_defined_types_parallel(dir: &Path) -> Vec<crate::defined_type::DefinedType> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            xml_parser::parse_defined_type_xml(&xml).ok()
        })
        .collect()
}

fn load_http_services_parallel(dir: &Path) -> Vec<crate::http_service::HTTPService> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let service_dir = entry.path();
            if !service_dir.is_dir() {
                return None;
            }

            let name = service_dir.file_name()?.to_str()?;
            let xml_path = dir.join(format!("{}.xml", name));

            if !xml_path.exists() {
                return None;
            }

            let xml = fs::read_to_string(&xml_path).ok()?;
            xml_parser::parse_http_service_xml(&xml, name).ok()
        })
        .collect()
}

fn load_web_services_parallel(dir: &Path) -> Vec<crate::web_service::WebService> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let service_dir = entry.path();
            if !service_dir.is_dir() {
                return None;
            }

            let name = service_dir.file_name()?.to_str()?;
            let xml_path = dir.join(format!("{}.xml", name));

            if !xml_path.exists() {
                return None;
            }

            let xml = fs::read_to_string(&xml_path).ok()?;
            xml_parser::parse_web_service_xml(&xml, name).ok()
        })
        .collect()
}

fn load_simple_metadata_objects_parallel(dir: &Path, mdo_type: MdoType) -> Vec<MetadataObject> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let obj_dir = entry.path();
            if !obj_dir.is_dir() {
                return None;
            }

            let name = obj_dir.file_name()?.to_str()?;
            let xml_path = dir.join(format!("{}.xml", name));

            if !xml_path.exists() {
                return None;
            }

            Some(MetadataObject::new(mdo_type, name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Module;

    #[test]
    fn test_load_from_directory() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/designer");
        let config = load_from_directory(path).unwrap();

        assert!(!config.common_modules().is_empty(), "No common modules loaded");
        assert_eq!(config.common_modules().len(), 4, "Expected 4 common modules");

        let global_server = config.find_common_module("ГлобальныйСерверныйМодуль");
        assert!(global_server.is_some(), "ГлобальныйСерверныйМодуль not found");
        let module = global_server.unwrap();
        assert!(module.is_server(), "Should be server module");
        assert!(module.is_global(), "Should be global module");
        assert!(module.uri().is_some(), "Should have URI");
        assert_eq!(module.uri().unwrap(), "CommonModules/ГлобальныйСерверныйМодуль/Ext/Module.bsl");

        assert!(!config.registers().is_empty(), "No registers loaded");

        let register = config.find_register("РегистрСведений1");
        if let Some(reg) = register {
            assert!(reg.is_information_register(), "Should be InformationRegister");
            assert_eq!(reg.dimensions().len(), 1, "Should have 1 dimension");
            assert_eq!(reg.dimensions()[0].name(), "Справочник1", "Dimension name should match");
            assert!(
                !reg.dimensions()[0].is_deny_incomplete_values(),
                "DenyIncompleteValues should be false"
            );
        }

        assert!(!config.metadata_objects().is_empty(), "No metadata objects loaded");

        let catalog = config.metadata_objects().iter().find(|obj| {
            obj.mdo_type == crate::metadata_object::MdoType::Catalog && obj.name == "Справочник1"
        });

        if let Some(cat) = catalog {
            assert!(
                cat.attributes.len() >= 3,
                "Expected at least 3 custom attributes in Справочник1"
            );

            assert!(cat.find_attribute("Реквизит1").is_some(), "Expected Реквизит1");
            assert!(cat.find_attribute("Реквизит2").is_some(), "Expected Реквизит2");
            assert!(cat.find_attribute("Реквизит3").is_some(), "Expected Реквизит3");

            let attr1 = cat.find_attribute("Реквизит1").unwrap();
            assert!(
                matches!(attr1.attr_type, crate::metadata_object::AttributeType::String { .. }),
                "Реквизит1 should be String type"
            );

            assert_eq!(cat.tabular_sections.len(), 1, "Expected 1 tabular section");
            let ts = &cat.tabular_sections[0];
            assert_eq!(ts.name(), "ТабличнаяЧасть1");
        }
    }

    #[test]
    fn loads_data_processors_with_attributes() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/designer");
        let config = load_from_directory(path).unwrap();

        let mdo = config
            .find_metadata_object(
                crate::metadata_object::MdoType::DataProcessor,
                "ТестоваяОбработка",
            )
            .expect("ТестоваяОбработка not loaded");
        assert_eq!(
            mdo.attributes.len(),
            2,
            "expected 2 user attributes (no standard for DataProcessor)"
        );
        assert!(mdo.find_attribute("АдресСайта").is_some());
        assert!(mdo.find_attribute("СоздаватьГруппы").is_some());
    }

    #[test]
    fn loads_reports_with_attributes() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/designer");
        let config = load_from_directory(path).unwrap();

        let mdo = config
            .find_metadata_object(crate::metadata_object::MdoType::Report, "ТестовыйОтчёт")
            .expect("ТестовыйОтчёт not loaded");
        assert_eq!(mdo.attributes.len(), 1);
        assert!(mdo.find_attribute("ПериодОтчёта").is_some());
    }

    #[test]
    fn test_load_protected_module() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/designer");
        let config = load_from_directory(path).unwrap();

        let protected_module = config.find_common_module("ЗащищенныйМодуль");
        assert!(protected_module.is_some(), "ЗащищенныйМодуль not found");

        let module = protected_module.unwrap();
        assert!(module.is_protected(), "Module should be protected");
        assert!(module.uri().is_none(), "Protected module should not have URI");
        assert!(module.is_server(), "Should be server module");
        assert!(module.is_server_call(), "Should have server call");
    }

    #[test]
    #[ignore]
    fn test_load_enum_values_from_doc3() {
        let doc3_path = concat!(env!("HOME"), "/src/doc3/src/cf");

        if !std::path::Path::new(doc3_path).exists() {
            eprintln!("Skipping test: doc3 project not found at {}", doc3_path);
            return;
        }

        let config = load_from_directory(doc3_path).expect("Failed to load doc3 configuration");

        let enums: Vec<_> = config
            .metadata_objects()
            .iter()
            .filter(|obj| obj.mdo_type == crate::metadata_object::MdoType::Enum)
            .collect();

        println!("Total Enums loaded: {}", enums.len());
        assert!(!enums.is_empty(), "No enums loaded");

        println!("\nFirst 10 Enums:");
        for (i, enum_obj) in enums.iter().take(10).enumerate() {
            println!("  {}: {} (values: {})", i + 1, enum_obj.name, enum_obj.enum_values.len());
        }

        let target_name = "СпособыУстановкиКурсаВалюты";
        let target_enum_specific = enums.iter().find(|e| e.name == target_name);

        if let Some(enum_obj) = target_enum_specific {
            println!("\n✅ Found target enum: {}", target_name);
            println!("  EnumValues count: {}", enum_obj.enum_values.len());
            for (i, ev) in enum_obj.enum_values.iter().enumerate() {
                println!("    {}: {}", i + 1, ev.name);
            }
        } else {
            println!("\n❌ Target enum '{}' NOT FOUND", target_name);
            println!("\nAll enum names:");
            for (i, e) in enums.iter().enumerate() {
                println!("  {}: {}", i + 1, e.name);
            }
        }

        let target_enum = enums.iter().find(|e| !e.enum_values.is_empty());

        if let Some(enum_obj) = target_enum {
            println!("✅ Found enum: {}", enum_obj.name);
            println!("  EnumValues count: {}", enum_obj.enum_values.len());

            assert!(!enum_obj.enum_values.is_empty(), "EnumValues should not be empty");

            println!("  First 5 EnumValues:");
            for (i, ev) in enum_obj.enum_values.iter().take(5).enumerate() {
                println!("    {}: {} (uuid: {})", i + 1, ev.name, ev.uuid);
            }

            if let Some(first_value) = enum_obj.enum_values.first() {
                let found = enum_obj.find_enum_value(&first_value.name);
                assert!(found.is_some(), "find_enum_value should work");

                let found_lower = enum_obj.find_enum_value(&first_value.name.to_lowercase());
                assert!(found_lower.is_some(), "find_enum_value should be case-insensitive");
            }
        } else {
            panic!("❌ Enum 'ЗаданияОчередиОбновленияПрав' not found");
        }
    }

    #[test]
    #[ignore]
    fn test_niagara_field_resolution() {
        let path = concat!(env!("HOME"), "/src/niagara_ut/src/cf");
        if !std::path::Path::new(path).exists() {
            return;
        }
        let config = load_from_directory(path).unwrap();

        let kl = config
            .metadata_objects()
            .iter()
            .find(|o| o.name == "КартыЛояльности" && o.mdo_type == MdoType::Catalog);
        if let Some(kl) = kl {
            println!("КартыЛояльности: {} attrs", kl.attributes.len());
            for a in &kl.attributes {
                println!("  {}", a.name);
            }
            assert!(kl.attributes.iter().any(|a| a.name == "Партнер"), "Партнер not found");
            assert!(kl.attributes.iter().any(|a| a.name == "Статус"), "Статус not found");
        } else {
            println!("КартыЛояльности not found in metadata");
        }

        let reg =
            config.registers().iter().find(|r| r.name() == "СостояниеАдресовЭлектроннойПочты");
        if let Some(reg) = reg {
            println!(
                "Регистр: dims={}, res={}, attrs={}",
                reg.dimensions().len(),
                reg.resources().len(),
                reg.attributes().len()
            );
            for d in reg.dimensions() {
                println!("  dim: {}", d.name());
            }
            for r in reg.resources() {
                println!("  res: {}", r.name());
            }
            for a in reg.attributes() {
                println!("  attr: {}", a.name());
            }
        } else {
            println!("Register not found");
        }

        let p = config
            .metadata_objects()
            .iter()
            .find(|o| o.name == "Партнеры" && o.mdo_type == MdoType::Catalog);
        if let Some(p) = p {
            for ts in &p.tabular_sections {
                if ts.name() == "КонтактнаяИнформация" {
                    println!("ТЧ КонтактнаяИнформация: {} attrs", ts.attributes().len());
                    for a in ts.attributes() {
                        println!("  {}", a.name());
                    }
                }
            }
        } else {
            println!("Партнеры not found");
        }
    }

    #[test]
    #[ignore]
    fn test_load_from_doc3_project() {
        let doc3_path = concat!(env!("HOME"), "/src/doc3/src/cf");

        if !std::path::Path::new(doc3_path).exists() {
            eprintln!("Skipping test: doc3 project not found at {}", doc3_path);
            return;
        }

        let config = load_from_directory(doc3_path).expect("Failed to load doc3 configuration");

        println!("Total registers loaded: {}", config.registers().len());

        let info_registers: Vec<_> =
            config.registers().iter().filter(|r| r.is_information_register()).collect();

        println!("InformationRegisters count: {}", info_registers.len());

        println!("\nFirst 20 InformationRegisters:");
        for (i, reg) in info_registers.iter().take(20).enumerate() {
            println!("  {}: {}", i + 1, reg.name());
        }

        println!("\nRegisters containing 'Значения':");
        for reg in info_registers.iter() {
            if reg.name().contains("Значения") {
                println!("  - {}", reg.name());
            }
        }

        println!("\nRegisters containing 'Действий':");
        for reg in info_registers.iter() {
            if reg.name().contains("Действий") {
                println!("  - {}", reg.name());
            }
        }

        println!("\nRegisters containing 'Писем':");
        for reg in info_registers.iter() {
            if reg.name().contains("Писем") {
                println!("  - {}", reg.name());
            }
        }

        let target_register = config.find_register("ЗначенияДействийПриОбработкеПисем");

        if let Some(register) = target_register {
            println!("✅ Found register: {}", register.name());
            println!("  Type: {:?}", register.mdo_type());
            println!("  Dimensions: {}", register.dimensions().len());
            println!("  Resources: {}", register.resources().len());
            println!("  Attributes: {}", register.attributes().len());

            for dim in register.dimensions() {
                println!("    Dimension: {}", dim.name());
            }

            for res in register.resources() {
                println!("    Resource: {} - Type: {:?}", res.name(), res.attr_type());
            }

            for attr in register.attributes() {
                println!("    Attribute: {} - Type: {:?}", attr.name(), attr.attr_type());
            }

            assert!(register.is_information_register(), "Should be InformationRegister");
        } else {
            panic!("❌ Register 'ЗначенияДействийПриОбработкеПисем' not found in loaded configuration!");
        }
    }

    #[test]
    #[ignore]
    fn test_catalog_xml_only_without_directory() {
        let doc3_path = concat!(env!("HOME"), "/src/doc3/src/cf");

        if !std::path::Path::new(doc3_path).exists() {
            eprintln!("Skipping test: doc3 project not found at {}", doc3_path);
            return;
        }

        let config = load_from_directory(doc3_path).expect("Failed to load doc3 configuration");

        let catalog_name = "ПоставляемыеДополнительныеОтчетыИОбработки";

        let xml_path = format!("{}/Catalogs/{}.xml", doc3_path, catalog_name);
        let dir_path = format!("{}/Catalogs/{}", doc3_path, catalog_name);
        assert!(std::path::Path::new(&xml_path).exists(), "XML file should exist");
        assert!(
            !std::path::Path::new(&dir_path).exists(),
            "Directory should NOT exist (this is the test case)"
        );

        let exists =
            config.has_metadata_object(crate::metadata_object::MdoType::Catalog, catalog_name);
        assert!(exists, "Catalog '{}' should be loaded from XML-only file", catalog_name);
    }
}
