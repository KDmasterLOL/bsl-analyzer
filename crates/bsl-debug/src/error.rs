use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum DebugError {
    #[error("module not found: {0}")]
    ModuleNotFound(String),

    #[error("object UUID not found in {0}")]
    ObjectIdNotFound(PathBuf),

    #[error("unknown module type: {dir_name}/{module_stem}")]
    UnknownModuleType { dir_name: String, module_stem: String },

    #[error("XML parse error in {path}: {source}")]
    XmlParse { path: PathBuf, source: quick_xml::Error },

    #[error("configuration root not found: {0}")]
    ConfigRootNotFound(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
