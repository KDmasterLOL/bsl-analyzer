#[salsa::input(singleton, debug)]
pub struct FeaturesInput {
    #[returns(copy)]
    pub type_narrowing: bool,
    #[returns(copy)]
    pub env_options: hir::execution_env::EnvOptions,
}
