use std::sync::Arc;

use bsl_metadata::Configuration;
use vfs::FileId;

use crate::DefDatabase;

use bsl_config::VisibleConfig;

#[salsa::db]
pub trait ConfigsDatabase: DefDatabase {
    fn configurations(&self, file_id: FileId) -> Vec<VisibleConfig>;

    fn merged_visible_configuration(&self, file_id: FileId) -> Option<Arc<Configuration>>;
}
