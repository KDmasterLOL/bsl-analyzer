#[salsa::input(singleton, debug)]
pub struct FeaturesInput {
    pub type_narrowing: bool,
}
