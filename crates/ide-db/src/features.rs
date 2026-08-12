use std::sync::Arc;

#[salsa::input(singleton, debug)]
pub struct FeaturesInput {
    #[returns(copy)]
    pub type_narrowing: bool,
    #[returns(copy)]
    pub env_options: hir::execution_env::EnvOptions,
    /// `None` selects the attested bundled catalog release.
    #[returns(clone)]
    pub target_platform_version: Option<Arc<str>>,
}
