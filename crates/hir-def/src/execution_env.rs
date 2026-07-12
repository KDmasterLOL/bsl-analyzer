//! Execution-environment model: in which runtime environments (thin client,
//! web client, server, …) a given body of code can execute.
//!
//! The platform documents API availability per environment ("Доступность" in
//! the syntax helper). To compare that against a call site, the call site
//! itself needs an environment set: the intersection of where the *module*
//! runs and what the method's *compilation directive* selects. This module
//! computes that set from data already resident in [`ModuleMetadata`] and the
//! item tree — no extra queries.
//!
//! An empty [`EnvFlags`] means "don't know where this runs" — either the
//! metadata is missing (unknown) or the module and directive contradict each
//! other (impossible). Consumers must skip availability checks in both cases
//! rather than treat it as "available nowhere".
//!
//! This is deliberately finer-grained than [`call_graph::MethodDispatch`],
//! which collapses everything to can-run-on-client/server for dispatch
//! purposes; availability verdicts need the individual client kinds
//! (e.g. `ЧтениеТекста` exists on the thin client but not the web client).
//!
//! [`call_graph::MethodDispatch`]: crate::call_graph::MethodDispatch

use crate::item_tree::{Annotation, AnnotationKind};
use crate::ModuleMetadata;
use bsl_metadata::ModuleType;

/// Bit set of 1C:Enterprise execution environments.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EnvFlags(u8);

impl EnvFlags {
    pub const THIN_CLIENT: EnvFlags = EnvFlags(1 << 0);
    pub const WEB_CLIENT: EnvFlags = EnvFlags(1 << 1);
    pub const THICK_CLIENT_MANAGED: EnvFlags = EnvFlags(1 << 2);
    pub const THICK_CLIENT_ORDINARY: EnvFlags = EnvFlags(1 << 3);
    pub const SERVER: EnvFlags = EnvFlags(1 << 4);
    pub const MOBILE_CLIENT: EnvFlags = EnvFlags(1 << 5);
    pub const EXTERNAL_CONNECTION: EnvFlags = EnvFlags(1 << 6);

    pub const EMPTY: EnvFlags = EnvFlags(0);
    pub const ALL: EnvFlags = EnvFlags(0x7f);
    /// Non-interactive server-side environments.
    pub const SERVER_SIDE: EnvFlags = EnvFlags(Self::SERVER.0 | Self::EXTERNAL_CONNECTION.0);
    /// Managed-application client environments — every environment
    /// `&НаКлиенте` / `ClientManagedApplication` may legally select. The
    /// legacy thick client (ordinary application) is separate: it is added
    /// only through [`EnvOptions::ordinary_app_support`].
    pub const MANAGED_CLIENTS: EnvFlags = EnvFlags(
        Self::THIN_CLIENT.0
            | Self::WEB_CLIENT.0
            | Self::THICK_CLIENT_MANAGED.0
            | Self::MOBILE_CLIENT.0,
    );

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, other: EnvFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn intersects(self, other: EnvFlags) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn union(self, other: EnvFlags) -> EnvFlags {
        EnvFlags(self.0 | other.0)
    }

    pub const fn intersection(self, other: EnvFlags) -> EnvFlags {
        EnvFlags(self.0 & other.0)
    }

    /// Environments in `self` that are absent from `other` — the "missing"
    /// set an availability diagnostic reports.
    pub const fn without(self, other: EnvFlags) -> EnvFlags {
        EnvFlags(self.0 & !other.0)
    }

    /// Iterate the individual environments in the set, in declaration order.
    pub fn iter(self) -> impl Iterator<Item = EnvFlags> {
        [
            Self::THIN_CLIENT,
            Self::WEB_CLIENT,
            Self::THICK_CLIENT_MANAGED,
            Self::THICK_CLIENT_ORDINARY,
            Self::SERVER,
            Self::MOBILE_CLIENT,
            Self::EXTERNAL_CONNECTION,
        ]
        .into_iter()
        .filter(move |flag| self.contains(*flag))
    }

    /// Russian name of a single environment, in the wording of the platform
    /// syntax helper ("Доступность").
    pub fn name_ru(self) -> &'static str {
        match self {
            Self::THIN_CLIENT => "Тонкий клиент",
            Self::WEB_CLIENT => "Веб-клиент",
            Self::THICK_CLIENT_MANAGED => "Толстый клиент (управляемое приложение)",
            Self::THICK_CLIENT_ORDINARY => "Толстый клиент (обычное приложение)",
            Self::SERVER => "Сервер",
            Self::MOBILE_CLIENT => "Мобильный клиент",
            Self::EXTERNAL_CONNECTION => "Внешнее соединение",
            _ => "?",
        }
    }

    /// English name of a single environment, in EDT qualifier wording
    /// ("… is not defined [Web client]").
    pub fn name_en(self) -> &'static str {
        match self {
            Self::THIN_CLIENT => "Thin client",
            Self::WEB_CLIENT => "Web client",
            Self::THICK_CLIENT_MANAGED => "Thick client (managed application)",
            Self::THICK_CLIENT_ORDINARY => "Thick client (ordinary application)",
            Self::SERVER => "Server",
            Self::MOBILE_CLIENT => "Mobile client",
            Self::EXTERNAL_CONNECTION => "External connection",
            _ => "?",
        }
    }
}

impl std::ops::BitOr for EnvFlags {
    type Output = EnvFlags;
    fn bitor(self, rhs: EnvFlags) -> EnvFlags {
        self.union(rhs)
    }
}

impl std::ops::BitAnd for EnvFlags {
    type Output = EnvFlags;
    fn bitand(self, rhs: EnvFlags) -> EnvFlags {
        self.intersection(rhs)
    }
}

impl std::fmt::Debug for EnvFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            return write!(f, "EnvFlags()");
        }
        write!(f, "EnvFlags(")?;
        for (i, flag) in self.iter().enumerate() {
            if i > 0 {
                write!(f, "|")?;
            }
            write!(f, "{}", flag.name_en())?;
        }
        write!(f, ")")
    }
}

/// Project-level settings that shape environment sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvOptions {
    /// Interactive client environments the configuration targets. Client
    /// code (`&НаКлиенте`, client-flagged common modules) is assumed to run
    /// in exactly these. Mobile client is excluded by default: most
    /// configurations do not target it, and its availability markup in the
    /// platform data is the least reliable.
    pub client_environments: EnvFlags,
    /// Whether the legacy thick client (ordinary application) is part of the
    /// project's runtime surface. Mirrors the diagnostics-level
    /// `ordinary_app_support` setting.
    pub ordinary_app_support: bool,
}

impl Default for EnvOptions {
    fn default() -> Self {
        Self {
            client_environments: EnvFlags::THIN_CLIENT
                | EnvFlags::WEB_CLIENT
                | EnvFlags::THICK_CLIENT_MANAGED,
            ordinary_app_support: false,
        }
    }
}

impl EnvOptions {
    /// `client_environments` restricted to what a client mask may legally
    /// contain — stray server/external bits from config are dropped, and the
    /// legacy thick client enters only through `ordinary_app_support`.
    pub fn managed_client_envs(&self) -> EnvFlags {
        self.client_environments & EnvFlags::MANAGED_CLIENTS
    }

    /// The full client set including the legacy thick client when enabled.
    pub fn client_envs(&self) -> EnvFlags {
        if self.ordinary_app_support {
            self.managed_client_envs() | EnvFlags::THICK_CLIENT_ORDINARY
        } else {
            self.managed_client_envs()
        }
    }

    fn server_envs(&self) -> EnvFlags {
        EnvFlags::SERVER_SIDE
    }
}

/// Environments a module's code can execute in, before any per-method
/// directive narrows it. Empty means unknown.
pub fn module_base_env(metadata: &ModuleMetadata, opts: &EnvOptions) -> EnvFlags {
    match metadata.module_type {
        ModuleType::CommonModule => match &metadata.common_module {
            Some(cm) => common_module_env(cm, opts),
            None => EnvFlags::EMPTY,
        },
        // Form and command methods pick client or server per directive; the
        // module as a whole spans both.
        ModuleType::FormModule | ModuleType::CommandModule => opts.client_envs() | EnvFlags::SERVER,
        ModuleType::ManagedApplicationModule | ModuleType::ApplicationModule => {
            opts.managed_client_envs()
        }
        ModuleType::OrdinaryApplicationModule => EnvFlags::THICK_CLIENT_ORDINARY,
        ModuleType::ObjectModule
        | ModuleType::ManagerModule
        | ModuleType::RecordSetModule
        | ModuleType::ValueManagerModule => {
            if opts.ordinary_app_support {
                EnvFlags::SERVER_SIDE | EnvFlags::THICK_CLIENT_ORDINARY
            } else {
                EnvFlags::SERVER_SIDE
            }
        }
        ModuleType::SessionModule => EnvFlags::SERVER_SIDE,
        ModuleType::ExternalConnectionModule => EnvFlags::EXTERNAL_CONNECTION,
        ModuleType::HTTPServiceModule
        | ModuleType::WebServiceModule
        | ModuleType::IntegrationServiceModule => EnvFlags::SERVER,
        ModuleType::Unknown => EnvFlags::EMPTY,
    }
}

/// Environment set of a common module from its metadata flags. Unlike
/// [`crate::compute_execution_context`], which collapses to a dispatch
/// verdict, this keeps the individual client kinds and honours
/// `ClientOrdinaryApplication` (gated on `ordinary_app_support`), because
/// availability — not naming or dispatch — is decided here.
fn common_module_env(cm: &bsl_metadata::CommonModule, opts: &EnvOptions) -> EnvFlags {
    let mut env = EnvFlags::EMPTY;
    if cm.is_server() || cm.is_server_call() {
        env = env | EnvFlags::SERVER;
    }
    if cm.is_external_connection() {
        env = env | EnvFlags::EXTERNAL_CONNECTION;
    }
    // `ServerCall` modules are *called from* the client but *run* on the
    // server, so client flags on them are not added here.
    if !cm.is_server_call() {
        if cm.is_client_managed_application() {
            env = env | opts.managed_client_envs();
        }
        if cm.is_client_ordinary_application() && opts.ordinary_app_support {
            env = env | EnvFlags::THICK_CLIENT_ORDINARY;
        }
    }
    env
}

/// Environments a compilation directive selects, as a mask to intersect with
/// the module's base set. `None` in a form/command module means the platform
/// default `&НаСервере`; elsewhere the module alone decides.
pub fn directive_env(
    kind: Option<AnnotationKind>,
    module_type: ModuleType,
    opts: &EnvOptions,
) -> EnvFlags {
    match kind {
        Some(AnnotationKind::AtClient) => opts.client_envs(),
        Some(AnnotationKind::AtServer | AnnotationKind::AtServerNoContext) => opts.server_envs(),
        Some(AnnotationKind::AtClientAtServer | AnnotationKind::AtClientAtServerNoContext) => {
            opts.client_envs() | opts.server_envs()
        }
        // Weaving annotations carry no compilation directive of their own —
        // the effective directive comes from the intercepted base method.
        Some(
            AnnotationKind::Before
            | AnnotationKind::After
            | AnnotationKind::Instead
            | AnnotationKind::ChangeAndValidate,
        ) => EnvFlags::ALL,
        None => match module_type {
            ModuleType::FormModule | ModuleType::CommandModule => opts.server_envs(),
            _ => EnvFlags::ALL,
        },
    }
}

/// Environment set of one method body: where the module runs, narrowed by the
/// method's compilation directive. Empty means unknown — skip availability
/// checks. `annotations` is the method's item-tree annotation slice; the
/// first compilation directive wins (more than one is itself a diagnostic,
/// see `SeveralCompilerDirectives`).
pub fn body_env(
    metadata: &ModuleMetadata,
    annotations: &[Annotation],
    opts: &EnvOptions,
) -> EnvFlags {
    let base = module_base_env(metadata, opts);
    if base.is_empty() {
        return EnvFlags::EMPTY;
    }
    let directive = annotations.iter().map(|a| a.kind).find(is_compilation_directive);
    // A weaving-only interceptor (`&Вместо("…")` with no directive of its
    // own) inherits the intercepted method's directive, which is unknowable
    // here — so it must not fall into the form-module `&НаСервере` default.
    if directive.is_none() && annotations.iter().any(|a| !is_compilation_directive(&a.kind)) {
        return base;
    }
    base & directive_env(directive, metadata.module_type, opts)
}

/// Environment set of module-level code (top-level statements and variable
/// initializers), which never carries a directive.
pub fn module_code_env(metadata: &ModuleMetadata, opts: &EnvOptions) -> EnvFlags {
    module_base_env(metadata, opts)
}

fn is_compilation_directive(kind: &AnnotationKind) -> bool {
    matches!(
        kind,
        AnnotationKind::AtClient
            | AnnotationKind::AtServer
            | AnnotationKind::AtServerNoContext
            | AnnotationKind::AtClientAtServer
            | AnnotationKind::AtClientAtServerNoContext
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use text_size::TextRange;

    fn ann(kind: AnnotationKind) -> Annotation {
        Annotation { kind, range: TextRange::default() }
    }

    fn form_metadata() -> ModuleMetadata {
        ModuleMetadata::unknown(ModuleType::FormModule)
    }

    fn common_metadata(cm: bsl_metadata::CommonModule) -> ModuleMetadata {
        let mut metadata = ModuleMetadata::unknown(ModuleType::CommonModule);
        metadata.execution_context = Some(crate::compute_execution_context(&cm));
        metadata.common_module = Some(Arc::new(cm));
        metadata
    }

    const CLIENTS: EnvFlags = EnvFlags(
        EnvFlags::THIN_CLIENT.0 | EnvFlags::WEB_CLIENT.0 | EnvFlags::THICK_CLIENT_MANAGED.0,
    );

    #[test]
    fn form_module_directives() {
        let opts = EnvOptions::default();
        let md = form_metadata();

        assert_eq!(body_env(&md, &[ann(AnnotationKind::AtClient)], &opts), CLIENTS);
        assert_eq!(body_env(&md, &[ann(AnnotationKind::AtServer)], &opts), EnvFlags::SERVER);
        assert_eq!(
            body_env(&md, &[ann(AnnotationKind::AtServerNoContext)], &opts),
            EnvFlags::SERVER
        );
        assert_eq!(
            body_env(&md, &[ann(AnnotationKind::AtClientAtServer)], &opts),
            CLIENTS | EnvFlags::SERVER
        );
        assert_eq!(
            body_env(&md, &[ann(AnnotationKind::AtClientAtServerNoContext)], &opts),
            CLIENTS | EnvFlags::SERVER
        );
    }

    #[test]
    fn form_module_without_directive_defaults_to_server() {
        let opts = EnvOptions::default();
        assert_eq!(body_env(&form_metadata(), &[], &opts), EnvFlags::SERVER);

        let cmd = ModuleMetadata::unknown(ModuleType::CommandModule);
        assert_eq!(body_env(&cmd, &[], &opts), EnvFlags::SERVER);
    }

    #[test]
    fn weaving_annotation_does_not_narrow() {
        let opts = EnvOptions::default();
        let md = form_metadata();
        assert_eq!(
            body_env(&md, &[ann(AnnotationKind::Before), ann(AnnotationKind::AtClient)], &opts),
            CLIENTS
        );
        // Weaving-only: the interceptor inherits the base method's directive,
        // so nothing narrows — the whole module base stays.
        assert_eq!(
            body_env(&md, &[ann(AnnotationKind::Instead)], &opts),
            CLIENTS | EnvFlags::SERVER
        );
    }

    #[test]
    fn server_common_module() {
        let opts = EnvOptions::default();
        let cm = bsl_metadata::CommonModuleBuilder::default()
            .server(true)
            .external_connection(true)
            .build();
        let md = common_metadata(cm);
        assert_eq!(body_env(&md, &[], &opts), EnvFlags::SERVER_SIDE);
    }

    #[test]
    fn client_server_common_module() {
        let opts = EnvOptions::default();
        let cm = bsl_metadata::CommonModuleBuilder::default()
            .server(true)
            .client_managed_application(true)
            .build();
        let md = common_metadata(cm);
        assert_eq!(body_env(&md, &[], &opts), CLIENTS | EnvFlags::SERVER);
    }

    #[test]
    fn client_only_common_module() {
        let opts = EnvOptions::default();
        let cm =
            bsl_metadata::CommonModuleBuilder::default().client_managed_application(true).build();
        let md = common_metadata(cm);
        assert_eq!(body_env(&md, &[], &opts), CLIENTS);
    }

    #[test]
    fn server_call_module_runs_on_server_only() {
        let opts = EnvOptions::default();
        let cm = bsl_metadata::CommonModuleBuilder::default()
            .server(true)
            .server_call(true)
            .client_managed_application(true)
            .build();
        let md = common_metadata(cm);
        assert_eq!(body_env(&md, &[], &opts), EnvFlags::SERVER);
    }

    #[test]
    fn ordinary_client_flag_gated_on_option() {
        let cm = || {
            bsl_metadata::CommonModuleBuilder::default()
                .server(true)
                .client_ordinary_application(true)
                .build()
        };
        let off = EnvOptions::default();
        assert_eq!(body_env(&common_metadata(cm()), &[], &off), EnvFlags::SERVER);

        let on = EnvOptions { ordinary_app_support: true, ..EnvOptions::default() };
        assert_eq!(
            body_env(&common_metadata(cm()), &[], &on),
            EnvFlags::SERVER | EnvFlags::THICK_CLIENT_ORDINARY
        );
    }

    #[test]
    fn common_module_without_metadata_is_unknown() {
        let opts = EnvOptions::default();
        let md = ModuleMetadata::unknown(ModuleType::CommonModule);
        assert!(body_env(&md, &[], &opts).is_empty());

        let cm = bsl_metadata::CommonModuleBuilder::default().build();
        assert!(body_env(&common_metadata(cm), &[], &opts).is_empty());
    }

    #[test]
    fn directive_in_common_module_does_not_widen() {
        // A stray &НаКлиенте in a server-only common module must not make the
        // body client-capable: the module base wins on intersection.
        let opts = EnvOptions::default();
        let cm = bsl_metadata::CommonModuleBuilder::default().server(true).build();
        let md = common_metadata(cm);
        assert_eq!(body_env(&md, &[ann(AnnotationKind::AtClient)], &opts), EnvFlags::EMPTY);
    }

    #[test]
    fn object_family_modules_are_server_side() {
        let opts = EnvOptions::default();
        for ty in [
            ModuleType::ObjectModule,
            ModuleType::ManagerModule,
            ModuleType::RecordSetModule,
            ModuleType::ValueManagerModule,
        ] {
            let md = ModuleMetadata::unknown(ty);
            assert_eq!(body_env(&md, &[], &opts), EnvFlags::SERVER_SIDE, "{ty:?}");
        }

        let on = EnvOptions { ordinary_app_support: true, ..EnvOptions::default() };
        let md = ModuleMetadata::unknown(ModuleType::ObjectModule);
        assert_eq!(
            body_env(&md, &[], &on),
            EnvFlags::SERVER_SIDE | EnvFlags::THICK_CLIENT_ORDINARY
        );
    }

    #[test]
    fn service_and_app_modules() {
        let opts = EnvOptions::default();
        for ty in [
            ModuleType::HTTPServiceModule,
            ModuleType::WebServiceModule,
            ModuleType::IntegrationServiceModule,
        ] {
            assert_eq!(
                body_env(&ModuleMetadata::unknown(ty), &[], &opts),
                EnvFlags::SERVER,
                "{ty:?}"
            );
        }
        assert_eq!(
            body_env(&ModuleMetadata::unknown(ModuleType::SessionModule), &[], &opts),
            EnvFlags::SERVER_SIDE
        );
        assert_eq!(
            body_env(&ModuleMetadata::unknown(ModuleType::ExternalConnectionModule), &[], &opts),
            EnvFlags::EXTERNAL_CONNECTION
        );
        assert_eq!(
            body_env(&ModuleMetadata::unknown(ModuleType::ManagedApplicationModule), &[], &opts),
            CLIENTS
        );
        assert_eq!(
            body_env(&ModuleMetadata::unknown(ModuleType::OrdinaryApplicationModule), &[], &opts),
            EnvFlags::THICK_CLIENT_ORDINARY
        );
        assert!(body_env(&ModuleMetadata::unknown(ModuleType::Unknown), &[], &opts).is_empty());
    }

    #[test]
    fn external_only_common_module() {
        let opts = EnvOptions::default();
        let cm = bsl_metadata::CommonModuleBuilder::default().external_connection(true).build();
        assert_eq!(body_env(&common_metadata(cm), &[], &opts), EnvFlags::EXTERNAL_CONNECTION);
    }

    #[test]
    fn server_call_without_server_flag_still_runs_on_server() {
        let opts = EnvOptions::default();
        let cm = bsl_metadata::CommonModuleBuilder::default().server_call(true).build();
        assert_eq!(body_env(&common_metadata(cm), &[], &opts), EnvFlags::SERVER);
    }

    #[test]
    fn application_modules_are_managed_clients() {
        let opts = EnvOptions::default();
        assert_eq!(
            body_env(&ModuleMetadata::unknown(ModuleType::ApplicationModule), &[], &opts),
            CLIENTS
        );
        // ordinary_app_support widens forms and object modules, but never the
        // managed-application module itself.
        let on = EnvOptions { ordinary_app_support: true, ..EnvOptions::default() };
        assert_eq!(
            body_env(&ModuleMetadata::unknown(ModuleType::ManagedApplicationModule), &[], &on),
            CLIENTS
        );
    }

    #[test]
    fn client_environments_config_is_masked() {
        // Stray non-client bits from config must not leak into client masks,
        // and the legacy thick client enters only via ordinary_app_support.
        let opts = EnvOptions {
            client_environments: EnvFlags::THIN_CLIENT
                | EnvFlags::SERVER
                | EnvFlags::EXTERNAL_CONNECTION
                | EnvFlags::THICK_CLIENT_ORDINARY,
            ordinary_app_support: false,
        };
        assert_eq!(opts.client_envs(), EnvFlags::THIN_CLIENT);

        let md = form_metadata();
        assert_eq!(body_env(&md, &[ann(AnnotationKind::AtClient)], &opts), EnvFlags::THIN_CLIENT);
    }

    #[test]
    fn mobile_client_opt_in() {
        let opts = EnvOptions {
            client_environments: EnvFlags::THIN_CLIENT
                | EnvFlags::WEB_CLIENT
                | EnvFlags::THICK_CLIENT_MANAGED
                | EnvFlags::MOBILE_CLIENT,
            ordinary_app_support: false,
        };
        let md = form_metadata();
        assert_eq!(
            body_env(&md, &[ann(AnnotationKind::AtClient)], &opts),
            CLIENTS | EnvFlags::MOBILE_CLIENT
        );
    }

    #[test]
    fn set_operations_and_names() {
        let e = EnvFlags::SERVER | EnvFlags::WEB_CLIENT;
        assert!(e.contains(EnvFlags::SERVER));
        assert!(!e.contains(EnvFlags::THIN_CLIENT));
        assert!(e.intersects(EnvFlags::WEB_CLIENT));
        assert_eq!(e.without(EnvFlags::SERVER), EnvFlags::WEB_CLIENT);
        assert_eq!(e.iter().count(), 2);
        assert_eq!(EnvFlags::WEB_CLIENT.name_en(), "Web client");
        assert_eq!(EnvFlags::WEB_CLIENT.name_ru(), "Веб-клиент");
        assert_eq!(format!("{:?}", e), "EnvFlags(Web client|Server)");
        assert_eq!(format!("{:?}", EnvFlags::EMPTY), "EnvFlags()");
    }
}
