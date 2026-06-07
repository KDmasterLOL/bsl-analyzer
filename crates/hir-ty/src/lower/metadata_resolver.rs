use bsl_config::VisibleConfig;
use bsl_metadata::{AttributeType, MetadataResolver};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConfigsResolver<'a>(pub &'a [VisibleConfig]);

impl MetadataResolver for ConfigsResolver<'_> {
    fn resolve_defined_type(&self, name: &str) -> Option<AttributeType> {
        self.0.iter().rev().find_map(|cfg| cfg.configuration.resolve_defined_type(name))
    }
}
