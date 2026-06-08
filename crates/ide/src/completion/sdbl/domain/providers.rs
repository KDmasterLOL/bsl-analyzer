use bsl_metadata::Configuration;
use sdbl_hir::Scope;
use std::sync::Arc;
use syntax::TextSize;
use vfs::FileId;

pub trait MetadataProvider {
    fn get_configuration(&self) -> Option<Arc<Configuration>>;
}

pub trait ScopeProvider {
    fn get_scope(
        &self,
        file_id: FileId,
        bsl_literal_range: syntax::TextRange,
        sdbl_offset: TextSize,
    ) -> Option<Scope<'static>>;
}
