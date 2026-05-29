pub mod builders;
pub mod display;
pub mod equality;
pub mod facet;
pub mod intern;
pub mod kind;
pub mod testing;

pub use kind::ConfigId;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {}
}
