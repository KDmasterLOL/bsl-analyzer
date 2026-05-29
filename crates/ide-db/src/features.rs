#[salsa::input(debug)]
pub struct FeaturesInput {
    pub type_narrowing: bool,
}
