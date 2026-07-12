#[salsa::input(singleton, debug)]
pub struct FeaturesInput {
    pub type_narrowing: bool,
    pub env_options: hir::execution_env::EnvOptions,
}
