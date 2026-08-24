//! Metadata resolution for a query that belongs to no file.
//!
//! SDBL lowering asks for metadata through [`QueryMetadataResolver`], and every existing
//! implementation is anchored to a `FileId` — which root a query may see follows from which
//! file it sits in. A query handed to a tool as bare text has no such anchor, so it gets the
//! configurator's view instead: the base configuration plus every extension.

use std::sync::Arc;

use bsl_metadata::{
    AttributeType, MdoType, MetadataObject, MetadataResolver, QueryMetadataResolver, Register,
};

use crate::RootDatabaseImpl;

/// The whole-configuration view of metadata, for a consumer with no file to anchor
/// visibility to.
///
/// Deliberately wider than any single file's view: an object defined only in an extension is
/// found. That matches the MCP `metadata object` tool, which answers the same
/// "what does this configuration contain" question.
pub struct AcrossRootsQueryResolver<'a> {
    db: &'a RootDatabaseImpl,
}

impl std::fmt::Debug for AcrossRootsQueryResolver<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcrossRootsQueryResolver").finish_non_exhaustive()
    }
}

impl<'a> AcrossRootsQueryResolver<'a> {
    pub fn new(db: &'a RootDatabaseImpl) -> Self {
        Self { db }
    }
}

impl MetadataResolver for AcrossRootsQueryResolver<'_> {
    fn resolve_defined_type(&self, name: &str) -> Option<AttributeType> {
        self.db.resolve_defined_type_across_roots(name)
    }
}

impl QueryMetadataResolver for AcrossRootsQueryResolver<'_> {
    fn resolve_metadata_object(
        &self,
        mdo_type: MdoType,
        name: &str,
    ) -> Option<Arc<MetadataObject>> {
        self.db.resolve_metadata_object_across_roots(mdo_type, name)
    }

    fn resolve_register(&self, mdo_type: MdoType, name: &str) -> Option<Arc<Register>> {
        self.db.resolve_register_across_roots(mdo_type, name)
    }
}
